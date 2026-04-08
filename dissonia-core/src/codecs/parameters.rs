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
    Flac,
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
pub struct FlacStreamInfo {
    pub min_block_size: u16,
    pub max_block_size: u16,
    pub min_frame_size: u32,
    pub max_frame_size: u32,
    pub bits_per_sample: u8,
    pub total_samples: u64,
    pub md5: [u8; 16],
}

impl FlacStreamInfo {
    #[must_use]
    pub fn new(bits_per_sample: u8) -> Self {
        Self {
            min_block_size: 0,
            max_block_size: 0,
            min_frame_size: 0,
            max_frame_size: 0,
            bits_per_sample,
            total_samples: 0,
            md5: [0; 16],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecSpecific {
    Opus(OpusStreamMapping),
    Flac(FlacStreamInfo),
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
    pub codec_specific: Option<CodecSpecific>,
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
            codec_specific: None,
            extradata: Box::new([]),
        }
    }

    #[must_use]
    pub fn opus_stream_mapping(&self) -> Option<&OpusStreamMapping> {
        match &self.codec_specific {
            Some(CodecSpecific::Opus(mapping)) => Some(mapping),
            _ => None,
        }
    }

    #[must_use]
    pub fn flac_stream_info(&self) -> Option<&FlacStreamInfo> {
        match &self.codec_specific {
            Some(CodecSpecific::Flac(info)) => Some(info),
            _ => None,
        }
    }
}
