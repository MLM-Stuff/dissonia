pub mod encoder;
pub mod parameters;

pub use encoder::{Encoder, PacketSink, VecPacketSink};
pub use parameters::{
    opus_family1_stream_mapping, opus_surround_channel_order, CodecId, CodecParameters,
    CodecSpecific, FlacStreamInfo, OpusStreamMapping,
};
