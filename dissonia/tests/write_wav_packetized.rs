use std::io::Cursor;

use dissonia::prelude::*;

#[test]
fn writes_wav_with_packetized_pcm() -> Result<()> {
    let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
    let mut encoder = PcmEncoder::builder(spec).frames_per_packet(2).build()?;

    let cursor = Cursor::new(Vec::<u8>::new());
    let mut muxer = WavMuxer::new(cursor);

    let track = muxer.add_track(TrackSpec::new(
        encoder.codec_parameters().clone(),
        TimeBase::audio_sample_rate(spec.sample_rate),
    ))?;

    let samples: [i16; 12] = [
        0, 0, 1000, -1000, 2000, -2000, 3000, -3000, 4000, -4000, 5000, -5000,
    ];

    {
        let mut sink = muxer.track_writer(track);
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;
    }

    let summary = muxer.finalize()?;
    assert_eq!(summary.packet_count, 3);

    let cursor = muxer.into_inner();
    let wav = cursor.into_inner();

    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 24);
    assert_eq!(wav.len(), 68);

    Ok(())
}
