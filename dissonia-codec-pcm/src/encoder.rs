use dissonia_core::audio::{AudioBufferRef, AudioSpec, SampleFormat};
use dissonia_core::codecs::{CodecId, CodecParameters, Encoder, PacketSink};
use dissonia_core::packet::{EncodedPacket, PacketFlags};
use dissonia_core::units::Timestamp;
use dissonia_core::{Error, Result};

use crate::PcmEncoderOptions;

#[derive(Debug)]
pub struct PcmEncoderBuilder {
    spec: AudioSpec,
    options: PcmEncoderOptions,
}

impl PcmEncoderBuilder {
    #[must_use]
    pub fn new(spec: AudioSpec) -> Self {
        Self {
            spec,
            options: PcmEncoderOptions::default(),
        }
    }

    #[must_use]
    pub fn options(mut self, options: PcmEncoderOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn frames_per_packet(mut self, frames: u32) -> Self {
        self.options.frames_per_packet = Some(frames);
        self
    }

    pub fn build(self) -> Result<PcmEncoder> {
        PcmEncoder::with_options(self.spec, self.options)
    }
}

#[derive(Debug)]
pub struct PcmEncoder {
    spec: AudioSpec,
    params: CodecParameters,
    options: PcmEncoderOptions,
    frame_bytes: usize,
    packet_bytes: Option<usize>,
    next_pts: u64,
    pending: Vec<u8>,
    pending_frames: u64,
}

impl PcmEncoder {
    pub fn new(spec: AudioSpec) -> Result<Self> {
        Self::builder(spec).build()
    }

    #[must_use]
    pub fn builder(spec: AudioSpec) -> PcmEncoderBuilder {
        PcmEncoderBuilder::new(spec)
    }

    pub fn with_options(spec: AudioSpec, options: PcmEncoderOptions) -> Result<Self> {
        let channel_count = spec.channel_count();
        if channel_count == 0 {
            return Err(Error::InvalidArgument(
                "audio spec must contain at least one channel",
            ));
        }

        let channel_count_usize = usize::try_from(channel_count)
            .map_err(|_| Error::Unsupported("channel count exceeds platform limits"))?;

        let sample_bytes = bytes_per_sample(spec.sample_format);
        let frame_bytes = channel_count_usize
            .checked_mul(sample_bytes)
            .ok_or(Error::Unsupported("pcm frame size exceeds platform limits"))?;

        let packet_bytes = match options.frames_per_packet {
            Some(0) => {
                return Err(Error::InvalidArgument(
                    "frames_per_packet must be greater than zero",
                ));
            }
            Some(frames) => {
                let frames_usize = usize::try_from(frames).map_err(|_| {
                    Error::Unsupported("packet frame count exceeds platform limits")
                })?;
                Some(
                    frames_usize
                        .checked_mul(frame_bytes)
                        .ok_or(Error::Unsupported("packet size exceeds platform limits"))?,
                )
            }
            None => None,
        };

        let codec = codec_id_for(spec.sample_format);
        let params = CodecParameters::new(codec, spec);

        Ok(Self {
            spec,
            params,
            options,
            frame_bytes,
            packet_bytes,
            next_pts: 0,
            pending: Vec::new(),
            pending_frames: 0,
        })
    }

    #[must_use]
    pub const fn options(&self) -> PcmEncoderOptions {
        self.options
    }

    fn encode_internal(
        &mut self,
        input: AudioBufferRef<'_>,
        sink: &mut dyn PacketSink,
    ) -> Result<()> {
        if input.sample_format() != self.spec.sample_format {
            return Err(Error::InvalidArgument(
                "buffer sample format does not match encoder input spec",
            ));
        }

        let channels = usize::try_from(self.spec.channel_count())
            .map_err(|_| Error::Unsupported("channel count exceeds platform limits"))?;

        if channels == 0 {
            return Err(Error::InvalidState("encoder has zero channels"));
        }

        let frame_count = frame_count(input.len(), channels)?;

        if frame_count == 0 {
            return Ok(());
        }

        let payload = encode_payload(input)?;

        match self.packet_bytes {
            None => self.emit_packet(payload, frame_count, sink),
            Some(packet_bytes) => {
                self.pending.extend_from_slice(&payload);
                self.pending_frames = self
                    .pending_frames
                    .checked_add(frame_count)
                    .ok_or(Error::InvalidState("pending frame count overflow"))?;

                while self.pending.len() >= packet_bytes {
                    let packet = self.pending.drain(..packet_bytes).collect::<Vec<_>>();
                    let packet_frames = u64::try_from(packet_bytes / self.frame_bytes)
                        .map_err(|_| Error::Unsupported("packet frame count exceeds u64"))?;
                    self.pending_frames = self
                        .pending_frames
                        .checked_sub(packet_frames)
                        .ok_or(Error::InvalidState("pending frame count underflow"))?;

                    self.emit_packet(packet, packet_frames, sink)?;
                }

                Ok(())
            }
        }
    }

    fn emit_packet(
        &mut self,
        payload: Vec<u8>,
        frame_count: u64,
        sink: &mut dyn PacketSink,
    ) -> Result<()> {
        let pts = Timestamp::new(self.next_pts);
        self.next_pts = self
            .next_pts
            .checked_add(frame_count)
            .ok_or(Error::InvalidState("timestamp overflow"))?;

        let mut packet = EncodedPacket::new(payload);
        packet.pts = Some(pts);
        packet.dts = Some(pts);
        packet.duration = Some(frame_count);
        packet.flags = PacketFlags::NONE;

        sink.write_packet(packet)
    }
}

impl Encoder for PcmEncoder {
    fn codec_id(&self) -> CodecId {
        self.params.codec
    }

    fn input_spec(&self) -> AudioSpec {
        self.spec
    }

    fn codec_parameters(&self) -> &CodecParameters {
        &self.params
    }

    fn encode(&mut self, input: AudioBufferRef<'_>, sink: &mut dyn PacketSink) -> Result<()> {
        self.encode_internal(input, sink)
    }

    fn flush(&mut self, sink: &mut dyn PacketSink) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }

        if self.pending.len() % self.frame_bytes != 0 {
            return Err(Error::InvalidState(
                "pending pcm payload is not aligned to complete frames",
            ));
        }

        let payload = std::mem::take(&mut self.pending);
        let frames = self.pending_frames;
        self.pending_frames = 0;

        self.emit_packet(payload, frames, sink)
    }

    fn reset(&mut self) -> Result<()> {
        self.next_pts = 0;
        self.pending.clear();
        self.pending_frames = 0;
        Ok(())
    }
}

fn codec_id_for(sample_format: SampleFormat) -> CodecId {
    match sample_format {
        SampleFormat::U8 => CodecId::PcmU8,
        SampleFormat::I16 => CodecId::PcmS16Le,
        SampleFormat::I24 => CodecId::PcmS24Le,
        SampleFormat::I32 => CodecId::PcmS32Le,
        SampleFormat::F32 => CodecId::PcmF32Le,
        SampleFormat::F64 => CodecId::PcmF64Le,
        _ => unreachable!("SampleFormat is non-exhaustive but all variants handled"),
    }
}

fn bytes_per_sample(sample_format: SampleFormat) -> usize {
    match sample_format {
        SampleFormat::U8 => 1,
        SampleFormat::I16 => 2,
        SampleFormat::I24 => 3,
        SampleFormat::I32 => 4,
        SampleFormat::F32 => 4,
        SampleFormat::F64 => 8,
        _ => todo!("SampleFormat is non-exhaustive but all variants handled"),
    }
}

fn frame_count(sample_count: usize, channels: usize) -> Result<u64> {
    if sample_count % channels != 0 {
        return Err(Error::InvalidArgument(
            "input buffer sample count is not divisible by channel count",
        ));
    }

    u64::try_from(sample_count / channels)
        .map_err(|_| Error::Unsupported("frame count exceeds u64"))
}

fn encode_payload(input: AudioBufferRef<'_>) -> Result<Vec<u8>> {
    match input {
        AudioBufferRef::U8(data) => Ok(data.to_vec()),
        AudioBufferRef::I16(data) => {
            let mut out = Vec::with_capacity(data.len() * 2);
            for &sample in data {
                out.extend_from_slice(&sample.to_le_bytes());
            }
            Ok(out)
        }
        AudioBufferRef::I24(data) => {
            let mut out = Vec::with_capacity(data.len() * 3);
            for &sample in data {
                if !(-8_388_608..=8_388_607).contains(&sample) {
                    return Err(Error::InvalidArgument("i24 sample out of range"));
                }

                let bytes = sample.to_le_bytes();
                out.extend_from_slice(&bytes[..3]);
            }
            Ok(out)
        }
        AudioBufferRef::I32(data) => {
            let mut out = Vec::with_capacity(data.len() * 4);
            for &sample in data {
                out.extend_from_slice(&sample.to_le_bytes());
            }
            Ok(out)
        }
        AudioBufferRef::F32(data) => {
            let mut out = Vec::with_capacity(data.len() * 4);
            for &sample in data {
                out.extend_from_slice(&sample.to_le_bytes());
            }
            Ok(out)
        }
        AudioBufferRef::F64(data) => {
            let mut out = Vec::with_capacity(data.len() * 8);
            for &sample in data {
                out.extend_from_slice(&sample.to_le_bytes());
            }
            Ok(out)
        }
        _ => unreachable!("AudioBufferRef is non-exhaustive but all variants handled"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dissonia_core::audio::{ChannelLayout, SampleFormat};
    use dissonia_core::codecs::VecPacketSink;

    #[test]
    fn encodes_i16_packets_without_packetization() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let mut encoder = PcmEncoder::new(spec)?;
        let mut sink = VecPacketSink::new();

        let samples = [1_i16, -1, 2, -2];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].pts, Some(Timestamp::new(0)));
        assert_eq!(packets[0].duration, Some(2));
        assert_eq!(packets[0].data.as_ref(), &[1, 0, 255, 255, 2, 0, 254, 255]);

        Ok(())
    }

    #[test]
    fn packetizes_into_fixed_frame_chunks() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let mut encoder = PcmEncoder::builder(spec).frames_per_packet(2).build()?;
        let mut sink = VecPacketSink::new();

        let samples = [1_i16, -1, 2, -2, 3, -3, 4, -4];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].duration, Some(2));
        assert_eq!(packets[1].duration, Some(2));

        Ok(())
    }

    #[test]
    fn flush_emit_residual_frames() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let mut encoder = PcmEncoder::builder(spec).frames_per_packet(4).build()?;
        let mut sink = VecPacketSink::new();

        let samples = [1_i16, -1, 2, -2, 3, -3];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].duration, Some(3));

        Ok(())
    }
}
