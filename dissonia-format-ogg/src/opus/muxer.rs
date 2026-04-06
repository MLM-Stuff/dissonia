use std::io::Write;

use dissonia_core::audio::ChannelLayout;
use dissonia_core::codecs::CodecId;
use dissonia_core::formats::{FinalizeSummary, FormatId, Muxer, TrackId, TrackSpec};
use dissonia_core::packet::EncodedPacket;
use dissonia_core::units::TimeBase;
use dissonia_core::{Error, Result};

use super::header::{build_opus_head, build_opus_tags};

const OGG_CAPTURE_PATTERN: &[u8; 4] = b"OggS";
const OGG_STREAM_STRUCTURE_VERSION: u8 = 0;
const OGG_HEADER_TYPE_CONTINUED: u8 = 0x01;
const OGG_HEADER_TYPE_BOS: u8 = 0x02;
const OGG_HEADER_TYPE_EOS: u8 = 0x04;

const DEFAULT_SERIAL_NUMBER: u32 = 0x6469_7373;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OggOpusMuxerOptions {
    pub serial_number: u32,
    pub vendor_string: String,
    pub comments: Vec<String>,
    pub pre_skip: Option<u16>,
    pub output_gain: i16,
}

impl Default for OggOpusMuxerOptions {
    fn default() -> Self {
        Self {
            serial_number: DEFAULT_SERIAL_NUMBER,
            vendor_string: String::from("dissonia"),
            comments: Vec::new(),
            pre_skip: None,
            output_gain: 0,
        }
    }
}

#[derive(Debug)]
pub struct OggOpusMuxerBuilder<W> {
    writer: W,
    options: OggOpusMuxerOptions,
}

impl<W> OggOpusMuxerBuilder<W> {
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            options: OggOpusMuxerOptions::default(),
        }
    }

    #[must_use]
    pub fn options(mut self, options: OggOpusMuxerOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn serial_number(mut self, serial_number: u32) -> Self {
        self.options.serial_number = serial_number;
        self
    }

    #[must_use]
    pub fn vendor_string(mut self, vendor_string: impl Into<String>) -> Self {
        self.options.vendor_string = vendor_string.into();
        self
    }

    #[must_use]
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.options.comments.push(comment.into());
        self
    }

    #[must_use]
    pub fn comments(mut self, comments: Vec<String>) -> Self {
        self.options.comments = comments;
        self
    }

    #[must_use]
    pub fn pre_skip(mut self, pre_skip: u16) -> Self {
        self.options.pre_skip = Some(pre_skip);
        self
    }

    #[must_use]
    pub fn output_gain(mut self, output_gain: i16) -> Self {
        self.options.output_gain = output_gain;
        self
    }

    #[must_use]
    pub fn build(self) -> OggOpusMuxer<W> {
        OggOpusMuxer {
            writer: self.writer,
            options: self.options,
            track: None,
            pending_packet: None,
            audio_granule_position: 0,
            page_sequence_number: 0,
            bytes_written: 0,
            packet_count: 0,
            finalized: false,
        }
    }
}

#[derive(Debug)]
pub struct OggOpusMuxer<W> {
    writer: W,
    options: OggOpusMuxerOptions,
    track: Option<TrackState>,
    pending_packet: Option<PendingAudioPacket>,
    audio_granule_position: u64,
    page_sequence_number: u64,
    bytes_written: u64,
    packet_count: u64,
    finalized: bool,
}

#[derive(Clone, Copy, Debug)]
struct TrackState {
    id: TrackId,
    time_base: TimeBase,
    pre_skip: u16,
}

#[derive(Debug)]
struct PendingAudioPacket {
    data: Vec<u8>,
    granule_position: u64,
    trim_end: u64,
}

impl<W> OggOpusMuxer<W> {
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self::builder(writer).build()
    }

    #[must_use]
    pub fn builder(writer: W) -> OggOpusMuxerBuilder<W> {
        OggOpusMuxerBuilder::new(writer)
    }

    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W> OggOpusMuxer<W>
where
    W: Write,
{
    fn ensure_writable(&self) -> Result<()> {
        if self.finalized {
            return Err(Error::InvalidState("muxer already finalized"));
        }

        Ok(())
    }

    fn write_headers(&mut self, spec: &TrackSpec) -> Result<TrackState> {
        if spec.codec_params.codec != CodecId::Opus {
            return Err(Error::InvalidArgument(
                "ogg opus muxer requires an opus track",
            ));
        }

        let channels = spec.codec_params.channels;
        let channel_count = if channels == ChannelLayout::MONO {
            1_u8
        } else if channels == ChannelLayout::STEREO {
            2_u8
        } else {
            return Err(Error::Unsupported(
                "ogg opus v1 currently supports only mono or stereo family-0 streams",
            ));
        };

        let pre_skip = match self.options.pre_skip {
            Some(value) => value,
            None => u16::try_from(spec.codec_params.encoder_delay).map_err(|_| {
                Error::Unsupported("opus encoder delay exceeds 16-bit pre-skip field")
            })?,
        };

        let id_header = build_opus_head(
            channel_count,
            pre_skip,
            spec.codec_params.sample_rate,
            self.options.output_gain,
        )?;

        self.write_ogg_packet(&id_header, 0, OGG_HEADER_TYPE_BOS, 0)?;

        let tags = build_opus_tags(&self.options.vendor_string, &self.options.comments)?;
        self.write_ogg_packet(&tags, 0, 0, 0)?;

        Ok(TrackState {
            id: TrackId(0),
            time_base: spec.time_base,
            pre_skip,
        })
    }

    fn write_pending_audio_packet(&mut self, packet: PendingAudioPacket, eos: bool) -> Result<()> {
        let complete_granule_position =
            if eos {
                packet.granule_position.checked_sub(packet.trim_end).ok_or(
                    Error::InvalidArgument("final opus trim exceeds cumulative audio duration"),
                )?
            } else {
                if packet.trim_end != 0 {
                    return Err(Error::InvalidArgument(
                        "opus trim_end may only be set on the final packet",
                    ));
                }

                packet.granule_position
            };

        let last_page_flags = if eos { OGG_HEADER_TYPE_EOS } else { 0 };
        self.write_ogg_packet(&packet.data, complete_granule_position, 0, last_page_flags)
    }

    fn write_ogg_packet(
        &mut self,
        packet: &[u8],
        complete_granule_position: u64,
        first_page_flags: u8,
        last_page_flags: u8,
    ) -> Result<()> {
        let lacing_values = build_lacing_values(packet.len())?;

        let mut data_offset = 0_usize;
        let mut lacing_offset = 0_usize;
        let mut first_page = true;

        while lacing_offset < lacing_values.len() {
            let page_segment_count = (lacing_values.len() - lacing_offset).min(255);
            let page_lacing = &lacing_values[lacing_offset..lacing_offset + page_segment_count];
            let page_data_len = page_lacing
                .iter()
                .map(|&value| usize::from(value))
                .sum::<usize>();
            let completes_packet = lacing_offset + page_segment_count == lacing_values.len();

            let mut header_type = if first_page {
                first_page_flags
            } else {
                OGG_HEADER_TYPE_CONTINUED
            };

            if completes_packet {
                header_type |= last_page_flags;
            }

            let granule_position = if completes_packet {
                complete_granule_position
            } else {
                u64::MAX
            };

            let page_data = &packet[data_offset..data_offset + page_data_len];
            self.write_page(header_type, granule_position, page_lacing, page_data)?;

            data_offset += page_data_len;
            lacing_offset += page_segment_count;
            first_page = false;
        }

        Ok(())
    }

    fn write_page(
        &mut self,
        header_type: u8,
        granule_position: u64,
        segment_table: &[u8],
        body: &[u8],
    ) -> Result<()> {
        let page_sequence_number = u32::try_from(self.page_sequence_number)
            .map_err(|_| Error::Unsupported("ogg page sequence number exceeds u32"))?;
        let page_segments = u8::try_from(segment_table.len())
            .map_err(|_| Error::Unsupported("ogg page segment count exceeds u8"))?;

        let capacity = 27_usize
            .checked_add(segment_table.len())
            .and_then(|value| value.checked_add(body.len()))
            .ok_or(Error::Unsupported("ogg page size exceeds platform limits"))?;

        let mut page = Vec::with_capacity(capacity);
        page.extend_from_slice(OGG_CAPTURE_PATTERN);
        page.push(OGG_STREAM_STRUCTURE_VERSION);
        page.push(header_type);
        page.extend_from_slice(&granule_position.to_le_bytes());
        page.extend_from_slice(&self.options.serial_number.to_le_bytes());
        page.extend_from_slice(&page_sequence_number.to_le_bytes());
        page.extend_from_slice(&0_u32.to_le_bytes());
        page.push(page_segments);
        page.extend_from_slice(segment_table);
        page.extend_from_slice(body);

        let checksum = ogg_crc(&page);
        page[22..26].copy_from_slice(&checksum.to_le_bytes());

        self.writer.write_all(&page)?;
        self.bytes_written = self
            .bytes_written
            .checked_add(
                u64::try_from(page.len())
                    .map_err(|_| Error::Unsupported("ogg page size exceeds u64"))?,
            )
            .ok_or(Error::InvalidState("bytes_written overflow"))?;
        self.page_sequence_number = self
            .page_sequence_number
            .checked_add(1)
            .ok_or(Error::InvalidState("ogg page sequence overflow"))?;

        Ok(())
    }
}

impl<W> Muxer for OggOpusMuxer<W>
where
    W: Write + Send,
{
    fn format_id(&self) -> FormatId {
        FormatId::Ogg
    }

    fn add_track(&mut self, spec: TrackSpec) -> Result<TrackId> {
        self.ensure_writable()?;

        if self.track.is_some() {
            return Err(Error::Unsupported(
                "ogg opus muxer supports only one audio track",
            ));
        }

        let state = self.write_headers(&spec)?;
        let track_id = state.id;
        self.track = Some(state);

        Ok(track_id)
    }

    fn write_packet(&mut self, track: TrackId, packet: EncodedPacket) -> Result<()> {
        self.ensure_writable()?;

        let state = self.track.ok_or(Error::InvalidState(
            "cannot write packet before adding a track",
        ))?;

        if track != state.id {
            return Err(Error::InvalidArgument(
                "packet written to unknown ogg opus track",
            ));
        }

        if packet.trim_start != 0 {
            return Err(Error::Unsupported(
                "ogg opus muxer does not support packet trim_start; use pre_skip instead",
            ));
        }

        if packet.data.is_empty() {
            return Err(Error::InvalidArgument("ogg opus packets must not be empty"));
        }

        let duration = packet.duration.ok_or(Error::InvalidArgument(
            "ogg opus packets must provide a duration",
        ))?;

        let decoded_samples = scale_to_opus_samples(duration, state.time_base)?;
        if decoded_samples == 0 {
            return Err(Error::InvalidArgument(
                "ogg opus packet duration converts to zero 48 kHz samples",
            ));
        }

        let trim_end = scale_to_opus_samples(u64::from(packet.trim_end), state.time_base)?;
        if trim_end > decoded_samples {
            return Err(Error::InvalidArgument(
                "ogg opus packet trim_end exceeds packet duration",
            ));
        }

        if let Some(previous) = self.pending_packet.as_ref() {
            if previous.trim_end != 0 {
                return Err(Error::InvalidArgument(
                    "opus trim_end may only be set on the final packet",
                ));
            }
        }

        if let Some(previous) = self.pending_packet.take() {
            self.write_pending_audio_packet(previous, false)?;
        }

        let data: Vec<u8> = packet.data.into();
        let granule_position = self
            .audio_granule_position
            .checked_add(decoded_samples)
            .ok_or(Error::InvalidState("ogg opus granule position overflow"))?;
        self.audio_granule_position = granule_position;

        self.pending_packet = Some(PendingAudioPacket {
            data,
            granule_position,
            trim_end,
        });

        self.packet_count = self
            .packet_count
            .checked_add(1)
            .ok_or(Error::InvalidState("packet count overflow"))?;

        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.ensure_writable()?;
        self.writer.flush()?;
        Ok(())
    }

    fn finalize(&mut self) -> Result<FinalizeSummary> {
        self.ensure_writable()?;

        let state = self.track.ok_or(Error::InvalidState(
            "cannot finalize ogg opus without a track",
        ))?;

        let pending = self.pending_packet.take().ok_or(Error::InvalidState(
            "cannot finalize ogg opus without at least one audio packet",
        ))?;

        let final_granule_position = pending
            .granule_position
            .checked_sub(pending.trim_end)
            .ok_or(Error::InvalidArgument(
                "final opus trim exceeds cumulative audio duration",
            ))?;

        if final_granule_position < u64::from(state.pre_skip) {
            return Err(Error::InvalidArgument(
                "final ogg opus granule position is smaller than pre_skip",
            ));
        }

        self.write_pending_audio_packet(pending, true)?;
        self.writer.flush()?;
        self.finalized = true;

        Ok(FinalizeSummary {
            bytes_written: Some(self.bytes_written),
            packet_count: self.packet_count,
        })
    }
}

fn build_lacing_values(packet_len: usize) -> Result<Vec<u8>> {
    let full_segments = packet_len / 255;
    let remainder = packet_len % 255;

    let mut lacing = vec![255_u8; full_segments];
    if remainder != 0 || packet_len == 0 {
        lacing.push(
            u8::try_from(remainder)
                .map_err(|_| Error::Unsupported("ogg lacing remainder exceeds u8"))?,
        );
    } else {
        lacing.push(0);
    }

    Ok(lacing)
}

fn scale_to_opus_samples(value: u64, time_base: TimeBase) -> Result<u64> {
    let numerator = u128::from(value)
        .checked_mul(u128::from(time_base.numer()))
        .and_then(|result| result.checked_mul(48_000))
        .ok_or(Error::InvalidState(
            "ogg opus timestamp conversion overflow",
        ))?;
    let denominator = u128::from(time_base.denom());

    if numerator % denominator != 0 {
        return Err(Error::InvalidArgument(
            "track time base does not convert cleanly to 48 kHz opus samples",
        ));
    }

    u64::try_from(numerator / denominator)
        .map_err(|_| Error::Unsupported("ogg opus sample count exceeds u64"))
}

fn ogg_crc(bytes: &[u8]) -> u32 {
    let mut crc = 0_u32;

    for &byte in bytes {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if (crc & 0x8000_0000) != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }

    crc
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use dissonia_core::audio::{AudioSpec, ChannelLayout, SampleFormat};
    use dissonia_core::codecs::{CodecId, CodecParameters};
    use dissonia_core::formats::TrackSpec;
    use dissonia_core::packet::EncodedPacket;
    use dissonia_core::units::TimeBase;

    use super::*;

    #[test]
    fn writes_headers_and_one_audio_page() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let params = CodecParameters::new(CodecId::Opus, spec);

        let mut muxer = OggOpusMuxer::builder(Cursor::new(Vec::<u8>::new()))
            .vendor_string("dissonia-test")
            .comment("ENCODER=dissonia")
            .serial_number(0x1234_5678)
            .build();

        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(48_000)))?;

        let mut packet = EncodedPacket::new(vec![0xF8, 0xFF, 0xFE]);
        packet.duration = Some(960);
        muxer.write_packet(track, packet)?;

        let summary = muxer.finalize()?;
        assert_eq!(summary.packet_count, 1);
        assert!(summary.bytes_written.is_some());

        let bytes = muxer.into_inner().into_inner();
        let pages = split_pages(&bytes);

        assert_eq!(pages.len(), 3);
        assert_eq!(page_header_type(pages[0]), OGG_HEADER_TYPE_BOS);
        assert_eq!(page_granule_position(pages[0]), 0);
        assert_eq!(page_serial(pages[0]), 0x1234_5678);
        assert_eq!(page_sequence(pages[0]), 0);
        assert!(page_payload(pages[0]).starts_with(b"OpusHead"));

        assert_eq!(page_granule_position(pages[1]), 0);
        assert_eq!(page_sequence(pages[1]), 1);
        assert!(page_payload(pages[1]).starts_with(b"OpusTags"));

        assert_eq!(page_header_type(pages[2]), OGG_HEADER_TYPE_EOS);
        assert_eq!(page_granule_position(pages[2]), 960);
        assert_eq!(page_sequence(pages[2]), 2);
        assert_eq!(page_payload(pages[2]), &[0xF8, 0xFF, 0xFE]);

        Ok(())
    }

    #[test]
    fn applies_end_trim_on_final_page() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::MONO, SampleFormat::I16);
        let params = CodecParameters::new(CodecId::Opus, spec);

        let mut muxer = OggOpusMuxer::new(Cursor::new(Vec::<u8>::new()));
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(48_000)))?;

        let mut packet = EncodedPacket::new(vec![0xF8, 0xAA]);
        packet.duration = Some(960);
        packet.trim_end = 480;
        muxer.write_packet(track, packet)?;
        muxer.finalize()?;

        let bytes = muxer.into_inner().into_inner();
        let pages = split_pages(&bytes);

        assert_eq!(pages.len(), 3);
        assert_eq!(page_header_type(pages[2]), OGG_HEADER_TYPE_EOS);
        assert_eq!(page_granule_position(pages[2]), 480);

        Ok(())
    }

    #[test]
    fn scales_track_time_base_to_48khz_granules() -> Result<()> {
        let spec = AudioSpec::new(24_000, ChannelLayout::MONO, SampleFormat::I16);
        let params = CodecParameters::new(CodecId::Opus, spec);

        let mut muxer = OggOpusMuxer::new(Cursor::new(Vec::<u8>::new()));
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(24_000)))?;

        let mut packet = EncodedPacket::new(vec![0xF8, 0xBB]);
        packet.duration = Some(480);
        muxer.write_packet(track, packet)?;
        muxer.finalize()?;

        let bytes = muxer.into_inner().into_inner();
        let pages = split_pages(&bytes);
        assert_eq!(page_granule_position(pages[2]), 960);

        Ok(())
    }

    #[test]
    fn rejects_non_opus_tracks() {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let params = CodecParameters::new(CodecId::PcmS16Le, spec);

        let mut muxer = OggOpusMuxer::new(Cursor::new(Vec::<u8>::new()));
        let error = muxer
            .add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(48_000)))
            .unwrap_err();

        match error {
            Error::InvalidArgument(_) => {}
            other => panic!("unexpected error: {other}"),
        }
    }

    fn split_pages(bytes: &[u8]) -> Vec<&[u8]> {
        let mut pages = Vec::new();
        let mut offset = 0_usize;

        while offset < bytes.len() {
            assert_eq!(&bytes[offset..offset + 4], b"OggS");
            let page_segments = usize::from(bytes[offset + 26]);
            let segment_table_start = offset + 27;
            let segment_table_end = segment_table_start + page_segments;
            let body_len = bytes[segment_table_start..segment_table_end]
                .iter()
                .map(|&value| usize::from(value))
                .sum::<usize>();
            let page_end = segment_table_end + body_len;
            pages.push(&bytes[offset..page_end]);
            offset = page_end;
        }

        pages
    }

    fn page_header_type(page: &[u8]) -> u8 {
        page[5]
    }

    fn page_granule_position(page: &[u8]) -> u64 {
        u64::from_le_bytes(page[6..14].try_into().unwrap())
    }

    fn page_serial(page: &[u8]) -> u32 {
        u32::from_le_bytes(page[14..18].try_into().unwrap())
    }

    fn page_sequence(page: &[u8]) -> u32 {
        u32::from_le_bytes(page[18..22].try_into().unwrap())
    }

    fn page_payload(page: &[u8]) -> &[u8] {
        let page_segments = usize::from(page[26]);
        &page[27 + page_segments..]
    }
}
