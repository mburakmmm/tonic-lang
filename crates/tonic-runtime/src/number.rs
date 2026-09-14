//! Correctly rounded integer ratio -> IEEE-754 binary64, without first rounding
//! each (possibly enormous) operand to f64. Quotient/remainder decides ties-even.
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
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
}
