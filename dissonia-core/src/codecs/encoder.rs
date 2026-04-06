use crate::audio::{AudioBufferRef, AudioSpec};
use crate::codecs::{CodecId, CodecParameters};
use crate::packet::EncodedPacket;
use crate::Result;

pub trait PacketSink {
    fn write_packet(&mut self, packet: EncodedPacket) -> Result<()>;
}

pub trait Encoder: Send {
    fn codec_id(&self) -> CodecId;

    fn input_spec(&self) -> AudioSpec;

    fn codec_parameters(&self) -> &CodecParameters;

    fn encode(&mut self, input: AudioBufferRef<'_>, sink: &mut dyn PacketSink) -> Result<()>;

    fn flush(&mut self, sink: &mut dyn PacketSink) -> Result<()>;

    fn reset(&mut self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct VecPacketSink {
    packets: Vec<EncodedPacket>,
}

impl VecPacketSink {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[EncodedPacket] {
        &self.packets
    }

    #[must_use]
    pub fn into_inner(self) -> Vec<EncodedPacket> {
        self.packets
    }
}

impl PacketSink for VecPacketSink {
    fn write_packet(&mut self, packet: EncodedPacket) -> Result<()> {
        self.packets.push(packet);
        Ok(())
    }
}
