use crate::audio::{ChannelLayout, SampleFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AudioSpec {
    pub sample_rate: u32,
    pub channels: ChannelLayout,
    pub sample_format: SampleFormat,
}

impl AudioSpec {
    #[must_use]
    pub fn new(sample_rate: u32, channels: ChannelLayout, sample_format: SampleFormat) -> Self {
        assert!(sample_rate != 0, "sample rate must be non-zero");

        Self {
            sample_rate,
            channels,
            sample_format,
        }
    }

    #[must_use]
    pub fn stereo_f32(sample_rate: u32) -> Self {
        Self::new(sample_rate, ChannelLayout::STEREO, SampleFormat::F32)
    }

    #[must_use]
    pub const fn channel_count(self) -> u32 {
        self.channels.count()
    }
}
