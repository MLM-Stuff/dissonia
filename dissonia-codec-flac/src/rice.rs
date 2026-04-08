use crate::bitwriter::BitWriter;

fn fold(value: i64) -> u64 {
    if value >= 0 {
        (value as u64) << 1
    } else {
        (((-value) as u64) << 1) - 1
    }
}

pub(crate) fn estimate_rice_bits(residuals: &[i64], rice_param: u8) -> u64 {
    let param = u64::from(rice_param);
    let mut total = 0_u64;
    for &r in residuals {
        let folded = fold(r);
        total += (folded >> param) + 1 + param;
    }
    total
}

pub(crate) fn find_best_rice_param(residuals: &[i64]) -> u8 {
    if residuals.is_empty() {
        return 0;
    }
    let mut best_param = 0_u8;
    let mut best_bits = estimate_rice_bits(residuals, 0);
    for param in 1..=14_u8 {
        let bits = estimate_rice_bits(residuals, param);
        if bits < best_bits {
            best_bits = bits;
            best_param = param;
        }
    }
    best_param
}

fn write_rice_residuals(writer: &mut BitWriter, residuals: &[i64], rice_param: u8) {
    for &r in residuals {
        let folded = fold(r);
        let quotient = (folded >> rice_param) as u32;
        writer.write_unary(quotient);
        if rice_param > 0 {
            let remainder = folded & ((1_u64 << rice_param) - 1);
            writer.write_bits(remainder, rice_param);
        }
    }
}

pub(crate) fn estimate_partitioned_rice_bits(
    residuals: &[i64],
    predictor_order: usize,
    partition_order: u8,
) -> u64 {
    let num_partitions = 1_usize << partition_order;
    let total_samples = residuals.len() + predictor_order;
    let partition_size = total_samples / num_partitions;

    let mut total = 6_u64;
    let mut offset = 0_usize;

    for i in 0..num_partitions {
        let count = if i == 0 {
            partition_size - predictor_order
        } else {
            partition_size
        };
        let partition = &residuals[offset..offset + count];
        let param = find_best_rice_param(partition);
        total += 4 + estimate_rice_bits(partition, param);
        offset += count;
    }

    total
}

pub(crate) fn find_best_partition_order(
    residuals: &[i64],
    predictor_order: usize,
    block_size: usize,
    max_order: u8,
) -> u8 {
    let mut best_order = 0_u8;
    let mut best_bits = u64::MAX;

    for order in 0..=max_order {
        let num_partitions = 1_usize << order;
        if block_size % num_partitions != 0 {
            continue;
        }
        let partition_size = block_size / num_partitions;
        if partition_size <= predictor_order {
            continue;
        }
        let bits = estimate_partitioned_rice_bits(residuals, predictor_order, order);
        if bits < best_bits {
            best_bits = bits;
            best_order = order;
        }
    }

    best_order
}

pub(crate) fn encode_partitioned_rice(
    writer: &mut BitWriter,
    residuals: &[i64],
    predictor_order: usize,
    partition_order: u8,
) {
    let num_partitions = 1_usize << partition_order;
    let total_samples = residuals.len() + predictor_order;
    let partition_size = total_samples / num_partitions;

    writer.write_bits(0, 2);
    writer.write_bits(u64::from(partition_order), 4);

    let mut offset = 0_usize;
    for i in 0..num_partitions {
        let count = if i == 0 {
            partition_size - predictor_order
        } else {
            partition_size
        };
        let partition = &residuals[offset..offset + count];
        let param = find_best_rice_param(partition);
        writer.write_bits(u64::from(param), 4);
        write_rice_residuals(writer, partition, param);
        offset += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_maps_correctly() {
        assert_eq!(fold(0), 0);
        assert_eq!(fold(1), 2);
        assert_eq!(fold(-1), 1);
        assert_eq!(fold(2), 4);
        assert_eq!(fold(-2), 3);
    }

    #[test]
    fn best_param_for_zeros_is_zero() {
        let residuals = vec![0_i64; 64];
        assert_eq!(find_best_rice_param(&residuals), 0);
    }
}
