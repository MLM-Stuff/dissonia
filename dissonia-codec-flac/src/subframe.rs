use crate::bitwriter::BitWriter;
use crate::rice;

pub(crate) fn fixed_residuals(samples: &[i64], order: usize) -> Vec<i64> {
    let n = samples.len();
    if n <= order {
        return Vec::new();
    }
    let mut residuals = Vec::with_capacity(n - order);
    for i in order..n {
        let prediction = match order {
            0 => 0,
            1 => samples[i - 1],
            2 => 2 * samples[i - 1] - samples[i - 2],
            3 => 3 * samples[i - 1] - 3 * samples[i - 2] + samples[i - 3],
            4 => 4 * samples[i - 1] - 6 * samples[i - 2] + 4 * samples[i - 3] - samples[i - 4],
            _ => unreachable!("fixed predictor order must be 0–4"),
        };
        residuals.push(samples[i] - prediction);
    }
    residuals
}

fn estimate_fixed_bits(
    samples: &[i64],
    order: usize,
    bps: u8,
    block_size: usize,
    max_rice_order: u8,
) -> u64 {
    if samples.len() <= order {
        return u64::MAX;
    }
    let residuals = fixed_residuals(samples, order);
    let rice_order = rice::find_best_partition_order(&residuals, order, block_size, max_rice_order);
    let rice_bits = rice::estimate_partitioned_rice_bits(&residuals, order, rice_order);
    8 + u64::from(bps) * order as u64 + rice_bits
}

fn verbatim_bits(count: usize, bps: u8) -> u64 {
    8 + u64::from(bps) * count as u64
}

#[derive(Debug, Clone, Copy)]
enum Choice {
    Constant(i64),
    Verbatim,
    Fixed(usize),
}

pub(crate) fn encode_subframe(
    writer: &mut BitWriter,
    samples: &[i64],
    bps: u8,
    block_size: usize,
    max_fixed_order: u8,
    max_rice_order: u8,
) {
    let choice = select_subframe(samples, bps, block_size, max_fixed_order, max_rice_order);

    match choice {
        Choice::Constant(value) => {
            writer.write_bits(0x00, 8);
            writer.write_signed(value, bps);
        }
        Choice::Verbatim => {
            writer.write_bits(0x02, 8);
            for &s in samples {
                writer.write_signed(s, bps);
            }
        }
        Choice::Fixed(order) => {
            let header = u64::from((8 + order as u8) << 1);
            writer.write_bits(header, 8);

            for &s in &samples[..order] {
                writer.write_signed(s, bps);
            }

            let residuals = fixed_residuals(samples, order);
            let rice_order =
                rice::find_best_partition_order(&residuals, order, block_size, max_rice_order);
            rice::encode_partitioned_rice(writer, &residuals, order, rice_order);
        }
    }
}

fn select_subframe(
    samples: &[i64],
    bps: u8,
    block_size: usize,
    max_fixed_order: u8,
    max_rice_order: u8,
) -> Choice {
    if !samples.is_empty() {
        let val = samples[0];
        if samples.iter().all(|&s| s == val) {
            return Choice::Constant(val);
        }
    }

    let mut best_bits = verbatim_bits(samples.len(), bps);
    let mut best = Choice::Verbatim;

    let limit = (max_fixed_order.min(4) as usize).min(samples.len().saturating_sub(1));
    for order in 0..=limit {
        let bits = estimate_fixed_bits(samples, order, bps, block_size, max_rice_order);
        if bits < best_bits {
            best_bits = bits;
            best = Choice::Fixed(order);
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_order0_residuals_are_identity() {
        let samples = vec![10, 20, 30];
        let r = fixed_residuals(&samples, 0);
        assert_eq!(r, vec![10, 20, 30]);
    }

    #[test]
    fn fixed_order1_residuals_are_deltas() {
        let samples = vec![0, 5, 10, 15];
        let r = fixed_residuals(&samples, 1);
        assert_eq!(r, vec![5, 5, 5]);
    }

    #[test]
    fn fixed_order2_residuals_of_linear_are_zero() {
        let samples = vec![0, 10, 20, 30, 40];
        let r = fixed_residuals(&samples, 2);
        assert_eq!(r, vec![0, 0, 0]);
    }

    #[test]
    fn constant_block_selects_constant() {
        let mut w = BitWriter::new();
        let samples = vec![42_i64; 256];
        encode_subframe(&mut w, &samples, 16, 256, 4, 6);
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), 3);
        assert_eq!(bytes[0], 0x00);
    }
}
