use std::io::{self, Seek, SeekFrom, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkHandle {
    size_offset: u64,
    size_data_start: u64,
}

impl ChunkHandle {
    #[must_use]
    pub const fn size_offset(self) -> u64 {
        self.size_offset
    }

    #[must_use]
    pub const fn size_data_start(self) -> u64 {
        self.size_data_start
    }
}

#[derive(Debug)]
pub struct RiffWriter<W> {
    inner: W,
}

impl<W> RiffWriter<W> {
    #[must_use]
    pub const fn new(inner: W) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn into_inner(self) -> W {
        self.inner
    }

    #[must_use]
    pub const fn get_ref(&self) -> &W {
        &self.inner
    }

    #[must_use]
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }
}

impl<W> RiffWriter<W>
where
    W: Write + Seek,
{
    pub fn position(&mut self) -> io::Result<u64> {
        self.inner.stream_position()
    }

    pub fn start_chunk(&mut self, id: [u8; 4]) -> io::Result<ChunkHandle> {
        self.inner.write_all(&id)?;
        let size_offset = self.position()?;
        self.inner.write_all(&0_u32.to_le_bytes())?;
        let size_data_start = self.position()?;

        Ok(ChunkHandle {
            size_offset,
            size_data_start,
        })
    }

    pub fn start_riff(&mut self, form_type: [u8; 4]) -> io::Result<ChunkHandle> {
        self.start_container_chunk(*b"RIFF", form_type)
    }

    pub fn start_list(&mut self, list_type: [u8; 4]) -> io::Result<ChunkHandle> {
        self.start_container_chunk(*b"LIST", list_type)
    }

    pub fn finish_chunk(&mut self, handle: ChunkHandle) -> io::Result<u32> {
        let end = self.position()?;
        let size = end.checked_sub(handle.size_data_start).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk end precedes chunk data start",
            )
        })?;

        let size_u32 = u32::try_from(size).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk size exceeds 32-bit RIFF limit",
            )
        })?;

        self.inner.seek(SeekFrom::Start(handle.size_offset))?;
        self.inner.write_all(&size_u32.to_le_bytes())?;
        self.inner.seek(SeekFrom::Start(end))?;

        if size & 1 == 1 {
            self.inner.write_all(&[0])?;
        }

        Ok(size_u32)
    }

    fn start_container_chunk(&mut self, id: [u8; 4], kind: [u8; 4]) -> io::Result<ChunkHandle> {
        self.inner.write_all(&id)?;
        let size_offset = self.position()?;
        self.inner.write_all(&0_u32.to_le_bytes())?;
        let size_data_start = self.position()?;
        self.inner.write_all(&kind)?;

        Ok(ChunkHandle {
            size_offset,
            size_data_start,
        })
    }
}

impl<W> Write for RiffWriter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.write_all(buf)
    }
}

impl<W> Seek for RiffWriter<W>
where
    W: Seek,
{
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn patches_regular_chunk_size_and_padding() {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut writer = RiffWriter::new(cursor);

        let chunk = writer.start_chunk(*b"test").unwrap();
        writer.write_all(&[1, 2, 3]).unwrap();
        let size = writer.finish_chunk(chunk).unwrap();

        assert_eq!(size, 3);

        let bytes = writer.into_inner().into_inner();
        assert_eq!(&bytes[0..4], b"test");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 3);
        assert_eq!(&bytes[8..11], &[1, 2, 3]);
        assert_eq!(bytes[11], 0);
    }

    #[test]
    fn patches_riff_container_size() {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut writer = RiffWriter::new(cursor);

        let riff = writer.start_riff(*b"WAVE").unwrap();
        let fmt = writer.start_chunk(*b"fmt ").unwrap();
        writer.write_all(&[1, 2, 3, 4]).unwrap();
        writer.finish_chunk(fmt).unwrap();
        let riff_size = writer.finish_chunk(riff).unwrap();

        assert_eq!(riff_size, 16);

        let bytes = writer.into_inner().into_inner();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 16);
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 4);
        assert_eq!(&bytes[20..24], &[1, 2, 3, 4]);
    }
}
