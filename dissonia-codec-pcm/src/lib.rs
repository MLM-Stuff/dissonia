#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

use dissonia_core::audio::{AudioBufferRef, AudioSpec, SampleFormat};
use dissonia_core::codecs::{CodecId, CodecParameters, Encoder, PacketSink};
use dissonia_core::packet::{EncodedPacket, PacketFlags};
use dissonia_core::units::Timestamp;
use dissonia_core::{Error, Result};

#[derive(Debug)]
pub struct PcmEncoder {
    spec: AudioSpec,
    params: CodecParameters,
    next_pts: u64,
}

impl PcmEncoder {
    pub fn new(spec: AudioSpec) -> Result<Self> {
        if spec.channel_count() == 0 {
            return Err(Error::InvalidArgument(
                "audio spec must contain at least one channel",
            ));
        }

        let codec = codec_id_for(spec.sample_format);
        let params = CodecParameters::new(codec, spec);

        Ok(Self {
            spec,
            params,
            next_pts: 0,
        })
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

    fn flush(&mut self, _sink: &mut dyn PacketSink) -> Result<()> {
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.next_pts = 0;
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
    fn encodes_i16_packets() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let mut encoder = PcmEncoder::new(spec)?;
        let mut sink = VecPacketSink::new();

        let samples = [1_i16, -1, 2, -2];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].duration, Some(2));
        assert_eq!(packets[0].data.as_ref(), &[1, 0, 255, 255, 2, 0, 254, 255]);

        Ok(())
    }
}
