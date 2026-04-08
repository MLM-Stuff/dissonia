use dissonia_common::vorbis::VorbisComments;
use dissonia_core::{Error, Result};

const BLOCK_TYPE_STREAMINFO: u8 = 0;
const BLOCK_TYPE_PADDING: u8 = 1;
const BLOCK_TYPE_SEEKTABLE: u8 = 3;
const BLOCK_TYPE_VORBIS_COMMENT: u8 = 4;

pub(crate) const STREAMINFO_LEN: usize = 34;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SeekPoint {
    pub sample_number: u64,
    pub stream_offset: u64,
    pub frame_samples: u16,
}

impl SeekPoint {
    pub const SIZE: usize = 18;

    pub const fn placeholder() -> Self {
        Self {
            sample_number: u64::MAX,
            stream_offset: 0,
            frame_samples: 0,
        }
    }

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut buf = [0_u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.sample_number.to_be_bytes());
        buf[8..16].copy_from_slice(&self.stream_offset.to_be_bytes());
        buf[16..18].copy_from_slice(&self.frame_samples.to_be_bytes());
        buf
    }
}

pub(crate) fn encode_stream_info(
    min_block_size: u16,
    max_block_size: u16,
    min_frame_size: u32,
    max_frame_size: u32,
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    total_samples: u64,
    md5: &[u8; 16],
) -> [u8; STREAMINFO_LEN] {
    let mut buf = [0_u8; STREAMINFO_LEN];

    buf[0..2].copy_from_slice(&min_block_size.to_be_bytes());
    buf[2..4].copy_from_slice(&max_block_size.to_be_bytes());

    buf[4] = (min_frame_size >> 16) as u8;
    buf[5] = (min_frame_size >> 8) as u8;
    buf[6] = min_frame_size as u8;

    buf[7] = (max_frame_size >> 16) as u8;
    buf[8] = (max_frame_size >> 8) as u8;
    buf[9] = max_frame_size as u8;

    let sr = sample_rate;
    let ch = u32::from(channels - 1) & 0x7;
    let bps = u32::from(bits_per_sample - 1) & 0x1F;
    let ts_hi = ((total_samples >> 32) & 0xF) as u32;

    buf[10] = (sr >> 12) as u8;
    buf[11] = (sr >> 4) as u8;
    buf[12] = ((sr & 0xF) << 4) as u8 | (ch << 1) as u8 | ((bps >> 4) & 1) as u8;
    buf[13] = ((bps & 0xF) << 4) as u8 | ts_hi as u8;

    let ts_lo = total_samples as u32;
    buf[14..18].copy_from_slice(&ts_lo.to_be_bytes());

    buf[18..34].copy_from_slice(md5);

    buf
}

pub(crate) fn metadata_block_header(is_last: bool, block_type: u8, length: u32) -> [u8; 4] {
    let mut hdr = [0_u8; 4];
    hdr[0] = if is_last { 0x80 } else { 0x00 } | (block_type & 0x7F);
    hdr[1] = (length >> 16) as u8;
    hdr[2] = (length >> 8) as u8;
    hdr[3] = length as u8;
    hdr
}

pub(crate) fn build_stream_info_block(
    is_last: bool,
    min_block_size: u16,
    max_block_size: u16,
    min_frame_size: u32,
    max_frame_size: u32,
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    total_samples: u64,
    md5: &[u8; 16],
) -> [u8; 4 + STREAMINFO_LEN] {
    let hdr = metadata_block_header(is_last, BLOCK_TYPE_STREAMINFO, STREAMINFO_LEN as u32);
    let payload = encode_stream_info(
        min_block_size,
        max_block_size,
        min_frame_size,
        max_frame_size,
        sample_rate,
        channels,
        bits_per_sample,
        total_samples,
        md5,
    );

    let mut block = [0_u8; 4 + STREAMINFO_LEN];
    block[0..4].copy_from_slice(&hdr);
    block[4..4 + STREAMINFO_LEN].copy_from_slice(&payload);
    block
}

pub(crate) fn build_seektable_block(is_last: bool, num_points: u32) -> Vec<u8> {
    let payload_len = num_points as usize * SeekPoint::SIZE;
    let length = u32::try_from(payload_len).unwrap_or(u32::MAX);
    let hdr = metadata_block_header(is_last, BLOCK_TYPE_SEEKTABLE, length);

    let mut block = Vec::with_capacity(4 + payload_len);
    block.extend_from_slice(&hdr);

    let placeholder = SeekPoint::placeholder();
    for _ in 0..num_points {
        block.extend_from_slice(&placeholder.encode());
    }

    block
}

pub(crate) fn build_vorbis_comment_block(
    is_last: bool,
    comments: &VorbisComments,
) -> Result<Vec<u8>> {
    let payload = comments
        .encode()
        .ok_or(Error::Unsupported("vorbis comment data exceeds u32 limits"))?;

    let length = u32::try_from(payload.len())
        .map_err(|_| Error::Unsupported("vorbis comment block exceeds 24-bit length"))?;

    if length > 0xFF_FFFF {
        return Err(Error::Unsupported(
            "vorbis comment block exceeds FLAC metadata block size limit",
        ));
    }

    let hdr = metadata_block_header(is_last, BLOCK_TYPE_VORBIS_COMMENT, length);

    let mut block = Vec::with_capacity(4 + payload.len());
    block.extend_from_slice(&hdr);
    block.extend_from_slice(&payload);
    Ok(block)
}

pub(crate) fn build_padding_block(is_last: bool, length: u32) -> Vec<u8> {
    let hdr = metadata_block_header(is_last, BLOCK_TYPE_PADDING, length);
    let mut block = Vec::with_capacity(4 + length as usize);
    block.extend_from_slice(&hdr);
    block.resize(4 + length as usize, 0);
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_info_block_is_correct_size() {
        let block = build_stream_info_block(false, 4096, 4096, 0, 0, 44_100, 2, 16, 0, &[0; 16]);
        assert_eq!(block.len(), 38);
        assert_eq!(block[0], 0x00);
        assert_eq!(block[3], 34);
    }

    #[test]
    fn seektable_block_has_correct_size() {
        let block = build_seektable_block(false, 10);
        assert_eq!(block.len(), 184);
        assert_eq!(block[0] & 0x7F, BLOCK_TYPE_SEEKTABLE);
        assert_eq!(block[4], 0xFF);
    }

    #[test]
    fn seek_point_round_trips() {
        let sp = SeekPoint {
            sample_number: 48000,
            stream_offset: 1234,
            frame_samples: 4096,
        };
        let encoded = sp.encode();
        assert_eq!(u64::from_be_bytes(encoded[0..8].try_into().unwrap()), 48000);
        assert_eq!(u64::from_be_bytes(encoded[8..16].try_into().unwrap()), 1234);
        assert_eq!(
            u16::from_be_bytes(encoded[16..18].try_into().unwrap()),
            4096
        );
    }

    #[test]
    fn vorbis_comment_block_round_trips() {
        let mut vc = VorbisComments::new("test");
        vc.add("TITLE", "Hello");
        let block = build_vorbis_comment_block(true, &vc).unwrap();
        assert_eq!(block[0] & 0x80, 0x80);
        assert_eq!(block[0] & 0x7F, 4);
    }
}
