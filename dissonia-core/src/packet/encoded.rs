use core::ops::{BitOr, BitOrAssign};

use crate::units::Timestamp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PacketFlags(u32);

impl PacketFlags {
    pub const NONE: Self = Self(0);
    pub const KEYFRAME: Self = Self(1 << 0);
    pub const HEADER: Self = Self(1 << 1);
    pub const EOS: Self = Self(1 << 2);

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
}

impl BitOr for PacketFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for PacketFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EncodedPacket {
    pub pts: Option<Timestamp>,
    pub dts: Option<Timestamp>,
    pub duration: Option<u64>,
    pub trim_start: u32,
    pub trim_end: u32,
    pub flags: PacketFlags,
    pub data: Box<[u8]>,
}

impl EncodedPacket {
    #[must_use]
    pub fn new(data: impl Into<Box<[u8]>>) -> Self {
        Self {
            pts: None,
            dts: None,
            duration: None,
            trim_start: 0,
            trim_end: 0,
            flags: PacketFlags::NONE,
            data: data.into(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
