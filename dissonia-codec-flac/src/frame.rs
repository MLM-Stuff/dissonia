use crate::bitwriter::BitWriter;
use crate::crc::{crc16, crc8};
use crate::subframe;

pub(crate) fn encode_frame(
    channels: &[&[i64]],
    frame_number: u32,
    sample_rate: u32,
    bits_per_sample: u8,
    block_size: u16,
    max_fixed_order: u8,
    max_rice_order: u8,
) -> Vec<u8> {
    let num_channels = channels.len();
    debug_assert!((1..=8).contains(&num_channels));
    let bs = usize::from(block_size);

    let mut hdr = BitWriter::with_capacity(16);

    hdr.write_bits(0xFFF8, 16);

    let (bs_code, bs_trailing) = encode_block_size(block_size);
    hdr.write_bits(u64::from(bs_code), 4);

    let (sr_code, sr_trailing) = encode_sample_rate(sample_rate);
    hdr.write_bits(u64::from(sr_code), 4);

    hdr.write_bits(u64::from(num_channels as u8 - 1), 4);

    hdr.write_bits(u64::from(encode_sample_size(bits_per_sample)), 3);

    hdr.write_bit(false);

    hdr.write_utf8_u32(frame_number);

    match bs_trailing {
        TrailingField::None => {}
        TrailingField::U8(v) => hdr.write_bits(u64::from(v), 8),
        TrailingField::U16(v) => hdr.write_bits(u64::from(v), 16),
    }

    match sr_trailing {
        TrailingField::None => {}
        TrailingField::U8(v) => hdr.write_bits(u64::from(v), 8),
        TrailingField::U16(v) => hdr.write_bits(u64::from(v), 16),
    }

    debug_assert!(hdr.is_byte_align());
    let header_bytes = hdr.into_bytes();
    let header_crc = crc8(&header_bytes);

    let mut frame = BitWriter::with_capacity(bs * num_channels * usize::from(bits_per_sample) / 8);
    for &b in &header_bytes {
        frame.write_bits(u64::from(b), 8);
    }
    frame.write_bits(u64::from(header_crc), 8);

    for ch in channels {
        subframe::encode_subframe(
            &mut frame,
            ch,
            bits_per_sample,
            bs,
            max_fixed_order,
            max_rice_order,
        );
    }

    frame.pad_to_byte();

    let frame_bytes = frame.into_bytes();

    let crc = crc16(&frame_bytes);

    let mut out = frame_bytes;
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

#[derive(Debug, Clone, Copy)]
enum TrailingField {
    None,
    U8(u8),
    U16(u16),
}

fn encode_block_size(block_size: u16) -> (u8, TrailingField) {
    match block_size {
        192 => (0x01, TrailingField::None),
        576 => (0x02, TrailingField::None),
        1152 => (0x03, TrailingField::None),
        2304 => (0x04, TrailingField::None),
        4608 => (0x05, TrailingField::None),
        256 => (0x08, TrailingField::None),
        512 => (0x09, TrailingField::None),
        1024 => (0x0A, TrailingField::None),
        2048 => (0x0B, TrailingField::None),
        4096 => (0x0C, TrailingField::None),
        8192 => (0x0D, TrailingField::None),
        16384 => (0x0E, TrailingField::None),
        32768 => (0x0F, TrailingField::None),
        1..=256 => (0x06, TrailingField::U8((block_size - 1) as u8)),
        _ => (0x07, TrailingField::U16(block_size - 1)),
    }
}

fn encode_sample_rate(sample_rate: u32) -> (u8, TrailingField) {
    match sample_rate {
        88_200 => (0x01, TrailingField::None),
        176_400 => (0x02, TrailingField::None),
        192_000 => (0x03, TrailingField::None),
        8_000 => (0x04, TrailingField::None),
        16_000 => (0x05, TrailingField::None),
        22_050 => (0x06, TrailingField::None),
        24_000 => (0x07, TrailingField::None),
        32_000 => (0x08, TrailingField::None),
        44_100 => (0x09, TrailingField::None),
        48_000 => (0x0A, TrailingField::None),
        96_000 => (0x0B, TrailingField::None),
        rate if rate % 1_000 == 0 && rate / 1_000 <= 255 => {
            (0x0C, TrailingField::U8((rate / 1_000) as u8))
        }
        rate if rate <= 65_535 => (0x0D, TrailingField::U16(rate as u16)),
        rate if rate % 10 == 0 && rate / 10 <= 65_535 => {
            (0x0E, TrailingField::U16((rate / 10) as u16))
        }
        _ => (0x00, TrailingField::None),
    }
}

fn encode_sample_size(bits_per_sample: u8) -> u8 {
    match bits_per_sample {
        8 => 0x01,
        12 => 0x02,
        16 => 0x04,
        20 => 0x05,
        24 => 0x06,
        32 => 0x07,
        _ => 0x00,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_starts_with_sync_code() {
        let samples = vec![0_i64; 256];
        let frame = encode_frame(&[&samples], 0, 48_000, 16, 256, 4, 6);
        assert_eq!(frame[0], 0xFF);
        assert_eq!(frame[1] & 0xFC, 0xF8);
    }

    #[test]
    fn frame_ends_with_crc16() {
        let samples = vec![0_i64; 256];
        let frame = encode_frame(&[&samples], 0, 44_100, 16, 256, 4, 6);
        let data_len = frame.len() - 2;
        let expected = crc16(&frame[..data_len]);
        let actual = u16::from_be_bytes([frame[data_len], frame[data_len + 1]]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn header_crc8_is_correct() {
        let samples = vec![0_i64; 256];
        let frame = encode_frame(&[&samples], 0, 48_000, 16, 256, 4, 6);
        let header_len = 5;
        let crc_byte = frame[header_len];
        let computed = crc8(&frame[..header_len]);
        assert_eq!(crc_byte, computed);
    }
}
