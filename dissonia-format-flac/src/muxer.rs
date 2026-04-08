use std::io::{Seek, SeekFrom, Write};

use dissonia_common::vorbis::VorbisComments;
use dissonia_core::codecs::{CodecId, CodecSpecific};
use dissonia_core::formats::{FinalizeSummary, FormatId, Muxer, TrackId, TrackSpec};
use dissonia_core::packet::EncodedPacket;
use dissonia_core::{Error, Result};

use crate::metadata;

const FLAC_MARKER: &[u8; 4] = b"fLaC";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlacMuxerOptions {
    pub comments: VorbisComments,
    pub padding: u32,
}

impl Default for FlacMuxerOptions {
    fn default() -> Self {
        Self {
            comments: VorbisComments::new("dissonia"),
            padding: 8192,
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
    pub fn build(self) -> FlacMuxer<W> {
        FlacMuxer {
            writer: self.writer,
            options: self.options,
            track: None,
            stream_info_offset: 0,
            min_frame_size: u32::MAX,
            max_frame_size: 0,
            total_samples: 0,
            bytes_written: 0,
            packet_count: 0,
            finalized: false,
        }
    }
}

#[derive(Debug)]
pub struct FlacMuxer<W> {
    writer: W,
    options: FlacMuxerOptions,
    track: Option<TrackState>,
    stream_info_offset: u64,
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

        let has_comments = true;
        let has_padding = self.options.padding > 0;

        let stream_info_block = metadata::build_stream_info_block(
            !has_comments && !has_padding,
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

        let comment_is_last = !has_padding;
        let comment_block =
            metadata::build_vorbis_comment_block(comment_is_last, &self.options.comments)?;
        self.write_all(&comment_block)?;

        if has_padding {
            let padding_block = metadata::build_padding_block(true, self.options.padding);
            self.write_all(&padding_block)?;
        }

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

        self.writer.seek(SeekFrom::End(0))?;

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

    #[test]
    fn writes_flac_marker() -> Result<()> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::builder(cursor).padding(0).build();

        let params = test_codec_params();
        muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)))?;

        let bytes = muxer.into_inner().into_inner();
        assert_eq!(&bytes[0..4], b"fLaC");

        Ok(())
    }

    #[test]
    fn writes_stream_info_block() -> Result<()> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::builder(cursor).padding(0).build();

        let params = test_codec_params();
        muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)))?;

        let bytes = muxer.into_inner().into_inner();

        assert_eq!(bytes[4] & 0x7F, 0);
        let si_len = u32::from(bytes[5]) << 16 | u32::from(bytes[6]) << 8 | u32::from(bytes[7]);
        assert_eq!(si_len, 34);

        Ok(())
    }

    #[test]
    fn patches_stream_info_on_finalize() -> Result<()> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::builder(cursor).padding(0).build();

        let params = test_codec_params();
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)))?;

        let mut fake_frame = vec![0xFF, 0xF8];
        fake_frame.extend_from_slice(&[0_u8; 50]);
        let mut packet = EncodedPacket::new(fake_frame);
        packet.duration = Some(4096);

        muxer.write_packet(track, packet)?;

        let summary = muxer.finalize()?;
        assert_eq!(summary.total_samples, Some(4096));
        assert_eq!(summary.packet_count, 1);

        let bytes = muxer.into_inner().into_inner();

        let si_payload = &bytes[8..42];
        let ts_hi = u64::from(si_payload[13] & 0x0F) << 32;
        let ts_lo = u64::from(u32::from_be_bytes(si_payload[14..18].try_into().unwrap()));
        assert_eq!(ts_hi | ts_lo, 4096);

        Ok(())
    }

    #[test]
    fn rejects_non_flac_codec() {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = FlacMuxer::new(cursor);

        let spec = AudioSpec::new(44_100, ChannelLayout::STEREO, SampleFormat::I16);
        let params = CodecParameters::new(CodecId::PcmS16Le, spec);

        let result = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)));
        assert!(result.is_err());
    }
}
