//! Bounded numeric conversions used by rendering and timestamp helpers.

#[must_use]
pub fn u128_to_u64_saturating(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[must_use]
pub fn usize_to_u16_saturating(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

#[must_use]
pub fn usize_to_u32_saturating(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[must_use]
pub fn usize_to_isize_saturating(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

#[must_use]
pub fn usize_to_i16_saturating(value: usize) -> i16 {
    i16::try_from(value).unwrap_or(i16::MAX)
}

#[must_use]
pub fn u16_to_i16_saturating(value: u16) -> i16 {
    i16::try_from(value).unwrap_or(i16::MAX)
}

#[must_use]
pub fn i16_to_u16_saturating(value: i16) -> u16 {
    u16::try_from(value).unwrap_or(0)
}

#[must_use]
pub fn i32_to_u16_saturating(value: i32) -> u16 {
    if value < 0 {
        0
    } else {
        u16::try_from(value).unwrap_or(u16::MAX)
    }
}

#[must_use]
pub fn offset_index(current: usize, len: usize, delta: isize) -> usize {
    let max = len.saturating_sub(1);
    if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta.unsigned_abs()).min(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_conversions_saturate_at_target_bounds() {
        assert_eq!(usize_to_u16_saturating(usize::MAX), u16::MAX);
        assert_eq!(usize_to_u32_saturating(usize::MAX), u32::MAX);
        assert_eq!(u128_to_u64_saturating(u128::MAX), u64::MAX);
    }

    #[test]
    fn offset_index_clamps_to_collection_bounds() {
        assert_eq!(offset_index(2, 5, -10), 0);
        assert_eq!(offset_index(2, 5, 10), 4);
        assert_eq!(offset_index(2, 5, 1), 3);
    }
}
