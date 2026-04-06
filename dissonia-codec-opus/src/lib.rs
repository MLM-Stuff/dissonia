#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

mod encoder;
mod options;

pub use encoder::{OpusEncoder, OpusEncoderBuilder};
pub use options::{
    OpusApplication, OpusBitrate, OpusBandwidth, OpusEncoderOptions, OpusFrameDuration,
    OpusSignal, DEFAULT_MAX_PACKET_BYTES,
};