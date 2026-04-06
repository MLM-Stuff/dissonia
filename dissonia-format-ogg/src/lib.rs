#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

pub mod opus;

pub use opus::{OggOpusMuxer, OggOpusMuxerBuilder, OggOpusMuxerOptions};