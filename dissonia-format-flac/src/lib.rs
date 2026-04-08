#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

mod metadata;
mod muxer;

pub use muxer::{FlacMuxer, FlacMuxerBuilder, FlacMuxerOptions};