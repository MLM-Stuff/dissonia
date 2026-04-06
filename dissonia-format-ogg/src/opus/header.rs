use dissonia_core::codecs::OpusStreamMapping;
use dissonia_core::{Error, Result};

pub(crate) fn build_opus_head(
    channel_count: u8,
    pre_skip: u16,
    input_sample_rate: u32,
    output_gain: i16,
    stream_mapping: Option<&OpusStreamMapping>,
) -> Result<Vec<u8>> {
    if channel_count == 0 {
        return Err(Error::InvalidArgument(
            "ogg opus channel count must be greater than zero",
        ));
    }

    let family = stream_mapping.map_or(0, |mapping| mapping.family);

    let extra_len = if family == 0 {
        0
    } else {
        let mapping = stream_mapping.ok_or(Error::InvalidArgument(
            "nonzero opus mapping family requires stream mapping metadata",
        ))?;

        if mapping.stream_count == 0 {
            return Err(Error::InvalidArgument(
                "opus stream_count must be greater than zero",
            ));
        }

        if mapping.coupled_stream_count > mapping.stream_count {
            return Err(Error::InvalidArgument(
                "opus coupled_stream_count must not exceed stream_count",
            ));
        }

        if mapping.mapping.len() != usize::from(channel_count) {
            return Err(Error::InvalidArgument(
                "opus channel mapping table length must equal channel count",
            ));
        }

        2 + mapping.mapping.len()
    };

    let mut packet = Vec::with_capacity(19 + extra_len);
    packet.extend_from_slice(b"OpusHead");
    packet.push(1);
    packet.push(channel_count);
    packet.extend_from_slice(&pre_skip.to_le_bytes());
    packet.extend_from_slice(&input_sample_rate.to_le_bytes());
    packet.extend_from_slice(&output_gain.to_le_bytes());
    packet.push(family);

    if family != 0 {
        let mapping = stream_mapping.unwrap();
        packet.push(mapping.stream_count);
        packet.push(mapping.coupled_stream_count);
        packet.extend_from_slice(&mapping.mapping);
    }

    Ok(packet)
}

pub(crate) fn build_opus_tags(vendor_string: &str, comments: &[String]) -> Result<Vec<u8>> {
    let vendor = vendor_string.as_bytes();
    let vendor_len = u32::try_from(vendor.len())
        .map_err(|_| Error::Unsupported("opus vendor string length exceeds u32"))?;
    let comment_count = u32::try_from(comments.len())
        .map_err(|_| Error::Unsupported("opus comment count exceeds u32"))?;

    let comments_len = comments.iter().try_fold(0_usize, |acc, comment| {
        let comment_len = comment.len();
        let _ = u32::try_from(comment_len)
            .map_err(|_| Error::Unsupported("opus comment length exceeds u32"))?;
        acc.checked_add(4 + comment_len).ok_or(Error::Unsupported(
            "opus comment header exceeds platform limits",
        ))
    })?;

    let capacity = 8_usize
        .checked_add(4)
        .and_then(|value| value.checked_add(vendor.len()))
        .and_then(|value| value.checked_add(4))
        .and_then(|value| value.checked_add(comments_len))
        .ok_or(Error::Unsupported(
            "opus comment header exceeds platform limits",
        ))?;

    let mut packet = Vec::with_capacity(capacity);
    packet.extend_from_slice(b"OpusTags");
    packet.extend_from_slice(&vendor_len.to_le_bytes());
    packet.extend_from_slice(vendor);
    packet.extend_from_slice(&comment_count.to_le_bytes());

    for comment in comments {
        let bytes = comment.as_bytes();
        let len = u32::try_from(bytes.len())
            .map_err(|_| Error::Unsupported("opus comment length exceeds u32"))?;
        packet.extend_from_slice(&len.to_le_bytes());
        packet.extend_from_slice(bytes);
    }

    Ok(packet)
}
