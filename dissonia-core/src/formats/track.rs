use crate::codecs::CodecParameters;
use crate::units::TimeBase;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackSpec {
    pub codec_params: CodecParameters,
    pub time_base: TimeBase,
    pub language: Option<String>,
    pub name: Option<String>,
}

impl TrackSpec {
    #[must_use]
    pub fn new(codec_params: CodecParameters, time_base: TimeBase) -> Self {
        Self {
            codec_params,
            time_base,
            language: None,
            name: None,
        }
    }
}
