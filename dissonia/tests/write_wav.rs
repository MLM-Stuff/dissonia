use std::io::Cursor;

use dissonia::prelude::*;

#[test]
fn writes_a_valid_minimal_wav_file() -> Result<()> {
    let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
    let mut encoder = PcmEncoder::new(spec)?;

    let cursor = Cursor::new(Vec::<u8>::new());
    let mut muxer = WavMuxer::new(cursor);

    let track = muxer.add_track(TrackSpec::new(
        encoder.codec_parameters().clone(),
        TimeBase::audio_sample_rate(spec.sample_rate),
    ))?;

    let samples: [i16; 8] = [0, 0, 1000, -1000, 2000, -2000, 3000, -3000];

    {
        let mut sink = muxer.track_writer(track);
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;
    }

    let summary = muxer.finalize()?;
    assert_eq!(summary.packet_count, 1);
    assert_eq!(summary.bytes_written, Some(60));

    let cursor = muxer.into_inner();
    let wav = cursor.into_inner();

    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(u32::from_le_bytes(wav[16..20].try_into().unwrap()), 16);
    assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1);
    assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 48_000);
    assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 192_000);
    assert_eq!(u16::from_le_bytes(wav[32..34].try_into().unwrap()), 4);
    assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 16);
    assert_eq!(wav.len(), 60);

    Ok(())
}
