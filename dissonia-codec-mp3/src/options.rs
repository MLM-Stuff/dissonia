#[derive(Debug, Clone, Copy)]
pub struct Mp3EncoderOptions {
    pub bitrate_bps: u32,
}

impl Default for Mp3EncoderOptions {
    fn default() -> Self {
        Self { bitrate_bps: 192_000 }
    }
}
