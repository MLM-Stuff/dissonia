use crate::audio::SampleFormat;

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum AudioBufferRef<'a> {
    U8(&'a [u8]),
    I16(&'a [i16]),
    I24(&'a [i32]),
    I32(&'a [i32]),
    F32(&'a [f32]),
    F64(&'a [f64]),
}

impl<'a> AudioBufferRef<'a> {
    #[must_use]
    pub const fn sample_format(self) -> SampleFormat {
        match self {
            Self::U8(_) => SampleFormat::U8,
            Self::I16(_) => SampleFormat::I16,
            Self::I24(_) => SampleFormat::I24,
            Self::I32(_) => SampleFormat::I32,
            Self::F32(_) => SampleFormat::F32,
            Self::F64(_) => SampleFormat::F64,
        }
    }

    #[must_use]
    pub fn len(self) -> usize {
        match self {
            Self::U8(data) => data.len(),
            Self::I16(data) => data.len(),
            Self::I24(data) => data.len(),
            Self::I32(data) => data.len(),
            Self::F32(data) => data.len(),
            Self::F64(data) => data.len(),
        }
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}
