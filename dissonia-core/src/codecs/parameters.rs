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
    Opus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpusStreamMapping {
    pub family: u8,
    pub stream_count: u8,
    pub coupled_stream_count: u8,
    pub mapping: Box<[u8]>,
}

impl OpusStreamMapping {
    #[must_use]
    pub fn new(
        family: u8,
        stream_count: u8,
        coupled_stream_count: u8,
        mapping: impl Into<Box<[u8]>>,
    ) -> Self {
        Self {
            family,
            stream_count,
            coupled_stream_count,
            mapping: mapping.into(),
        }
    }
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
    pub opus_stream_mapping: Option<OpusStreamMapping>,
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
            opus_stream_mapping: None,
            extradata: Box::new([]),
        }
    }
}
