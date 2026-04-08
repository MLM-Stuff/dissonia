use crate::codecs::PacketSink;
use crate::formats::{TrackId, TrackSpec};
use crate::packet::EncodedPacket;
use crate::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FormatId {
    Riff,
    Ogg,
    Flac,
    IsoBmff,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FinalizeSummary {
    pub bytes_written: Option<u64>,
    pub packet_count: u64,
    pub total_samples: Option<u64>,
}

pub trait Muxer: Send {
    fn format_id(&self) -> FormatId;

    fn add_track(&mut self, spec: TrackSpec) -> Result<TrackId>;

    fn write_packet(&mut self, track: TrackId, packet: EncodedPacket) -> Result<()>;

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn finalize(&mut self) -> Result<FinalizeSummary>;
}

#[derive(Debug)]
pub struct TrackWriter<'a, M: ?Sized + Muxer> {
    muxer: &'a mut M,
    track: TrackId,
}

impl<'a, M: ?Sized + Muxer> TrackWriter<'a, M> {
    #[must_use]
    pub fn new(muxer: &'a mut M, track: TrackId) -> Self {
        Self { muxer, track }
    }

    #[must_use]
    pub const fn track(&self) -> TrackId {
        self.track
    }
}

impl<M: ?Sized + Muxer> PacketSink for TrackWriter<'_, M> {
    fn write_packet(&mut self, packet: EncodedPacket) -> Result<()> {
        self.muxer.write_packet(self.track, packet)
    }
}

pub trait MuxerExt: Muxer {
    fn track_writer(&mut self, track: TrackId) -> TrackWriter<'_, Self>
    where
        Self: Sized,
    {
        TrackWriter::new(self, track)
    }
}

impl<T: Muxer + ?Sized> MuxerExt for T {}
