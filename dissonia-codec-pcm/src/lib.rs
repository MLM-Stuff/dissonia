#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

mod encoder;
mod options;

pub use encoder::{PcmEncoder, PcmEncoderBuilder};
pub use options::PcmEncoderOptions;