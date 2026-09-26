use crate::{
    classes::ClassDictionaryKey,
    heap::{Heap, Object},
    value::Value,
};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use tonic_core::{
    bytecode::Op,
    diagnostic::{Diagnostic, Result},
};
fn type_error() -> Diagnostic {
    Diagnostic::new("TypeError", "unsupported operand types")
}
fn zero() -> Diagnostic {
    Diagnostic::new("ZeroDivisionError", "division or modulo by zero")
}
const MAX_INTEGER_RESULT_BITS: u64 = 1 << 26;

impl Heap {
    pub fn inplace_add(&mut self, a: Value, b: Value) -> Result<Value> {
        let storage = self.native_value(a);
        // Probe tags without constructing a TypeError on immediate arithmetic.
        if storage.heap_index().is_none() || !matches!(self.get(storage), Ok(Object::List(_))) {
            return self.binary(Op::Add, a, b);
        }
        if a == b {
            // list += itself copies its original contents exactly once.
            let Object::List(values) = self.get(storage)? else {
                unreachable!()
            };
            let values = values.clone();
            for value in values {
                self.append_list(a, value)?;
            }
        } else {
            let iterator = self.iterator(b)?;
            while let Some(value) = self.next(iterator)? {
                self.append_list(a, value)?;
            }
        }
        Ok(a)
    }

    pub fn binary(&mut self, op: Op, a: Value, b: Value) -> Result<Value> {
        if let (Some(x), Some(y)) = (a.as_bool(), b.as_bool()) {
            let value = match op {
                Op::BitOr => Some(x | y),
                Op::BitXor => Some(x ^ y),
                Op::BitAnd => Some(x & y),
                _ => None,
            };
            if let Some(value) = value {
                return Ok(Value::bool(value));
            }
        }
        // Common integer path never constructs BigInt or allocates an object.
        if let (Some(x), Some(y)) = (a.integer(), b.integer()) {
            let n = match op {
                Op::Add => x.checked_add(y),
                Op::Sub => x.checked_sub(y),
                Op::Mul => x.checked_mul(y),
                Op::BitOr => Some(x | y),
                Op::BitXor => Some(x ^ y),
                Op::BitAnd => Some(x & y),
                Op::LeftShift | Op::RightShift => {
                    if y < 0 {
                        return Err(Diagnostic::new("ValueError", "negative shift count"));
                    }
                    let shift = u32::try_from(y).ok();
                    shift.and_then(|shift| {
                        if op == Op::LeftShift {
                            x.checked_shl(shift)
                        } else {
                            x.checked_shr(shift).or(Some(if x < 0 { -1 } else { 0 }))
                        }
                    })
                }
                Op::Pow if y >= 0 => u32::try_from(y).ok().and_then(|power| {
                    let mut result = 1i64;
                    let mut base = x;
                    let mut remaining = power;
                    while remaining != 0 {
                        if remaining & 1 != 0 {
                            result = result.checked_mul(base)?;
                        }
                        remaining >>= 1;
                        if remaining != 0 {
                            base = base.checked_mul(base)?;
                        }
                    }
                    Some(result)
                }),
                Op::Pow => {
                    if x == 0 {
                        return Err(zero());
                    }
                    return self.float_power(x as f64, y as f64);
                }
                Op::FloorDiv | Op::Mod => {
                    if y == 0 {
                        return Err(zero());
                    }
                    let mut q = x / y;
                    let mut r = x % y;
                    if r != 0 && (r < 0) != (y < 0) {
                        q -= 1;
                        r += y;
                    }
                    Some(if op == Op::FloorDiv { q } else { r })
                }
                Op::Div => {
                    if y == 0 {
                        return Err(zero());
                    }
                    if x.abs() <= (1i64 << 53) && y.abs() <= (1i64 << 53) {
                        return self.alloc(Object::Float(x as f64 / y as f64));
                    }
                    None
                }
                _ => None,
            };
            if let Some(v) = n.and_then(Value::int) {
                return Ok(v);
            }
        }
        if self.is_integer(a) && self.is_integer(b) {
            let x = self.integer(a)?;
            let y = self.integer(b)?;
            return match op {
                Op::Add => self.int(x + y),
                Op::Sub => self.int(x - y),
                Op::Mul => self.int(x * y),
                Op::BitOr => self.int(x | y),
                Op::BitXor => self.int(x ^ y),
                Op::BitAnd => self.int(x & y),
                Op::LeftShift | Op::RightShift => {
                    if y.is_negative() {
                        return Err(Diagnostic::new("ValueError", "negative shift count"));
                    }
                    if op == Op::LeftShift && x.is_zero() {
                        return self.int(BigInt::zero());
                    }
                    let Some(shift) = y.to_usize() else {
                        return if op == Op::RightShift {
                            self.int(if x.is_negative() {
                                -BigInt::one()
                            } else {
                                BigInt::zero()
                            })
                        } else {
                            Err(Diagnostic::new("MemoryError", "shift count is too large"))
                        };
                    };
                    if op == Op::LeftShift
                        && x.bits()
                            .saturating_add(u64::try_from(shift).unwrap_or(u64::MAX))
                            > MAX_INTEGER_RESULT_BITS
                    {
                        return Err(Diagnostic::new("MemoryError", "shift count is too large"));
                    }
                    self.int(if op == Op::LeftShift {
                        x << shift
                    } else {
                        x >> shift
                    })
                }
                Op::Pow => {
                    if x.is_zero() {
                        if y.is_negative() {
                            return Err(zero());
                        }
                        return self.int(if y.is_zero() {
                            BigInt::one()
                        } else {
                            BigInt::zero()
                        });
                    }
                    if x.is_one() {
                        return if y.is_negative() {
                            self.alloc(Object::Float(1.0))
                        } else {
                            self.int(BigInt::one())
                        };
                    }
                    if x == -BigInt::one() {
                        let odd = !(&y & BigInt::one()).is_zero();
                        let result = if odd { -1 } else { 1 };
                        return if y.is_negative() {
                            self.alloc(Object::Float(result as f64))
                        } else {
                            self.int(BigInt::from(result))
                        };
                    }
                    if y.is_negative() {
                        if x.is_zero() {
                            return Err(zero());
                        }
                        let base = x.to_f64().unwrap_or_else(|| {
                            if x.is_negative() {
                                f64::NEG_INFINITY
                            } else {
                                f64::INFINITY
                            }
                        });
                        let exponent = y.to_f64().unwrap_or(f64::NEG_INFINITY);
                        self.float_power(base, exponent)
                    } else {
                        let power = y.to_u32().ok_or_else(|| {
                            Diagnostic::new("MemoryError", "exponent is too large")
                        })?;
                        if x.bits().saturating_mul(u64::from(power)) > MAX_INTEGER_RESULT_BITS {
                            return Err(Diagnostic::new("MemoryError", "exponent is too large"));
                        }
                        self.int(x.pow(power))
                    }
                }
                Op::FloorDiv | Op::Mod => {
                    if y.is_zero() {
                        return Err(zero());
                    }
                    let mut q = &x / &y;
                    let mut r = &x % &y;
                    if !r.is_zero() && r.is_negative() != y.is_negative() {
                        q -= 1;
                        r += y;
                    }
                    self.int(if op == Op::FloorDiv { q } else { r })
                }
                Op::Div => {
                    if y.is_zero() {
                        return Err(zero());
                    }
                    let f = crate::number::ratio(x, y)?;
                    self.alloc(Object::Float(f))
                }
                _ => Err(type_error()),
            };
        }
        if (self.is_float(a) || self.is_integer(a)) && (self.is_float(b) || self.is_integer(b)) {
            let x = self.float(a)?;
            let y = self.float(b)?;
            let f = match op {
                Op::Add => x + y,
                Op::Sub => x - y,
                Op::Mul => x * y,
                Op::Pow => return self.float_power(x, y),
                Op::Div => {
                    if y == 0.0 {
                        return Err(zero());
                    }
                    x / y
                }
                Op::FloorDiv | Op::Mod => {
                    if y == 0.0 {
                        return Err(zero());
                    }
                    let mut rem = x % y;
                    let mut div = (x - rem) / y;
                    if rem != 0.0 {
                        if (y < 0.0) != (rem < 0.0) {
                            rem += y;
                            div -= 1.0;
                        }
                    } else {
                        rem = 0.0_f64.copysign(y);
                    }
                    let quotient = if div != 0.0 {
                        let floor = div.floor();
                        if div - floor > 0.5 {
                            floor + 1.0
                        } else {
                            floor
                        }
                    } else {
                        0.0_f64.copysign(x / y)
                    };
                    if op == Op::Mod {
                        rem
                    } else {
                        quotient
                    }
                }
                _ => return Err(type_error()),
            };
            return self.alloc(Object::Float(f));
        }
        if op == Op::Add {
            let a = self.native_value(a);
            let b = self.native_value(b);
            let result = match (self.get(a), self.get(b)) {
                (Ok(Object::Str(x)), Ok(Object::Str(y))) => {
                    let mut s = String::with_capacity(x.len() + y.len());
                    s.push_str(x);
                    s.push_str(y);
                    Object::Str(s)
                }
                (Ok(Object::List(x)), Ok(Object::List(y))) => {
                    Object::List(x.iter().chain(y).copied().collect())
                }
                (Ok(Object::Tuple(x)), Ok(Object::Tuple(y))) => {
                    Object::Tuple(x.iter().chain(y).copied().collect())
                }
                _ => return Err(type_error()),
            };
            return self.alloc(result);
        }
        Err(type_error())
    }
    pub fn unary(&mut self, op: Op, v: Value) -> Result<Value> {
        if op == Op::Not {
            return Ok(Value::bool(!self.truth(v)?));
        }
        if let Some(n) = v.integer() {
            return self.i64(match op {
                Op::Neg => -n,
                Op::Invert => !n,
                _ => n,
            });
        }
        if self.is_integer(v) {
            let n = self.integer(v)?;
            return self.int(match op {
                Op::Neg => -n,
                Op::Invert => !n,
                _ => n,
            });
        }
        let native = self.native_value(v);
        if let Object::Float(n) = self.get(native)? {
            return if op == Op::Neg {
                self.alloc(Object::Float(-n))
            } else {
                Ok(native)
            };
        }
        Err(type_error())
    }

    fn float_power(&mut self, base: f64, exponent: f64) -> Result<Value> {
        if base == 0.0 && exponent < 0.0 {
            return Err(zero());
        }
        if base < 0.0 && exponent.fract() != 0.0 {
            return Err(Diagnostic::new(
                "TypeError",
                "complex power results are not supported",
            ));
        }
        let result = base.powf(exponent);
        if result.is_infinite() && base.is_finite() && exponent.is_finite() {
            return Err(Diagnostic::new(
                "OverflowError",
                "numeric result out of range",
            ));
        }
        self.alloc(Object::Float(result))
    }
    pub fn compare(&self, op: Op, a: Value, b: Value) -> Result<Value> {
        if matches!(op, Op::Eq | Op::Ne) {
            let equal = self.equal(a, b, 0)?;
            return Ok(Value::bool(if op == Op::Eq { equal } else { !equal }));
        }
        let ordering = self.order(a, b, 0)?;
        Ok(Value::bool(match op {
            Op::Lt => ordering == Some(Ordering::Less),
            Op::Le => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
            Op::Gt => ordering == Some(Ordering::Greater),
            Op::Ge => matches!(ordering, Some(Ordering::Greater | Ordering::Equal)),
            _ => false,
        }))
    }
    fn equal(&self, a: Value, b: Value, depth: usize) -> Result<bool> {
        if depth > 100 {
            return Err(Diagnostic::new(
                "RecursionError",
                "comparison nesting limit",
            ));
        }
        if self.numeric(a) && self.numeric(b) {
            return Ok(self.number_order(a, b)? == Some(Ordering::Equal));
        }
        if a == b {
            return Ok(true);
        }
        let native_a = self.native_value(a);
        let native_b = self.native_value(b);
        Ok(match (self.get(native_a), self.get(native_b)) {
            (
                Ok(Object::BoundMethod {
                    function: a,
                    receiver: x,
                }),
                Ok(Object::BoundMethod {
                    function: b,
                    receiver: y,
                }),
            ) => {
                (*a == *b || self.equal(*a, *b, depth + 1)?)
                    && (*x == *y || self.equal(*x, *y, depth + 1)?)
            }
            (Ok(Object::Dict(x)), Ok(Object::Dict(y))) => {
                if x.entries.len() != y.entries.len() {
                    false
                } else {
                    let mut equal = true;
                    for (key, value) in &x.entries {
                        match self.dict_get(b, *key)? {
                            Some(other)
                                if *value == other || self.equal(*value, other, depth + 1)? => {}
                            _ => {
                                equal = false;
                                break;
                            }
                        }
                    }
                    equal
                }
            }
            (Ok(Object::Str(x)), Ok(Object::Str(y))) => x == y,
            (Ok(Object::Tuple(x)), Ok(Object::Tuple(y)))
            | (Ok(Object::List(x)), Ok(Object::List(y))) => {
                if x.len() != y.len() {
                    false
                } else {
                    let mut eq = true;
                    for (a, b) in x.iter().zip(y) {
                        if a != b && !self.equal(*a, *b, depth + 1)? {
                            eq = false;
                            break;
                        }
                    }
                    eq
                }
            }
            (
                Ok(Object::Range {
                    start: a,
                    stop: b,
                    step: c,
                }),
                Ok(Object::Range {
                    start: x,
                    stop: y,
                    step: z,
                }),
            ) => {
                let n = range_len(*a, *b, *c);
                let m = range_len(*x, *y, *z);
                n == m && (n == 0 || (a == x && (n == 1 || c == z)))
            }
            _ => false,
        })
    }
    fn numeric(&self, v: Value) -> bool {
        self.is_integer(v) || self.is_float(v)
    }
    fn number_order(&self, a: Value, b: Value) -> Result<Option<Ordering>> {
        if let (Some(x), Some(y)) = (a.integer(), b.integer()) {
            return Ok(Some(x.cmp(&y)));
        }
        if self.is_integer(a) && self.is_integer(b) {
            return Ok(Some(self.integer(a)?.cmp(&self.integer(b)?)));
        }
        if self.is_float(a) && self.is_float(b) {
            return Ok(self.float(a)?.partial_cmp(&self.float(b)?));
        }
        let reverse = self.is_float(a);
        let (int, float) = if reverse { (b, a) } else { (a, b) };
        let i = self.integer(int)?;
        let f = self.float(float)?;
        let cmp = if f.is_nan() {
            None
        } else if f == f64::INFINITY {
            Some(Ordering::Less)
        } else if f == f64::NEG_INFINITY {
            Some(Ordering::Greater)
        } else {
            let truncated = BigInt::from_f64(f).ok_or_else(type_error)?;
            let order = i.cmp(&truncated);
            Some(if order == Ordering::Equal {
                if f.fract() > 0.0 {
                    Ordering::Less
                } else if f.fract() < 0.0 {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            } else {
                order
            })
        };
        Ok(cmp.map(|c| if reverse { c.reverse() } else { c }))
    }
    fn order(&self, a: Value, b: Value, depth: usize) -> Result<Option<Ordering>> {
        if depth > 100 {
            return Err(Diagnostic::new(
                "RecursionError",
                "comparison nesting limit",
            ));
        }
        if self.numeric(a) && self.numeric(b) {
            return self.number_order(a, b);
        }
        let a = self.native_value(a);
        let b = self.native_value(b);
        match (self.get(a), self.get(b)) {
            (Ok(Object::Str(x)), Ok(Object::Str(y))) => Ok(Some(x.cmp(y))),
            (Ok(Object::Tuple(x)), Ok(Object::Tuple(y)))
            | (Ok(Object::List(x)), Ok(Object::List(y))) => {
                for (a, b) in x.iter().zip(y) {
                    if a != b && !self.equal(*a, *b, depth + 1)? {
                        return self.order(*a, *b, depth + 1);
                    }
                }
                Ok(Some(x.len().cmp(&y.len())))
            }
            _ => Err(type_error()),
        }
    }
    pub fn length(&mut self, v: Value) -> Result<Value> {
        let v = self.native_value(v);
        let n = match self.get(v)? {
            Object::Str(s) => BigInt::from(s.chars().count()),
            Object::Tuple(v) | Object::List(v) => BigInt::from(v.len()),
            Object::Buffer(buffer) => BigInt::from(buffer.len()),
            Object::Dict(dict) => BigInt::from(dict.entries.len()),
            Object::MappingProxy { class } => BigInt::from(self.class(*class)?.dictionary_len()),
            Object::Range { start, stop, step } => BigInt::from(range_len(*start, *stop, *step)),
            _ => return Err(Diagnostic::new("TypeError", "object has no len()")),
        };
        self.int(n)
    }
    pub fn item(&mut self, v: Value, index: Value) -> Result<Value> {
        let v = self.native_value(v);
        if matches!(self.get(v), Ok(Object::Dict(_))) {
            return self.dict_get(v, index)?.ok_or_else(|| {
                Diagnostic::new(
                    "KeyError",
                    self.format(index, true)
                        .unwrap_or_else(|_| "missing key".into()),
                )
            });
        }
        if let Ok(Object::MappingProxy { class }) = self.get(v) {
            let class = *class;
            if let Ok(Object::Str(name)) = self.get(index) {
                if let Some(value) = self
                    .class(class)?
                    .attributes
                    .iter()
                    .find(|(attribute, _)| attribute == name)
                    .map(|(_, value)| *value)
                {
                    return Ok(value);
                }
            }
            let extras = self.class(class)?.extra_attributes.clone();
            for (key, value) in extras {
                if self.dict_keys_equal(key, index)? {
                    return Ok(value);
                }
            }
            return Err(Diagnostic::new(
                "KeyError",
                self.format(index, true)
                    .unwrap_or_else(|_| "missing key".into()),
            ));
        }
        if let Ok(Object::Slice(components)) = self.get(index) {
            return self.slice(v, *components);
        }
        let i = self
            .integer(index)?
            .to_i128()
            .ok_or_else(|| Diagnostic::new("IndexError", "index out of range"))?;
        let len = match self.get(v)? {
            Object::Tuple(v) | Object::List(v) => v.len() as i128,
            Object::Str(s) => s.chars().count() as i128,
            Object::Range { start, stop, step } => range_len(*start, *stop, *step),
            _ => return Err(Diagnostic::new("TypeError", "object is not subscriptable")),
        };
        let i = if i < 0 { len + i } else { i };
        if i < 0 || i >= len {
            return Err(Diagnostic::new("IndexError", "index out of range"));
        }
        match self.get(v)? {
            Object::Tuple(v) | Object::List(v) => Ok(v[i as usize]),
            Object::Str(s) => {
                let s = s
                    .chars()
                    .nth(i as usize)
                    .expect("checked character index")
                    .to_string();
                self.alloc(Object::Str(s))
            }
            Object::Range { start, step, .. } => {
                self.int(BigInt::from(*start as i128 + i * (*step as i128)))
            }
            _ => Err(type_error()),
        }
    }
    fn slice(&mut self, source: Value, components: [Value; 3]) -> Result<Value> {
        let source = self.native_value(source);
        let start = self.slice_component(components[0])?;
        let stop = self.slice_component(components[1])?;
        let step = self.slice_component(components[2])?.unwrap_or(1);
        if step == 0 {
            return Err(Diagnostic::new("ValueError", "slice step cannot be zero"));
        }
        let len = match self.get(source)? {
            Object::Tuple(values) | Object::List(values) => values.len(),
            Object::Str(value) => value.chars().count(),
            _ => return Err(Diagnostic::new("TypeError", "object is not subscriptable")),
        };
        let indices = slice_indices(len, start, stop, step);
        let result = match self.get(source)? {
            Object::List(values) => Object::List(indices.iter().map(|&i| values[i]).collect()),
            Object::Tuple(values) => Object::Tuple(indices.iter().map(|&i| values[i]).collect()),
            Object::Str(value) => {
                let characters: Vec<char> = value.chars().collect();
                Object::Str(indices.iter().map(|&i| characters[i]).collect())
            }
            _ => unreachable!("source type checked above"),
        };
        self.alloc(result)
    }
    fn slice_component(&self, value: Value) -> Result<Option<i128>> {
        if value == Value::NONE {
            return Ok(None);
        }
        let value = self
            .integer(value)
            .map_err(|_| Diagnostic::new("TypeError", "slice indices must be integers or None"))?;
        Ok(Some(value.to_i128().unwrap_or_else(|| {
            if value.is_negative() {
                i128::MIN
            } else {
                i128::MAX
            }
        })))
    }
    pub fn iterator(&mut self, v: Value) -> Result<Value> {
        let v = self.native_value(v);
        let object = match self.get(v)? {
            Object::Range { start, stop, step } => Object::RangeIterator {
                next: *start as i128,
                stop: *stop as i128,
                step: *step as i128,
            },
            Object::Tuple(_) | Object::List(_) | Object::Str(_) => Object::Iterator {
                source: v,
                index: 0,
            },
            Object::Dict(dict) => Object::DictIterator {
                source: v,
                index: 0,
                version: dict.version,
            },
            Object::MappingProxy { class } => Object::MappingProxyIterator {
                class: *class,
                index: 0,
                size: self.class(*class)?.dictionary_len(),
            },
            Object::Iterator { .. }
            | Object::RangeIterator { .. }
            | Object::DictIterator { .. }
            | Object::MappingProxyIterator { .. } => return Ok(v),
            Object::Generator(_) => return Ok(v),
            _ => return Err(Diagnostic::new("TypeError", "object is not iterable")),
        };
        self.alloc(object)
    }
    pub fn is_iterator(&self, value: Value) -> bool {
        self.try_get(value).is_some_and(|object| {
            matches!(
                object,
                Object::Iterator { .. }
                    | Object::RangeIterator { .. }
                    | Object::DictIterator { .. }
                    | Object::MappingProxyIterator { .. }
            )
        })
    }
    pub fn next(&mut self, v: Value) -> Result<Option<Value>> {
        match self.get_mut(v)? {
            Object::DictIterator {
                source,
                index,
                version,
            } => {
                let (source, i, version) = (*source, *index, *version);
                let Object::Dict(dict) = self.get(source)? else {
                    unreachable!()
                };
                if dict.version != version {
                    return Err(Diagnostic::new(
                        "RuntimeError",
                        "dictionary changed size during iteration",
                    ));
                }
                let value = dict.entries.get(i).map(|(key, _)| *key);
                if value.is_some() {
                    if let Object::DictIterator { index, .. } = self.get_mut(v)? {
                        *index += 1;
                    }
                }
                Ok(value)
            }
            Object::MappingProxyIterator { class, index, size } => {
                let (class, index, size) = (*class, *index, *size);
                let class_object = self.class(class)?;
                if class_object.dictionary_len() != size {
                    return Err(Diagnostic::new(
                        "RuntimeError",
                        "dictionary changed size during iteration",
                    ));
                }
                let key = class_object.dictionary_entry(index).map(|(key, _)| key);
                if let Some(key) = key {
                    if let Object::MappingProxyIterator { index, .. } = self.get_mut(v)? {
                        *index += 1;
                    }
                    return Ok(Some(match key {
                        ClassDictionaryKey::String(name) => self.alloc(Object::Str(name))?,
                        ClassDictionaryKey::Other(value) => value,
                    }));
                }
                Ok(None)
            }
            Object::RangeIterator { next, stop, step } => {
                if (*step > 0 && *next >= *stop) || (*step < 0 && *next <= *stop) {
                    return Ok(None);
                }
                let n = *next;
                *next += *step;
                Ok(Some(self.i64(n as i64)?))
            }
            Object::Iterator { source, index } => {
                let source = *source;
                let i = *index;
                let item = match self.get(source)? {
                    Object::Tuple(v) | Object::List(v) => v.get(i).copied(),
                    Object::Str(s) => {
                        let ch = s.chars().nth(i);
                        if let Some(ch) = ch {
                            Some(self.alloc(Object::Str(ch.to_string()))?)
                        } else {
                            None
                        }
                    }
                    _ => return Err(type_error()),
                };
                if item.is_some() {
                    if let Object::Iterator { index, .. } = self.get_mut(v)? {
                        *index += 1;
                    }
                }
                Ok(item)
            }
            _ => Err(Diagnostic::new("TypeError", "object is not an iterator")),
        }
    }
}

fn slice_indices(length: usize, start: Option<i128>, stop: Option<i128>, step: i128) -> Vec<usize> {
    let len = length as i128;
    let mut indices = Vec::with_capacity(length);
    if step > 0 {
        let normalize = |value: i128| {
            let value = if value < 0 {
                value.saturating_add(len)
            } else {
                value
            };
            value.clamp(0, len)
        };
        let mut current = start.map(normalize).unwrap_or(0);
        let stop = stop.map(normalize).unwrap_or(len);
        while current < stop {
            indices.push(current as usize);
            let Some(next) = current.checked_add(step) else {
                break;
            };
            current = next;
        }
    } else {
        let upper = len - 1;
        let normalize = |value: i128| {
            let value = if value < 0 {
                value.saturating_add(len)
            } else {
                value
            };
            value.clamp(-1, upper)
        };
        let mut current = start.map(normalize).unwrap_or(upper);
        // An omitted stop is the sentinel before index zero. Explicit -1 is
        // normalized relative to the sequence length, as Python requires.
        let stop = stop.map(normalize).unwrap_or(-1);
        while current > stop {
            indices.push(current as usize);
            let Some(next) = current.checked_add(step) else {
                break;
            };
            current = next;
        }
    }
    indices
}
fn range_len(start: i64, stop: i64, step: i64) -> i128 {
    let (a, b, s) = (start as i128, stop as i128, step as i128);
    if s > 0 {
        if a >= b {
            0
        } else {
            (b - a - 1) / s + 1
        }
    } else if a <= b {
        0
    } else {
        (a - b - 1) / (-s) + 1
    }
}
