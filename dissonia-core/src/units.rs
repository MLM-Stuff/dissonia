#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub u64);

impl Timestamp {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimeBase {
    numer: u32,
    denom: u32,
}

impl TimeBase {
    #[must_use]
    pub fn new(numer: u32, denom: u32) -> Self {
        assert!(numer != 0, "time base numerator must be non-zero");
        assert!(denom != 0, "time base denominator must be non-zero");

        Self { numer, denom }
    }

    #[must_use]
    pub fn audio_sample_rate(sample_rate: u32) -> Self {
        Self::new(1, sample_rate)
    }

    #[must_use]
    pub const fn numer(self) -> u32 {
        self.numer
    }

    #[must_use]
    pub const fn denom(self) -> u32 {
        self.denom
    }

    #[must_use]
    pub fn as_seconds(self, timestamp: Timestamp) -> f64 {
        (timestamp.0 as f64 * self.numer as f64) / self.denom as f64
    }
}

impl Default for TimeBase {
    fn default() -> Self {
        Self { numer: 1, denom: 1 }
    }
}
