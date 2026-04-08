#[derive(Debug)]
pub(crate) struct BitWriter {
    buf: Vec<u8>,
    current: u8,
    bits: u8,
}

impl BitWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            current: 0,
            bits: 0,
        }
    }

    pub fn with_capacity(bytes: usize) -> Self {
        Self {
            buf: Vec::with_capacity(bytes),
            current: 0,
            bits: 0,
        }
    }

    pub fn write_bits(&mut self, value: u64, n: u8) {
        debug_assert!(n <= 64);
        let mut remaining = n;
        while remaining > 0 {
            let space = 8 - self.bits;
            let chunk = remaining.min(space);
            let shift = remaining - chunk;
            let mask = if chunk == 64 {
                u64::MAX
            } else {
                (1_u64 << chunk) - 1
            };
            let bits_value = ((value >> shift) & mask) as u8;
            self.current |= bits_value << (space - chunk);
            self.bits += chunk;
            remaining -= chunk;
            if self.bits == 8 {
                self.buf.push(self.current);
                self.current = 0;
                self.bits = 0;
            }
        }
    }

    pub fn write_bit(&mut self, bit: bool) {
        self.write_bits(u64::from(bit), 1);
    }

    pub fn write_signed(&mut self, value: i64, bits: u8) {
        debug_assert!(bits > 0 && bits <= 64);
        let mask = if bits == 64 {
            u64::MAX
        } else {
            (1_u64 << bits) - 1
        };
        self.write_bits(value as u64 & mask, bits);
    }

    pub fn write_unary(&mut self, count: u32) {
        for _ in 0..count {
            self.write_bit(true);
        }
        self.write_bit(false);
    }

    pub fn write_utf8_u32(&mut self, value: u32) {
        if value < 0x80 {
            self.write_bits(u64::from(value), 8);
        } else if value < 0x800 {
            self.write_bits(u64::from(0xC0 | (value >> 6)), 8);
            self.write_bits(u64::from(0x80 | (value & 0x3F)), 8);
        } else if value < 0x1_0000 {
            self.write_bits(u64::from(0xE0 | (value >> 12)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 6) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | (value & 0x3F)), 8);
        } else if value < 0x20_0000 {
            self.write_bits(u64::from(0xF0 | (value >> 18)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 12) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 6) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | (value & 0x3F)), 8);
        } else if value < 0x400_0000 {
            self.write_bits(u64::from(0xF8 | (value >> 24)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 18) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 12) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 6) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | (value & 0x3F)), 8);
        } else {
            self.write_bits(u64::from(0xFC | (value >> 30)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 24) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 18) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 12) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | ((value >> 6) & 0x3F)), 8);
            self.write_bits(u64::from(0x80 | (value & 0x3F)), 8);
        }
    }

    pub fn pad_to_byte(&mut self) {
        if self.bits > 0 {
            self.buf.push(self.current);
            self.current = 0;
            self.bits = 0;
        }
    }

    pub fn is_byte_align(&self) -> bool {
        self.bits == 0
    }

    pub fn into_bytes(mut self) -> Vec<u8> {
        self.pad_to_byte();
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_single_bits() {
        let mut w = BitWriter::new();
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(true);
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(false);
        w.write_bit(true);
        w.write_bit(false);
        let bytes = w.into_bytes();
        assert_eq!(bytes, &[0b1011_0010]);
    }

    #[test]
    fn writes_multi_bit_values() {
        let mut w = BitWriter::new();
        w.write_bits(0x3FFE, 14);
        w.write_bits(0, 1);
        w.write_bits(0, 1);
        let bytes = w.into_bytes();
        assert_eq!(bytes, &[0xFF, 0xF8]);
    }

    #[test]
    fn pads_partial_byte() {
        let mut w = BitWriter::new();
        w.write_bits(0b110, 3);
        let bytes = w.into_bytes();
        assert_eq!(bytes, &[0b1100_0000]);
    }

    #[test]
    fn utf8_encodes_small_value() {
        let mut w = BitWriter::new();
        w.write_utf8_u32(0);
        assert_eq!(w.into_bytes(), &[0x00]);
    }

    #[test]
    fn utf8_encodes_two_byte_value() {
        let mut w = BitWriter::new();
        w.write_utf8_u32(0x80);
        let bytes = w.into_bytes();
        assert_eq!(bytes, &[0xC2, 0x80]);
    }
}
