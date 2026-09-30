//! Correctly rounded integer ratio -> IEEE-754 binary64, without first rounding
//! each (possibly enormous) operand to f64. Quotient/remainder decides ties-even.
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use tonic_core::diagnostic::{Diagnostic, Result};

pub(crate) fn ratio(x: BigInt, y: BigInt) -> Result<f64> {
    if y.is_zero() {
        return Err(Diagnostic::new("ZeroDivisionError", "division by zero"));
    }
    let negative = x.is_negative() != y.is_negative();
    let signed = |f: f64| if negative { -f } else { f };
    if x.is_zero() {
        return Ok(signed(0.0));
    }
    let (x, y) = (x.abs(), y.abs());
    let mut exponent = x.bits() as i64 - y.bits() as i64;
    if exponent >= 0 {
        if x < (&y << exponent as usize) {
            exponent -= 1;
        }
    } else if (&x << (-exponent) as usize) < y {
        exponent -= 1;
    }
    if exponent > 1023 {
        return Err(overflow());
    }
    if exponent < -1075 {
        return Ok(signed(0.0));
    }
    let shift = if exponent < -1022 {
        1074
    } else {
        52 - exponent
    };
    let (numerator, denominator) = if shift >= 0 {
        (x << shift as usize, y)
    } else {
        (x, y << (-shift) as usize)
    };
    let mut q = &numerator / &denominator;
    let rem = (&numerator % &denominator) << 1;
    if rem > denominator || (rem == denominator && (&q & BigInt::from(1)) != BigInt::from(0)) {
        q += 1;
    }
    let scale = -shift;
    let power = if scale >= -1022 {
        f64::from_bits(((scale + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (scale + 1074))
    };
    let result = q.to_u64().ok_or_else(overflow)? as f64 * power;
    if !result.is_finite() {
        return Err(overflow());
    }
    Ok(signed(result))
}

pub(crate) fn round_ratio(numerator: &BigInt, denominator: &BigInt) -> BigInt {
    debug_assert!(denominator.is_positive());
    let negative = numerator.is_negative();
    let numerator = numerator.abs();
    let mut quotient = &numerator / denominator;
    let remainder = numerator % denominator;
    let twice_remainder = remainder << 1usize;
    if twice_remainder > *denominator
        || (twice_remainder == *denominator && (&quotient & BigInt::one()) != BigInt::zero())
    {
        quotient += 1;
    }
    if negative {
        -quotient
    } else {
        quotient
    }
}

fn float_ratio(value: f64) -> (BigInt, BigInt) {
    debug_assert!(value.is_finite());
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (significand, binary_exponent) = if exponent == 0 {
        (fraction, -1074)
    } else {
        ((1u64 << 52) | fraction, exponent - 1023 - 52)
    };
    let mut numerator = BigInt::from(significand);
    let denominator = if binary_exponent >= 0 {
        numerator <<= binary_exponent as usize;
        BigInt::one()
    } else {
        BigInt::one() << (-binary_exponent) as usize
    };
    if negative {
        numerator = -numerator;
    }
    (numerator, denominator)
}

pub(crate) fn round_float_to_integer(value: f64) -> Result<BigInt> {
    if value.is_nan() {
        return Err(Diagnostic::new(
            "ValueError",
            "cannot convert float NaN to integer",
        ));
    }
    if value.is_infinite() {
        return Err(Diagnostic::new(
            "OverflowError",
            "cannot convert float infinity to integer",
        ));
    }
    let (numerator, denominator) = float_ratio(value);
    Ok(round_ratio(&numerator, &denominator))
}

pub(crate) fn round_float(value: f64, ndigits: &BigInt) -> Result<f64> {
    if !value.is_finite() {
        return Ok(value);
    }
    if ndigits > &BigInt::from(323) {
        return Ok(value);
    }
    if ndigits < &BigInt::from(-308) {
        return Ok(0.0f64.copysign(value));
    }
    let digits = ndigits
        .to_i32()
        .expect("bounded decimal digit count fits i32");
    let (numerator, denominator) = float_ratio(value);
    let ten = BigInt::from(10);
    if digits >= 0 {
        let scale = ten.pow(digits as u32);
        let rounded = round_ratio(&(numerator * &scale), &denominator);
        if rounded.is_zero() {
            return Ok(0.0f64.copysign(value));
        }
        ratio(rounded, scale)
    } else {
        let scale = ten.pow((-digits) as u32);
        let rounded = round_ratio(&numerator, &(denominator * &scale));
        if rounded.is_zero() {
            return Ok(0.0f64.copysign(value));
        }
        ratio(rounded * scale, BigInt::one())
    }
}
fn overflow() -> Diagnostic {
    Diagnostic::new(
        "OverflowError",
        "integer division result too large for float",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_ratios_and_subnormal_ties() {
        let huge = BigInt::from(1) << 4000usize;
        assert_eq!(ratio(huge.clone(), huge).unwrap(), 1.0);
        let denominator = BigInt::from(1) << 1075usize;
        assert_eq!(
            ratio(BigInt::from(1), denominator.clone())
                .unwrap()
                .to_bits(),
            0
        );
        assert_eq!(ratio(BigInt::from(3), denominator).unwrap().to_bits(), 2);
        assert_eq!(
            ratio(BigInt::from(0), BigInt::from(-1)).unwrap().to_bits(),
            (-0.0f64).to_bits()
        );
        assert_eq!(
            ratio(BigInt::from(1) << 1024, BigInt::from(1))
                .unwrap_err()
                .kind,
            "OverflowError"
        );
    }

    #[test]
    fn decimal_rounding_uses_exact_binary_ratio_and_ties_even() {
        assert_eq!(round_float_to_integer(2.5).unwrap(), BigInt::from(2));
        assert_eq!(round_float_to_integer(3.5).unwrap(), BigInt::from(4));
        assert_eq!(round_float(2.675, &BigInt::from(2)).unwrap(), 2.67);
        assert_eq!(round_float(1.005, &BigInt::from(2)).unwrap(), 1.0);
        assert_eq!(round_float(0.045, &BigInt::from(2)).unwrap(), 0.04);
        assert_eq!(round_float(25.0, &BigInt::from(-1)).unwrap(), 20.0);
        assert_eq!(round_float(35.0, &BigInt::from(-1)).unwrap(), 40.0);
        assert!(round_float(-2.5, &BigInt::from(-400))
            .unwrap()
            .is_sign_negative());
        assert_eq!(
            round_float_to_integer(f64::NAN).unwrap_err().kind,
            "ValueError"
        );
        assert_eq!(
            round_float_to_integer(f64::INFINITY).unwrap_err().kind,
            "OverflowError"
        );
    }
}
