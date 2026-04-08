#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

mod bitwriter;
mod crc;
mod encoder;
mod frame;
mod options;
mod rice;
mod subframe;

pub use encoder::{FlacEncoder, FlacEncoderBuilder};
pub use options::FlacEncoderOptions;