//! Insertion-ordered bootstrap dictionary. Keys own immutable hash material;
//! managed key/value identities are retained separately in traced entries.
use crate::{
    hashing::{
        hash_range_parts, hash_string, hash_u64, normalize_bigint, range_length, sequence_finish,
        sequence_start, sequence_step,
    },
    heap::{Heap, Object},
    value::Value,
};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive};
use std::collections::HashMap;
use tonic_core::diagnostic::{Diagnostic, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    None,
    NotImplemented,
    SmallInt(i64),
    Int(BigInt),
    Float(u64),
    NaN(u64),
    Str(String),
    Tuple(Vec<Key>),
    Slice(Vec<Key>),
    Range {
        length: i128,
        start: Option<i64>,
        step: Option<i64>,
    },
    Identity(u64),
    Method(Box<Key>, Box<Key>),
}
#[derive(Debug, Default)]
pub(crate) struct Dict {
    pub entries: Vec<(Value, Value)>,
    materials: Vec<Key>,
    hashes: Vec<u64>,
    index: HashMap<u64, Vec<usize>>,
    pub version: u64,
}
impl Dict {
    pub fn estimated_bytes(&self) -> usize {
        self.entries.capacity() * 16
            + self.materials.capacity() * std::mem::size_of::<Key>()
            + self.hashes.capacity() * std::mem::size_of::<u64>()
            + self.index.capacity() * std::mem::size_of::<(u64, Vec<usize>)>()
            + self
                .index
                .values()
                .map(|bucket| bucket.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
    }
}
fn key_hash(key: &Key) -> u64 {
    let hash = match key {
        Key::None => 0x421,
        Key::NotImplemented => 0x422,
        Key::SmallInt(value) => crate::hashing::normalize_i64(*value),
        Key::Int(value) => normalize_bigint(value),
        Key::Float(bits) => hash_u64(*bits),
        Key::NaN(identity) | Key::Identity(identity) => hash_u64(*identity),
        Key::Str(value) => hash_string(value),
        Key::Tuple(values) | Key::Slice(values) => {
            let accumulator = values.iter().fold(sequence_start(), |accumulator, value| {
                sequence_step(accumulator, key_hash(value) as i64)
            });
            sequence_finish(accumulator, values.len())
        }
        Key::Range {
            length,
            start,
            step,
        } => hash_range_parts(*length, *start, *step),
        Key::Method(function, receiver) => {
            let accumulator = sequence_step(sequence_start(), key_hash(function) as i64);
            sequence_finish(sequence_step(accumulator, key_hash(receiver) as i64), 2)
        }
    };
    hash as u64
}
fn find_material(dict: &Dict, material: &Key) -> Option<usize> {
    dict.index
        .get(&key_hash(material))?
        .iter()
        .copied()
        .find(|index| dict.materials[*index] == *material)
}
impl Heap {
    pub(crate) fn dict_get_str(&self, owner: Value, name: &str) -> Result<Option<Value>> {
        let owner = self.native_value(owner);
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        let material = Key::Str(name.to_owned());
        Ok(find_material(dict, &material).map(|index| dict.entries[index].1))
    }
    pub(crate) fn dict_set_str(&mut self, owner: Value, name: &str, value: Value) -> Result<()> {
        let key = self.alloc(Object::Str(name.to_owned()))?;
        self.dict_set(owner, key, value)
    }
    pub(crate) fn dict_entries(&self, owner: Value) -> Result<Vec<(Value, Value)>> {
        let owner = self.native_value(owner);
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
        let identity = value;
        let value = self.native_value(value);
        if value == Value::NONE {
            return Ok(Key::None);
        }
        if value == Value::NOT_IMPLEMENTED {
            return Ok(Key::NotImplemented);
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
            Object::Float(n) if n.is_nan() => Key::NaN(identity.raw()),
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
            Object::Slice(values) => Key::Slice(
                values
                    .iter()
                    .map(|value| self.dict_key(*value, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            Object::Range { start, stop, step } => {
                let length = range_length(*start, *stop, *step);
                Key::Range {
                    length,
                    start: (length != 0).then_some(*start),
                    step: (length > 1).then_some(*step),
                }
            }
            Object::Function { .. }
            | Object::Builtin(_)
            | Object::Native(_)
            | Object::Class(_)
            | Object::Instance { .. }
            | Object::Exception { .. }
            | Object::StaticMethod(_)
            | Object::ClassMethod(_)
            | Object::Property { .. }
            | Object::PropertySetter(_) => Key::Identity(value.raw()),
            Object::BoundMethod { function, receiver } => Key::Method(
                Box::new(self.dict_key(*function, depth + 1)?),
                Box::new(self.dict_key(*receiver, depth + 1)?),
            ),
            _ => return Err(Diagnostic::new("TypeError", "unhashable dictionary key")),
        })
    }
    pub fn dict_get(&self, owner: Value, key: Value) -> Result<Option<Value>> {
        let key = self.dict_key(key, 0)?;
        let owner = self.native_value(owner);
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        Ok(find_material(dict, &key).map(|index| dict.entries[index].1))
    }
    pub fn dict_keys_equal(&self, a: Value, b: Value) -> Result<bool> {
        Ok(self.dict_key(a, 0)? == self.dict_key(b, 0)?)
    }
    /// Single dictionary mutation boundary for the generational write barrier.
    pub fn dict_set(&mut self, owner: Value, key: Value, value: Value) -> Result<()> {
        let owner = self.native_value(owner);
        let material = self.dict_key(key, 0)?;
        self.write_barrier_pair(owner, key, value);
        let Object::Dict(dict) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        let before = dict.estimated_bytes();
        if let Some(index) = find_material(dict, &material) {
            dict.entries[index].1 = value;
        } else {
            let version = dict
                .version
                .checked_add(1)
                .ok_or_else(|| Diagnostic::new("RuntimeError", "dict version exhausted"))?;
            let index = dict.entries.len();
            dict.index
                .entry(key_hash(&material))
                .or_default()
                .push(index);
            dict.entries.push((key, value));
            dict.materials.push(material);
            dict.hashes.push(key_hash(&dict.materials[index]));
            dict.version = version;
        }
        self.bytes += dict.estimated_bytes() - before;
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    pub fn dict_delete(&mut self, owner: Value, key: Value) -> Result<()> {
        let owner = self.native_value(owner);
        let material = self.dict_key(key, 0)?;
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        let Some(index) = find_material(dict, &material) else {
            return Err(Diagnostic::new(
                "KeyError",
                self.format(key, true)
                    .unwrap_or_else(|_| "missing key".into()),
            ));
        };
        let Object::Dict(dict) = self.get_mut(owner)? else {
            unreachable!("validated dict changed kind")
        };
        let hash = dict.hashes[index];
        let remove_bucket = {
            let bucket = dict
                .index
                .get_mut(&hash)
                .expect("located material has a hash bucket");
            let slot = bucket
                .iter()
                .position(|candidate| *candidate == index)
                .expect("located material is present in its hash bucket");
            bucket.remove(slot);
            bucket.is_empty()
        };
        if remove_bucket {
            dict.index.remove(&hash);
        }
        dict.entries.remove(index);
        dict.materials.remove(index);
        dict.hashes.remove(index);
        dict.index.values_mut().for_each(|bucket| {
            bucket.iter_mut().for_each(|slot| {
                if *slot > index {
                    *slot -= 1;
                }
            });
        });
        dict.version = dict
            .version
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("RuntimeError", "dict version exhausted"))?;
        Ok(())
    }
    pub fn dict_merge(&mut self, owner: Value, other: Value) -> Result<()> {
        let owner = self.native_value(owner);
        let other = self.native_value(other);
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
    pub(crate) fn dict_clear(&mut self, owner: Value) -> Result<()> {
        let owner = self.native_value(owner);
        let Object::Dict(dict) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        let before = dict.estimated_bytes();
        dict.entries.clear();
        dict.materials.clear();
        dict.hashes.clear();
        dict.index.clear();
        dict.version = dict
            .version
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("RuntimeError", "dict version exhausted"))?;
        let after = dict.estimated_bytes();
        self.bytes -= before - after;
        Ok(())
    }
    pub fn set_item(&mut self, owner: Value, key: Value, value: Value) -> Result<()> {
        let owner = self.native_value(owner);
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
        let owner = self.native_value(owner);
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

    pub(crate) fn dict_candidates(&self, owner: Value, hash: i64) -> Result<Vec<usize>> {
        let owner = self.native_value(owner);
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        Ok(dict.index.get(&(hash as u64)).cloned().unwrap_or_default())
    }

    pub(crate) fn dict_version(&self, owner: Value) -> Result<u64> {
        let owner = self.native_value(owner);
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        Ok(dict.version)
    }

    pub(crate) fn dict_len(&self, owner: Value) -> Result<usize> {
        let owner = self.native_value(owner);
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        Ok(dict.entries.len())
    }

    pub(crate) fn dict_entry_at(&self, owner: Value, index: usize) -> Result<(Value, Value)> {
        let owner = self.native_value(owner);
        let Object::Dict(dict) = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        dict.entries
            .get(index)
            .copied()
            .ok_or_else(|| Diagnostic::new("RuntimeError", "stale dictionary candidate"))
    }

    pub(crate) fn dict_set_hashed(
        &mut self,
        owner: Value,
        key: Value,
        value: Value,
        hash: i64,
        matched: Option<usize>,
    ) -> Result<()> {
        let owner = self.native_value(owner);
        let material = self.dict_key(key, 0)?;
        self.write_barrier_pair(owner, key, value);
        let Object::Dict(dict) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        let before = dict.estimated_bytes();
        if let Some(index) = matched {
            let entry = dict
                .entries
                .get_mut(index)
                .ok_or_else(|| Diagnostic::new("RuntimeError", "stale dictionary candidate"))?;
            entry.1 = value;
        } else {
            let version = dict
                .version
                .checked_add(1)
                .ok_or_else(|| Diagnostic::new("RuntimeError", "dict version exhausted"))?;
            let index = dict.entries.len();
            dict.index.entry(hash as u64).or_default().push(index);
            dict.entries.push((key, value));
            dict.materials.push(material);
            dict.hashes.push(hash as u64);
            dict.version = version;
        }
        self.bytes += dict.estimated_bytes() - before;
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }

    pub(crate) fn dict_delete_at(&mut self, owner: Value, index: usize) -> Result<()> {
        let owner = self.native_value(owner);
        let Object::Dict(dict) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected dict"));
        };
        if index >= dict.entries.len() {
            return Err(Diagnostic::new(
                "RuntimeError",
                "stale dictionary candidate",
            ));
        }
        let before = dict.estimated_bytes();
        let hash = dict.hashes[index];
        let remove_bucket = {
            let bucket = dict
                .index
                .get_mut(&hash)
                .expect("stored dictionary hash has a bucket");
            let slot = bucket
                .iter()
                .position(|candidate| *candidate == index)
                .expect("stored dictionary index is present in its bucket");
            bucket.remove(slot);
            bucket.is_empty()
        };
        if remove_bucket {
            dict.index.remove(&hash);
        }
        dict.entries.remove(index);
        dict.materials.remove(index);
        dict.hashes.remove(index);
        dict.index.values_mut().for_each(|bucket| {
            bucket.iter_mut().for_each(|candidate| {
                if *candidate > index {
                    *candidate -= 1;
                }
            });
        });
        dict.version = dict
            .version
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("RuntimeError", "dict version exhausted"))?;
        let after = dict.estimated_bytes();
        self.bytes -= before - after;
        Ok(())
    }
}
