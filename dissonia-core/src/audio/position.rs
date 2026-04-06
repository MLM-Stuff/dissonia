use crate::audio::channels::ChannelLayout;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelPosition {
    FrontLeft,
    FrontRight,
    FrontCenter,
    LowFrequency,
    BackLeft,
    BackRight,
    BackCenter,
    SideLeft,
    SideRight,
}

impl ChannelPosition {
    #[must_use]
    pub const fn layout(self) -> ChannelLayout {
        match self {
            Self::FrontLeft => ChannelLayout::FRONT_LEFT,
            Self::FrontRight => ChannelLayout::FRONT_RIGHT,
            Self::FrontCenter => ChannelLayout::FRONT_CENTER,
            Self::LowFrequency => ChannelLayout::LOW_FREQUENCY,
            Self::BackLeft => ChannelLayout::BACK_LEFT,
            Self::BackRight => ChannelLayout::BACK_RIGHT,
            Self::BackCenter => ChannelLayout::BACK_CENTER,
            Self::SideLeft => ChannelLayout::SIDE_LEFT,
            Self::SideRight => ChannelLayout::SIDE_RIGHT,
        }
    }
}
