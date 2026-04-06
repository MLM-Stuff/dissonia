#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpusApplication {
    Voip,
    Audio,
    LowDelay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpusBitrate {
    Auto,
    Max,
    Bits(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpusBandwidth {
    Narrowband,
    Mediumband,
    Wideband,
    Superwideband,
    Fullband,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpusSignal {
    Auto,
    Voice,
    Music,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpusFrameDuration {
    Auto,
    Ms2_5,
    Ms5,
    Ms10,
    Ms20,
    Ms40,
    Ms60,
    Ms80,
    Ms100,
    Ms120,
}

pub const DEFAULT_MAX_PACKET_BYTES: usize = 1276 * 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpusEncoderOptions {
    pub application: OpusApplication,
    pub bitrate: Option<OpusBitrate>,
    pub complexity: Option<u8>,
    pub vbr: Option<bool>,
    pub constrained_vbr: Option<bool>,
    pub max_bandwidth: Option<OpusBandwidth>,
    pub signal: Option<OpusSignal>,
    pub inband_fec: Option<bool>,
    pub packet_loss_perc: Option<u8>,
    pub dtx: Option<bool>,
    pub lsb_depth: Option<u8>,
    pub frame_duration: OpusFrameDuration,
    pub prediction_disabled: Option<bool>,
    pub mapping_family: Option<u8>,
    pub max_packet_bytes: usize,
    pub pad_flush: bool,
}

impl Default for OpusEncoderOptions {
    fn default() -> Self {
        Self {
            application: OpusApplication::Audio,
            bitrate: None,
            complexity: None,
            vbr: None,
            constrained_vbr: None,
            max_bandwidth: None,
            signal: None,
            inband_fec: None,
            packet_loss_perc: None,
            dtx: None,
            lsb_depth: None,
            frame_duration: OpusFrameDuration::Ms20,
            prediction_disabled: None,
            mapping_family: None,
            max_packet_bytes: DEFAULT_MAX_PACKET_BYTES,
            pad_flush: true,
        }
    }
}
