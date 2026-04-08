#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlacEncoderOptions {
    pub block_size: u16,
    pub max_fixed_order: u8,
    pub max_rice_partition_order: u8,
    pub stereo_decorrelation: bool,
}

impl Default for FlacEncoderOptions {
    fn default() -> Self {
        Self {
            block_size: 4096,
            max_fixed_order: 4,
            max_rice_partition_order: 6,
            stereo_decorrelation: true,
        }
    }
}
