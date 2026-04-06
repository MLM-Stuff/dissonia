#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SampleFormat {
    U8,
    I16,
    I24,
    I32,
    F32,
    F64,
}

impl SampleFormat {
    #[must_use]
    pub const fn bits_per_sample(self) -> u32 {
        match self {
            Self::U8 => 8,
            Self::I16 => 16,
            Self::I24 => 24,
            Self::I32 => 32,
            Self::F32 => 32,
            Self::F64 => 64,
        }
    }
}
