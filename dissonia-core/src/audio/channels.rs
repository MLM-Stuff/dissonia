use core::ops::{BitOr, BitOrAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ChannelLayout(u32);

impl ChannelLayout {
    pub const FRONT_LEFT: Self = Self(1 << 0);
    pub const FRONT_RIGHT: Self = Self(1 << 1);
    pub const FRONT_CENTER: Self = Self(1 << 2);
    pub const LOW_FREQUENCY: Self = Self(1 << 3);
    pub const BACK_LEFT: Self = Self(1 << 4);
    pub const BACK_RIGHT: Self = Self(1 << 5);

    pub const MONO: Self = Self::FRONT_CENTER;
    pub const STEREO: Self = Self(Self::FRONT_LEFT.0 | Self::FRONT_RIGHT.0);

    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }
}

impl BitOr for ChannelLayout {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for ChannelLayout {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
