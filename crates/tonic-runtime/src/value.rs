/// Internal representation, deliberately not the public native ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct Value(u64);
impl Value {
    pub const NONE: Self = Self(4);
    pub const UNBOUND: Self = Self(5);
    pub const NOT_IMPLEMENTED: Self = Self(6);
    pub(crate) fn raw(self) -> u64 {
        self.0
    }
    pub(crate) fn from_jit(raw: u64) -> Self {
        Self(raw)
    }
    pub const MIN_INT: i64 = -(1_i64 << 60);
    pub const MAX_INT: i64 = (1_i64 << 60) - 1;
    pub fn int(n: i64) -> Option<Self> {
        if (Self::MIN_INT..=Self::MAX_INT).contains(&n) {
            Some(Self(((n as u64) << 3) | 1))
        } else {
            None
        }
    }
    pub fn as_int(self) -> Option<i64> {
        if self.0 & 7 == 1 {
            Some((self.0 as i64) >> 3)
        } else {
            None
        }
    }
    pub fn bool(b: bool) -> Self {
        Self(if b { 3 } else { 2 })
    }
    pub fn as_bool(self) -> Option<bool> {
        match self.0 {
            2 => Some(false),
            3 => Some(true),
            _ => None,
        }
    }
    pub const MAX_GENERATION: u32 = (1 << 29) - 1;
    pub fn heap(index: u32, generation: u32) -> Self {
        debug_assert!(generation > 0 && generation <= Self::MAX_GENERATION);
        Self(((generation as u64) << 35) | ((index as u64) << 3))
    }
    pub fn heap_index(self) -> Option<usize> {
        if self.0 & 7 == 0 {
            Some(((self.0 >> 3) as u32) as usize)
        } else {
            None
        }
    }
    pub fn generation(self) -> u32 {
        (self.0 >> 35) as u32
    }
    pub fn integer(self) -> Option<i64> {
        self.as_int().or_else(|| self.as_bool().map(i64::from))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoding() {
        assert_eq!(std::mem::size_of::<Value>(), 8);
        for n in [Value::MIN_INT, -1, 0, 1, Value::MAX_INT] {
            assert_eq!(Value::int(n).unwrap().as_int(), Some(n));
        }
        assert!(Value::int(Value::MAX_INT + 1).is_none());
        assert!(Value::NONE.as_int().is_none());
    }
}
