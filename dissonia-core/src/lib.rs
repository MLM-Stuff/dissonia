#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

pub mod audio;
pub mod codecs;
pub mod errors;
pub mod formats;
pub mod packet;
pub mod units;

pub use errors::{Error, Result};