//! Insertion-ordered bootstrap dictionary. Keys own immutable hash material;
//! managed key/value identities are retained separately in traced entries.
use crate::{
    heap::{Heap, Object},
    value::Value,
};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive};
use std::collections::HashMap;
use tonic_core::diagnostic::{Diagnostic, Result};

#[derive(Debug, PartialEq, Eq, Hash)]
enum Key {
    None,
    SmallInt(i64),
    Int(BigInt),
    Float(u64),
    NaN(usize),
    Str(String),
    Tuple(Vec<Key>),
    Identity(usize),
    Method(Box<Key>, Box<Key>),
}
#[derive(Debug, Default)]
pub(crate) struct Dict {
    pub entries: Vec<(Value, Value)>,
    index: HashMap<Key, usize>,
    pub version: u64,
}
impl Dict {
    pub fn estimated_bytes(&self) -> usize {
        self.entries.capacity() * 16 + self.index.capacity() * std::mem::size_of::<(Key, usize)>()
    }
}
impl Heap {
    pub(crate) fn dict_get_str(&self, owner: Value, name: &str) -> Result<Option<Value>> {
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        Ok(dict
            .index
            .get(&Key::Str(name.to_owned()))
            .map(|index| dict.entries[*index].1))
    }
    pub(crate) fn dict_set_str(&mut self, owner: Value, name: &str, value: Value) -> Result<()> {
        let key = self.alloc(Object::Str(name.to_owned()))?;
        self.dict_set(owner, key, value)
    }
    pub(crate) fn dict_entries(&self, owner: Value) -> Result<Vec<(Value, Value)>> {
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        Ok(dict.entries.clone())
    }
    fn dict_key(&self, value: Value, depth: usize) -> Result<Key> {
        if depth > 100 {
            return Err(Diagnostic::new(
                "RecursionError",
                "dictionary key nesting limit",
            ));
        }
        if value == Value::NONE {
            return Ok(Key::None);
        }
        if let Some(n) = value.integer() {
            return Ok(Key::SmallInt(n));
        }
        Ok(match self.get(value)? {
            Object::Int(n) => {
                if let Some(n) = n.to_i64() {
                    Key::SmallInt(n)
                } else {
                    Key::Int(n.clone())
                }
            }
            Object::Float(n) if n.is_nan() => Key::NaN(value.heap_index().expect("heap float")),
            Object::Float(n) if n.is_finite() && n.fract() == 0.0 => {
                let n = BigInt::from_f64(*n).expect("finite integer float");
                if let Some(n) = n.to_i64() {
                    Key::SmallInt(n)
                } else {
                    Key::Int(n)
                }
            }
            Object::Float(n) => Key::Float(n.to_bits()),
            Object::Str(s) => Key::Str(s.clone()),
            Object::Tuple(values) => Key::Tuple(
                values
                    .iter()
                    .map(|v| self.dict_key(*v, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            Object::Function { .. }
            | Object::Builtin(_)
            | Object::Native(_)
            | Object::Class(_)
            | Object::Instance { .. }
            | Object::Exception { .. }
            | Object::StaticMethod(_)
            | Object::ClassMethod(_)
            | Object::Property { .. }
            | Object::PropertySetter(_) => {
                Key::Identity(value.heap_index().expect("heap callable"))
            }
            Object::BoundMethod { function, receiver } => Key::Method(
                Box::new(self.dict_key(*function, depth + 1)?),
                Box::new(self.dict_key(*receiver, depth + 1)?),
            ),
            _ => return Err(Diagnostic::new("TypeError", "unhashable dictionary key")),
        })
    }
    pub fn dict_get(&self, owner: Value, key: Value) -> Result<Option<Value>> {
        let key = self.dict_key(key, 0)?;
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        Ok(dict.index.get(&key).map(|i| dict.entries[*i].1))
    }
    pub fn dict_keys_equal(&self, a: Value, b: Value) -> Result<bool> {
        Ok(self.dict_key(a, 0)? == self.dict_key(b, 0)?)
    }
    /// Single dictionary mutation boundary for the generational write barrier.
    pub fn dict_set(&mut self, owner: Value, key: Value, value: Value) -> Result<()> {
        let material = self.dict_key(key, 0)?;
        self.write_barrier_pair(owner, key, value);
        let Object::Dict(dict) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        let before = dict.estimated_bytes();
        if let Some(index) = dict.index.get(&material) {
            dict.entries[*index].1 = value;
        } else {
            let version = dict
                .version
                .checked_add(1)
                .ok_or_else(|| Diagnostic::new("RuntimeError", "dict version exhausted"))?;
            dict.index.insert(material, dict.entries.len());
            dict.entries.push((key, value));
            dict.version = version;
        }
        self.bytes += dict.estimated_bytes() - before;
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    pub fn dict_delete(&mut self, owner: Value, key: Value) -> Result<()> {
        let material = self.dict_key(key, 0)?;
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        let Some(index) = dict.index.get(&material).copied() else {
            return Err(Diagnostic::new(
                "KeyError",
                self.format(key, true)
                    .unwrap_or_else(|_| "missing key".into()),
            ));
        };
        let Object::Dict(dict) = self.get_mut(owner)? else {
            unreachable!("validated dict changed kind")
        };
        dict.index.remove(&material);
        dict.entries.remove(index);
        dict.index.values_mut().for_each(|slot| {
            if *slot > index {
                *slot -= 1;
            }
        });
        dict.version = dict
            .version
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("RuntimeError", "dict version exhausted"))?;
        Ok(())
    }
    pub fn dict_merge(&mut self, owner: Value, other: Value) -> Result<()> {
        let Object::Dict(dict) = self.get(other)? else {
            return Err(Diagnostic::new(
                "TypeError",
                "dictionary unpacking requires a mapping",
            ));
        };
        if owner == other {
            return Ok(());
        }
        let len = dict.entries.len();
        for i in 0..len {
            let Object::Dict(dict) = self.get(other)? else {
                unreachable!()
            };
            let (key, value) = dict.entries[i];
            self.dict_set(owner, key, value)?;
        }
        Ok(())
    }
    pub fn set_item(&mut self, owner: Value, key: Value, value: Value) -> Result<()> {
        if matches!(self.get(owner)?, Object::Dict(_)) {
            return self.dict_set(owner, key, value);
        }
        if matches!(self.get(owner)?, Object::MappingProxy { .. }) {
            return Err(Diagnostic::new(
                "TypeError",
                "mappingproxy does not support item assignment",
            ));
        }
        let i = self
            .integer(key)?
            .to_i128()
            .ok_or_else(|| Diagnostic::new("IndexError", "list index out of range"))?;
        let Object::List(values) = self.get_mut(owner)? else {
            return Err(Diagnostic::new(
                "TypeError",
                "object does not support item assignment",
            ));
        };
        let i = if i < 0 { i + values.len() as i128 } else { i };
        if i < 0 || i >= values.len() as i128 {
            return Err(Diagnostic::new("IndexError", "list index out of range"));
        }
        let index = i as usize;
        let _ = values;
        self.write_barrier(owner, value);
        let Object::List(values) = self.get_mut(owner)? else {
            unreachable!()
        };
        values[index] = value;
        Ok(())
    }
    pub fn delete_item(&mut self, owner: Value, key: Value) -> Result<()> {
        if matches!(self.get(owner)?, Object::Dict(_)) {
            return self.dict_delete(owner, key);
        }
        if matches!(self.get(owner)?, Object::MappingProxy { .. }) {
            return Err(Diagnostic::new(
                "TypeError",
                "mappingproxy does not support item deletion",
            ));
        }
        let i = self
            .integer(key)?
            .to_i128()
            .ok_or_else(|| Diagnostic::new("IndexError", "list assignment index out of range"))?;
        let Object::List(values) = self.get_mut(owner)? else {
            return Err(Diagnostic::new(
                "TypeError",
                "object does not support item deletion",
            ));
        };
        let i = if i < 0 { i + values.len() as i128 } else { i };
        if i < 0 || i >= values.len() as i128 {
            return Err(Diagnostic::new(
                "IndexError",
                "list assignment index out of range",
            ));
        }
        values.remove(i as usize);
        Ok(())
    }
}
