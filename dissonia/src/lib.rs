#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![deny(missing_debug_implementations)]

pub use dissonia_common as common;
pub use dissonia_core as core;

pub use dissonia_common::vorbis::VorbisComments;

#[cfg(feature = "pcm")]
pub use dissonia_codec_pcm as codec_pcm;

#[cfg(feature = "pcm")]
pub use dissonia_codec_pcm::{PcmEncoder, PcmEncoderBuilder, PcmEncoderOptions};

#[cfg(feature = "opus")]
pub use dissonia_codec_opus as codec_opus;

#[cfg(feature = "opus")]
pub use dissonia_codec_opus::{
    OpusApplication, OpusBandwidth, OpusBitrate, OpusEncoder, OpusEncoderBuilder,
    OpusEncoderOptions, OpusFrameDuration, OpusSignal,
};

#[cfg(feature = "riff")]
pub use dissonia_format_riff as format_riff;

#[cfg(feature = "riff")]
pub use dissonia_format_riff::{WavMuxer, WavMuxerBuilder, WavMuxerOptions};

#[cfg(feature = "ogg")]
pub use dissonia_format_ogg as format_ogg;

#[cfg(feature = "ogg")]
pub use dissonia_format_ogg::{OggOpusMuxer, OggOpusMuxerBuilder, OggOpusMuxerOptions};

pub mod prelude {
    pub use dissonia_common::vorbis::VorbisComments;
    pub use dissonia_core::audio::{
        AudioBufferRef, AudioSpec, ChannelLayout, ChannelPosition, SampleFormat,
    };
    pub use dissonia_core::codecs::{
        CodecId, CodecParameters, CodecSpecific, Encoder, FlacStreamInfo, OpusStreamMapping,
        PacketSink, VecPacketSink,
    };
    pub use dissonia_core::formats::{
        FinalizeSummary, FormatId, Muxer, MuxerExt, TrackId, TrackSpec, TrackWriter,
    };
    pub use dissonia_core::packet::{EncodedPacket, PacketFlags};
    pub use dissonia_core::units::{TimeBase, Timestamp};
    pub use dissonia_core::{Error, Result};

    #[cfg(feature = "pcm")]
    pub use dissonia_codec_pcm::{PcmEncoder, PcmEncoderBuilder, PcmEncoderOptions};

    #[cfg(feature = "opus")]
    pub use dissonia_codec_opus::{
        OpusApplication, OpusBandwidth, OpusBitrate, OpusEncoder, OpusEncoderBuilder,
        OpusEncoderOptions, OpusFrameDuration, OpusSignal,
    };

    #[cfg(feature = "riff")]
    pub use dissonia_format_riff::{WavMuxer, WavMuxerBuilder, WavMuxerOptions};

    #[cfg(feature = "ogg")]
    pub use dissonia_format_ogg::{OggOpusMuxer, OggOpusMuxerBuilder, OggOpusMuxerOptions};
}
