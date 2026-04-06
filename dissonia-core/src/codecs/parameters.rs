use crate::audio::{AudioSpec, ChannelLayout, SampleFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CodecId {
    PcmU8,
    PcmS16Le,
    PcmS24Le,
    PcmS32Le,
    PcmF32Le,
    PcmF64Le,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecParameters {
    pub codec: CodecId,
    pub sample_rate: u32,
    pub channels: ChannelLayout,
    pub sample_format: Option<SampleFormat>,
    pub bit_depth: Option<u32>,
    pub frame_samples: Option<u32>,
    pub encoder_delay: u32,
    pub encoder_padding: u32,
    pub extradata: Box<[u8]>,
}

impl CodecParameters {
    #[must_use]
    pub fn new(codec: CodecId, spec: AudioSpec) -> Self {
        Self {
            codec,
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            sample_format: Some(spec.sample_format),
            bit_depth: Some(spec.sample_format.bits_per_sample()),
            frame_samples: None,
            encoder_delay: 0,
            encoder_padding: 0,
            extradata: Box::new([]),
        }
    }
}
