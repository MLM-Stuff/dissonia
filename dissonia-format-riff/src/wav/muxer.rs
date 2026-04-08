use std::io::{Seek, Write};

use dissonia_common::riff::{ChunkHandle, RiffWriter};
use dissonia_core::formats::{FinalizeSummary, FormatId, Muxer, TrackId, TrackSpec};
use dissonia_core::packet::EncodedPacket;
use dissonia_core::{Error, Result};

use super::header::{wav_codec_info, write_classic_fmt_payload, write_extensible_fmt_payload};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WavMuxerOptions {
    pub force_extensible: bool,
}

#[derive(Debug)]
pub struct WavMuxerBuilder<W> {
    writer: W,
    options: WavMuxerOptions,
}

impl<W> WavMuxerBuilder<W> {
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            options: WavMuxerOptions::default(),
        }
    }

    #[must_use]
    pub fn force_extensible(mut self, force: bool) -> Self {
        self.options.force_extensible = force;
        self
    }

    #[must_use]
    pub fn options(mut self, options: WavMuxerOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn build(self) -> WavMuxer<W> {
        WavMuxer {
            writer: RiffWriter::new(self.writer),
            options: self.options,
            track: None,
            data_bytes: 0,
            packet_count: 0,
            finalized: false,
        }
    }
}

#[derive(Debug)]
pub struct WavMuxer<W> {
    writer: RiffWriter<W>,
    options: WavMuxerOptions,
    track: Option<TrackState>,
    data_bytes: u64,
    packet_count: u64,
    finalized: bool,
}

#[derive(Clone, Copy, Debug)]
struct TrackState {
    id: TrackId,
    block_align: u16,
    riff_chunk: ChunkHandle,
    data_chunk: ChunkHandle,
    max_padded_data_len: u64,
}

impl<W> WavMuxer<W> {
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self::builder(writer).build()
    }

    #[must_use]
    pub fn builder(writer: W) -> WavMuxerBuilder<W> {
        WavMuxerBuilder::new(writer)
    }

    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer.into_inner()
    }
}

impl<W> WavMuxer<W>
where
    W: Write + Seek,
{
    fn ensure_writable(&self) -> Result<()> {
        if self.finalized {
            return Err(Error::InvalidState("muxer already finalized"));
        }

        Ok(())
    }

    fn write_header(&mut self, spec: &TrackSpec) -> Result<TrackState> {
        let codec = wav_codec_info(spec.codec_params.codec)?;

        let channels = u16::try_from(spec.codec_params.channels.count())
            .map_err(|_| Error::Unsupported("wav channel count exceeds u16"))?;

        if channels == 0 {
            return Err(Error::InvalidArgument(
                "wav track must have at least one channel",
            ));
        }

        let sample_rate = spec.codec_params.sample_rate;
        if sample_rate == 0 {
            return Err(Error::InvalidArgument("wav sample rate must be non-zero"));
        }

        let block_align_u32 = u32::from(channels) * u32::from(codec.bytes_per_sample);
        let block_align = u16::try_from(block_align_u32)
            .map_err(|_| Error::Unsupported("wav block align exceeds u16"))?;

        let byte_rate = sample_rate
            .checked_mul(block_align_u32)
            .ok_or(Error::Unsupported("wav byte rate exceeds u32"))?;

        let riff_chunk = self.writer.start_riff(*b"WAVE")?;

        let fmt_chunk = self.writer.start_chunk(*b"fmt ")?;
        let use_extensible = self.options.force_extensible || channels > 2;

        if use_extensible {
            write_extensible_fmt_payload(
                &mut self.writer,
                codec,
                channels,
                sample_rate,
                byte_rate,
                block_align,
                spec.codec_params.channels.bits(),
            )?;
        } else {
            write_classic_fmt_payload(
                &mut self.writer,
                codec,
                channels,
                sample_rate,
                byte_rate,
                block_align,
            )?;
        }

        self.writer.finish_chunk(fmt_chunk)?;

        let data_chunk = self.writer.start_chunk(*b"data")?;

        let base_riff_size = data_chunk
            .size_data_start()
            .checked_sub(8)
            .ok_or(Error::InvalidState("invalid data chunk position"))?;

        let max_padded_data_len = (u32::MAX as u64)
            .checked_sub(base_riff_size)
            .ok_or(Error::Unsupported("wav header exceeds riff size limits"))?;

        Ok(TrackState {
            id: TrackId(0),
            block_align,
            riff_chunk,
            data_chunk,
            max_padded_data_len,
        })
    }
}

impl<W> Muxer for WavMuxer<W>
where
    W: Write + Seek + Send,
{
    fn format_id(&self) -> FormatId {
        FormatId::Riff
    }

    fn add_track(&mut self, spec: TrackSpec) -> Result<TrackId> {
        self.ensure_writable()?;

        if self.track.is_some() {
            return Err(Error::Unsupported("wav supports only one audio track"));
        }

        let state = self.write_header(&spec)?;
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
                "packet written to unknown wav track",
            ));
        }

        if packet.data.is_empty() {
            return Ok(());
        }

        if packet.data.len() % usize::from(state.block_align) != 0 {
            return Err(Error::InvalidArgument(
                "packet payload is not aligned to complete pcm frames",
            ));
        }

        let packet_len = u64::try_from(packet.data.len())
            .map_err(|_| Error::Unsupported("packet size exceeds u64"))?;

        let next_data_bytes = self
            .data_bytes
            .checked_add(packet_len)
            .ok_or(Error::InvalidState("wav data size overflow"))?;

        let padded_len = next_data_bytes
            .checked_add(next_data_bytes & 1)
            .ok_or(Error::InvalidState("wav padded data size overflow"))?;

        if padded_len > state.max_padded_data_len {
            return Err(Error::Unsupported("wav file exceeds riff size limits"));
        }

        self.writer.write_all(&packet.data)?;
        self.data_bytes = next_data_bytes;
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

        let state = self
            .track
            .ok_or(Error::InvalidState("cannot finalize wav without a track"))?;

        self.writer.finish_chunk(state.data_chunk)?;
        self.writer.finish_chunk(state.riff_chunk)?;

        let bytes_written = self.writer.position()?;
        self.writer.flush()?;
        self.finalized = true;

        Ok(FinalizeSummary {
            bytes_written: Some(bytes_written),
            packet_count: self.packet_count,
            total_samples: None,
        })
    }
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
    fn writes_classic_wave_header_for_stereo_pcm() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let params = CodecParameters::new(CodecId::PcmS16Le, spec);

        let mut muxer = WavMuxer::new(Cursor::new(Vec::<u8>::new()));
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(48_000)))?;

        muxer.write_packet(track, EncodedPacket::new(vec![0_u8; 8]))?;
        let summary = muxer.finalize()?;

        assert_eq!(summary.packet_count, 1);
        assert_eq!(summary.bytes_written, Some(52));

        let bytes = muxer.into_inner().into_inner();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(bytes[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);

        Ok(())
    }

    #[test]
    fn writes_extensible_wave_header_for_surround_pcm() -> Result<()> {
        let channels = ChannelLayout::FRONT_LEFT
            | ChannelLayout::FRONT_RIGHT
            | ChannelLayout::FRONT_CENTER
            | ChannelLayout::LOW_FREQUENCY
            | ChannelLayout::BACK_LEFT
            | ChannelLayout::BACK_RIGHT;

        let spec = AudioSpec::new(48_000, channels, SampleFormat::I16);
        let params = CodecParameters::new(CodecId::PcmS16Le, spec);

        let mut muxer = WavMuxer::new(Cursor::new(Vec::<u8>::new()));
        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(48_000)))?;

        muxer.write_packet(track, EncodedPacket::new(vec![0_u8; 12]))?;
        let summary = muxer.finalize()?;

        assert_eq!(summary.packet_count, 1);
        assert_eq!(summary.bytes_written, Some(80));

        let bytes = muxer.into_inner().into_inner();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 40);
        assert_eq!(
            u16::from_le_bytes(bytes[20..22].try_into().unwrap()),
            0xfffe
        );
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 6);
        assert_eq!(u16::from_le_bytes(bytes[32..34].try_into().unwrap()), 12);
        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 0x3f);
        assert_eq!(
            &bytes[44..60],
            &[
                0x01, 0x00, 0x00, 0x00, //
                0x00, 0x00, //
                0x10, 0x00, //
                0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
            ]
        );
        assert_eq!(&bytes[60..64], b"data");
        assert_eq!(u32::from_le_bytes(bytes[64..68].try_into().unwrap()), 12);

        Ok(())
    }

    #[test]
    fn can_force_extensible_for_stereo() -> Result<()> {
        let spec = AudioSpec::new(44_100, ChannelLayout::STEREO, SampleFormat::F32);
        let params = CodecParameters::new(CodecId::PcmF32Le, spec);

        let mut muxer = WavMuxer::builder(Cursor::new(Vec::<u8>::new()))
            .force_extensible(true)
            .build();

        let track = muxer.add_track(TrackSpec::new(params, TimeBase::audio_sample_rate(44_100)))?;

        muxer.write_packet(track, EncodedPacket::new(vec![0_u8; 16]))?;
        muxer.finalize()?;

        let bytes = muxer.into_inner().into_inner();
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 40);
        assert_eq!(
            u16::from_le_bytes(bytes[20..22].try_into().unwrap()),
            0xfffe
        );
        assert_eq!(
            &bytes[44..60],
            &[
                0x03, 0x00, 0x00, 0x00, //
                0x00, 0x00, //
                0x10, 0x00, //
                0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
            ]
        );

        Ok(())
    }
}
