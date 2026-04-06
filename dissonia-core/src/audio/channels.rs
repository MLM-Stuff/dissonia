use core::ops::{BitOr, BitOrAssign};

use crate::audio::position::ChannelPosition;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ChannelLayout(u32);

impl ChannelLayout {
    pub const FRONT_LEFT: Self = Self(1 << 0);
    pub const FRONT_RIGHT: Self = Self(1 << 1);
    pub const FRONT_CENTER: Self = Self(1 << 2);
    pub const LOW_FREQUENCY: Self = Self(1 << 3);
    pub const BACK_LEFT: Self = Self(1 << 4);
    pub const BACK_RIGHT: Self = Self(1 << 5);
    pub const BACK_CENTER: Self = Self(1 << 6);
    pub const SIDE_LEFT: Self = Self(1 << 7);
    pub const SIDE_RIGHT: Self = Self(1 << 8);

    pub const MONO: Self = Self::FRONT_CENTER;
    pub const STEREO: Self = Self(Self::FRONT_LEFT.0 | Self::FRONT_RIGHT.0);

    pub const LINEAR_SURROUND: Self =
        Self(Self::FRONT_LEFT.0 | Self::FRONT_CENTER.0 | Self::FRONT_RIGHT.0);

    pub const QUAD: Self =
        Self(Self::FRONT_LEFT.0 | Self::FRONT_RIGHT.0 | Self::BACK_LEFT.0 | Self::BACK_RIGHT.0);

    pub const SURROUND_5_0: Self = Self(
        Self::FRONT_LEFT.0
            | Self::FRONT_CENTER.0
            | Self::FRONT_RIGHT.0
            | Self::BACK_LEFT.0
            | Self::BACK_RIGHT.0,
    );

    pub const SURROUND_5_1: Self = Self(Self::SURROUND_5_0.0 | Self::LOW_FREQUENCY.0);

    pub const SURROUND_6_1: Self = Self(
        Self::FRONT_LEFT.0
            | Self::FRONT_CENTER.0
            | Self::FRONT_RIGHT.0
            | Self::SIDE_LEFT.0
            | Self::SIDE_RIGHT.0
            | Self::BACK_CENTER.0
            | Self::LOW_FREQUENCY.0,
    );

    pub const SURROUND_7_1: Self = Self(
        Self::FRONT_LEFT.0
            | Self::FRONT_CENTER.0
            | Self::FRONT_RIGHT.0
            | Self::SIDE_LEFT.0
            | Self::SIDE_RIGHT.0
            | Self::BACK_LEFT.0
            | Self::BACK_RIGHT.0
            | Self::LOW_FREQUENCY.0,
    );

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

    #[must_use]
    pub fn positions(self) -> Box<[ChannelPosition]> {
        let mut positions = Vec::new();

        for position in [
            ChannelPosition::FrontLeft,
            ChannelPosition::FrontRight,
            ChannelPosition::FrontCenter,
            ChannelPosition::LowFrequency,
            ChannelPosition::BackLeft,
            ChannelPosition::BackRight,
            ChannelPosition::BackCenter,
            ChannelPosition::SideLeft,
            ChannelPosition::SideRight,
        ] {
            if self.contains(position.layout()) {
                positions.push(position);
            }
        }

        positions.into_boxed_slice()
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
