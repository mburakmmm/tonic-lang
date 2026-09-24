//! Language-level hash normalization shared by `hash()` and dictionary buckets.
//!
//! These helpers deliberately consume logical values rather than heap addresses.
//! The concrete mixing algorithm is an implementation detail; equal Tonic values
//! must nevertheless produce equal results so custom `__hash__` implementations
//! can interoperate with builtin keys.
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::hash::{DefaultHasher, Hash, Hasher};

const HASH_MODULUS: i64 = (1_i64 << 61) - 1;

pub(crate) fn normalize_i64(value: i64) -> i64 {
    if value == -1 {
        -2
    } else {
        value
    }
}

pub(crate) fn normalize_bigint(value: &BigInt) -> i64 {
    if let Some(value) = value.to_i64() {
        return normalize_i64(value);
    }
    let modulus = BigInt::from(HASH_MODULUS);
    normalize_i64(
        (value % modulus)
            .to_i64()
            .expect("hash remainder fits the signed hash width"),
    )
}

pub(crate) fn hash_u64(value: u64) -> i64 {
    normalize_i64((value % HASH_MODULUS as u64) as i64)
}

pub(crate) fn hash_string(value: &str) -> i64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hash_u64(hasher.finish())
}

pub(crate) fn sequence_start() -> i64 {
    0x345678
}

pub(crate) fn sequence_step(accumulator: i64, item: i64) -> i64 {
    ((accumulator as i128 * 1_000_003 + item as i128).rem_euclid(HASH_MODULUS as i128)) as i64
}

pub(crate) fn sequence_finish(accumulator: i64, length: usize) -> i64 {
    normalize_i64(((accumulator as i128 + length as i128).rem_euclid(HASH_MODULUS as i128)) as i64)
}

pub(crate) fn range_length(start: i64, stop: i64, step: i64) -> i128 {
    let (start, stop, step) = (start as i128, stop as i128, step as i128);
    if step > 0 {
        if start >= stop {
            0
        } else {
            1 + (stop - 1 - start) / step
        }
    } else if start <= stop {
        0
    } else {
        1 + (start - 1 - stop) / -step
    }
}

pub(crate) fn hash_range(start: i64, stop: i64, step: i64) -> i64 {
    let length = range_length(start, stop, step);
    hash_range_parts(
        length,
        (length != 0).then_some(start),
        (length > 1).then_some(step),
    )
}

pub(crate) fn hash_range_parts(length: i128, start: Option<i64>, step: Option<i64>) -> i64 {
    let mut accumulator = sequence_start();
    accumulator = sequence_step(accumulator, normalize_bigint(&BigInt::from(length)));
    if let Some(start) = start {
        accumulator = sequence_step(accumulator, normalize_i64(start));
    }
    if let Some(step) = step {
        accumulator = sequence_step(accumulator, normalize_i64(step));
    }
    sequence_finish(
        accumulator,
        if length == 0 {
            1
        } else if length == 1 {
            2
        } else {
            3
        },
    )
}
