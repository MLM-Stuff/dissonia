#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

use std::io::{Seek, SeekFrom, Write};

use dissonia_core::codecs::CodecId;
use dissonia_core::formats::{FinalizeSummary, FormatId, Muxer, TrackId, TrackSpec};
use dissonia_core::packet::EncodedPacket;
use dissonia_core::{Error, Result};

const RIFF_SIZE_OFFSET: u64 = 4;
const DATA_SIZE_OFFSET: u64 = 40;
const HEADER_LEN: u64 = 44;
const MAX_PADDED_DATA_LEN: u64 = u32::MAX as u64 - 36;

#[derive(Debug)]
pub struct WavMuxer<W> {
    writer: W,
    track: Option<TrackState>,
    data_bytes: u64,
    packet_count: u64,
    finalized: bool,
}

#[derive(Clone, Copy, Debug)]
struct TrackState {
    id: TrackId,
    block_align: u16,
}

#[derive(Clone, Copy, Debug)]
struct WavCodecInfo {
    format_tag: u16,
    bits_per_sample: u16,
    bytes_per_sample: u16,
}

impl<W> WavMuxer<W> {
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            track: None,
            data_bytes: 0,
            packet_count: 0,
            finalized: false,
        }
    }

    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer
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

        self.writer.seek(SeekFrom::Start(0))?;

        self.writer.write_all(b"RIFF")?;
        write_u32_le(&mut self.writer, 0)?;
        self.writer.write_all(b"WAVE")?;

        self.writer.write_all(b"fmt ")?;
        write_u32_le(&mut self.writer, 16)?;
        write_u16_le(&mut self.writer, codec.format_tag)?;
        write_u16_le(&mut self.writer, channels)?;
        write_u32_le(&mut self.writer, sample_rate)?;
        write_u32_le(&mut self.writer, byte_rate)?;
        write_u16_le(&mut self.writer, block_align)?;
        write_u16_le(&mut self.writer, codec.bits_per_sample)?;

        self.writer.write_all(b"data")?;
        write_u32_le(&mut self.writer, 0)?;

        Ok(TrackState {
            id: TrackId(0),
            block_align,
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

        let padded_len = next_data_bytes + (next_data_bytes & 1);

        if padded_len > MAX_PADDED_DATA_LEN {
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

        if self.track.is_none() {
            return Err(Error::InvalidState("cannot finalize wav without a track"));
        }

        if self.data_bytes & 1 == 1 {
            self.writer.write_all(&[0])?;
        }

        let padded_data_len = self.data_bytes + (self.data_bytes & 1);
        let riff_size = 36_u64
            .checked_add(padded_data_len)
            .ok_or(Error::InvalidState("riff size overflow"))?;

        let riff_size_u32 =
            u32::try_from(riff_size).map_err(|_| Error::Unsupported("riff size exceeds u32"))?;
        let data_size_u32 = u32::try_from(self.data_bytes)
            .map_err(|_| Error::Unsupported("data chunk size exceeds u32"))?;

        self.writer.seek(SeekFrom::Start(RIFF_SIZE_OFFSET))?;
        write_u32_le(&mut self.writer, riff_size_u32)?;

        self.writer.seek(SeekFrom::Start(DATA_SIZE_OFFSET))?;
        write_u32_le(&mut self.writer, data_size_u32)?;

        self.writer
            .seek(SeekFrom::Start(HEADER_LEN + padded_data_len))?;
        self.writer.flush()?;

        self.finalized = true;

        Ok(FinalizeSummary {
            bytes_written: Some(HEADER_LEN + padded_data_len),
            packet_count: self.packet_count,
        })
    }
}

fn wav_codec_info(codec: CodecId) -> Result<WavCodecInfo> {
    match codec {
        CodecId::PcmU8 => Ok(WavCodecInfo {
            format_tag: 0x0001,
            bits_per_sample: 8,
            bytes_per_sample: 1,
        }),
        CodecId::PcmS16Le => Ok(WavCodecInfo {
            format_tag: 0x0001,
            bits_per_sample: 16,
            bytes_per_sample: 2,
        }),
        CodecId::PcmS24Le => Ok(WavCodecInfo {
            format_tag: 0x0001,
            bits_per_sample: 24,
            bytes_per_sample: 3,
        }),
        CodecId::PcmS32Le => Ok(WavCodecInfo {
            format_tag: 0x0001,
            bits_per_sample: 32,
            bytes_per_sample: 4,
        }),
        CodecId::PcmF32Le => Ok(WavCodecInfo {
            format_tag: 0x0003,
            bits_per_sample: 32,
            bytes_per_sample: 4,
        }),
        CodecId::PcmF64Le => Ok(WavCodecInfo {
            format_tag: 0x0003,
            bits_per_sample: 64,
            bytes_per_sample: 8,
        }),
        _ => Err(Error::Unsupported(
            "wav muxer does not support this codec id",
        )),
    }
}

fn write_u16_le<W>(writer: &mut W, value: u16) -> Result<()>
where
    W: Write,
{
    writer.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn write_u32_le<W>(writer: &mut W, value: u32) -> Result<()>
where
    W: Write,
{
    writer.write_all(&value.to_le_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    use dissonia_core::audio::{AudioBufferRef, AudioSpec, ChannelLayout, SampleFormat};
    use dissonia_core::codecs::{Encoder, PacketSink};
    use dissonia_core::formats::MuxerExt;
    use dissonia_core::units::TimeBase;

    struct FakePcmEncoder {
        spec: AudioSpec,
        params: dissonia_core::codecs::CodecParameters,
    }

    impl FakePcmEncoder {
        fn new() -> Self {
            let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
            let params = dissonia_core::codecs::CodecParameters::new(
                dissonia_core::codecs::CodecId::PcmS16Le,
                spec,
            );

            Self { spec, params }
        }
    }

    impl Encoder for FakePcmEncoder {
        fn codec_id(&self) -> dissonia_core::codecs::CodecId {
            self.params.codec
        }

        fn input_spec(&self) -> AudioSpec {
            self.spec
        }

        fn codec_parameters(&self) -> &dissonia_core::codecs::CodecParameters {
            &self.params
        }

        fn encode(&mut self, input: AudioBufferRef<'_>, sink: &mut dyn PacketSink) -> Result<()> {
            let data = match input {
                AudioBufferRef::I16(data) => {
                    let mut out = Vec::with_capacity(data.len() * 2);
                    for &sample in data {
                        out.extend_from_slice(&sample.to_le_bytes());
                    }
                    out
                }
                _ => return Err(Error::InvalidArgument("expected i16 input")),
            };

            sink.write_packet(EncodedPacket::new(data))
        }

        fn flush(&mut self, _sink: &mut dyn PacketSink) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn writes_basic_wav_header() -> Result<()> {
        let mut encoder = FakePcmEncoder::new();
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut muxer = WavMuxer::new(cursor);

        let track = muxer.add_track(TrackSpec::new(
            encoder.codec_parameters().clone(),
            TimeBase::audio_sample_rate(48_000),
        ))?;

        {
            let samples = [0_i16, 0, 1000, -1000];
            let mut sink = muxer.track_writer(track);
            encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;
            encoder.flush(&mut sink)?;
        }

        let summary = muxer.finalize()?;
        assert_eq!(summary.packet_count, 1);
        assert_eq!(summary.bytes_written, Some(52));

        let bytes = muxer.into_inner().into_inner();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);

        Ok(())
    }
}
