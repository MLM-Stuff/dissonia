use dissonia_common::vorbis::VorbisComments;
use dissonia_core::{Error, Result};

const BLOCK_TYPE_STREAMINFO: u8 = 0;
const BLOCK_TYPE_VORBIS_COMMENT: u8 = 4;
const BLOCK_TYPE_PADDING: u8 = 1;

pub(crate) const STREAMINFO_LEN: usize = 34;

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
        assert_eq!(block[1], 0);
        assert_eq!(block[2], 0);
        assert_eq!(block[3], 34);
    }

    #[test]
    fn stream_info_encodes_sample_rate() {
        let data = encode_stream_info(4096, 4096, 0, 0, 44_100, 2, 16, 0, &[0; 16]);
        let sr =
            (u32::from(data[10]) << 12) | (u32::from(data[11]) << 4) | (u32::from(data[12]) >> 4);
        assert_eq!(sr, 44_100);
    }

    #[test]
    fn stream_info_encodes_channels_and_bps() {
        let data = encode_stream_info(4096, 4096, 0, 0, 48_000, 2, 24, 0, &[0; 16]);
        let ch = ((data[12] >> 1) & 0x07) + 1;
        let bps = (u8::from(data[12] & 1) << 4 | (data[13] >> 4)) + 1;
        assert_eq!(ch, 2);
        assert_eq!(bps, 24);
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
