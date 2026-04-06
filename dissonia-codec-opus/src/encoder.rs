use dissonia_core::audio::{AudioBufferRef, AudioSpec, ChannelLayout, SampleFormat};
use dissonia_core::codecs::{CodecId, CodecParameters, Encoder, OpusStreamMapping, PacketSink};
use dissonia_core::packet::{EncodedPacket, PacketFlags};
use dissonia_core::units::Timestamp;
use dissonia_core::{Error, Result};

use mousiki::c_style_api::opus_encoder::{opus_encoder_ctl, OpusEncoderCtlRequest};
use mousiki::{
    Application as MousikiApplication, Bitrate as MousikiBitrate, Channels as MousikiChannels,
    Encoder as MousikiEncoder, EncoderBuilderError as MousikiEncoderBuilderError,
    FrameDuration as MousikiFrameDuration, OpusEncodeError as MousikiOpusEncodeError,
    OpusEncoderCtlError as MousikiOpusEncoderCtlError,
    OpusEncoderInitError as MousikiOpusEncoderInitError, Signal as MousikiSignal,
};

use crate::options::{
    OpusApplication, OpusBandwidth, OpusBitrate, OpusEncoderOptions, OpusFrameDuration, OpusSignal,
};

#[derive(Debug)]
pub struct OpusEncoderBuilder {
    spec: AudioSpec,
    options: OpusEncoderOptions,
}

impl OpusEncoderBuilder {
    #[must_use]
    pub fn new(spec: AudioSpec) -> Self {
        Self {
            spec,
            options: OpusEncoderOptions::default(),
        }
    }

    #[must_use]
    pub fn options(mut self, options: OpusEncoderOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn application(mut self, application: OpusApplication) -> Self {
        self.options.application = application;
        self
    }

    #[must_use]
    pub fn bitrate(mut self, bitrate: OpusBitrate) -> Self {
        self.options.bitrate = Some(bitrate);
        self
    }

    #[must_use]
    pub fn bitrate_bits(mut self, bits: u32) -> Self {
        self.options.bitrate = Some(OpusBitrate::Bits(bits));
        self
    }

    #[must_use]
    pub fn complexity(mut self, complexity: u8) -> Self {
        self.options.complexity = Some(complexity);
        self
    }

    #[must_use]
    pub fn vbr(mut self, enabled: bool) -> Self {
        self.options.vbr = Some(enabled);
        self
    }

    #[must_use]
    pub fn constrained_vbr(mut self, enabled: bool) -> Self {
        self.options.constrained_vbr = Some(enabled);
        self
    }

    #[must_use]
    pub fn max_bandwidth(mut self, bandwidth: OpusBandwidth) -> Self {
        self.options.max_bandwidth = Some(bandwidth);
        self
    }

    #[must_use]
    pub fn signal(mut self, signal: OpusSignal) -> Self {
        self.options.signal = Some(signal);
        self
    }

    #[must_use]
    pub fn inband_fec(mut self, enabled: bool) -> Self {
        self.options.inband_fec = Some(enabled);
        self
    }

    #[must_use]
    pub fn packet_loss_perc(mut self, percent: u8) -> Self {
        self.options.packet_loss_perc = Some(percent);
        self
    }

    #[must_use]
    pub fn dtx(mut self, enabled: bool) -> Self {
        self.options.dtx = Some(enabled);
        self
    }

    #[must_use]
    pub fn lsb_depth(mut self, bits: u8) -> Self {
        self.options.lsb_depth = Some(bits);
        self
    }

    #[must_use]
    pub fn frame_duration(mut self, frame_duration: OpusFrameDuration) -> Self {
        self.options.frame_duration = frame_duration;
        self
    }

    #[must_use]
    pub fn prediction_disabled(mut self, disabled: bool) -> Self {
        self.options.prediction_disabled = Some(disabled);
        self
    }

    #[must_use]
    pub fn mapping_family(mut self, family: u8) -> Self {
        self.options.mapping_family = Some(family);
        self
    }

    #[must_use]
    pub fn max_packet_bytes(mut self, bytes: usize) -> Self {
        self.options.max_packet_bytes = bytes;
        self
    }

    #[must_use]
    pub fn pad_flush(mut self, pad: bool) -> Self {
        self.options.pad_flush = pad;
        self
    }

    pub fn build(self) -> Result<OpusEncoder> {
        OpusEncoder::with_options(self.spec, self.options)
    }
}

#[derive(Debug)]
pub struct OpusEncoder {
    spec: AudioSpec,
    params: CodecParameters,
    options: OpusEncoderOptions,
    encoder: MousikiEncoder,
    channels: usize,
    frame_samples: Option<usize>,
    next_pts: u64,
    pending: PendingPcm,
}

#[derive(Debug)]
enum PendingPcm {
    I16(Vec<i16>),
    I24(Vec<i32>),
    F32(Vec<f32>),
}

impl PendingPcm {
    fn with_format(sample_format: SampleFormat) -> Result<Self> {
        match sample_format {
            SampleFormat::I16 => Ok(Self::I16(Vec::new())),
            SampleFormat::I24 => Ok(Self::I24(Vec::new())),
            SampleFormat::F32 => Ok(Self::F32(Vec::new())),
            SampleFormat::U8 | SampleFormat::I32 | SampleFormat::F64 => Err(Error::Unsupported(
                "opus encoder input sample format must be i16, i24, or f32",
            )),
            _ => unreachable!("SampleFormat is non-exhaustive but all variants handled"),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::I16(data) => data.len(),
            Self::I24(data) => data.len(),
            Self::F32(data) => data.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn clear(&mut self) {
        match self {
            Self::I16(data) => data.clear(),
            Self::I24(data) => data.clear(),
            Self::F32(data) => data.clear(),
        }
    }

    fn extend(&mut self, input: AudioBufferRef<'_>) -> Result<()> {
        match (self, input) {
            (Self::I16(dst), AudioBufferRef::I16(src)) => {
                dst.extend_from_slice(src);
                Ok(())
            }
            (Self::I24(dst), AudioBufferRef::I24(src)) => {
                dst.extend_from_slice(src);
                Ok(())
            }
            (Self::F32(dst), AudioBufferRef::F32(src)) => {
                dst.extend_from_slice(src);
                Ok(())
            }
            _ => Err(Error::InvalidState(
                "pending opus buffer format does not match encoder format",
            )),
        }
    }
}

impl OpusEncoder {
    pub fn new(spec: AudioSpec) -> Result<Self> {
        Self::builder(spec).build()
    }

    #[must_use]
    pub fn builder(spec: AudioSpec) -> OpusEncoderBuilder {
        OpusEncoderBuilder::new(spec)
    }

    pub fn with_options(spec: AudioSpec, options: OpusEncoderOptions) -> Result<Self> {
        validate_spec(spec)?;
        validate_options(options)?;

        let stream_mapping = resolve_stream_mapping(spec.channels, options.mapping_family)?;

        let (mousiki_channels, channels) = to_mousiki_channels(spec.channels)?;

        let mut builder = MousikiEncoder::builder(
            spec.sample_rate,
            mousiki_channels,
            to_mousiki_application(options.application),
        );

        if let Some(value) = options.bitrate {
            builder = builder.bitrate(to_mousiki_bitrate(value)?);
        }

        if let Some(value) = options.complexity {
            builder = builder.complexity(i32::from(value));
        }

        if let Some(value) = options.vbr {
            builder = builder.vbr(value);
        }

        if let Some(value) = options.constrained_vbr {
            builder = builder.vbr_constraint(value);
        }

        if let Some(value) = options.max_bandwidth {
            builder = builder.max_bandwidth(to_mousiki_bandwidth(value));
        }

        if let Some(value) = options.signal {
            builder = builder.signal(to_mousiki_signal(value));
        }

        if let Some(value) = options.inband_fec {
            builder = builder.inband_fec(value);
        }

        if let Some(value) = options.packet_loss_perc {
            builder = builder.packet_loss_perc(i32::from(value));
        }

        if let Some(value) = options.dtx {
            builder = builder.dtx(value);
        }

        if let Some(value) = options.lsb_depth {
            builder = builder.lsb_depth(i32::from(value));
        }

        builder = builder.frame_duration(to_mousiki_frame_duration(options.frame_duration));

        if let Some(value) = options.prediction_disabled {
            builder = builder.prediction_disabled(value);
        }

        let mut encoder = builder.build().map_err(map_builder_error)?;
        let encoder_delay = query_lookahead(&mut encoder)?;

        let frame_samples = frame_samples_for_duration(options.frame_duration, spec.sample_rate);

        let mut params = CodecParameters::new(CodecId::Opus, spec);
        params.sample_format = None;
        params.bit_depth = None;
        params.frame_samples = frame_samples
            .map(|value| {
                u32::try_from(value).map_err(|_| Error::Unsupported("opus frame size exceeds u32"))
            })
            .transpose()?;
        params.encoder_delay = encoder_delay;
        params.encoder_padding = 0;
        params.opus_stream_mapping = Some(stream_mapping);

        Ok(Self {
            spec,
            params,
            options,
            encoder,
            channels,
            frame_samples,
            next_pts: 0,
            pending: PendingPcm::with_format(spec.sample_format)?,
        })
    }

    #[must_use]
    pub const fn options(&self) -> OpusEncoderOptions {
        self.options
    }

    fn encode_internal(
        &mut self,
        input: AudioBufferRef<'_>,
        sink: &mut dyn PacketSink,
    ) -> Result<()> {
        if input.sample_format() != self.spec.sample_format {
            return Err(Error::InvalidArgument(
                "buffer sample format does not match encoder input spec",
            ));
        }

        let frame_count = frame_count(input.len(), self.channels)?;
        if frame_count == 0 {
            return Ok(());
        }

        match self.frame_samples {
            None => {
                let payload = self.encode_packet(input)?;
                self.emit_packet(payload, frame_count, 0, sink)
            }
            Some(_) => {
                self.pending.extend(input)?;
                self.drain_ready_packets(sink)
            }
        }
    }

    fn encode_packet(&mut self, input: AudioBufferRef<'_>) -> Result<Vec<u8>> {
        let mut packet = vec![0_u8; self.options.max_packet_bytes];

        let len = match input {
            AudioBufferRef::I16(data) => self.encoder.encode(data, &mut packet),
            AudioBufferRef::I24(data) => self.encoder.encode_24bit(data, &mut packet),
            AudioBufferRef::F32(data) => self.encoder.encode_float(data, &mut packet),
            AudioBufferRef::U8(_) | AudioBufferRef::I32(_) | AudioBufferRef::F64(_) => {
                return Err(Error::Unsupported(
                    "opus encoder input sample format must be i16, i24, or f32",
                ));
            }
            _ => unreachable!("AudioBufferRef is non-exhaustive but all variants handled"),
        }
        .map_err(map_encode_error)?;

        packet.truncate(len);
        Ok(packet)
    }

    fn drain_ready_packets(&mut self, sink: &mut dyn PacketSink) -> Result<()> {
        let Some(frame_samples) = self.frame_samples else {
            return Ok(());
        };

        let packet_samples = frame_samples
            .checked_mul(self.channels)
            .ok_or(Error::Unsupported(
                "opus packet sample count exceeds platform limits",
            ))?;

        let frame_samples_u64 = u64::try_from(frame_samples)
            .map_err(|_| Error::Unsupported("opus frame size exceeds u64"))?;

        let max_packet_bytes = self.options.max_packet_bytes;
        let mut payloads = Vec::<Vec<u8>>::new();

        {
            let encoder = &mut self.encoder;
            let pending = &mut self.pending;
            let mut packet = vec![0_u8; max_packet_bytes];

            match pending {
                PendingPcm::I16(data) => {
                    while data.len() >= packet_samples {
                        let len = encoder
                            .encode(&data[..packet_samples], &mut packet)
                            .map_err(map_encode_error)?;
                        payloads.push(packet[..len].to_vec());
                        data.drain(..packet_samples);
                    }
                }
                PendingPcm::I24(data) => {
                    while data.len() >= packet_samples {
                        let len = encoder
                            .encode_24bit(&data[..packet_samples], &mut packet)
                            .map_err(map_encode_error)?;
                        payloads.push(packet[..len].to_vec());
                        data.drain(..packet_samples);
                    }
                }
                PendingPcm::F32(data) => {
                    while data.len() >= packet_samples {
                        let len = encoder
                            .encode_float(&data[..packet_samples], &mut packet)
                            .map_err(map_encode_error)?;
                        payloads.push(packet[..len].to_vec());
                        data.drain(..packet_samples);
                    }
                }
            }
        }

        for payload in payloads {
            self.emit_packet(payload, frame_samples_u64, 0, sink)?;
        }

        Ok(())
    }

    fn emit_packet(
        &mut self,
        payload: Vec<u8>,
        frame_count: u64,
        trim_end: u32,
        sink: &mut dyn PacketSink,
    ) -> Result<()> {
        let pts = Timestamp::new(self.next_pts);
        self.next_pts = self
            .next_pts
            .checked_add(frame_count)
            .ok_or(Error::InvalidState("timestamp overflow"))?;

        let mut packet = EncodedPacket::new(payload);
        packet.pts = Some(pts);
        packet.dts = Some(pts);
        packet.duration = Some(frame_count);
        packet.trim_end = trim_end;
        packet.flags = PacketFlags::NONE;

        sink.write_packet(packet)
    }
}

impl Encoder for OpusEncoder {
    fn codec_id(&self) -> CodecId {
        self.params.codec
    }

    fn input_spec(&self) -> AudioSpec {
        self.spec
    }

    fn codec_parameters(&self) -> &CodecParameters {
        &self.params
    }

    fn encode(&mut self, input: AudioBufferRef<'_>, sink: &mut dyn PacketSink) -> Result<()> {
        self.encode_internal(input, sink)
    }

    fn flush(&mut self, sink: &mut dyn PacketSink) -> Result<()> {
        self.drain_ready_packets(sink)?;

        if self.pending.is_empty() {
            return Ok(());
        }

        let Some(frame_samples) = self.frame_samples else {
            return Err(Error::InvalidState(
                "cannot flush partial opus data when frame duration is automatic",
            ));
        };

        let packet_samples = frame_samples
            .checked_mul(self.channels)
            .ok_or(Error::Unsupported(
                "opus packet sample count exceeds platform limits",
            ))?;

        let pending_samples = self.pending.len();

        if pending_samples % self.channels != 0 {
            return Err(Error::InvalidState(
                "pending opus samples are not aligned to complete frames",
            ));
        }

        if pending_samples >= packet_samples {
            return Err(Error::InvalidState(
                "pending opus samples should have been drained before flush padding",
            ));
        }

        if !self.options.pad_flush {
            return Err(Error::InvalidState(
                "cannot flush partial opus frame without padding",
            ));
        }

        let pad_samples = packet_samples
            .checked_sub(pending_samples)
            .ok_or(Error::InvalidState("opus flush padding underflow"))?;

        let trim_end_frames = u32::try_from(pad_samples / self.channels)
            .map_err(|_| Error::Unsupported("opus trim_end exceeds u32"))?;

        let frame_samples_u64 = u64::try_from(frame_samples)
            .map_err(|_| Error::Unsupported("opus frame size exceeds u64"))?;

        let payload = {
            let encoder = &mut self.encoder;
            let pending = &mut self.pending;
            let mut packet = vec![0_u8; self.options.max_packet_bytes];

            match pending {
                PendingPcm::I16(data) => {
                    data.resize(packet_samples, 0);
                    let len = encoder
                        .encode(&data[..], &mut packet)
                        .map_err(map_encode_error)?;
                    data.clear();
                    packet[..len].to_vec()
                }
                PendingPcm::I24(data) => {
                    data.resize(packet_samples, 0);
                    let len = encoder
                        .encode_24bit(&data[..], &mut packet)
                        .map_err(map_encode_error)?;
                    data.clear();
                    packet[..len].to_vec()
                }
                PendingPcm::F32(data) => {
                    data.resize(packet_samples, 0.0);
                    let len = encoder
                        .encode_float(&data[..], &mut packet)
                        .map_err(map_encode_error)?;
                    data.clear();
                    packet[..len].to_vec()
                }
            }
        };

        self.emit_packet(payload, frame_samples_u64, trim_end_frames, sink)
    }

    fn reset(&mut self) -> Result<()> {
        self.next_pts = 0;
        self.pending.clear();
        self.encoder.reset_state().map_err(map_ctl_error)
    }
}

fn validate_spec(spec: AudioSpec) -> Result<()> {
    if !matches!(spec.sample_rate, 8_000 | 12_000 | 16_000 | 24_000 | 48_000) {
        return Err(Error::Unsupported(
            "opus encoder input sample rate must be one of 8000, 12000, 16000, 24000, or 48000",
        ));
    }

    match spec.channels {
        channels if channels == ChannelLayout::MONO => {}
        channels if channels == ChannelLayout::STEREO => {}
        _ => {
            return Err(Error::Unsupported(
                "opus top-level encoder currently supports only mono or stereo channel layouts",
            ));
        }
    }

    match spec.sample_format {
        SampleFormat::I16 | SampleFormat::I24 | SampleFormat::F32 => Ok(()),
        SampleFormat::U8 | SampleFormat::I32 | SampleFormat::F64 => Err(Error::Unsupported(
            "opus encoder input sample format must be i16, i24, or f32",
        )),
        _ => unreachable!("SampleFormat is non-exhaustive but all variants handled"),
    }
}

fn validate_options(options: OpusEncoderOptions) -> Result<()> {
    if options.max_packet_bytes == 0 {
        return Err(Error::InvalidArgument(
            "opus max_packet_bytes must be greater than zero",
        ));
    }

    if let Some(OpusBitrate::Bits(0)) = options.bitrate {
        return Err(Error::InvalidArgument(
            "opus bitrate must be greater than zero",
        ));
    }

    if let Some(value) = options.complexity {
        if value > 10 {
            return Err(Error::InvalidArgument(
                "opus complexity must be in the range 0..=10",
            ));
        }
    }

    if let Some(value) = options.packet_loss_perc {
        if value > 100 {
            return Err(Error::InvalidArgument(
                "opus packet_loss_perc must be in the range 0..=100",
            ));
        }
    }

    if let Some(value) = options.lsb_depth {
        if value == 0 {
            return Err(Error::InvalidArgument(
                "opus lsb_depth must be greater than zero",
            ));
        }
    }

    Ok(())
}

fn resolve_stream_mapping(
    layout: ChannelLayout,
    requested_family: Option<u8>,
) -> Result<OpusStreamMapping> {
    match requested_family.unwrap_or(0) {
        0 => family0_stream_mapping(layout),
        family => Err(Error::Unsupported(match family {
            1 => "opus surround/family-1 metadata is supported in CodecParameters and Ogg headers, but the multistream encoder backend is not wired yet",
            255 => "opus family-255 metadata is supported in CodecParameters and Ogg headers, but the multistream encoder backend is not wired yet",
            _ => "requested opus mapping family is not supported by the current encoder implementation",
        })),
    }
}

fn family0_stream_mapping(layout: ChannelLayout) -> Result<OpusStreamMapping> {
    if layout == ChannelLayout::MONO {
        Ok(OpusStreamMapping::new(0, 1, 0, Box::<[u8]>::default()))
    } else if layout == ChannelLayout::STEREO {
        Ok(OpusStreamMapping::new(0, 1, 1, Box::<[u8]>::default()))
    } else {
        Err(Error::Unsupported(
            "opus mapping family 0 supports only mono or stereo",
        ))
    }
}

fn frame_count(sample_count: usize, channels: usize) -> Result<u64> {
    if channels == 0 {
        return Err(Error::InvalidState("encoder has zero channels"));
    }

    if sample_count % channels != 0 {
        return Err(Error::InvalidArgument(
            "input buffer sample count is not divisible by channel count",
        ));
    }

    u64::try_from(sample_count / channels)
        .map_err(|_| Error::Unsupported("frame count exceeds u64"))
}

fn frame_samples_for_duration(
    frame_duration: OpusFrameDuration,
    sample_rate: u32,
) -> Option<usize> {
    let samples = match frame_duration {
        OpusFrameDuration::Auto => return None,
        OpusFrameDuration::Ms2_5 => sample_rate / 400,
        OpusFrameDuration::Ms5 => sample_rate / 200,
        OpusFrameDuration::Ms10 => sample_rate / 100,
        OpusFrameDuration::Ms20 => sample_rate / 50,
        OpusFrameDuration::Ms40 => sample_rate / 25,
        OpusFrameDuration::Ms60 => (sample_rate * 3) / 50,
        OpusFrameDuration::Ms80 => (sample_rate * 2) / 25,
        OpusFrameDuration::Ms100 => sample_rate / 10,
        OpusFrameDuration::Ms120 => (sample_rate * 3) / 25,
    };

    Some(samples as usize)
}

fn to_mousiki_channels(layout: ChannelLayout) -> Result<(MousikiChannels, usize)> {
    if layout == ChannelLayout::MONO {
        Ok((MousikiChannels::Mono, 1))
    } else if layout == ChannelLayout::STEREO {
        Ok((MousikiChannels::Stereo, 2))
    } else {
        Err(Error::Unsupported(
            "opus top-level encoder currently supports only mono or stereo channel layouts",
        ))
    }
}

const fn to_mousiki_application(value: OpusApplication) -> MousikiApplication {
    match value {
        OpusApplication::Voip => MousikiApplication::Voip,
        OpusApplication::Audio => MousikiApplication::Audio,
        OpusApplication::LowDelay => MousikiApplication::LowDelay,
    }
}

fn to_mousiki_bitrate(value: OpusBitrate) -> Result<MousikiBitrate> {
    match value {
        OpusBitrate::Auto => Ok(MousikiBitrate::Auto),
        OpusBitrate::Max => Ok(MousikiBitrate::Max),
        OpusBitrate::Bits(bits) => {
            let bits =
                i32::try_from(bits).map_err(|_| Error::Unsupported("opus bitrate exceeds i32"))?;
            Ok(MousikiBitrate::Bits(bits))
        }
    }
}

const fn to_mousiki_bandwidth(value: OpusBandwidth) -> mousiki::Bandwidth {
    match value {
        OpusBandwidth::Narrowband => mousiki::Bandwidth::Narrowband,
        OpusBandwidth::Mediumband => mousiki::Bandwidth::Mediumband,
        OpusBandwidth::Wideband => mousiki::Bandwidth::Wideband,
        OpusBandwidth::Superwideband => mousiki::Bandwidth::Superwideband,
        OpusBandwidth::Fullband => mousiki::Bandwidth::Fullband,
    }
}

const fn to_mousiki_signal(value: OpusSignal) -> MousikiSignal {
    match value {
        OpusSignal::Auto => MousikiSignal::Auto,
        OpusSignal::Voice => MousikiSignal::Voice,
        OpusSignal::Music => MousikiSignal::Music,
    }
}

const fn to_mousiki_frame_duration(value: OpusFrameDuration) -> MousikiFrameDuration {
    match value {
        OpusFrameDuration::Auto => MousikiFrameDuration::Auto,
        OpusFrameDuration::Ms2_5 => MousikiFrameDuration::Ms2_5,
        OpusFrameDuration::Ms5 => MousikiFrameDuration::Ms5,
        OpusFrameDuration::Ms10 => MousikiFrameDuration::Ms10,
        OpusFrameDuration::Ms20 => MousikiFrameDuration::Ms20,
        OpusFrameDuration::Ms40 => MousikiFrameDuration::Ms40,
        OpusFrameDuration::Ms60 => MousikiFrameDuration::Ms60,
        OpusFrameDuration::Ms80 => MousikiFrameDuration::Ms80,
        OpusFrameDuration::Ms100 => MousikiFrameDuration::Ms100,
        OpusFrameDuration::Ms120 => MousikiFrameDuration::Ms120,
    }
}

fn query_lookahead(encoder: &mut MousikiEncoder) -> Result<u32> {
    let mut lookahead = 0_i32;
    opus_encoder_ctl(
        encoder.as_raw_mut(),
        OpusEncoderCtlRequest::GetLookahead(&mut lookahead),
    )
    .map_err(map_ctl_error)?;

    u32::try_from(lookahead)
        .map_err(|_| Error::InvalidState("opus encoder reported a negative lookahead"))
}

fn map_builder_error(error: MousikiEncoderBuilderError) -> Error {
    match error {
        MousikiEncoderBuilderError::Init(error) => map_init_error(error),
        MousikiEncoderBuilderError::Ctl(error) => map_ctl_error(error),
    }
}

fn map_init_error(error: MousikiOpusEncoderInitError) -> Error {
    match error {
        MousikiOpusEncoderInitError::BadArgument => {
            Error::InvalidArgument("invalid opus encoder initialization arguments")
        }
        MousikiOpusEncoderInitError::SilkInit | MousikiOpusEncoderInitError::CeltInit => {
            Error::InvalidState("failed to initialize opus encoder internals")
        }
    }
}

fn map_ctl_error(error: MousikiOpusEncoderCtlError) -> Error {
    match error {
        MousikiOpusEncoderCtlError::BadArgument => {
            Error::InvalidArgument("invalid opus encoder control value")
        }
        MousikiOpusEncoderCtlError::Unimplemented => {
            Error::Unsupported("opus encoder control is unimplemented")
        }
        MousikiOpusEncoderCtlError::Silk(_) => {
            Error::InvalidState("opus silk control operation failed")
        }
        MousikiOpusEncoderCtlError::InternalError => {
            Error::InvalidState("opus encoder internal error")
        }
    }
}

fn map_encode_error(error: MousikiOpusEncodeError) -> Error {
    match error {
        MousikiOpusEncodeError::BadArgument => {
            Error::InvalidArgument("invalid opus input or frame size")
        }
        MousikiOpusEncodeError::BufferTooSmall => {
            Error::InvalidArgument("opus packet buffer too small")
        }
        MousikiOpusEncodeError::InternalError => Error::InvalidState("opus encoder internal error"),
        MousikiOpusEncodeError::Unimplemented => {
            Error::Unsupported("requested opus encode path is unimplemented")
        }
        MousikiOpusEncodeError::Silk(_) => Error::InvalidState("opus silk encode operation failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dissonia_core::audio::{ChannelLayout, SampleFormat};
    use dissonia_core::codecs::VecPacketSink;

    #[test]
    fn encodes_one_stereo_i16_packet() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let mut encoder = OpusEncoder::new(spec)?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0_i16; 960 * 2];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].pts, Some(Timestamp::new(0)));
        assert_eq!(packets[0].duration, Some(960));
        assert_eq!(packets[0].trim_end, 0);
        assert!(!packets[0].data.is_empty());

        Ok(())
    }

    #[test]
    fn packetizes_multiple_frames_from_one_input_buffer() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let mut encoder = OpusEncoder::new(spec)?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0_i16; 1_920 * 2];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].pts, Some(Timestamp::new(0)));
        assert_eq!(packets[1].pts, Some(Timestamp::new(960)));
        assert_eq!(packets[0].duration, Some(960));
        assert_eq!(packets[1].duration, Some(960));

        Ok(())
    }

    #[test]
    fn flush_pads_partial_packet_and_sets_trim_end() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let mut encoder = OpusEncoder::new(spec)?;
        let mut sink = VecPacketSink::new();

        let samples = vec![0_i16; 480 * 2];
        encoder.encode(AudioBufferRef::I16(&samples), &mut sink)?;
        encoder.flush(&mut sink)?;

        let packets = sink.into_inner();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].pts, Some(Timestamp::new(0)));
        assert_eq!(packets[0].duration, Some(960));
        assert_eq!(packets[0].trim_end, 480);

        Ok(())
    }

    #[test]
    fn rejects_surround_layout() {
        let spec = AudioSpec::new(48_000, ChannelLayout::SURROUND_5_1, SampleFormat::I16);
        let error = OpusEncoder::new(spec).unwrap_err();

        match error {
            Error::Unsupported(_) => {}
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn fills_encoder_delay_from_lookahead() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let encoder = OpusEncoder::new(spec)?;

        assert!(encoder.codec_parameters().encoder_delay > 0);

        Ok(())
    }

    #[test]
    fn fills_family0_stream_mapping_metadata() -> Result<()> {
        let spec = AudioSpec::new(48_000, ChannelLayout::STEREO, SampleFormat::I16);
        let encoder = OpusEncoder::new(spec)?;

        let mapping = encoder
            .codec_parameters()
            .opus_stream_mapping
            .as_ref()
            .unwrap();

        assert_eq!(mapping.family, 0);
        assert_eq!(mapping.stream_count, 1);
        assert_eq!(mapping.coupled_stream_count, 1);
        assert!(mapping.mapping.is_empty());

        Ok(())
    }
}
