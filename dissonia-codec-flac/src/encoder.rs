use dissonia_core::audio::{AudioBufferRef, AudioSpec, SampleFormat};
use dissonia_core::codecs::{
    CodecId, CodecParameters, CodecSpecific, Encoder, FlacStreamInfo, PacketSink,
};
use dissonia_core::packet::{EncodedPacket, PacketFlags};
use dissonia_core::units::Timestamp;
use dissonia_core::{Error, Result};

use crate::frame;
use crate::options::FlacEncoderOptions;

#[derive(Debug)]
pub struct FlacEncoderBuilder {
    spec: AudioSpec,
    options: FlacEncoderOptions,
}

impl FlacEncoderBuilder {
    #[must_use]
    pub fn new(spec: AudioSpec) -> Self {
        Self {
            spec,
            options: FlacEncoderOptions::default(),
        }
    }

    #[must_use]
    pub fn options(mut self, options: FlacEncoderOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn block_size(mut self, block_size: u16) -> Self {
        self.options.block_size = block_size;
        self
    }

    #[must_use]
    pub fn stereo_decorrelation(mut self, enabled: bool) -> Self {
        self.options.stereo_decorrelation = enabled;
        self
    }

    pub fn build(self) -> Result<FlacEncoder> {
        FlacEncoder::with_options(self.spec, self.options)
    }
}

#[derive(Debug)]
pub struct FlacEncoder {
    spec: AudioSpec,
    params: CodecParameters,
    options: FlacEncoderOptions,
    channels: usize,
    bits_per_sample: u8,
    pending: Vec<i64>,
    next_pts: u64,
    frame_number: u32,
}

impl FlacEncoder {
    pub fn new(spec: AudioSpec) -> Result<Self> {
        Self::builder(spec).build()
    }

    #[must_use]
    pub fn builder(spec: AudioSpec) -> FlacEncoderBuilder {
        FlacEncoderBuilder::new(spec)
    }

    pub fn with_options(spec: AudioSpec, options: FlacEncoderOptions) -> Result<Self> {
        validate_spec(spec)?;
        validate_options(options)?;

        let bits_per_sample = match spec.sample_format {
            SampleFormat::I16 => 16,
            SampleFormat::I24 => 24,
            _ => {
                return Err(Error::Unsupported(
                    "flac encoder supports only i16 or i24 input",
                ));
            }
        };

        let channels = spec.channels.count() as usize;
        let block_size = options.block_size;

        let stream_info = FlacStreamInfo {
            min_block_size: block_size,
            max_block_size: block_size,
            min_frame_size: 0,
            max_frame_size: 0,
            bits_per_sample,
            total_samples: 0,
            md5: [0; 16],
        };

        let mut params = CodecParameters::new(CodecId::Flac, spec);
        params.sample_format = Some(spec.sample_format);
        params.bit_depth = Some(u32::from(bits_per_sample));
        params.frame_samples = Some(u32::from(block_size));
        params.codec_specific = Some(CodecSpecific::Flac(stream_info));

        Ok(Self {
            spec,
            params,
            options,
            channels,
            bits_per_sample,
            pending: Vec::new(),
            next_pts: 0,
            frame_number: 0,
        })
    }

    #[must_use]
    pub const fn options(&self) -> FlacEncoderOptions {
        self.options
    }

    fn samples_to_i64(input: AudioBufferRef<'_>) -> Result<Vec<i64>> {
        match input {
            AudioBufferRef::I16(data) => Ok(data.iter().map(|&s| i64::from(s)).collect()),
            AudioBufferRef::I24(data) => {
                for &s in data {
                    if !(-8_388_608..=8_388_607).contains(&s) {
                        return Err(Error::InvalidArgument("i24 sample out of range"));
                    }
                }
                Ok(data.iter().map(|&s| i64::from(s)).collect())
            }
            _ => Err(Error::Unsupported(
                "flac encoder supports only i16 or i24 input",
            )),
        }
    }

    fn drain_blocks(&mut self, sink: &mut dyn PacketSink) -> Result<()> {
        let block_samples = usize::from(self.options.block_size) * self.channels;

        while self.pending.len() >= block_samples {
            let block: Vec<i64> = self.pending.drain(..block_samples).collect();
            self.encode_block(&block, self.options.block_size, sink)?;
        }

        Ok(())
    }

    fn encode_block(
        &mut self,
        interleaved: &[i64],
        block_size: u16,
        sink: &mut dyn PacketSink,
    ) -> Result<()> {
        let bs = usize::from(block_size);

        let mut channel_bufs: Vec<Vec<i64>> =
            (0..self.channels).map(|_| Vec::with_capacity(bs)).collect();
        for (i, &sample) in interleaved.iter().enumerate() {
            channel_bufs[i % self.channels].push(sample);
        }

        let channel_refs: Vec<&[i64]> = channel_bufs.iter().map(|v| v.as_slice()).collect();

        let try_stereo = self.options.stereo_decorrelation && self.channels == 2;

        let frame_bytes = frame::encode_frame(
            &channel_refs,
            self.frame_number,
            self.spec.sample_rate,
            self.bits_per_sample,
            block_size,
            self.options.max_fixed_order,
            self.options.max_rice_partition_order,
            try_stereo,
        );

        let pts = Timestamp::new(self.next_pts);
        self.next_pts = self
            .next_pts
            .checked_add(u64::from(block_size))
            .ok_or(Error::InvalidState("timestamp overflow"))?;

        self.frame_number = self
            .frame_number
            .checked_add(1)
            .ok_or(Error::InvalidState("frame number overflow"))?;

        let mut packet = EncodedPacket::new(frame_bytes);
        packet.pts = Some(pts);
        packet.dts = Some(pts);
        packet.duration = Some(u64::from(block_size));
        packet.flags = PacketFlags::KEYFRAME;

        sink.write_packet(packet)
    }
}

impl Encoder for FlacEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Flac
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

        if input.is_empty() {
            return Ok(());
        }

        let sample_count = input.len();
        if sample_count % self.channels != 0 {
            return Err(Error::InvalidArgument(
                "input buffer sample count is not divisible by channel count",
            ));
        }

        let samples = Self::samples_to_i64(input)?;
        self.pending.extend_from_slice(&samples);
        self.drain_blocks(sink)
    }

    fn flush(&mut self, sink: &mut dyn PacketSink) -> Result<()> {
        self.drain_blocks(sink)?;

        if self.pending.is_empty() {
            return Ok(());
        }

        if self.pending.len() % self.channels != 0 {
            return Err(Error::InvalidState(
                "pending flac samples are not aligned to complete frames",
            ));
        }

        let remaining_frames = u16::try_from(self.pending.len() / self.channels)
            .map_err(|_| Error::Unsupported("remaining flac frame count exceeds block size u16"))?;

        if remaining_frames == 0 {
            return Ok(());
        }

        let block = std::mem::take(&mut self.pending);
        self.encode_block(&block, remaining_frames, sink)
    }

    fn reset(&mut self) -> Result<()> {
        self.pending.clear();
        self.next_pts = 0;
        self.frame_number = 0;
        Ok(())
    }
}

fn validate_spec(spec: AudioSpec) -> Result<()> {
    if spec.sample_rate == 0 || spec.sample_rate > 655_350 {
        return Err(Error::Unsupported(
            "flac encoder sample rate must be 1–655350 Hz",
        ));
    }

    let ch = spec.channels.count();
    if ch == 0 || ch > 8 {
        return Err(Error::Unsupported("flac encoder supports 1–8 channels"));
    }

    match spec.sample_format {
        SampleFormat::I16 | SampleFormat::I24 => Ok(()),
        _ => Err(Error::Unsupported(
            "flac encoder supports only i16 or i24 input",
        )),
    }
}

fn validate_options(options: FlacEncoderOptions) -> Result<()> {
    if options.block_size < 16 {
        return Err(Error::InvalidArgument(
            "flac block_size must be at least 16",
        ));
    }

    if options.max_fixed_order > 4 {
        return Err(Error::InvalidArgument("flac max_fixed_order must be 0–4"));
    }

    if options.max_rice_partition_order > 15 {
        return Err(Error::InvalidArgument(
            "flac max_rice_partition_order must be 0–15",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dissonia_core::audio::{ChannelLayout, SampleFormat};
    use dissonia_core::codecs::VecPacketSink;

    #[test]
    fn encodes_one_block_of_silence() -> Result<()> {
        let spec = AudioSpec::new(44_100, ChannelLayout::STEREO, SampleFormat::I16);
        let mut encoder = FlacEncoder::builder(spec).block_size(256).build()?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0_i16; 256 * 2];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].pts, Some(Timestamp::new(0)));
        assert_eq!(packets[0].duration, Some(256));
        assert!(packets[0].flags.contains(PacketFlags::KEYFRAME));
        assert_eq!(packets[0].data[0], 0xFF);
        assert_eq!(packets[0].data[1] & 0xFC, 0xF8);

        Ok(())
    }

    #[test]
    fn packetizes_multiple_blocks() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::MONO, SampleFormat::I16);
        let mut encoder = FlacEncoder::builder(spec).block_size(128).build()?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0_i16; 384];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 3);
        assert_eq!(packets[0].pts, Some(Timestamp::new(0)));
        assert_eq!(packets[1].pts, Some(Timestamp::new(128)));
        assert_eq!(packets[2].pts, Some(Timestamp::new(256)));

        Ok(())
    }

    #[test]
    fn flush_emits_partial_block() -> Result<()> {
        let spec = AudioSpec::new(44_100, ChannelLayout::MONO, SampleFormat::I16);
        let mut encoder = FlacEncoder::builder(spec).block_size(256).build()?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0_i16; 100];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].duration, Some(100));

        Ok(())
    }

    #[test]
    fn fills_flac_stream_info() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let encoder = FlacEncoder::builder(spec).block_size(4096).build()?;

        let info = encoder.codec_parameters().flac_stream_info().unwrap();
        assert_eq!(info.min_block_size, 4096);
        assert_eq!(info.max_block_size, 4096);
        assert_eq!(info.bits_per_sample, 16);

        Ok(())
    }

    #[test]
    fn rejects_f32_input() {
        let spec = AudioSpec::new(44_100, ChannelLayout::MONO, SampleFormat::F32);
        let result = FlacEncoder::new(spec);
        assert!(result.is_err());
    }
}
