use dissonia_core::audio::{AudioBufferRef, AudioSpec, SampleFormat};
use dissonia_core::codecs::{
    CodecId, CodecParameters, CodecSpecific, Encoder, Mp3StreamInfo, PacketSink,
};
use dissonia_core::packet::{EncodedPacket, PacketFlags};
use dissonia_core::units::Timestamp;
use dissonia_core::{Error, Result};

use meporus::Mp3Encoder as MeporusEncoder;

use crate::options::Mp3EncoderOptions;

#[derive(Debug)]
pub struct Mp3EncoderBuilder {
    spec: AudioSpec,
    options: Mp3EncoderOptions,
}

impl Mp3EncoderBuilder {
    #[must_use]
    pub fn new(spec: AudioSpec) -> Self {
        Self {
            spec,
            options: Mp3EncoderOptions::default(),
        }
    }

    #[must_use]
    pub fn options(mut self, options: Mp3EncoderOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn bitrate_bps(mut self, bitrate_bps: u32) -> Self {
        self.options.bitrate_bps = bitrate_bps;
        self
    }

    pub fn build(self) -> Result<Mp3Encoder> {
        Mp3Encoder::with_options(self.spec, self.options)
    }
}

pub struct Mp3Encoder {
    inner: MeporusEncoder,
    spec: AudioSpec,
    options: Mp3EncoderOptions,
    params: CodecParameters,
    pts_samples: u64,
    pending: Vec<f32>,
}

impl std::fmt::Debug for Mp3Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mp3Encoder")
            .field("spec", &self.spec)
            .field("options", &self.options)
            .field("pts_samples", &self.pts_samples)
            .finish()
    }
}

impl Mp3Encoder {
    pub fn new(spec: AudioSpec) -> Result<Self> {
        Self::builder(spec).build()
    }

    #[must_use]
    pub fn builder(spec: AudioSpec) -> Mp3EncoderBuilder {
        Mp3EncoderBuilder::new(spec)
    }

    pub fn with_options(spec: AudioSpec, options: Mp3EncoderOptions) -> Result<Self> {
        validate_spec(spec)?;

        let channels = spec.channels.count() as u32;
        let bitrate_kbps = options.bitrate_bps / 1000;

        let inner = MeporusEncoder::new(spec.sample_rate, channels, bitrate_kbps)
            .map_err(|e| Error::Unsupported(e))?;

        let stream_info = Mp3StreamInfo::new(options.bitrate_bps);
        let mut params = CodecParameters::new(CodecId::Mp3, spec);
        params.sample_format = Some(spec.sample_format);
        params.bit_depth = Some(16);
        params.codec_specific = Some(CodecSpecific::Mp3(stream_info));

        Ok(Self {
            inner,
            spec,
            options,
            params,
            pts_samples: 0,
            pending: Vec::new(),
        })
    }

    #[must_use]
    pub const fn options(&self) -> Mp3EncoderOptions {
        self.options
    }
}

impl Encoder for Mp3Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Mp3
    }

    fn input_spec(&self) -> AudioSpec {
        self.spec
    }

    fn codec_parameters(&self) -> &CodecParameters {
        &self.params
    }

    fn encode(&mut self, input: AudioBufferRef<'_>, sink: &mut dyn PacketSink) -> Result<()> {
        if input.is_empty() {
            return Ok(());
        }

        let samples = match input {
            AudioBufferRef::F32(data) => data,
            _ => return Err(Error::Unsupported("mp3 encoder expects f32 input")),
        };

        self.pending.extend_from_slice(samples);

        let frame_size = 1152 * self.spec.channel_count() as usize;
        let mut out_buf = Vec::new();

        while self.pending.len() >= frame_size {
            let frame: Vec<f32> = self.pending.drain(..frame_size).collect();
            let written = self
                .inner
                .encode(&frame, &mut out_buf)
                .map_err(|e| Error::Unsupported(e))?;

            if written > 0 {
                let pts = Timestamp::new(self.pts_samples);
                self.pts_samples = self
                    .pts_samples
                    .checked_add(1152)
                    .ok_or(Error::InvalidState("timestamp overflow"))?;

                let data = out_buf.drain(..written).collect::<Vec<_>>();
                let mut packet = EncodedPacket::new(data);
                packet.pts = Some(pts);
                packet.dts = Some(pts);
                packet.flags = PacketFlags::KEYFRAME;
                sink.write_packet(packet)?;
            }
        }

        Ok(())
    }

    fn flush(&mut self, sink: &mut dyn PacketSink) -> Result<()> {
        let mut out_buf = Vec::new();

        if !self.pending.is_empty() {
            let written = self
                .inner
                .encode(&self.pending, &mut out_buf)
                .map_err(|e| Error::Unsupported(e))?;
            self.pending.clear();
            if written > 0 {
                let data = out_buf.drain(..written).collect::<Vec<_>>();
                let packet = EncodedPacket::new(data);
                sink.write_packet(packet)?;
            }
        }

        let written = self
            .inner
            .flush(&mut out_buf)
            .map_err(|e| Error::Unsupported(e))?;
        if written > 0 {
            let data = out_buf.into_iter().collect::<Vec<_>>();
            let packet = EncodedPacket::new(data);
            sink.write_packet(packet)?;
        }

        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        let bitrate_kbps = self.options.bitrate_bps / 1000;
        let channels = self.spec.channel_count();
        self.inner = MeporusEncoder::new(self.spec.sample_rate, channels, bitrate_kbps)
            .map_err(|e| Error::Unsupported(e))?;
        self.pending.clear();
        self.pts_samples = 0;
        Ok(())
    }
}

fn validate_spec(spec: AudioSpec) -> Result<()> {
    let valid_rates = [32000, 44100, 48000];
    if !valid_rates.contains(&spec.sample_rate) {
        return Err(Error::Unsupported(
            "mp3 encoder sample rate must be 32000, 44100, or 48000",
        ));
    }

    let ch = spec.channels.count();
    if ch == 0 || ch > 2 {
        return Err(Error::Unsupported("mp3 encoder supports only mono or stereo"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dissonia_core::audio::ChannelLayout;
    use dissonia_core::codecs::VecPacketSink;

    #[test]
    fn encodes_mono_silence() -> Result<()> {
        let spec = AudioSpec::new(44100, ChannelLayout::MONO, SampleFormat::F32);
        let mut encoder = Mp3Encoder::new(spec)?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0.0f32; 44100];
        encoder.encode(AudioBufferRef::F32(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;

        let packets = sink.into_inner();
        assert!(!packets.is_empty());
        assert!(!packets[0].data.is_empty());

        Ok(())
    }

    #[test]
    fn encodes_stereo_silence() -> Result<()> {
        let spec = AudioSpec::new(44100, ChannelLayout::STEREO, SampleFormat::F32);
        let mut encoder = Mp3Encoder::builder(spec).bitrate_bps(256_000).build()?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0.0f32; 44100 * 2];
        encoder.encode(AudioBufferRef::F32(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;

        let packets = sink.into_inner();
        assert!(!packets.is_empty());
        assert!(!packets[0].data.is_empty());

        Ok(())
    }

    #[test]
    fn produces_valid_mp3_header() -> Result<()> {
        let spec = AudioSpec::new(44100, ChannelLayout::MONO, SampleFormat::F32);
        let mut encoder = Mp3Encoder::new(spec)?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0.0f32; 44100];
        encoder.encode(AudioBufferRef::F32(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;

        let packets = sink.into_inner();
        let all_data: Vec<u8> = packets.iter().flat_map(|p| p.data.iter().copied()).collect();
        assert!(all_data.len() > 200);
        assert_eq!(all_data[0] & 0xFF, 0xFF);
        assert_eq!((all_data[1] & 0xE0) >> 5, 0b111);

        Ok(())
    }

    #[test]
    fn sets_codec_parameters_correctly() -> Result<()> {
        let spec = AudioSpec::new(48000, ChannelLayout::STEREO, SampleFormat::F32);
        let encoder = Mp3Encoder::builder(spec).bitrate_bps(320_000).build()?;

        assert_eq!(encoder.codec_id(), CodecId::Mp3);
        assert_eq!(encoder.input_spec().sample_rate, 48000);

        Ok(())
    }

    #[test]
    fn rejects_unsupported_sample_rate() {
        let spec = AudioSpec::new(96000, ChannelLayout::MONO, SampleFormat::F32);
        let result = Mp3Encoder::new(spec);
        assert!(result.is_err());
    }
}
