use std::io::Cursor;

use dissonia::prelude::*;

#[test]
fn writes_ogg_opus_with_preskip_from_encoder_delay() -> Result<()> {
    let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
    let mut encoder = OpusEncoder::new(spec)?;
    assert!(encoder.codec_parameters().encoder_delay > 0);

    let cursor = Cursor::new(Vec::<u8>::new());
    let mut muxer = OggOpusMuxer::new(cursor);

    let track = muxer.add_track(TrackSpec::new(
        encoder.codec_parameters().clone(),
        TimeBase::audio_sample_rate(spec.sample_rate),
    ))?;

    let samples = vec![0_i16; 960 * 2];

    {
        let mut sink = muxer.track_writer(track);
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;
    }

    let summary = muxer.finalize()?;
    assert_eq!(summary.packet_count, 1);
    assert!(summary.bytes_written.is_some());

    let ogg = muxer.into_inner().into_inner();
    let pages = split_pages(&ogg);

    assert_eq!(pages.len(), 3);

    let id_header = page_payload(pages[0]);
    assert!(id_header.starts_with(b"OpusHead"));
    assert_eq!(id_header[8], 1);
    assert_eq!(id_header[9], 2);

    let pre_skip = u16::from_le_bytes(id_header[10..12].try_into().unwrap());
    assert_eq!(
        u32::from(pre_skip),
        encoder.codec_parameters().encoder_delay
    );

    let input_sample_rate = u32::from_le_bytes(id_header[12..16].try_into().unwrap());
    assert_eq!(input_sample_rate, 48_000);

    assert!(page_payload(pages[1]).starts_with(b"OpusTags"));
    assert_eq!(page_granule_position(pages[2]), 960);

    Ok(())
}

#[test]
fn writes_ogg_opus_with_scaled_preskip_for_24khz_input() -> Result<()> {
    let spec = AudioSpec::new(24_000, ChannelLayout::MONO, SampleFormat::I16);
    let mut encoder = OpusEncoder::new(spec)?;
    assert!(encoder.codec_parameters().encoder_delay > 0);

    let expected_pre_skip =
        scale_encoder_delay_to_preskip(encoder.codec_parameters().encoder_delay, spec.sample_rate);

    let cursor = Cursor::new(Vec::<u8>::new());
    let mut muxer = OggOpusMuxer::new(cursor);

    let track = muxer.add_track(TrackSpec::new(
        encoder.codec_parameters().clone(),
        TimeBase::audio_sample_rate(spec.sample_rate),
    ))?;

    let samples = vec![0_i16; 480];

    {
        let mut sink = muxer.track_writer(track);
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;
    }

    let summary = muxer.finalize()?;
    assert_eq!(summary.packet_count, 1);
    assert!(summary.bytes_written.is_some());

    let ogg = muxer.into_inner().into_inner();
    let pages = split_pages(&ogg);

    assert_eq!(pages.len(), 3);

    let id_header = page_payload(pages[0]);
    assert!(id_header.starts_with(b"OpusHead"));
    assert_eq!(id_header[8], 1);
    assert_eq!(id_header[9], 1);

    let pre_skip = u16::from_le_bytes(id_header[10..12].try_into().unwrap());
    assert_eq!(u32::from(pre_skip), expected_pre_skip);

    let input_sample_rate = u32::from_le_bytes(id_header[12..16].try_into().unwrap());
    assert_eq!(input_sample_rate, 24_000);

    assert_eq!(page_granule_position(pages[2]), 960);

    Ok(())
}

fn scale_encoder_delay_to_preskip(encoder_delay: u32, sample_rate: u32) -> u32 {
    assert!(sample_rate != 0);
    let numerator = u128::from(encoder_delay) * 48_000;
    let denominator = u128::from(sample_rate);
    assert_eq!(numerator % denominator, 0);

    u32::try_from(numerator / denominator).unwrap()
}

fn split_pages(bytes: &[u8]) -> Vec<&[u8]> {
    let mut pages = Vec::new();
    let mut offset = 0_usize;

    while offset < bytes.len() {
        assert_eq!(&bytes[offset..offset + 4], b"OggS");

        let page_segments = usize::from(bytes[offset + 26]);
        let segment_table_start = offset + 27;
        let segment_table_end = segment_table_start + page_segments;
        let body_len = bytes[segment_table_start..segment_table_end]
            .iter()
            .map(|&value| usize::from(value))
            .sum::<usize>();

        let page_end = segment_table_end + body_len;
        pages.push(&bytes[offset..page_end]);
        offset = page_end;
    }

    pages
}

fn page_granule_position(page: &[u8]) -> u64 {
    u64::from_le_bytes(page[6..14].try_into().unwrap())
}

fn page_payload(page: &[u8]) -> &[u8] {
    let page_segments = usize::from(page[26]);
    &page[27 + page_segments..]
}
