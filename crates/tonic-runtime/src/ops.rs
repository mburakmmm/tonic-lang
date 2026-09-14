use crate::{
    heap::{Heap, Object},
    value::Value,
};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, Signed, ToPrimitive, Zero};
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

impl Heap {
    pub fn inplace_add(&mut self, a: Value, b: Value) -> Result<Value> {
        // Probe tags without constructing a TypeError on immediate arithmetic.
        if a.heap_index().is_none() || !matches!(self.get(a), Ok(Object::List(_))) {
            return self.binary(Op::Add, a, b);
        }
        if a == b {
            // list += itself copies its original contents exactly once.
            let Object::List(values) = self.get(a)? else {
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
        // Common integer path never constructs BigInt or allocates an object.
        if let (Some(x), Some(y)) = (a.integer(), b.integer()) {
            let n = match op {
                Op::Add => x.checked_add(y),
                Op::Sub => x.checked_sub(y),
                Op::Mul => x.checked_mul(y),
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
            return self.i64(if op == Op::Neg { -n } else { n });
        }
        if self.is_integer(v) {
            let n = self.integer(v)?;
            return self.int(if op == Op::Neg { -n } else { n });
        }
        if let Object::Float(n) = self.get(v)? {
            return if op == Op::Neg {
                self.alloc(Object::Float(-n))
            } else {
                Ok(v)
            };
        }
        Err(type_error())
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
        Ok(match (self.get(a), self.get(b)) {
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
        let n = match self.get(v)? {
            Object::Str(s) => BigInt::from(s.chars().count()),
            Object::Tuple(v) | Object::List(v) => BigInt::from(v.len()),
            Object::Buffer(buffer) => BigInt::from(buffer.len()),
            Object::Dict(dict) => BigInt::from(dict.entries.len()),
            Object::Range { start, stop, step } => BigInt::from(range_len(*start, *stop, *step)),
            _ => return Err(Diagnostic::new("TypeError", "object has no len()")),
        };
        self.int(n)
    }
    pub fn item(&mut self, v: Value, index: Value) -> Result<Value> {
        if matches!(self.get(v), Ok(Object::Dict(_))) {
            return self.dict_get(v, index)?.ok_or_else(|| {
                Diagnostic::new(
                    "KeyError",
                    self.format(index, true)
                        .unwrap_or_else(|_| "missing key".into()),
                )
            });
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
            Object::Iterator { .. }
            | Object::RangeIterator { .. }
            | Object::DictIterator { .. } => return Ok(v),
            _ => return Err(Diagnostic::new("TypeError", "object is not iterable")),
        };
        self.alloc(object)
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
