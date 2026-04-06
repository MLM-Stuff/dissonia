#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PcmEncoderOptions {
    pub frames_per_packet: Option<u32>,
}
