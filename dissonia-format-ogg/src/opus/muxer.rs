use std::io::Write;

use dissonia_common::vorbis::VorbisComments;
use dissonia_core::codecs::{CodecId, CodecParameters, CodecSpecific, OpusStreamMapping};
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
    pub comments: VorbisComments,
    pub pre_skip: Option<u16>,
    pub output_gain: i16,
}

impl Default for OggOpusMuxerOptions {
    fn default() -> Self {
        Self {
            serial_number: DEFAULT_SERIAL_NUMBER,
            comments: VorbisComments::new("dissonia"),
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
        self.options.comments.set_vendor(vendor_string);
        self
    }

    #[must_use]
    pub fn comment(mut self, field: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.comments.add(field, value);
        self
    }

    #[must_use]
    pub fn comments(mut self, comments: VorbisComments) -> Self {
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

        let channel_count = u8::try_from(spec.codec_params.channels.count())
            .map_err(|_| Error::Unsupported("ogg opus channel count exceeds u8"))?;

        if channel_count == 0 {
            return Err(Error::InvalidArgument(
                "ogg opus channel count must be greater than zero",
            ));
        }

        let stream_mapping = extract_opus_stream_mapping(&spec.codec_params)?;
        validate_opus_stream_mapping(channel_count, stream_mapping)?;

        let pre_skip = match self.options.pre_skip {
            Some(value) => value,
            None => default_pre_skip_from_codec_params(&spec.codec_params)?,
        };

        let id_header = build_opus_head(
            channel_count,
            pre_skip,
            spec.codec_params.sample_rate,
            self.options.output_gain,
            stream_mapping,
        )?;

        self.write_ogg_packet(&id_header, 0, OGG_HEADER_TYPE_BOS, 0)?;

        let tags = build_opus_tags(&self.options.comments)?;
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
            total_samples: None,
        })
    }
}

fn extract_opus_stream_mapping(params: &CodecParameters) -> Result<Option<&OpusStreamMapping>> {
    match &params.codec_specific {
        Some(CodecSpecific::Opus(mapping)) => Ok(Some(mapping)),
        Some(_) => Err(Error::InvalidArgument(
            "ogg opus muxer received non-opus codec-specific parameters",
        )),
        None => Ok(None),
    }
}

fn validate_opus_stream_mapping(
    channel_count: u8,
    stream_mapping: Option<&OpusStreamMapping>,
) -> Result<()> {
    let Some(stream_mapping) = stream_mapping else {
        if channel_count > 2 {
            return Err(Error::InvalidArgument(
                "multichannel ogg opus tracks must provide opus_stream_mapping metadata",
            ));
        }

        return Ok(());
    };

    if stream_mapping.family == 0 {
        if channel_count > 2 {
            return Err(Error::InvalidArgument(
                "opus mapping family 0 supports only mono or stereo",
            ));
        }

        if !stream_mapping.mapping.is_empty() {
            return Err(Error::InvalidArgument(
                "opus mapping family 0 must not provide a coded channel mapping table",
            ));
        }

        if stream_mapping.stream_count != 1 {
            return Err(Error::InvalidArgument(
                "opus mapping family 0 must use exactly one stream",
            ));
        }

        let expected_coupled = if channel_count == 2 { 1 } else { 0 };
        if stream_mapping.coupled_stream_count != expected_coupled {
            return Err(Error::InvalidArgument(
                "opus mapping family 0 has an invalid coupled stream count",
            ));
        }

        return Ok(());
    }

    if stream_mapping.stream_count == 0 {
        return Err(Error::InvalidArgument(
            "opus stream_count must be greater than zero",
        ));
    }

    if stream_mapping.coupled_stream_count > stream_mapping.stream_count {
        return Err(Error::InvalidArgument(
            "opus coupled_stream_count must not exceed stream_count",
        ));
    }

    if stream_mapping.mapping.len() != usize::from(channel_count) {
        return Err(Error::InvalidArgument(
            "opus channel mapping table length must equal channel count",
        ));
    }

    Ok(())
}

fn default_pre_skip_from_codec_params(params: &CodecParameters) -> Result<u16> {
    if params.sample_rate == 0 {
        return Err(Error::InvalidArgument(
            "ogg opus input sample rate must be non-zero",
        ));
    }

    let numerator = u128::from(params.encoder_delay)
        .checked_mul(48_000)
        .ok_or(Error::InvalidState("ogg opus pre_skip overflow"))?;
    let denominator = u128::from(params.sample_rate);

    if numerator % denominator != 0 {
        return Err(Error::InvalidArgument(
            "opus encoder delay does not convert cleanly to 48 kHz pre_skip units",
        ));
    }

    let pre_skip = numerator / denominator;

    u16::try_from(pre_skip)
        .map_err(|_| Error::Unsupported("opus pre_skip exceeds 16-bit header field"))
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
    use super::*;
    use dissonia_core::audio::{AudioSpec, ChannelLayout, SampleFormat};
    use dissonia_core::codecs::{CodecId, CodecParameters, CodecSpecific, OpusStreamMapping};
    use dissonia_core::formats::TrackSpec;
    use dissonia_core::units::TimeBase;
    use std::io::Cursor;

    #[test]
    fn writes_opus_head_and_tags_pages() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let mut params = CodecParameters::new(CodecId::Opus, spec);
        params.encoder_delay = 312;
        params.codec_specific = Some(CodecSpecific::Opus(OpusStreamMapping::new(
            0,
            1,
            1,
            Box::<[u8]>::default(),
        )));

        let mut muxer = OggOpusMuxer::builder(Cursor::new(Vec::<u8>::new()))
            .vendor_string("dissonia-test")
            .comment("ENCODER", "dissonia")
            .serial_number(0x1234_5678)
            .build();

        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(48_000)))?;

        let mut packet = dissonia_core::packet::EncodedPacket::new(vec![0_u8; 100]);
        packet.duration = Some(960);

        muxer.write_packet(track, packet)?;

        let summary = muxer.finalize()?;
        assert_eq!(summary.packet_count, 1);
        assert!(summary.bytes_written.is_some());

        let ogg = muxer.into_inner().into_inner();
        let pages = split_pages(&ogg);

        assert_eq!(pages.len(), 3);

        let id_header = page_payload(pages[0]);
        assert!(id_header.starts_with(b"OpusHead"));

        let tags = page_payload(pages[1]);
        assert!(tags.starts_with(b"OpusTags"));

        Ok(())
    }

    #[test]
    fn writes_multistream_opus_head_for_surround() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::SURROUND_5_1, SampleFormat::I16);
        let mut params = CodecParameters::new(CodecId::Opus, spec);
        params.codec_specific = Some(CodecSpecific::Opus(OpusStreamMapping::new(
            1,
            4,
            2,
            vec![0_u8, 4, 1, 2, 3, 5],
        )));

        let mut muxer = OggOpusMuxer::new(Cursor::new(Vec::<u8>::new()));
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(48_000)))?;

        let mut packet = dissonia_core::packet::EncodedPacket::new(vec![0_u8; 200]);
        packet.duration = Some(960);

        muxer.write_packet(track, packet)?;
        let summary = muxer.finalize()?;
        assert_eq!(summary.packet_count, 1);

        let ogg = muxer.into_inner().into_inner();
        let pages = split_pages(&ogg);

        let id_header = page_payload(pages[0]);
        assert!(id_header.starts_with(b"OpusHead"));
        assert_eq!(id_header[8], 1);
        assert_eq!(id_header[9], 6);
        assert_eq!(id_header[18], 1);
        assert_eq!(id_header[19], 4);
        assert_eq!(id_header[20], 2);
        assert_eq!(&id_header[21..27], &[0, 4, 1, 2, 3, 5]);

        Ok(())
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

    fn page_payload(page: &[u8]) -> &[u8] {
        let page_segments = usize::from(page[26]);
        &page[27 + page_segments..]
    }
}
