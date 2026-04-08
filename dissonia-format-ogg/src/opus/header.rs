use dissonia_common::vorbis::VorbisComments;
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

pub(crate) fn build_opus_tags(comments: &VorbisComments) -> Result<Vec<u8>> {
    let comment_payload = comments
        .encode()
        .ok_or(Error::Unsupported("opus comment header exceeds u32 limits"))?;

    let capacity = 8_usize
        .checked_add(comment_payload.len())
        .ok_or(Error::Unsupported(
            "opus comment header exceeds platform limits",
        ))?;

    let mut packet = Vec::with_capacity(capacity);
    packet.extend_from_slice(b"OpusTags");
    packet.extend_from_slice(&comment_payload);

    Ok(packet)
}
