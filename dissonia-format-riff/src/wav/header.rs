use std::io::Write;

use dissonia_core::codecs::CodecId;
use dissonia_core::{Error, Result};

pub(crate) const RIFF_SIZE_OFFSET: u64 = 4;

pub(crate) const CLASSIC_DATA_SIZE_OFFSET: u64 = 40;
pub(crate) const CLASSIC_HEADER_LEN: u64 = 44;

pub(crate) const EXTENSIBLE_DATA_SIZE_OFFSET: u64 = 64;
pub(crate) const EXTENSIBLE_HEADER_LEN: u64 = 68;

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;

const KSDATAFORMAT_SUBTYPE_PCM: [u8; 16] = [
    0x01, 0x00, 0x00, 0x00, //
    0x00, 0x00, //
    0x10, 0x00, //
    0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];

const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: [u8; 16] = [
    0x03, 0x00, 0x00, 0x00, //
    0x00, 0x00, //
    0x10, 0x00, //
    0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct WavCodecInfo {
    pub classic_format_tag: u16,
    pub bits_per_sample: u16,
    pub bytes_per_sample: u16,
    pub extensible_subformat: [u8; 16],
}

pub(crate) fn wav_codec_info(codec: CodecId) -> Result<WavCodecInfo> {
    match codec {
        CodecId::PcmU8 => Ok(WavCodecInfo {
            classic_format_tag: WAVE_FORMAT_PCM,
            bits_per_sample: 8,
            bytes_per_sample: 1,
            extensible_subformat: KSDATAFORMAT_SUBTYPE_PCM,
        }),
        CodecId::PcmS16Le => Ok(WavCodecInfo {
            classic_format_tag: WAVE_FORMAT_PCM,
            bits_per_sample: 16,
            bytes_per_sample: 2,
            extensible_subformat: KSDATAFORMAT_SUBTYPE_PCM,
        }),
        CodecId::PcmS24Le => Ok(WavCodecInfo {
            classic_format_tag: WAVE_FORMAT_PCM,
            bits_per_sample: 24,
            bytes_per_sample: 3,
            extensible_subformat: KSDATAFORMAT_SUBTYPE_PCM,
        }),
        CodecId::PcmS32Le => Ok(WavCodecInfo {
            classic_format_tag: WAVE_FORMAT_PCM,
            bits_per_sample: 32,
            bytes_per_sample: 4,
            extensible_subformat: KSDATAFORMAT_SUBTYPE_PCM,
        }),
        CodecId::PcmF32Le => Ok(WavCodecInfo {
            classic_format_tag: WAVE_FORMAT_IEEE_FLOAT,
            bits_per_sample: 32,
            bytes_per_sample: 4,
            extensible_subformat: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        }),
        CodecId::PcmF64Le => Ok(WavCodecInfo {
            classic_format_tag: WAVE_FORMAT_IEEE_FLOAT,
            bits_per_sample: 64,
            bytes_per_sample: 8,
            extensible_subformat: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        }),
        _ => Err(Error::Unsupported(
            "wav muxer does not support this codec id",
        )),
    }
}

pub(crate) fn write_classic_header<W>(
    writer: &mut W,
    codec: WavCodecInfo,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    block_align: u16,
) -> Result<()>
where
    W: Write,
{
    writer.write_all(b"RIFF")?;
    write_u32_le(writer, 0)?;
    writer.write_all(b"WAVE")?;

    writer.write_all(b"fmt ")?;
    write_u32_le(writer, 16)?;
    write_u16_le(writer, codec.classic_format_tag)?;
    write_u16_le(writer, channels)?;
    write_u32_le(writer, sample_rate)?;
    write_u32_le(writer, byte_rate)?;
    write_u16_le(writer, block_align)?;
    write_u16_le(writer, codec.bits_per_sample)?;

    writer.write_all(b"data")?;
    write_u32_le(writer, 0)?;

    Ok(())
}

pub(crate) fn write_extensible_header<W>(
    writer: &mut W,
    codec: WavCodecInfo,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    block_align: u16,
    channel_mask: u32,
) -> Result<()>
where
    W: Write,
{
    writer.write_all(b"RIFF")?;
    write_u32_le(writer, 0)?;
    writer.write_all(b"WAVE")?;

    writer.write_all(b"fmt ")?;
    write_u32_le(writer, 40)?;
    write_u16_le(writer, WAVE_FORMAT_EXTENSIBLE)?;
    write_u16_le(writer, channels)?;
    write_u32_le(writer, sample_rate)?;
    write_u32_le(writer, byte_rate)?;
    write_u16_le(writer, block_align)?;
    write_u16_le(writer, codec.bits_per_sample)?;
    write_u16_le(writer, 22)?;
    write_u16_le(writer, codec.bits_per_sample)?;
    write_u32_le(writer, channel_mask)?;
    writer.write_all(&codec.extensible_subformat)?;

    writer.write_all(b"data")?;
    write_u32_le(writer, 0)?;

    Ok(())
}

fn write_u16_le<W>(writer: &mut W, value: u16) -> Result<()>
where
    W: Write,
{
    writer.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn write_u32_le<W>(writer: &mut W, value: u32) -> Result<()>
where
    W: Write,
{
    writer.write_all(&value.to_le_bytes())?;
    Ok(())
}
