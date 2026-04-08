use std::io::{Seek, SeekFrom, Write};

use dissonia_common::vorbis::VorbisComments;
use dissonia_core::codecs::{CodecId, CodecSpecific};
use dissonia_core::formats::{FinalizeSummary, FormatId, Muxer, TrackId, TrackSpec};
use dissonia_core::packet::EncodedPacket;
use dissonia_core::{Error, Result};

use crate::metadata::{self, SeekPoint};

const FLAC_MARKER: &[u8; 4] = b"fLaC";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlacMuxerOptions {
    pub comments: VorbisComments,
    pub padding: u32,
    pub max_seek_points: u32,
}

impl Default for FlacMuxerOptions {
    fn default() -> Self {
        Self {
            comments: VorbisComments::new("dissonia"),
            padding: 8192,
            max_seek_points: 100,
        }
    }
}

#[derive(Debug)]
pub struct FlacMuxerBuilder<W> {
    writer: W,
    options: FlacMuxerOptions,
}

impl<W> FlacMuxerBuilder<W> {
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            options: FlacMuxerOptions::default(),
        }
    }

    #[must_use]
    pub fn options(mut self, options: FlacMuxerOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn vendor_string(mut self, vendor: impl Into<String>) -> Self {
        self.options.comments.set_vendor(vendor);
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
    pub fn padding(mut self, bytes: u32) -> Self {
        self.options.padding = bytes;
        self
    }

    #[must_use]
    pub fn max_seek_points(mut self, count: u32) -> Self {
        self.options.max_seek_points = count;
        self
    }

    #[must_use]
    pub fn build(self) -> FlacMuxer<W> {
        FlacMuxer {
            writer: self.writer,
            options: self.options,
            track: None,
            stream_info_offset: 0,
            seektable_payload_offset: 0,
            seektable_num_points: 0,
            audio_start_offset: 0,
            frame_records: Vec::new(),
            min_frame_size: u32::MAX,
            max_frame_size: 0,
            total_samples: 0,
            bytes_written: 0,
            packet_count: 0,
            finalized: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct FrameRecord {
    sample_number: u64,
    stream_offset: u64,
    frame_samples: u16,
}

#[derive(Debug)]
pub struct FlacMuxer<W> {
    writer: W,
    options: FlacMuxerOptions,
    track: Option<TrackState>,
    stream_info_offset: u64,
    seektable_payload_offset: u64,
    seektable_num_points: u32,
    audio_start_offset: u64,
    frame_records: Vec<FrameRecord>,
    min_frame_size: u32,
    max_frame_size: u32,
    total_samples: u64,
    bytes_written: u64,
    packet_count: u64,
    finalized: bool,
}

#[derive(Clone, Debug)]
struct TrackState {
    id: TrackId,
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    min_block_size: u16,
    max_block_size: u16,
}

impl<W> FlacMuxer<W> {
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self::builder(writer).build()
    }

    #[must_use]
    pub fn builder(writer: W) -> FlacMuxerBuilder<W> {
        FlacMuxerBuilder::new(writer)
    }

    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W> FlacMuxer<W>
where
    W: Write + Seek,
{
    fn ensure_writable(&self) -> Result<()> {
        if self.finalized {
            return Err(Error::InvalidState("muxer already finalized"));
        }
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.writer.write_all(buf)?;
        self.bytes_written = self
            .bytes_written
            .checked_add(buf.len() as u64)
            .ok_or(Error::InvalidState("bytes_written overflow"))?;
        Ok(())
    }

    fn write_header(&mut self, spec: &TrackSpec) -> Result<TrackState> {
        if spec.codec_params.codec != CodecId::Flac {
            return Err(Error::InvalidArgument("flac muxer requires a flac track"));
        }

        let flac_info = match &spec.codec_params.codec_specific {
            Some(CodecSpecific::Flac(info)) => info,
            Some(_) => {
                return Err(Error::InvalidArgument(
                    "flac muxer received non-flac codec-specific parameters",
                ));
            }
            None => {
                return Err(Error::InvalidArgument(
                    "flac muxer requires FlacStreamInfo in codec parameters",
                ));
            }
        };

        let channels = u8::try_from(spec.codec_params.channels.count())
            .map_err(|_| Error::Unsupported("flac channel count exceeds u8"))?;

        if channels == 0 || channels > 8 {
            return Err(Error::InvalidArgument("flac supports 1–8 channels"));
        }

        let sample_rate = spec.codec_params.sample_rate;
        if sample_rate == 0 || sample_rate > 655_350 {
            return Err(Error::InvalidArgument("flac sample rate must be 1–655350"));
        }

        let bits_per_sample = flac_info.bits_per_sample;
        if !(4..=32).contains(&bits_per_sample) {
            return Err(Error::InvalidArgument("flac bits_per_sample must be 4–32"));
        }

        self.write_all(FLAC_MARKER)?;

        self.stream_info_offset = self.bytes_written;

        let has_seektable = self.options.max_seek_points > 0;
        let has_comments = true;
        let has_padding = self.options.padding > 0;

        let si_is_last = !has_seektable && !has_comments && !has_padding;
        let stream_info_block = metadata::build_stream_info_block(
            si_is_last,
            flac_info.min_block_size,
            flac_info.max_block_size,
            0,
            0,
            sample_rate,
            channels,
            bits_per_sample,
            0,
            &[0; 16],
        );
        self.write_all(&stream_info_block)?;

        if has_seektable {
            let st_is_last = !has_comments && !has_padding;
            self.seektable_payload_offset = self.bytes_written + 4;
            self.seektable_num_points = self.options.max_seek_points;
            let seektable_block =
                metadata::build_seektable_block(st_is_last, self.options.max_seek_points);
            self.write_all(&seektable_block)?;
        }

        let vc_is_last = !has_padding;
        let comment_block =
            metadata::build_vorbis_comment_block(vc_is_last, &self.options.comments)?;
        self.write_all(&comment_block)?;

        if has_padding {
            let padding_block = metadata::build_padding_block(true, self.options.padding);
            self.write_all(&padding_block)?;
        }

        self.audio_start_offset = self.bytes_written;

        Ok(TrackState {
            id: TrackId(0),
            sample_rate,
            channels,
            bits_per_sample,
            min_block_size: flac_info.min_block_size,
            max_block_size: flac_info.max_block_size,
        })
    }

    fn patch_stream_info(&mut self) -> Result<()> {
        let state = self.track.as_ref().ok_or(Error::InvalidState(
            "cannot patch streaminfo without a track",
        ))?;

        let min_frame = if self.min_frame_size == u32::MAX {
            0
        } else {
            self.min_frame_size
        };

        let block = metadata::build_stream_info_block(
            false,
            state.min_block_size,
            state.max_block_size,
            min_frame,
            self.max_frame_size,
            state.sample_rate,
            state.channels,
            state.bits_per_sample,
            self.total_samples,
            &[0; 16],
        );

        self.writer
            .seek(SeekFrom::Start(self.stream_info_offset + 4))?;
        self.writer.write_all(&block[4..])?;

        Ok(())
    }

    fn patch_seektable(&mut self) -> Result<()> {
        if self.seektable_num_points == 0 {
            return Ok(());
        }

        let num_points = self.seektable_num_points as usize;
        let num_records = self.frame_records.len();

        let mut selected: Vec<SeekPoint> = Vec::with_capacity(num_points);

        if num_records > 0 && num_points > 0 {
            if num_records <= num_points {
                for rec in &self.frame_records {
                    selected.push(SeekPoint {
                        sample_number: rec.sample_number,
                        stream_offset: rec.stream_offset,
                        frame_samples: rec.frame_samples,
                    });
                }
            } else {
                for i in 0..num_points {
                    let idx = i * num_records / num_points;
                    let rec = &self.frame_records[idx];
                    if selected
                        .last()
                        .is_some_and(|last| last.sample_number == rec.sample_number)
                    {
                        continue;
                    }
                    selected.push(SeekPoint {
                        sample_number: rec.sample_number,
                        stream_offset: rec.stream_offset,
                        frame_samples: rec.frame_samples,
                    });
                }
            }
        }

        while selected.len() < num_points {
            selected.push(SeekPoint::placeholder());
        }

        self.writer
            .seek(SeekFrom::Start(self.seektable_payload_offset))?;
        for sp in &selected {
            self.writer.write_all(&sp.encode())?;
        }

        Ok(())
    }
}

impl<W> Muxer for FlacMuxer<W>
where
    W: Write + Seek + Send,
{
    fn format_id(&self) -> FormatId {
        FormatId::Flac
    }

    fn add_track(&mut self, spec: TrackSpec) -> Result<TrackId> {
        self.ensure_writable()?;

        if self.track.is_some() {
            return Err(Error::Unsupported("flac supports only one audio track"));
        }

        let state = self.write_header(&spec)?;
        let track_id = state.id;
        self.track = Some(state);

        Ok(track_id)
    }

    fn write_packet(&mut self, track: TrackId, packet: EncodedPacket) -> Result<()> {
        self.ensure_writable()?;

        let state = self.track.as_ref().ok_or(Error::InvalidState(
            "cannot write packet before adding a track",
        ))?;

        if track != state.id {
            return Err(Error::InvalidArgument(
                "packet written to unknown flac track",
            ));
        }

        if packet.data.is_empty() {
            return Ok(());
        }

        if packet.data.len() < 2 || packet.data[0] != 0xFF || (packet.data[1] & 0xFC) != 0xF8 {
            return Err(Error::InvalidArgument(
                "flac packet does not start with a valid frame sync code",
            ));
        }

        let frame_size = u32::try_from(packet.data.len())
            .map_err(|_| Error::Unsupported("flac frame size exceeds u32"))?;

        if frame_size < self.min_frame_size {
            self.min_frame_size = frame_size;
        }
        if frame_size > self.max_frame_size {
            self.max_frame_size = frame_size;
        }

        let duration = packet.duration.unwrap_or(0);

        let stream_offset = self
            .bytes_written
            .checked_sub(self.audio_start_offset)
            .ok_or(Error::InvalidState("audio offset underflow"))?;
        let frame_samples = u16::try_from(duration).unwrap_or(u16::MAX);
        self.frame_records.push(FrameRecord {
            sample_number: self.total_samples,
            stream_offset,
            frame_samples,
        });

        self.total_samples = self
            .total_samples
            .checked_add(duration)
            .ok_or(Error::InvalidState("total sample count overflow"))?;

        self.write_all(&packet.data)?;

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

        if self.track.is_none() {
            return Err(Error::InvalidState("cannot finalize flac without a track"));
        }

        self.patch_stream_info()?;
        self.patch_seektable()?;

        self.writer.seek(SeekFrom::End(0))?;
        self.writer.flush()?;
        self.finalized = true;

        Ok(FinalizeSummary {
            bytes_written: Some(self.bytes_written),
            packet_count: self.packet_count,
            total_samples: Some(self.total_samples),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dissonia_core::audio::{AudioSpec, ChannelLayout, SampleFormat};
    use dissonia_core::codecs::{CodecId, CodecParameters, CodecSpecific, FlacStreamInfo};
    use dissonia_core::formats::TrackSpec;
    use dissonia_core::units::TimeBase;
    use std::io::Cursor;

    fn test_codec_params() -> CodecParameters {
        let spec = AudioSpec::new(44_100, ChannelLayout::STEREO, SampleFormat::I16);
        let mut params = CodecParameters::new(CodecId::Flac, spec);
        params.codec_specific = Some(CodecSpecific::Flac(FlacStreamInfo {
            min_block_size: 4096,
            max_block_size: 4096,
            min_frame_size: 0,
            max_frame_size: 0,
            bits_per_sample: 16,
            total_samples: 0,
            md5: [0; 16],
        }));
        params
    }

    fn fake_frame(duration: u64) -> EncodedPacket {
        let mut data = vec![0xFF, 0xF8];
        data.extend_from_slice(&[0_u8; 50]);
        let mut packet = EncodedPacket::new(data);
        packet.duration = Some(duration);
        packet
    }

    #[test]
    fn writes_flac_marker() -> Result<()> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::builder(cursor)
            .padding(0)
            .max_seek_points(0)
            .build();

        muxer.add_track(TrackSpec::new(
            test_codec_params(),
            TimeBase::audio_sample_rate(44_100),
        ))?;

        let bytes = muxer.into_inner().into_inner();
        assert_eq!(&bytes[0..4], b"fLaC");

        Ok(())
    }

    #[test]
    fn writes_seektable_block() -> Result<()> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::builder(cursor)
            .padding(0)
            .max_seek_points(5)
            .build();

        let params = test_codec_params();
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)))?;

        for _ in 0..10 {
            muxer.write_packet(track, fake_frame(4096))?;
        }

        let summary = muxer.finalize()?;
        assert_eq!(summary.total_samples, Some(40960));
        assert_eq!(summary.packet_count, 10);

        let bytes = muxer.into_inner().into_inner();

        let st_offset = 4 + 38;
        assert_eq!(bytes[st_offset] & 0x7F, 3);

        let st_len = u32::from(bytes[st_offset + 1]) << 16
            | u32::from(bytes[st_offset + 2]) << 8
            | u32::from(bytes[st_offset + 3]);
        assert_eq!(st_len, 5 * 18);

        let first_sample =
            u64::from_be_bytes(bytes[st_offset + 4..st_offset + 12].try_into().unwrap());
        assert_eq!(first_sample, 0);

        Ok(())
    }

    #[test]
    fn seektable_has_valid_offsets() -> Result<()> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::builder(cursor)
            .padding(0)
            .max_seek_points(3)
            .build();

        let params = test_codec_params();
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)))?;

        for _ in 0..6 {
            muxer.write_packet(track, fake_frame(4096))?;
        }

        muxer.finalize()?;

        let bytes = muxer.into_inner().into_inner();
        let st_offset = 4 + 38;
        let sp_base = st_offset + 4;

        let sp0_sample = u64::from_be_bytes(bytes[sp_base..sp_base + 8].try_into().unwrap());
        let sp0_offset = u64::from_be_bytes(bytes[sp_base + 8..sp_base + 16].try_into().unwrap());
        assert_eq!(sp0_sample, 0);
        assert_eq!(sp0_offset, 0);

        let sp1_sample = u64::from_be_bytes(bytes[sp_base + 18..sp_base + 26].try_into().unwrap());
        assert!(sp1_sample > 0);

        Ok(())
    }

    #[test]
    fn patches_stream_info_on_finalize() -> Result<()> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::builder(cursor)
            .padding(0)
            .max_seek_points(0)
            .build();

        let params = test_codec_params();
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)))?;

        muxer.write_packet(track, fake_frame(4096))?;

        let summary = muxer.finalize()?;
        assert_eq!(summary.total_samples, Some(4096));

        let bytes = muxer.into_inner().into_inner();
        let si = &bytes[8..42];
        let ts_hi = u64::from(si[13] & 0x0F) << 32;
        let ts_lo = u64::from(u32::from_be_bytes(si[14..18].try_into().unwrap()));
        assert_eq!(ts_hi | ts_lo, 4096);

        Ok(())
    }

    #[test]
    fn no_seektable_when_zero_points() -> Result<()> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::builder(cursor)
            .padding(0)
            .max_seek_points(0)
            .build();

        let params = test_codec_params();
        muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)))?;

        let bytes = muxer.into_inner().into_inner();

        assert_eq!(bytes[42] & 0x7F, 4);

        Ok(())
    }
}
