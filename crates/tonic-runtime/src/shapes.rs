//! Bounded shared shape transitions; instance fast storage contains only Values.
//! Metadata has no managed edges. Dictionary fallback bounds retained shape names.
use crate::value::Value;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ShapeId(pub u32);
#[derive(Debug)]
struct Shape {
    parent: ShapeId,
    name: String,
    depth: usize,
}
#[derive(Debug, Default)]
pub(crate) struct Shapes {
    nodes: Vec<Shape>,
    transitions: HashMap<ShapeId, HashMap<String, ShapeId>>,
}
impl Shapes {
    pub fn len(&self) -> usize {
        self.nodes.len() + 1
    }
    pub fn slot(&self, mut id: ShapeId, name: &str) -> Option<usize> {
        while id.0 != 0 {
            let shape = &self.nodes[id.0 as usize - 1];
            if shape.name == name {
                return Some(shape.depth - 1);
            }
            id = shape.parent;
        }
        None
    }
    pub fn transition(&mut self, id: ShapeId, name: &str) -> Option<ShapeId> {
        if let Some(next) = self.transitions.get(&id).and_then(|m| m.get(name)) {
            return Some(*next);
        }
        let depth = if id.0 == 0 {
            0
        } else {
            self.nodes[id.0 as usize - 1].depth
        };
        // BOOTSTRAP bounds, not language limits: objects beyond these go dynamic.
        if depth >= 64 || self.nodes.len() >= 4096 || name.len() > 1024 {
            return None;
        }
        let next = ShapeId(self.nodes.len() as u32 + 1);
        self.nodes.push(Shape {
            parent: id,
            name: name.into(),
            depth: depth + 1,
        });
        self.transitions
            .entry(id)
            .or_default()
            .insert(name.into(), next);
        Some(next)
    }
    pub fn materialize(&self, mut id: ShapeId, values: &[Value]) -> HashMap<String, Value> {
        let mut map = HashMap::new();
        while id.0 != 0 {
            let shape = &self.nodes[id.0 as usize - 1];
            map.insert(shape.name.clone(), values[shape.depth - 1]);
            id = shape.parent;
        }
        map
    }
}
#[derive(Debug)]
pub(crate) enum Attributes {
    Slots { shape: ShapeId, values: Vec<Value> },
    Dictionary(HashMap<String, Value>),
}
impl Default for Attributes {
    fn default() -> Self {
        Self::Slots {
            shape: ShapeId::default(),
            values: Vec::new(),
        }
    }
}
impl Attributes {
    pub fn slot_cache(&self, shapes: &Shapes, name: &str) -> Option<(ShapeId, usize)> {
        let Self::Slots { shape, .. } = self else {
            return None;
        };
        shapes.slot(*shape, name).map(|slot| (*shape, slot))
    }
    pub fn cached_slot(&self, shape: ShapeId, slot: usize) -> Option<Value> {
        match self {
            Self::Slots {
                shape: actual,
                values,
            } if *actual == shape => values.get(slot).copied(),
            Self::Slots { .. } | Self::Dictionary(_) => None,
        }
    }
    pub fn trace(&self, visit: impl FnMut(Value)) {
        match self {
            Self::Slots { values, .. } => values.iter().copied().for_each(visit),
            Self::Dictionary(values) => values.values().copied().for_each(visit),
        }
    }
    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::Slots { values, .. } => values.capacity() * 8,
            Self::Dictionary(values) => {
                values.capacity() * std::mem::size_of::<(String, Value)>()
                    + values.keys().map(String::capacity).sum::<usize>()
            }
        }
    }
    pub fn get(&self, shapes: &Shapes, name: &str) -> Option<Value> {
        match self {
            Self::Slots { shape, values } => shapes.slot(*shape, name).map(|i| values[i]),
            Self::Dictionary(values) => values.get(name).copied(),
        }
    }
    pub fn set(&mut self, shapes: &mut Shapes, name: &str, value: Value) {
        if let Self::Slots { shape, values } = self {
            if let Some(i) = shapes.slot(*shape, name) {
                values[i] = value;
                return;
            }
            if let Some(next) = shapes.transition(*shape, name) {
                *shape = next;
                values.push(value);
                return;
            }
            *self = Self::Dictionary(shapes.materialize(*shape, values));
        }
        if let Self::Dictionary(values) = self {
            values.insert(name.to_owned(), value);
        }
    }
    pub fn delete(&mut self, shapes: &Shapes, name: &str) -> Option<Value> {
        if let Self::Slots { shape, values } = self {
            let mut materialized = shapes.materialize(*shape, values);
            let removed = materialized.remove(name);
            if removed.is_some() {
                *self = Self::Dictionary(materialized);
            }
            return removed;
        }
        match self {
            Self::Dictionary(values) => values.remove(name),
            Self::Slots { .. } => unreachable!(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions_share_layouts_and_preserve_values() {
        let mut shapes = Shapes::default();
        let mut a = Attributes::default();
        let mut b = Attributes::default();
        a.set(&mut shapes, "x", Value::int(1).unwrap());
        b.set(&mut shapes, "x", Value::int(2).unwrap());
        assert_eq!(shapes.len(), 2);
        assert_eq!(a.get(&shapes, "x"), Value::int(1));
        assert_eq!(b.get(&shapes, "x"), Value::int(2));
        for i in 0..100 {
            a.set(&mut shapes, &format!("n{i}"), Value::int(i).unwrap());
        }
        assert!(matches!(a, Attributes::Dictionary(_)));
        assert_eq!(a.get(&shapes, "x"), Value::int(1));
        assert_eq!(a.get(&shapes, "n99"), Value::int(99));
    }
    #[test]
    fn metadata_limits_do_not_become_guest_attribute_limits() {
        let mut shapes = Shapes::default();
        for i in 0..5000 {
            let mut attrs = Attributes::default();
            let name = format!("attr{i}");
            attrs.set(&mut shapes, &name, Value::int(i).unwrap());
            assert_eq!(attrs.get(&shapes, &name), Value::int(i));
        }
        assert_eq!(shapes.len(), 4097);
        let name = "x".repeat(2048);
        let mut attrs = Attributes::default();
        attrs.set(&mut shapes, &name, Value::NONE);
        assert_eq!(attrs.get(&shapes, &name), Some(Value::NONE));
        assert_eq!(shapes.len(), 4097);
    }
}
