pub(crate) fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0_u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

pub(crate) fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0_u16;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc8_empty() {
        assert_eq!(crc8(&[]), 0);
    }

    #[test]
    fn crc16_empty() {
        assert_eq!(crc16(&[]), 0);
    }

    #[test]
    fn crc8_deterministic() {
        let data = [0xFF, 0xF8, 0x19, 0x12, 0x00];
        let a = crc8(&data);
        let b = crc8(&data);
        assert_eq!(a, b);
    }
}
