//! Generic class/attribute semantics. No persistent raw object pointers or caches.
use crate::{
    heap::{Builtin, Heap, Object},
    shapes::{Attributes, ShapeId},
    value::Value,
};
use tonic_core::diagnostic::{Diagnostic, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TypeId(pub u32);
pub(crate) enum DescriptorAccess {
    Value(Value),
    Call {
        callable: Value,
        receiver: Option<Value>,
        positional: [Value; 3],
        count: usize,
    },
}
#[derive(Clone, Copy)]
pub(crate) struct DescriptorCall {
    pub callable: Value,
    pub receiver: Option<Value>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectMethodKind {
    Static,
    Instance,
    Class,
}
#[derive(Clone, Debug)]
pub(crate) enum ClassDictionaryKey {
    String(String),
    Other(Value),
}
#[derive(Debug)]
pub(crate) struct Class {
    pub root: bool,
    pub id: TypeId,
    pub version: u64,
    pub name: String,
    /// Logical class handle for this class object's metaclass.
    pub metaclass: Value,
    /// Dict returned by a custom metaclass `__prepare__` while the class body
    /// runs. Completed classes materialize it into `attributes`.
    pub namespace_mapping: Option<Value>,
    pub attributes: Vec<(String, Value)>,
    /// Non-string keys are not part of attribute lookup, but Python exposes
    /// them through a programmatically-created class's mappingproxy.
    pub extra_attributes: Vec<(Value, Value)>,
    /// Insertion order for the complete class dictionary without duplicating
    /// string attribute values or putting a hash table on the attribute path.
    pub dictionary_order: Vec<ClassDictionaryKey>,
    pub bases: Vec<Value>,
    /// C3 ancestors, excluding self. These are traced logical handles.
    pub mro: Vec<Value>,
    /// Weak descendant handles used only for dependency invalidation.
    pub dependents: Vec<Value>,
}
impl Class {
    pub fn trace(&self, mut visit: impl FnMut(Value)) {
        visit(self.metaclass);
        self.namespace_mapping.iter().copied().for_each(&mut visit);
        self.attributes.iter().for_each(|(_, v)| visit(*v));
        self.extra_attributes.iter().for_each(|(key, value)| {
            visit(*key);
            visit(*value);
        });
        self.bases.iter().chain(&self.mro).copied().for_each(visit);
    }
    pub fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.capacity()
            + self.attributes.capacity() * std::mem::size_of::<(String, Value)>()
            + self.extra_attributes.capacity() * std::mem::size_of::<(Value, Value)>()
            + self.dictionary_order.capacity() * std::mem::size_of::<ClassDictionaryKey>()
            + self
                .attributes
                .iter()
                .map(|(n, _)| n.capacity())
                .sum::<usize>()
            + self
                .dictionary_order
                .iter()
                .map(|key| match key {
                    ClassDictionaryKey::String(name) => name.capacity(),
                    ClassDictionaryKey::Other(_) => 0,
                })
                .sum::<usize>()
            + (self.bases.capacity() + self.mro.capacity()) * 8
            + self.dependents.capacity() * 8
    }
    pub fn dictionary_len(&self) -> usize {
        self.dictionary_order.len()
    }
    pub fn dictionary_entry(&self, index: usize) -> Option<(ClassDictionaryKey, Value)> {
        let key = self.dictionary_order.get(index)?.clone();
        let value = match &key {
            ClassDictionaryKey::String(name) => self
                .attributes
                .iter()
                .find(|(attribute, _)| attribute == name)
                .map(|(_, value)| *value),
            ClassDictionaryKey::Other(key) => self
                .extra_attributes
                .iter()
                .find(|(candidate, _)| candidate == key)
                .map(|(_, value)| *value),
        }?;
        Some((key, value))
    }
}
fn unsupported_hook(name: &str) -> Result<()> {
    if name.starts_with("__")
        && name.ends_with("__")
        && !matches!(
            name,
            "__init__"
                | "__new__"
                | "__call__"
                | "__len__"
                | "__bool__"
                | "__get__"
                | "__getattr__"
                | "__getattribute__"
                | "__setattr__"
                | "__delattr__"
                | "__iter__"
                | "__next__"
                | "__enter__"
                | "__exit__"
                | "__set__"
                | "__set_name__"
                | "__prepare__"
                | "__getitem__"
                | "__setitem__"
                | "__delitem__"
                | "__delete__"
                | "__add__"
                | "__radd__"
                | "__iadd__"
                | "__sub__"
                | "__rsub__"
                | "__isub__"
                | "__mul__"
                | "__rmul__"
                | "__imul__"
                | "__truediv__"
                | "__rtruediv__"
                | "__itruediv__"
                | "__floordiv__"
                | "__rfloordiv__"
                | "__ifloordiv__"
                | "__mod__"
                | "__rmod__"
                | "__imod__"
                | "__pow__"
                | "__rpow__"
                | "__ipow__"
                | "__or__"
                | "__ror__"
                | "__ior__"
                | "__xor__"
                | "__rxor__"
                | "__ixor__"
                | "__and__"
                | "__rand__"
                | "__iand__"
                | "__lshift__"
                | "__rlshift__"
                | "__ilshift__"
                | "__rshift__"
                | "__rrshift__"
                | "__irshift__"
                | "__eq__"
                | "__ne__"
                | "__lt__"
                | "__le__"
                | "__gt__"
                | "__ge__"
                | "__neg__"
                | "__pos__"
                | "__abs__"
                | "__invert__"
                | "__int__"
                | "__float__"
                | "__index__"
                | "__module__"
                | "__qualname__"
                | "__doc__"
                | "__name__"
        )
    {
        return Err(Diagnostic::new(
            "UnsupportedFeature",
            format!("class protocol '{name}' is not implemented yet"),
        ));
    }
    Ok(())
}
fn missing(name: &str) -> Diagnostic {
    Diagnostic::new(
        "AttributeError",
        format!("object has no attribute '{name}'"),
    )
}
impl Heap {
    pub fn root_object_class(
        &mut self,
        object_new: Value,
        object_getattribute: Value,
        object_setattr: Value,
        object_delattr: Value,
    ) -> Result<Value> {
        let value = self.namespace("object", Vec::new())?;
        self.namespace_set(value, "__new__", object_new)?;
        self.namespace_set(value, "__getattribute__", object_getattribute)?;
        self.namespace_set(value, "__setattr__", object_setattr)?;
        self.namespace_set(value, "__delattr__", object_delattr)?;
        self.finish_class(value)?;
        let module = self.alloc(Object::Str("builtins".into()))?;
        self.set_attr(value, "__module__", module)?;
        let Object::Class(class) = self.get_mut(value)? else {
            unreachable!()
        };
        class.root = true;
        Ok(value)
    }
    pub fn root_type_class(
        &mut self,
        object_class: Value,
        type_new: Value,
        type_getattribute: Value,
        type_setattr: Value,
        type_delattr: Value,
    ) -> Result<Value> {
        let value = self.namespace("type", vec![object_class])?;
        self.namespace_set(value, "__getattribute__", type_getattribute)?;
        self.namespace_set(value, "__setattr__", type_setattr)?;
        self.namespace_set(value, "__delattr__", type_delattr)?;
        self.finish_class(value)?;
        let module = self.alloc(Object::Str("builtins".into()))?;
        self.set_attr(value, "__module__", module)?;
        self.set_attr(value, "__new__", type_new)?;
        self.set_class_metaclass(object_class, value)?;
        self.set_class_metaclass(value, value)?;
        Ok(value)
    }
    pub fn builtin_class(
        &mut self,
        name: &str,
        bases: Vec<Value>,
        metaclass: Value,
    ) -> Result<Value> {
        let value = self.namespace_with_metaclass(name, bases, metaclass)?;
        self.finish_class(value)?;
        let module = self.alloc(Object::Str("builtins".into()))?;
        self.set_attr(value, "__module__", module)?;
        Ok(value)
    }
    fn set_class_metaclass(&mut self, class: Value, metaclass: Value) -> Result<()> {
        self.class(metaclass)?;
        self.write_barrier(class, metaclass);
        let Object::Class(class) = self.get_mut(class)? else {
            return Err(Diagnostic::new("TypeError", "expected a Tonic class"));
        };
        class.metaclass = metaclass;
        Ok(())
    }
    pub fn namespace(&mut self, qualname: &str, bases: Vec<Value>) -> Result<Value> {
        self.namespace_with_metaclass(qualname, bases, Value::UNBOUND)
    }
    pub fn namespace_with_metaclass(
        &mut self,
        qualname: &str,
        bases: Vec<Value>,
        metaclass: Value,
    ) -> Result<Value> {
        let module = self.alloc(Object::Str("__main__".into()))?;
        let qualified = self.alloc(Object::Str(qualname.to_owned()))?;
        let attributes: Vec<(String, Value)> = vec![
            ("__module__".into(), module),
            ("__qualname__".into(), qualified),
            ("__doc__".into(), Value::NONE),
        ];
        let dictionary_order = attributes
            .iter()
            .map(|(name, _)| ClassDictionaryKey::String(name.clone()))
            .collect();
        self.alloc(Object::Namespace(Box::new(Class {
            root: false,
            id: TypeId(0),
            version: 0,
            name: qualname.rsplit('.').next().unwrap_or(qualname).into(),
            metaclass,
            namespace_mapping: None,
            attributes,
            extra_attributes: Vec::new(),
            dictionary_order,
            bases,
            mro: Vec::new(),
            dependents: Vec::new(),
        })))
    }
    pub fn namespace_from_mapping(
        &mut self,
        qualname: &str,
        bases: Vec<Value>,
        metaclass: Value,
        mapping: Value,
    ) -> Result<Value> {
        let mapping_protocol = matches!(self.get(mapping)?, Object::Dict(_))
            || (self.special_method_call(mapping, "__getitem__")?.is_some()
                && self.special_method_call(mapping, "__setitem__")?.is_some());
        if !mapping_protocol {
            return Err(Diagnostic::new(
                "TypeError",
                "metaclass __prepare__ must return a mapping",
            ));
        }
        self.namespace_from_mapping_inner(qualname, bases, metaclass, mapping)
    }
    pub fn namespace_from_type_mapping(
        &mut self,
        name: &str,
        bases: Vec<Value>,
        metaclass: Value,
        source: Value,
    ) -> Result<Value> {
        if !matches!(self.get(source)?, Object::Dict(_)) {
            return Err(Diagnostic::new(
                "TypeError",
                "type namespace must be a dict",
            ));
        }
        // Python's three-argument type copies its input mapping. Defaults belong
        // to the class dictionary and must not mutate the caller-owned dict.
        let mapping = self.alloc(Object::Dict(Default::default()))?;
        self.dict_merge(mapping, source)?;
        for (attribute, value) in [
            ("__module__", Some("__main__")),
            ("__qualname__", Some(name)),
            ("__doc__", None),
        ] {
            if self.dict_get_str(mapping, attribute)?.is_none() {
                let value = if let Some(value) = value {
                    self.alloc(Object::Str(value.to_owned()))?
                } else {
                    Value::NONE
                };
                self.dict_set_str(mapping, attribute, value)?;
            }
        }
        self.namespace_from_mapping_inner(name, bases, metaclass, mapping)
    }
    fn namespace_from_mapping_inner(
        &mut self,
        qualname: &str,
        bases: Vec<Value>,
        metaclass: Value,
        mapping: Value,
    ) -> Result<Value> {
        let namespace = self.alloc(Object::Namespace(Box::new(Class {
            root: false,
            id: TypeId(0),
            version: 0,
            name: qualname.rsplit('.').next().unwrap_or(qualname).into(),
            metaclass,
            namespace_mapping: Some(mapping),
            attributes: Vec::new(),
            extra_attributes: Vec::new(),
            dictionary_order: Vec::new(),
            bases,
            mro: Vec::new(),
            dependents: Vec::new(),
        })))?;
        Ok(namespace)
    }
    fn class_is_subclass(&self, class: Value, target: Value) -> Result<bool> {
        let class = self.class(class)?;
        Ok(class.id == self.class(target)?.id || class.mro.contains(&target))
    }
    pub fn select_metaclass(
        &self,
        explicit: Option<Value>,
        bases: &[Value],
        default: Value,
    ) -> Result<Value> {
        self.class(default)?;
        let base_metaclass = |base| match self.try_get(base) {
            Some(Object::Class(base)) if base.metaclass != Value::UNBOUND => base.metaclass,
            // Primitive and otherwise non-class bases still have `type` as
            // their logical metaclass. Their invalid-base diagnostic belongs
            // to class completion, after the observable body has executed.
            _ => default,
        };
        let mut candidate = if let Some(explicit) = explicit {
            if !self.class_is_subclass(explicit, default)? {
                return Err(Diagnostic::new(
                    "TypeError",
                    "metaclass must derive from type",
                ));
            }
            explicit
        } else if let Some(base) = bases.first() {
            base_metaclass(*base)
        } else {
            default
        };
        for base in bases {
            let base_metaclass = base_metaclass(*base);
            if self.class_is_subclass(candidate, base_metaclass)? {
                continue;
            }
            if self.class_is_subclass(base_metaclass, candidate)? {
                candidate = base_metaclass;
                continue;
            }
            return Err(Diagnostic::new(
                "TypeError",
                "metaclass conflict: derived class requires a common metaclass subclass",
            ));
        }
        Ok(candidate)
    }
    pub fn has_custom_metaclass_hook(
        &self,
        metaclass: Value,
        default: Value,
        name: &str,
    ) -> Result<bool> {
        let metaclass_class = self.class(metaclass)?;
        for owner in std::iter::once(metaclass).chain(metaclass_class.mro.iter().copied()) {
            if owner == default {
                break;
            }
            if self
                .class(owner)?
                .attributes
                .iter()
                .any(|(attribute, _)| attribute == name)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn class(&self, value: Value) -> Result<&Class> {
        match self.get(value)? {
            Object::Class(c) => Ok(c),
            _ => Err(Diagnostic::new("TypeError", "expected a Tonic class")),
        }
    }
    pub fn namespace_get(&self, namespace: Value, name: &str) -> Result<Option<Value>> {
        let Object::Namespace(c) = self.get(namespace)? else {
            return Err(Diagnostic::new("BytecodeError", "missing class namespace"));
        };
        if let Some(mapping) = c.namespace_mapping {
            if !matches!(self.get(mapping)?, Object::Dict(_)) {
                return Err(Diagnostic::new(
                    "BytecodeError",
                    "custom class mapping requires VM protocol dispatch",
                ));
            }
            return self.dict_get_str(mapping, name);
        }
        Ok(c.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v))
    }
    pub fn namespace_set(&mut self, namespace: Value, name: &str, value: Value) -> Result<()> {
        let mapping = match self.get(namespace)? {
            Object::Namespace(class) => class.namespace_mapping,
            _ => return Err(Diagnostic::new("BytecodeError", "missing class namespace")),
        };
        if let Some(mapping) = mapping {
            if !matches!(self.get(mapping)?, Object::Dict(_)) {
                return Err(Diagnostic::new(
                    "BytecodeError",
                    "custom class mapping requires VM protocol dispatch",
                ));
            }
            return self.dict_set_str(mapping, name, value);
        }
        self.write_barrier(namespace, value);
        let Object::Namespace(c) = self.get_mut(namespace)? else {
            return Err(Diagnostic::new("BytecodeError", "missing class namespace"));
        };
        let before = c.estimated_bytes();
        if let Some((_, v)) = c.attributes.iter_mut().find(|(n, _)| n == name) {
            *v = value;
        } else {
            c.attributes.push((name.into(), value));
            c.dictionary_order
                .push(ClassDictionaryKey::String(name.into()));
        }
        self.bytes += c.estimated_bytes() - before;
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    pub fn namespace_delete(&mut self, namespace: Value, name: &str) -> Result<()> {
        let mapping = match self.get(namespace)? {
            Object::Namespace(class) => class.namespace_mapping,
            _ => return Err(Diagnostic::new("BytecodeError", "missing class namespace")),
        };
        if let Some(mapping) = mapping {
            let key = self.alloc(Object::Str(name.to_owned()))?;
            return self.dict_delete(mapping, key);
        }
        let Object::Namespace(class) = self.get_mut(namespace)? else {
            unreachable!("validated namespace changed kind")
        };
        let Some(index) = class
            .attributes
            .iter()
            .position(|(candidate, _)| candidate == name)
        else {
            return Err(Diagnostic::new(
                "NameError",
                format!("name '{name}' is not defined"),
            ));
        };
        let before = class.estimated_bytes();
        class.attributes.remove(index);
        if let Some(order) = class.dictionary_order.iter().position(
            |key| matches!(key, ClassDictionaryKey::String(candidate) if candidate == name),
        ) {
            class.dictionary_order.remove(order);
        }
        self.bytes -= before - class.estimated_bytes();
        Ok(())
    }
    /// Return the exact mapping passed to metaclass hooks. Bootstrap namespaces
    /// start in compact vector form and are materialized only when a hook needs
    /// the Python-visible mapping object.
    pub fn namespace_mapping(&mut self, namespace: Value) -> Result<Value> {
        let (mapping, attributes) = match self.get(namespace)? {
            Object::Namespace(class) => (class.namespace_mapping, class.attributes.clone()),
            _ => return Err(Diagnostic::new("BytecodeError", "missing class namespace")),
        };
        if let Some(mapping) = mapping {
            return Ok(mapping);
        }
        let mapping = self.alloc(Object::Dict(Default::default()))?;
        for (name, value) in attributes {
            self.dict_set_str(mapping, &name, value)?;
        }
        self.write_barrier(namespace, mapping);
        let Object::Namespace(class) = self.get_mut(namespace)? else {
            unreachable!("validated namespace changed kind")
        };
        class.namespace_mapping = Some(mapping);
        Ok(mapping)
    }
    pub fn namespace_backing_mapping(&self, namespace: Value) -> Result<Option<Value>> {
        match self.get(namespace)? {
            Object::Namespace(class) => Ok(class.namespace_mapping),
            _ => Err(Diagnostic::new("BytecodeError", "missing class namespace")),
        }
    }
    pub fn namespace_use_mapping(&mut self, namespace: Value, mapping: Value) -> Result<()> {
        if !matches!(self.get(mapping)?, Object::Dict(_)) {
            return Err(Diagnostic::new(
                "TypeError",
                "type.__new__() argument 3 must be a dict",
            ));
        }
        self.write_barrier(namespace, mapping);
        let Object::Namespace(class) = self.get_mut(namespace)? else {
            return Err(Diagnostic::new("BytecodeError", "missing class namespace"));
        };
        class.namespace_mapping = Some(mapping);
        Ok(())
    }
    pub fn finish_class(&mut self, namespace: Value) -> Result<()> {
        let mapping = match self.get(namespace)? {
            Object::Namespace(class) => class.namespace_mapping,
            _ => return Err(Diagnostic::new("BytecodeError", "invalid class completion")),
        };
        if let Some(mapping) = mapping {
            if !matches!(self.get(mapping)?, Object::Dict(_)) {
                return Err(Diagnostic::new(
                    "TypeError",
                    "type.__new__() argument 3 must be a dict",
                ));
            }
            let entries = self.dict_entries(mapping)?;
            for (key, value) in &entries {
                self.write_barrier_pair(namespace, *key, *value);
            }
            let mut attributes = Vec::new();
            let mut extra_attributes = Vec::new();
            let mut dictionary_order = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                match self.try_get(key) {
                    Some(Object::Str(name)) => {
                        attributes.push((name.clone(), value));
                        dictionary_order.push(ClassDictionaryKey::String(name.clone()));
                    }
                    _ => {
                        extra_attributes.push((key, value));
                        dictionary_order.push(ClassDictionaryKey::Other(key));
                    }
                }
            }
            let Object::Namespace(class) = self.get_mut(namespace)? else {
                unreachable!()
            };
            let before = class.estimated_bytes();
            class.attributes = attributes;
            class.extra_attributes = extra_attributes;
            class.dictionary_order = dictionary_order;
            class.namespace_mapping = None;
            let after = class.estimated_bytes();
            self.bytes += after.saturating_sub(before);
            self.bytes = self.bytes.saturating_sub(before.saturating_sub(after));
            self.peak_bytes = self.peak_bytes.max(self.bytes);
        }
        if let Some(function) = self.namespace_get(namespace, "__new__")? {
            if matches!(self.get(function), Ok(Object::Function { .. })) {
                let wrapper = self.alloc(Object::StaticMethod(function))?;
                self.namespace_set(namespace, "__new__", wrapper)?;
            }
        }
        let Object::Namespace(c) = self.get(namespace)? else {
            return Err(Diagnostic::new("BytecodeError", "invalid class completion"));
        };
        for (name, _) in &c.attributes {
            unsupported_hook(name)?;
        }
        if let Some((_, v)) = c.attributes.iter().find(|(n, _)| n == "__qualname__") {
            if !matches!(self.get(*v), Ok(Object::Str(_))) {
                return Err(Diagnostic::new(
                    "TypeError",
                    "__qualname__ must be a string",
                ));
            }
        }
        let mut sequences = Vec::new();
        for (i, base) in c.bases.iter().enumerate() {
            if c.bases[..i].contains(base) {
                return Err(Diagnostic::new("TypeError", "duplicate base class"));
            }
            let class = self.class(*base)?;
            let mut lineage = vec![*base];
            lineage.extend_from_slice(&class.mro);
            sequences.push(lineage);
        }
        sequences.push(c.bases.clone());
        let mut mro = Vec::new();
        while sequences.iter().any(|s| !s.is_empty()) {
            let head = sequences
                .iter()
                .filter_map(|s| s.first().copied())
                .find(|candidate| {
                    !sequences
                        .iter()
                        .any(|s| s.get(1..).is_some_and(|tail| tail.contains(candidate)))
                })
                .ok_or_else(|| {
                    Diagnostic::new(
                        "TypeError",
                        "cannot create a consistent method resolution order",
                    )
                })?;
            mro.push(head);
            for sequence in &mut sequences {
                if sequence.first() == Some(&head) {
                    sequence.remove(0);
                }
            }
        }
        let id = self
            .next_type
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("ResourceError", "type identity space exhausted"))?;
        self.next_type = id;
        let ancestors = mro.clone();
        let Object::Namespace(mut class) =
            std::mem::replace(self.get_mut(namespace)?, Object::Cell(Value::UNBOUND))
        else {
            unreachable!()
        };
        let before = class.estimated_bytes();
        class.id = TypeId(id);
        class.version = 1;
        class.mro = mro;
        self.bytes += class.estimated_bytes() - before;
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        *self.get_mut(namespace)? = Object::Class(class);
        for ancestor in ancestors {
            let Object::Class(class) = self.get_mut(ancestor)? else {
                unreachable!("validated MRO ancestor changed kind")
            };
            let old_capacity = class.dependents.capacity();
            class.dependents.push(namespace);
            self.bytes += (class.dependents.capacity() - old_capacity) * 8;
            self.peak_bytes = self.peak_bytes.max(self.bytes);
        }
        Ok(())
    }
    pub fn class_lookup(&self, owner: Value, name: &str) -> Result<Option<Value>> {
        let c = self.class(owner)?;
        for class in std::iter::once(owner).chain(c.mro.iter().copied()) {
            if let Some((_, value)) = self
                .class(class)?
                .attributes
                .iter()
                .find(|(n, _)| n == name)
            {
                return Ok(Some(*value));
            }
        }
        Ok(None)
    }
    /// Resolve an implicitly invoked protocol on the instance's class.
    /// Instance attributes are deliberately ignored, matching Python's special
    /// method lookup. Built-in descriptor kinds are returned without allocating
    /// a temporary BoundMethod object.
    pub fn special_method_call(
        &self,
        instance: Value,
        name: &str,
    ) -> Result<Option<DescriptorCall>> {
        let Ok(object) = self.get(instance) else {
            return Ok(None);
        };
        let Some(class) = object.instance_class() else {
            return Ok(None);
        };
        let Some(callable) = self.class_lookup(class, name)? else {
            return Ok(None);
        };
        self.descriptor_callable(callable, instance, class)
            .map(Some)
    }

    fn is_default_attribute_method(&self, value: Value, expected: Builtin) -> bool {
        matches!(self.get(value), Ok(Object::Builtin(actual)) if *actual == expected)
    }

    /// Resolve an attribute interception hook, excluding Tonic's canonical
    /// `object` implementation. The receiver comparison distinguishes the
    /// inherited method descriptor from static/classmethod wrappers around it.
    pub fn custom_attribute_method(
        &self,
        owner: Value,
        name: &str,
        default: Builtin,
    ) -> Result<Option<DescriptorCall>> {
        let call = if matches!(self.get(owner), Ok(Object::Class(_))) {
            self.metaclass_method_call(owner, name)?
        } else {
            self.special_method_call(owner, name)?
        };
        Ok(call.filter(|call| {
            !(call.receiver == Some(owner)
                && self.is_default_attribute_method(call.callable, default))
        }))
    }

    pub fn has_custom_getattribute(&self, owner: Value) -> bool {
        let default = if matches!(self.get(owner), Ok(Object::Class(_))) {
            Builtin::TypeGetAttribute
        } else {
            Builtin::ObjectGetAttribute
        };
        self.custom_attribute_method(owner, "__getattribute__", default)
            .is_ok_and(|call| call.is_some())
    }
    /// Resolve a protocol implemented by a class object's metaclass, binding
    /// an ordinary function to the class object just as Python's type call path
    /// does for `__init__`.
    pub fn metaclass_method_call(
        &self,
        class: Value,
        name: &str,
    ) -> Result<Option<DescriptorCall>> {
        let metaclass = self.class(class)?.metaclass;
        if metaclass == Value::UNBOUND {
            return Ok(None);
        }
        let Some(callable) = self.class_lookup(metaclass, name)? else {
            return Ok(None);
        };
        self.descriptor_callable(callable, class, metaclass)
            .map(Some)
    }
    pub fn super_value(&mut self, start_class: Value, receiver: Value) -> Result<Value> {
        self.class(start_class).map_err(|_| {
            Diagnostic::new("TypeError", "super() argument 1 must be a Tonic class")
        })?;
        let owner_class = match self.get(receiver) {
            Ok(object) if object.instance_class().is_some() => {
                object.instance_class().expect("guarded instance class")
            }
            Ok(Object::Class(_)) => receiver,
            _ => {
                return Err(Diagnostic::new(
                    "TypeError",
                    "super() argument 2 must be an instance or subclass",
                ))
            }
        };
        let owner = self.class(owner_class)?;
        if owner_class != start_class && !owner.mro.contains(&start_class) {
            return Err(Diagnostic::new(
                "TypeError",
                "super() argument 2 is not an instance or subclass of argument 1",
            ));
        }
        self.alloc(Object::Super {
            start_class,
            receiver,
        })
    }
    pub fn super_getter(&mut self, owner: Value, name: &str) -> Result<Option<DescriptorAccess>> {
        let (start_class, receiver) = match self.get(owner) {
            Ok(Object::Super {
                start_class,
                receiver,
            }) => (*start_class, *receiver),
            _ => return Ok(None),
        };
        let (owner_class, instance) = match self.get(receiver)? {
            object if object.instance_class().is_some() => (
                object.instance_class().expect("guarded instance class"),
                Some(receiver),
            ),
            Object::Class(_) => (receiver, None),
            _ => return Err(Diagnostic::new("TypeError", "invalid super receiver")),
        };
        match name {
            "__self__" => return Ok(Some(DescriptorAccess::Value(receiver))),
            "__thisclass__" => return Ok(Some(DescriptorAccess::Value(start_class))),
            "__self_class__" => return Ok(Some(DescriptorAccess::Value(owner_class))),
            _ => {}
        }
        let class = self.class(owner_class)?;
        let mut lineage = Vec::with_capacity(class.mro.len() + 1);
        lineage.push(owner_class);
        lineage.extend_from_slice(&class.mro);
        let start = lineage
            .iter()
            .position(|class| *class == start_class)
            .ok_or_else(|| Diagnostic::new("TypeError", "invalid super search class"))?;
        let mut descriptor = None;
        for class in &lineage[start + 1..] {
            if let Some((_, value)) = self
                .class(*class)?
                .attributes
                .iter()
                .find(|(attribute, _)| attribute == name)
            {
                descriptor = Some(*value);
                break;
            }
        }
        let descriptor = descriptor.ok_or_else(|| missing(name))?;
        if let Ok(Object::Property { getter, .. }) = self.get(descriptor) {
            return if let Some(instance) = instance {
                let getter = getter.ok_or_else(|| missing(name))?;
                Ok(Some(DescriptorAccess::Call {
                    callable: getter,
                    receiver: Some(instance),
                    positional: [Value::UNBOUND; 3],
                    count: 0,
                }))
            } else {
                Ok(Some(DescriptorAccess::Value(descriptor)))
            };
        }
        if let Ok(descriptor_object) = self.get(descriptor) {
            if let Some(descriptor_class) = descriptor_object.instance_class() {
                if let Some(getter) = self.class_lookup(descriptor_class, "__get__")? {
                    let call = self.descriptor_callable(getter, descriptor, descriptor_class)?;
                    return Ok(Some(DescriptorAccess::Call {
                        callable: call.callable,
                        receiver: call.receiver,
                        positional: [instance.unwrap_or(Value::NONE), owner_class, Value::UNBOUND],
                        count: 2,
                    }));
                }
            }
        }
        Ok(Some(DescriptorAccess::Value(self.bind_descriptor(
            descriptor,
            instance,
            owner_class,
        )?)))
    }
    pub fn instance(&mut self, class: Value) -> Result<Value> {
        self.class(class)?;
        self.alloc(Object::Instance {
            class,
            attributes: Attributes::default(),
            native: None,
        })
    }
    pub fn property_getter(&self, owner: Value, name: &str) -> Result<Option<Value>> {
        let Ok(object) = self.get(owner) else {
            return Ok(None);
        };
        let Some(class) = object.instance_class() else {
            return Ok(None);
        };
        let Some(value) = self.class_lookup(class, name)? else {
            return Ok(None);
        };
        match self.get(value) {
            Ok(Object::Property {
                getter: Some(f), ..
            }) => Ok(Some(*f)),
            Ok(Object::Property { getter: None, .. }) => Err(missing(name)),
            _ => Ok(None),
        }
    }
    pub fn property_setter(&self, owner: Value, name: &str) -> Result<Option<Value>> {
        let Ok(object) = self.get(owner) else {
            return Ok(None);
        };
        let Some(class) = object.instance_class() else {
            return Ok(None);
        };
        let Some(value) = self.class_lookup(class, name)? else {
            return Ok(None);
        };
        match self.get(value) {
            Ok(Object::Property {
                setter: Some(f), ..
            }) => Ok(Some(*f)),
            Ok(Object::Property { setter: None, .. }) => Err(Diagnostic::new(
                "AttributeError",
                format!("property '{name}' has no setter"),
            )),
            _ => Ok(None),
        }
    }
    pub fn property_deleter(&self, owner: Value, name: &str) -> Result<Option<Value>> {
        let Ok(object) = self.get(owner) else {
            return Ok(None);
        };
        let Some(class) = object.instance_class() else {
            return Ok(None);
        };
        let Some(value) = self.class_lookup(class, name)? else {
            return Ok(None);
        };
        match self.get(value) {
            Ok(Object::Property {
                deleter: Some(f), ..
            }) => Ok(Some(*f)),
            Ok(Object::Property { deleter: None, .. }) => Err(Diagnostic::new(
                "AttributeError",
                format!("property '{name}' has no deleter"),
            )),
            _ => Ok(None),
        }
    }
    pub fn descriptor_getter(
        &mut self,
        owner: Value,
        name: &str,
    ) -> Result<Option<DescriptorAccess>> {
        let (descriptor, instance, owner_class, instance_has_value) = match self.get(owner) {
            Ok(object) if object.instance_parts().is_some() => {
                let (class, attributes) = object.instance_parts().expect("guarded instance parts");
                let Some(descriptor) = self.class_lookup(class, name)? else {
                    return Ok(None);
                };
                (
                    descriptor,
                    owner,
                    class,
                    attributes.get(&self.shapes, name).is_some(),
                )
            }
            Ok(Object::Class(_)) => {
                let Some(descriptor) = self.class_lookup(owner, name)? else {
                    return Ok(None);
                };
                (descriptor, Value::NONE, owner, false)
            }
            _ => return Ok(None),
        };
        let descriptor_class = match self.get(descriptor) {
            Ok(object) if object.instance_class().is_some() => {
                object.instance_class().expect("guarded instance class")
            }
            _ => return Ok(None),
        };
        let setter = self.class_lookup(descriptor_class, "__set__")?;
        let deleter = self.class_lookup(descriptor_class, "__delete__")?;
        if instance_has_value && setter.is_none() && deleter.is_none() {
            return Ok(None);
        }
        let Some(getter) = self.class_lookup(descriptor_class, "__get__")? else {
            return Ok(setter
                .or(deleter)
                .map(|_| DescriptorAccess::Value(descriptor)));
        };
        let call = self.descriptor_callable(getter, descriptor, descriptor_class)?;
        Ok(Some(DescriptorAccess::Call {
            callable: call.callable,
            receiver: call.receiver,
            positional: [instance, owner_class, Value::UNBOUND],
            count: 2,
        }))
    }
    /// Resolve a descriptor stored on a class object's metaclass. Data
    /// descriptors participate before the class dictionary; non-data
    /// descriptors are consulted only after the class dictionary misses.
    pub fn metaclass_getter(
        &mut self,
        owner: Value,
        name: &str,
        data_only: bool,
    ) -> Result<Option<DescriptorAccess>> {
        let metaclass = match self.get(owner) {
            Ok(Object::Class(class)) if class.metaclass != Value::UNBOUND => class.metaclass,
            _ => return Ok(None),
        };
        let Some(descriptor) = self.class_lookup(metaclass, name)? else {
            return Ok(None);
        };
        if let Ok(Object::Property { getter, .. }) = self.get(descriptor) {
            let getter = getter.ok_or_else(|| missing(name))?;
            return Ok(Some(DescriptorAccess::Call {
                callable: getter,
                receiver: Some(owner),
                positional: [Value::UNBOUND; 3],
                count: 0,
            }));
        }
        if let Ok(descriptor_object) = self.get(descriptor) {
            if let Some(descriptor_class) = descriptor_object.instance_class() {
                let setter = self.class_lookup(descriptor_class, "__set__")?;
                let deleter = self.class_lookup(descriptor_class, "__delete__")?;
                let data = setter.is_some() || deleter.is_some();
                if data_only && !data {
                    return Ok(None);
                }
                if let Some(getter) = self.class_lookup(descriptor_class, "__get__")? {
                    let call = self.descriptor_callable(getter, descriptor, descriptor_class)?;
                    return Ok(Some(DescriptorAccess::Call {
                        callable: call.callable,
                        receiver: call.receiver,
                        positional: [owner, metaclass, Value::UNBOUND],
                        count: 2,
                    }));
                }
                if data {
                    return Ok(Some(DescriptorAccess::Value(descriptor)));
                }
            }
        }
        if data_only {
            return Ok(None);
        }
        Ok(Some(DescriptorAccess::Value(self.bind_descriptor(
            descriptor,
            Some(owner),
            metaclass,
        )?)))
    }

    pub fn metaclass_property_setter(&self, owner: Value, name: &str) -> Result<Option<Value>> {
        let metaclass = match self.get(owner) {
            Ok(Object::Class(class)) if class.metaclass != Value::UNBOUND => class.metaclass,
            _ => return Ok(None),
        };
        let Some(descriptor) = self.class_lookup(metaclass, name)? else {
            return Ok(None);
        };
        match self.get(descriptor) {
            Ok(Object::Property {
                setter: Some(setter),
                ..
            }) => Ok(Some(*setter)),
            Ok(Object::Property { setter: None, .. }) => Err(Diagnostic::new(
                "AttributeError",
                format!("property '{name}' has no setter"),
            )),
            _ => Ok(None),
        }
    }

    pub fn metaclass_property_deleter(&self, owner: Value, name: &str) -> Result<Option<Value>> {
        let metaclass = match self.get(owner) {
            Ok(Object::Class(class)) if class.metaclass != Value::UNBOUND => class.metaclass,
            _ => return Ok(None),
        };
        let Some(descriptor) = self.class_lookup(metaclass, name)? else {
            return Ok(None);
        };
        match self.get(descriptor) {
            Ok(Object::Property {
                deleter: Some(deleter),
                ..
            }) => Ok(Some(*deleter)),
            Ok(Object::Property { deleter: None, .. }) => Err(Diagnostic::new(
                "AttributeError",
                format!("property '{name}' has no deleter"),
            )),
            _ => Ok(None),
        }
    }

    pub fn metaclass_descriptor_setter(
        &self,
        owner: Value,
        name: &str,
    ) -> Result<Option<DescriptorCall>> {
        let metaclass = match self.get(owner) {
            Ok(Object::Class(class)) if class.metaclass != Value::UNBOUND => class.metaclass,
            _ => return Ok(None),
        };
        let Some(descriptor) = self.class_lookup(metaclass, name)? else {
            return Ok(None);
        };
        let descriptor_class = match self.get(descriptor) {
            Ok(object) if object.instance_class().is_some() => {
                object.instance_class().expect("guarded descriptor class")
            }
            _ => return Ok(None),
        };
        let Some(setter) = self.class_lookup(descriptor_class, "__set__")? else {
            if self.class_lookup(descriptor_class, "__delete__")?.is_some() {
                return Err(Diagnostic::new("AttributeError", "__set__"));
            }
            return Ok(None);
        };
        self.descriptor_callable(setter, descriptor, descriptor_class)
            .map(Some)
    }

    pub fn metaclass_descriptor_deleter(
        &self,
        owner: Value,
        name: &str,
    ) -> Result<Option<DescriptorCall>> {
        let metaclass = match self.get(owner) {
            Ok(Object::Class(class)) if class.metaclass != Value::UNBOUND => class.metaclass,
            _ => return Ok(None),
        };
        let Some(descriptor) = self.class_lookup(metaclass, name)? else {
            return Ok(None);
        };
        let descriptor_class = match self.get(descriptor) {
            Ok(object) if object.instance_class().is_some() => {
                object.instance_class().expect("guarded descriptor class")
            }
            _ => return Ok(None),
        };
        let Some(deleter) = self.class_lookup(descriptor_class, "__delete__")? else {
            if self.class_lookup(descriptor_class, "__set__")?.is_some() {
                return Err(Diagnostic::new("AttributeError", "__delete__"));
            }
            return Ok(None);
        };
        self.descriptor_callable(deleter, descriptor, descriptor_class)
            .map(Some)
    }
    pub fn descriptor_setter(&self, owner: Value, name: &str) -> Result<Option<DescriptorCall>> {
        let Ok(object) = self.get(owner) else {
            return Ok(None);
        };
        let Some(class) = object.instance_class() else {
            return Ok(None);
        };
        let Some(descriptor) = self.class_lookup(class, name)? else {
            return Ok(None);
        };
        let descriptor_class = match self.get(descriptor) {
            Ok(object) if object.instance_class().is_some() => {
                object.instance_class().expect("guarded instance class")
            }
            _ => return Ok(None),
        };
        let Some(setter) = self.class_lookup(descriptor_class, "__set__")? else {
            if self.class_lookup(descriptor_class, "__delete__")?.is_some() {
                return Err(Diagnostic::new("AttributeError", "__set__"));
            }
            return Ok(None);
        };
        self.descriptor_callable(setter, descriptor, descriptor_class)
            .map(Some)
    }
    pub fn descriptor_deleter(&self, owner: Value, name: &str) -> Result<Option<DescriptorCall>> {
        let Ok(object) = self.get(owner) else {
            return Ok(None);
        };
        let Some(class) = object.instance_class() else {
            return Ok(None);
        };
        let Some(descriptor) = self.class_lookup(class, name)? else {
            return Ok(None);
        };
        let descriptor_class = match self.get(descriptor) {
            Ok(object) if object.instance_class().is_some() => {
                object.instance_class().expect("guarded instance class")
            }
            _ => return Ok(None),
        };
        let Some(deleter) = self.class_lookup(descriptor_class, "__delete__")? else {
            if self.class_lookup(descriptor_class, "__set__")?.is_some() {
                return Err(Diagnostic::new("AttributeError", "__delete__"));
            }
            return Ok(None);
        };
        self.descriptor_callable(deleter, descriptor, descriptor_class)
            .map(Some)
    }
    pub fn descriptor_set_names(&mut self, owner: Value) -> Result<Vec<(DescriptorCall, Value)>> {
        let attributes = self.class(owner)?.attributes.clone();
        let mut calls = Vec::new();
        for (name, descriptor) in attributes {
            let descriptor_class = match self.get(descriptor) {
                Ok(object) if object.instance_class().is_some() => {
                    object.instance_class().expect("guarded instance class")
                }
                _ => continue,
            };
            let Some(set_name) = self.class_lookup(descriptor_class, "__set_name__")? else {
                continue;
            };
            let call = self.descriptor_callable(set_name, descriptor, descriptor_class)?;
            let name = self.alloc(Object::Str(name))?;
            calls.push((call, name));
        }
        Ok(calls)
    }
    fn descriptor_callable(
        &self,
        value: Value,
        descriptor: Value,
        descriptor_class: Value,
    ) -> Result<DescriptorCall> {
        Ok(match self.get(value) {
            Ok(Object::StaticMethod(function)) => DescriptorCall {
                callable: *function,
                receiver: None,
            },
            Ok(Object::ClassMethod(function)) => DescriptorCall {
                callable: *function,
                receiver: Some(descriptor_class),
            },
            Ok(Object::Function { .. }) => DescriptorCall {
                callable: value,
                receiver: Some(descriptor),
            },
            Ok(Object::Builtin(
                Builtin::ObjectGetAttribute
                | Builtin::ObjectSetAttr
                | Builtin::ObjectDelAttr
                | Builtin::TypeGetAttribute
                | Builtin::TypeSetAttr
                | Builtin::TypeDelAttr,
            )) => DescriptorCall {
                callable: value,
                receiver: Some(descriptor),
            },
            _ => DescriptorCall {
                callable: value,
                receiver: None,
            },
        })
    }
    fn bind_descriptor(
        &mut self,
        value: Value,
        instance: Option<Value>,
        owner_class: Value,
    ) -> Result<Value> {
        enum Binding {
            Plain,
            Static(Value),
            Class(Value),
            Instance(Value),
        }
        let binding = match self.get(value) {
            Ok(Object::StaticMethod(function)) => Binding::Static(*function),
            Ok(Object::ClassMethod(function)) => Binding::Class(*function),
            Ok(Object::Function { .. }) if instance.is_some() => Binding::Instance(value),
            Ok(Object::Builtin(
                Builtin::ObjectGetAttribute
                | Builtin::ObjectSetAttr
                | Builtin::ObjectDelAttr
                | Builtin::TypeGetAttribute
                | Builtin::TypeSetAttr
                | Builtin::TypeDelAttr,
            )) if instance.is_some() => Binding::Instance(value),
            _ => Binding::Plain,
        };
        match binding {
            Binding::Plain => Ok(value),
            Binding::Static(function) => Ok(function),
            Binding::Class(function) => self.alloc(Object::BoundMethod {
                function,
                receiver: owner_class,
            }),
            Binding::Instance(function) => self.alloc(Object::BoundMethod {
                function,
                receiver: instance.expect("instance binding"),
            }),
        }
    }
    pub fn attr(&mut self, owner: Value, name: &str) -> Result<Value> {
        let value = match self.get(owner) {
            Ok(Object::Module(values)) => {
                return values
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| *v)
                    .ok_or_else(|| missing(name))
            }
            Ok(Object::Class(c)) => {
                match name {
                    "__class__" if c.metaclass != Value::UNBOUND => return Ok(c.metaclass),
                    "__name__" => {
                        let name = c.name.clone();
                        return self.alloc(Object::Str(name));
                    }
                    "__bases__" => {
                        let bases = c.bases.clone();
                        return self.alloc(Object::Tuple(bases));
                    }
                    "__mro__" => {
                        let mut mro = vec![owner];
                        mro.extend_from_slice(&c.mro);
                        return self.alloc(Object::Tuple(mro));
                    }
                    "__dict__" => return self.alloc(Object::MappingProxy { class: owner }),
                    _ => {}
                }
                let Some(value) = self.class_lookup(owner, name)? else {
                    return Err(missing(name));
                };
                return self.bind_descriptor(value, None, owner);
            }
            Ok(Object::Instance {
                class, attributes, ..
            }) => {
                let class = *class;
                if name == "__class__" {
                    return Ok(class);
                }
                if name == "__dict__" {
                    return Err(Diagnostic::new(
                        "UnsupportedFeature",
                        "instance __dict__ view is not implemented",
                    ));
                }
                if let Some(value) = attributes.get(&self.shapes, name) {
                    return Ok(value);
                }
                let Some(value) = self.class_lookup(class, name)? else {
                    return Err(missing(name));
                };
                return self.bind_descriptor(value, Some(owner), class);
            }
            Ok(Object::Exception {
                class,
                arguments,
                attributes,
                cause,
                context,
                suppress_context,
                traceback,
                ..
            }) => {
                let class = *class;
                match name {
                    "__class__" => return Ok(class),
                    "__cause__" => return Ok(cause.unwrap_or(Value::NONE)),
                    "__context__" => return Ok(context.unwrap_or(Value::NONE)),
                    "__suppress_context__" => return Ok(Value::bool(*suppress_context)),
                    "__traceback__" => return Ok(traceback.unwrap_or(Value::NONE)),
                    "args" => {
                        return self.alloc(Object::Tuple(arguments.clone()));
                    }
                    "__dict__" => {
                        return Err(Diagnostic::new(
                            "UnsupportedFeature",
                            "exception __dict__ view is not implemented",
                        ))
                    }
                    _ => {}
                }
                if let Some(value) = attributes.get(&self.shapes, name) {
                    return Ok(value);
                }
                let Some(value) = self.class_lookup(class, name)? else {
                    return Err(missing(name));
                };
                return self.bind_descriptor(value, Some(owner), class);
            }
            Ok(Object::BoundMethod { function, receiver }) => match name {
                "__func__" => Some(*function),
                "__self__" => Some(*receiver),
                _ => None,
            },
            Ok(Object::StaticMethod(function)) | Ok(Object::ClassMethod(function)) => match name {
                "__func__" => Some(*function),
                _ => None,
            },
            Ok(Object::Property {
                getter,
                setter,
                deleter,
            }) => match name {
                "fget" => Some(getter.unwrap_or(Value::NONE)),
                "fset" => Some(setter.unwrap_or(Value::NONE)),
                "fdel" => Some(deleter.unwrap_or(Value::NONE)),
                "setter" => return self.alloc(Object::PropertySetter(owner)),
                "deleter" => return self.alloc(Object::PropertyDeleter(owner)),
                _ => None,
            },
            _ => None,
        };
        value.ok_or_else(|| missing(name))
    }
    /// Returns an allocation-free description when ordinary instance/class
    /// access resolves directly to a Tonic function wrapper. Instance
    /// shadowing and every custom descriptor remain on the generic path.
    pub fn direct_method(
        &self,
        owner: Value,
        name: &str,
    ) -> Option<(Value, DirectMethodKind, Option<Value>)> {
        if self.has_custom_getattribute(owner) {
            return None;
        }
        let (class, instance) = match self.get(owner).ok()? {
            object if object.instance_parts().is_some() => {
                let (class, attributes) = object.instance_parts().expect("guarded instance parts");
                if attributes.get(&self.shapes, name).is_some() {
                    return None;
                }
                (class, true)
            }
            Object::Class(_) => (owner, false),
            _ => return None,
        };
        let descriptor = self.class_lookup(class, name).ok()??;
        match self.get(descriptor).ok()? {
            Object::Function { .. } if instance => {
                Some((descriptor, DirectMethodKind::Instance, Some(owner)))
            }
            Object::Function { .. } => Some((descriptor, DirectMethodKind::Static, None)),
            Object::StaticMethod(function)
                if matches!(self.get(*function), Ok(Object::Function { .. })) =>
            {
                Some((*function, DirectMethodKind::Static, None))
            }
            Object::ClassMethod(function)
                if matches!(self.get(*function), Ok(Object::Function { .. })) =>
            {
                Some((*function, DirectMethodKind::Class, Some(class)))
            }
            _ => None,
        }
    }
    pub fn instance_slot_cache(
        &self,
        owner: Value,
        name: &str,
    ) -> Option<(Value, ShapeId, usize, u64)> {
        if self.has_custom_getattribute(owner) {
            return None;
        }
        let (class, attributes) = self.get(owner).ok()?.instance_parts()?;
        if matches!(
            name,
            "__class__"
                | "__cause__"
                | "__context__"
                | "__suppress_context__"
                | "__traceback__"
                | "args"
        ) {
            return None;
        }
        // A class value that is inert today may become a data descriptor if its
        // own class is mutated later. Tracking that transitive dependency would
        // make the common slot guard expensive, so only cache names absent from
        // the full instance MRO.
        if self.class_lookup(class, name).ok().flatten().is_some() {
            return None;
        }
        let (shape, slot) = attributes.slot_cache(&self.shapes, name)?;
        Some((class, shape, slot, self.class(class).ok()?.version))
    }
    pub fn cached_instance_slot(
        &self,
        owner: Value,
        class: Value,
        shape: ShapeId,
        slot: usize,
        epoch: u64,
    ) -> Option<Value> {
        let (actual, attributes) = self.get(owner).ok()?.instance_parts()?;
        if actual != class || self.class(actual).ok()?.version != epoch {
            return None;
        }
        attributes.cached_slot(shape, slot)
    }
    pub fn set_attr(&mut self, owner: Value, name: &str, value: Value) -> Result<()> {
        if let Some(class) = self.get(owner)?.instance_class() {
            if self.class(class)?.root {
                return Err(missing(name));
            }
            if matches!(
                name,
                "__class__"
                    | "__dict__"
                    | "__cause__"
                    | "__context__"
                    | "__suppress_context__"
                    | "__traceback__"
                    | "args"
            ) {
                return Err(Diagnostic::new(
                    "UnsupportedFeature",
                    "instance layout replacement is not implemented",
                ));
            }
            self.write_barrier(owner, value);
            let attributes = self
                .get_mut(owner)?
                .instance_attributes_mut()
                .expect("guarded instance attributes");
            let mut attrs = std::mem::take(attributes);
            let before = attrs.estimated_bytes();
            attrs.set(&mut self.shapes, name, value);
            self.bytes += attrs.estimated_bytes() - before;
            self.peak_bytes = self.peak_bytes.max(self.bytes);
            let attributes = self
                .get_mut(owner)?
                .instance_attributes_mut()
                .expect("guarded instance attributes");
            *attributes = attrs;
            return Ok(());
        }
        unsupported_hook(name)?;
        if matches!(name, "__name__" | "__qualname__") {
            return Err(Diagnostic::new(
                "UnsupportedFeature",
                "class naming mutation is not implemented",
            ));
        }
        let Object::Class(c) = self.get_mut(owner)? else {
            return Err(missing(name));
        };
        if c.root {
            return Err(Diagnostic::new(
                "TypeError",
                "cannot modify immutable object type",
            ));
        }
        self.write_barrier(owner, value);
        let (owner_location, dependents) = self.class_invalidation_targets(owner)?;
        let Object::Class(c) = self.get_mut(owner)? else {
            unreachable!()
        };
        let before = c.estimated_bytes();
        if let Some((_, old)) = c.attributes.iter_mut().find(|(n, _)| n == name) {
            *old = value;
        } else {
            c.attributes.push((name.into(), value));
            c.dictionary_order
                .push(ClassDictionaryKey::String(name.into()));
        }
        let growth = c.estimated_bytes() - before;
        self.apply_class_invalidation(owner_location, &dependents);
        self.bytes += growth;
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    pub fn del_attr(&mut self, owner: Value, name: &str) -> Result<()> {
        if let Some(class) = self.get(owner)?.instance_class() {
            if self.class(class)?.root
                || matches!(
                    name,
                    "__class__"
                        | "__dict__"
                        | "__cause__"
                        | "__context__"
                        | "__suppress_context__"
                        | "__traceback__"
                        | "args"
                )
            {
                return Err(missing(name));
            }
            let attributes = self
                .get_mut(owner)?
                .instance_attributes_mut()
                .expect("guarded instance attributes");
            let mut attrs = std::mem::take(attributes);
            let before = attrs.estimated_bytes();
            if attrs.delete(&self.shapes, name).is_none() {
                let attributes = self
                    .get_mut(owner)?
                    .instance_attributes_mut()
                    .expect("guarded instance attributes");
                *attributes = attrs;
                return Err(missing(name));
            }
            let after = attrs.estimated_bytes();
            self.bytes = self.bytes.saturating_sub(before.saturating_sub(after));
            let attributes = self
                .get_mut(owner)?
                .instance_attributes_mut()
                .expect("guarded instance attributes");
            *attributes = attrs;
            return Ok(());
        }
        let Object::Class(c) = self.get_mut(owner)? else {
            return Err(missing(name));
        };
        if c.root || matches!(name, "__name__" | "__qualname__" | "__bases__" | "__mro__") {
            return Err(Diagnostic::new(
                "TypeError",
                "cannot delete immutable class attribute",
            ));
        }
        let Some(index) = c
            .attributes
            .iter()
            .position(|(attribute, _)| attribute == name)
        else {
            return Err(missing(name));
        };
        let (owner_location, dependents) = self.class_invalidation_targets(owner)?;
        let Object::Class(c) = self.get_mut(owner)? else {
            unreachable!()
        };
        let before = c.estimated_bytes();
        c.attributes.remove(index);
        c.dictionary_order.retain(
            |key| !matches!(key, ClassDictionaryKey::String(attribute) if attribute == name),
        );
        let after = c.estimated_bytes();
        self.apply_class_invalidation(owner_location, &dependents);
        self.bytes = self.bytes.saturating_sub(before.saturating_sub(after));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Builtin;
    #[test]
    fn type_identity_version_and_mro_survive_heap_movement() {
        let mut heap = Heap::default();
        heap.alloc(Object::Str("dead".into())).unwrap();
        let a = heap.namespace("A", Vec::new()).unwrap();
        heap.finish_class(a).unwrap();
        let b = heap.namespace("B", vec![a]).unwrap();
        heap.finish_class(b).unwrap();
        let c = heap.namespace("C", Vec::new()).unwrap();
        heap.finish_class(c).unwrap();
        let a_id = heap.class(a).unwrap().id;
        let b_id = heap.class(b).unwrap().id;
        assert_ne!(a_id, b_id);
        let a_version = heap.class(a).unwrap().version;
        let b_version = heap.class(b).unwrap().version;
        let c_version = heap.class(c).unwrap().version;
        heap.set_attr(a, "x", Value::int(1).unwrap()).unwrap();
        assert!(heap.class(a).unwrap().version > a_version);
        assert!(heap.class(b).unwrap().version > b_version);
        assert_eq!(heap.class(c).unwrap().version, c_version);
        assert_eq!(heap.class_lookup(b, "x").unwrap(), Value::int(1));
        assert!(heap.collect([b, c]).unwrap().moved > 0);
        assert_eq!(heap.class(a).unwrap().id, a_id);
        assert_eq!(heap.class(b).unwrap().id, b_id);
        heap.set_attr(a, "x", Value::int(2).unwrap()).unwrap();
        assert_eq!(heap.class_lookup(b, "x").unwrap(), Value::int(2));
    }
    #[test]
    fn exhausted_type_ids_do_not_wrap() {
        let mut heap = Heap::default();
        heap.next_type = u32::MAX;
        let namespace = heap.namespace("C", Vec::new()).unwrap();
        assert!(heap.finish_class(namespace).is_err());
        assert!(matches!(heap.get(namespace), Ok(Object::Namespace(_))));
        assert_eq!(heap.collect([]).unwrap().survivors, 0);
    }
    #[test]
    fn class_dependency_links_are_weak_and_pruned_by_collection() {
        let mut heap = Heap::default();
        let base = heap.namespace("Base", Vec::new()).unwrap();
        heap.finish_class(base).unwrap();
        let child = heap.namespace("Child", vec![base]).unwrap();
        heap.finish_class(child).unwrap();
        assert_eq!(heap.class(base).unwrap().dependents, vec![child]);
        assert_eq!(heap.collect([base]).unwrap().reclaimed, 3);
        assert!(heap.class(base).unwrap().dependents.is_empty());
        heap.set_attr(base, "x", Value::int(1).unwrap()).unwrap();
    }
    #[test]
    fn class_namespace_and_instance_write_barriers_retain_young_values() {
        let mut heap = Heap::default();
        let object_new = heap.alloc(Object::Builtin(Builtin::ObjectNew)).unwrap();
        let object_getattribute = heap
            .alloc(Object::Builtin(Builtin::ObjectGetAttribute))
            .unwrap();
        let object_setattr = heap.alloc(Object::Builtin(Builtin::ObjectSetAttr)).unwrap();
        let object_delattr = heap.alloc(Object::Builtin(Builtin::ObjectDelAttr)).unwrap();
        let object = heap
            .root_object_class(
                object_new,
                object_getattribute,
                object_setattr,
                object_delattr,
            )
            .unwrap();
        let namespace = heap.namespace("C", vec![object]).unwrap();
        heap.collect_young([object, namespace]).unwrap();
        let namespace_value = heap.alloc(Object::Str("namespace".into())).unwrap();
        heap.namespace_set(namespace, "pending", namespace_value)
            .unwrap();
        heap.assert_remembered_set_complete();
        heap.collect_young([object, namespace]).unwrap();
        assert!(heap.get(namespace_value).is_ok());

        heap.finish_class(namespace).unwrap();
        let instance = heap.instance(namespace).unwrap();
        heap.collect_young([object, namespace, instance]).unwrap();
        let class_value = heap.alloc(Object::List(vec![])).unwrap();
        let instance_value = heap.alloc(Object::List(vec![])).unwrap();
        heap.set_attr(namespace, "shared", class_value).unwrap();
        heap.set_attr(instance, "value", instance_value).unwrap();
        heap.assert_remembered_set_complete();
        let minor = heap.collect_young([object, namespace, instance]).unwrap();
        assert_eq!(minor.promoted, 2);
        assert!(heap.get(class_value).is_ok());
        assert!(heap.get(instance_value).is_ok());
    }
}
