use std::io::Write;

use dissonia_core::codecs::CodecId;
use dissonia_core::{Error, Result};

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;

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

pub(crate) fn write_classic_fmt_payload<W>(
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
    writer.write_all(&codec.classic_format_tag.to_le_bytes())?;
    writer.write_all(&channels.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&codec.bits_per_sample.to_le_bytes())?;
    Ok(())
}

pub(crate) fn write_extensible_fmt_payload<W>(
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
    writer.write_all(&0xfffe_u16.to_le_bytes())?;
    writer.write_all(&channels.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&codec.bits_per_sample.to_le_bytes())?;
    writer.write_all(&22_u16.to_le_bytes())?;
    writer.write_all(&codec.bits_per_sample.to_le_bytes())?;
    writer.write_all(&channel_mask.to_le_bytes())?;
    writer.write_all(&codec.extensible_subformat)?;
    Ok(())
}
