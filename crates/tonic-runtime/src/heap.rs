#[path = "gc.rs"]
mod gc;
use crate::{classes::ClassDictionaryKey, value::Value};
pub use gc::CollectionStats;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::collections::HashSet;
use tonic_core::diagnostic::{Diagnostic, Result, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TracebackEntry {
    pub function: String,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Print,
    Len,
    Abs,
    IsInstance,
    IsSubclass,
    GetAttr,
    SetAttr,
    DelAttr,
    HasAttr,
    StaticMethod,
    ClassMethod,
    Property,
    Super,
    ObjectNew,
    ObjectGetAttribute,
    ObjectSetAttr,
    ObjectDelAttr,
    TypeNew,
    TypeGetAttribute,
    TypeSetAttr,
    TypeDelAttr,
}
#[derive(Debug)]
pub(crate) enum Object {
    Class(Box<crate::classes::Class>),
    Namespace(Box<crate::classes::Class>),
    MappingProxy {
        class: Value,
    },
    Instance {
        class: Value,
        attributes: crate::shapes::Attributes,
        /// Exact builtin storage retained behind a user-visible subclass
        /// instance.  The indirection keeps builtin layouts compact while the
        /// outer object owns class identity and instance attributes.
        native: Option<Value>,
    },
    BoundMethod {
        function: Value,
        receiver: Value,
    },
    StaticMethod(Value),
    ClassMethod(Value),
    Property {
        getter: Option<Value>,
        setter: Option<Value>,
        deleter: Option<Value>,
    },
    PropertySetter(Value),
    PropertyDeleter(Value),
    Super {
        start_class: Value,
        receiver: Value,
    },
    Int(BigInt),
    Float(f64),
    Buffer(crate::buffer::Buffer),
    Foreign(crate::foreign::ForeignObject),
    Str(String),
    Tuple(Vec<Value>),
    List(Vec<Value>),
    Slice([Value; 3]),
    Dict(crate::dict::Dict),
    Exception {
        class: Value,
        message: String,
        arguments: Vec<Value>,
        attributes: crate::shapes::Attributes,
        cause: Option<Value>,
        context: Option<Value>,
        suppress_context: bool,
        traceback: Option<Value>,
    },
    Traceback {
        entries: Vec<TracebackEntry>,
    },
    Function {
        code: u16,
        execution: u64,
        captures: Vec<Value>,
        defaults: Vec<Value>,
    },
    Cell(Value),
    Builtin(Builtin),
    Native(usize),
    Module(Vec<(String, Value)>),
    Range {
        start: i64,
        stop: i64,
        step: i64,
    },
    Iterator {
        source: Value,
        index: usize,
    },
    DictIterator {
        source: Value,
        index: usize,
        version: u64,
    },
    MappingProxyIterator {
        class: Value,
        index: usize,
        size: usize,
    },
    RangeIterator {
        next: i128,
        stop: i128,
        step: i128,
    },
}
impl Object {
    pub(crate) fn instance_class(&self) -> Option<Value> {
        match self {
            Self::Instance { class, .. } | Self::Exception { class, .. } => Some(*class),
            _ => None,
        }
    }

    pub(crate) fn instance_parts(&self) -> Option<(Value, &crate::shapes::Attributes)> {
        match self {
            Self::Instance {
                class, attributes, ..
            }
            | Self::Exception {
                class, attributes, ..
            } => Some((*class, attributes)),
            _ => None,
        }
    }

    pub(crate) fn instance_attributes_mut(&mut self) -> Option<&mut crate::shapes::Attributes> {
        match self {
            Self::Instance { attributes, .. } | Self::Exception { attributes, .. } => {
                Some(attributes)
            }
            _ => None,
        }
    }

    /// Every managed edge must be visible to precise tracing, including cycles.
    pub fn trace(&self, mut visit: impl FnMut(Value)) {
        match self {
            Self::Class(c) | Self::Namespace(c) => c.trace(visit),
            Self::MappingProxy { class } | Self::MappingProxyIterator { class, .. } => {
                visit(*class)
            }
            Self::Instance {
                class,
                attributes,
                native,
            } => {
                visit(*class);
                attributes.trace(&mut visit);
                native.iter().copied().for_each(visit);
            }
            Self::Exception {
                class,
                cause,
                context,
                traceback,
                arguments,
                attributes,
                ..
            } => {
                visit(*class);
                attributes.trace(&mut visit);
                arguments.iter().copied().for_each(&mut visit);
                cause
                    .iter()
                    .chain(context)
                    .chain(traceback)
                    .copied()
                    .for_each(visit);
            }
            Self::BoundMethod { function, receiver } => {
                visit(*function);
                visit(*receiver);
            }
            Self::StaticMethod(function) | Self::ClassMethod(function) => visit(*function),
            Self::Property {
                getter,
                setter,
                deleter,
            } => getter
                .iter()
                .chain(setter)
                .chain(deleter)
                .copied()
                .for_each(visit),
            Self::PropertySetter(property) | Self::PropertyDeleter(property) => visit(*property),
            Self::Super {
                start_class,
                receiver,
            } => {
                visit(*start_class);
                visit(*receiver);
            }
            Self::Tuple(v) | Self::List(v) => v.iter().copied().for_each(visit),
            Self::Slice(v) => v.iter().copied().for_each(visit),
            Self::Module(m) => m.iter().for_each(|(_, v)| visit(*v)),
            Self::Iterator { source, .. } => visit(*source),
            Self::Function {
                captures, defaults, ..
            } => captures.iter().chain(defaults).copied().for_each(visit),
            Self::Dict(dict) => {
                for (key, value) in &dict.entries {
                    visit(*key);
                    visit(*value);
                }
            }
            Self::DictIterator { source, .. } => visit(*source),
            Self::Cell(value) => visit(*value),
            Self::Foreign(foreign) => foreign.trace(visit),
            _ => {}
        }
    }
}
/// Stable generational logical slots point into compactable object storage.
/// Collection only occurs at explicit VM safepoints, never during allocation.
struct Slot {
    generation: u32,
    location: Option<usize>,
}
struct HeapObject {
    object: Object,
    slot: u32,
    young: bool,
}
#[derive(Default)]
pub(crate) struct Heap {
    pub shapes: crate::shapes::Shapes,
    pub next_type: u32,
    objects: Vec<HeapObject>,
    slots: Vec<Slot>,
    free: Vec<u32>,
    remembered: HashSet<u32>,
    pub allocations: u64,
    pub bytes: usize,
    pub peak_bytes: usize,
    pub last_collection_allocations: u64,
    pub buffer_exports: u64,
    pub buffer_copies: u64,
    pub foreign_wrapper_creations: u64,
    pub foreign_trace_calls: u64,
    pub foreign_destructor_calls: u64,
    pub foreign_destructor_panics: u64,
    pending_foreign: Vec<crate::foreign::PendingForeign>,
}
impl Heap {
    /// Return the exact builtin backing value for a builtin subclass instance.
    /// Native payloads never nest, so one lookup is sufficient and keeps the
    /// exact builtin fast path unchanged.
    pub(crate) fn native_value(&self, value: Value) -> Value {
        match self.try_get(value) {
            Some(Object::Instance {
                native: Some(native),
                ..
            }) => *native,
            _ => value,
        }
    }

    pub(crate) fn native_instance(&mut self, class: Value, native: Value) -> Result<Value> {
        self.class(class)?;
        self.alloc(Object::Instance {
            class,
            attributes: Default::default(),
            native: Some(native),
        })
    }

    pub(crate) fn refresh_foreign_references(
        &mut self,
        handles: &mut crate::native::HandleTable,
    ) -> Result<u64> {
        let mut trace_calls = 0;
        for entry in &mut self.objects {
            let Object::Foreign(foreign) = &mut entry.object else {
                continue;
            };
            let called = u64::from(foreign.has_trace());
            self.foreign_trace_calls += called;
            trace_calls += called;
            let removed = foreign.refresh(handles)?;
            for handle in removed {
                handles.release_foreign_reference(handle)?;
            }
        }
        let mut remember = Vec::new();
        for entry in &self.objects {
            if entry.young {
                continue;
            }
            let Object::Foreign(foreign) = &entry.object else {
                continue;
            };
            let has_young_reference = foreign.references().iter().any(|value| {
                self.location(*value)
                    .is_some_and(|location| self.objects[location].young)
            });
            if has_young_reference {
                remember.push(entry.slot);
            }
        }
        self.remembered.extend(remember);
        Ok(trace_calls)
    }

    pub(crate) fn drain_foreign_finalizers(
        &mut self,
        handles: &mut crate::native::HandleTable,
    ) -> (u64, u64) {
        let mut destructor_calls = 0;
        let mut destructor_panics = 0;
        for mut pending in self.pending_foreign.drain(..) {
            if pending.will_destroy() {
                self.foreign_destructor_calls += 1;
                destructor_calls += 1;
            }
            if pending.run() {
                self.foreign_destructor_panics += 1;
                destructor_panics += 1;
            }
            // Trace edges are finalization roots: foreign code sees its complete
            // managed graph until destruction returns. Reclamation remains a
            // separate step after the logical finalizer has run.
            for handle in &pending.reference_handles {
                let _ = handles.release_foreign_reference(*handle);
            }
        }
        (destructor_calls, destructor_panics)
    }
    /// Owner-aware mutation boundary for the native module registry.
    pub fn add_module_member(&mut self, owner: Value, name: &str, value: Value) -> Result<()> {
        self.write_barrier(owner, value);
        let Object::Module(members) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected module"));
        };
        let before = members.capacity() * std::mem::size_of::<(String, Value)>();
        let name = name.to_owned();
        let name_bytes = name.capacity();
        members.push((name, value));
        self.bytes +=
            members.capacity() * std::mem::size_of::<(String, Value)>() - before + name_bytes;
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    pub fn cell(&self, owner: Value) -> Result<Value> {
        match self.get(owner)? {
            Object::Cell(value) => Ok(*value),
            _ => Err(Diagnostic::new("BytecodeError", "expected closure cell")),
        }
    }
    /// Owner-aware write barrier boundary for captured binding mutation.
    pub fn store_cell(&mut self, owner: Value, value: Value) -> Result<()> {
        self.write_barrier(owner, value);
        match self.get_mut(owner)? {
            Object::Cell(slot) => {
                *slot = value;
                Ok(())
            }
            _ => Err(Diagnostic::new("BytecodeError", "expected closure cell")),
        }
    }
    pub fn set_exception_cause(
        &mut self,
        owner: Value,
        cause: Option<Value>,
        suppress_context: bool,
    ) -> Result<()> {
        if let Some(cause) = cause {
            self.write_barrier(owner, cause);
        }
        let Object::Exception {
            cause: slot,
            suppress_context: suppress,
            ..
        } = self.get_mut(owner)?
        else {
            return Err(Diagnostic::new("TypeError", "expected exception instance"));
        };
        *slot = cause;
        *suppress = suppress_context;
        Ok(())
    }
    pub fn set_exception_context(&mut self, owner: Value, context: Value) -> Result<()> {
        self.write_barrier(owner, context);
        let Object::Exception { context: slot, .. } = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected exception instance"));
        };
        if slot.is_none() {
            *slot = Some(context);
        }
        Ok(())
    }
    pub fn exception_traceback(&self, owner: Value) -> Result<Option<Value>> {
        let Object::Exception { traceback, .. } = self.get(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected exception instance"));
        };
        Ok(*traceback)
    }
    pub fn record_exception_trace(
        &mut self,
        owner: Value,
        entries: Vec<TracebackEntry>,
    ) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let traceback = match self.exception_traceback(owner)? {
            Some(traceback) => traceback,
            None => {
                let traceback = self.alloc(Object::Traceback {
                    entries: Vec::new(),
                })?;
                self.write_barrier(owner, traceback);
                let Object::Exception {
                    traceback: slot, ..
                } = self.get_mut(owner)?
                else {
                    unreachable!("validated exception changed kind")
                };
                *slot = Some(traceback);
                traceback
            }
        };
        let before = self.get(traceback)?.estimated_bytes();
        {
            let Object::Traceback {
                entries: traceback_entries,
            } = self.get_mut(traceback)?
            else {
                return Err(Diagnostic::new("RuntimeError", "invalid traceback object"));
            };
            for entry in entries {
                if traceback_entries.last() != Some(&entry) {
                    traceback_entries.push(entry);
                }
            }
        }
        let after = self.get(traceback)?.estimated_bytes();
        self.bytes += after.saturating_sub(before);
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    /// Managed mutation boundary. Native extensions and the VM cannot bypass
    /// the owner/value write barrier by mutating list storage directly.
    pub fn append_list(&mut self, owner: Value, value: Value) -> Result<()> {
        let owner = self.native_value(owner);
        self.write_barrier(owner, value);
        let Object::List(values) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected list"));
        };
        let old_capacity = values.capacity();
        values.push(value);
        let growth = values.capacity() - old_capacity;
        self.bytes += growth * std::mem::size_of::<Value>();
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    pub fn alloc(&mut self, object: Object) -> Result<Value> {
        let slot = if let Some(slot) = self.free.pop() {
            slot
        } else {
            let slot = u32::try_from(self.slots.len())
                .map_err(|_| Diagnostic::new("MemoryError", "heap handle capacity exhausted"))?;
            self.slots.push(Slot {
                generation: 1,
                location: None,
            });
            slot
        };
        self.bytes += object.estimated_bytes();
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        self.slots[slot as usize].location = Some(self.objects.len());
        self.objects.push(HeapObject {
            object,
            slot,
            young: true,
        });
        self.allocations += 1;
        Ok(Value::heap(slot, self.slots[slot as usize].generation))
    }
    fn location(&self, value: Value) -> Option<usize> {
        let slot = self.slots.get(value.heap_index()?)?;
        if slot.generation != value.generation() {
            return None;
        }
        slot.location
    }
    pub(crate) fn try_get(&self, value: Value) -> Option<&Object> {
        self.objects
            .get(self.location(value)?)
            .map(|entry| &entry.object)
    }
    pub(crate) fn write_barrier(&mut self, owner: Value, value: Value) {
        let (Some(owner_location), Some(value_location)) =
            (self.location(owner), self.location(value))
        else {
            return;
        };
        if !self.objects[owner_location].young && self.objects[value_location].young {
            self.remembered.insert(self.objects[owner_location].slot);
        }
    }
    pub(crate) fn write_barrier_pair(&mut self, owner: Value, first: Value, second: Value) {
        self.write_barrier(owner, first);
        self.write_barrier(owner, second);
    }
    #[cfg(test)]
    pub(crate) fn assert_remembered_set_complete(&self) {
        for entry in &self.objects {
            if entry.young {
                continue;
            }
            let mut has_young_child = false;
            entry.object.trace(|value| {
                if let Some(location) = self.location(value) {
                    has_young_child |= self.objects[location].young;
                }
            });
            assert!(
                !has_young_child || self.remembered.contains(&entry.slot),
                "old object slot {} has an unremembered nursery edge",
                entry.slot
            );
        }
    }
    pub fn get(&self, value: Value) -> Result<&Object> {
        self.try_get(value).ok_or_else(|| invalid_value(value))
    }
    pub fn get_mut(&mut self, value: Value) -> Result<&mut Object> {
        let location = self.location(value).ok_or_else(|| invalid_value(value))?;
        Ok(&mut self.objects[location].object)
    }
    pub(crate) fn foreign_payload(&self, value: Value, adapter_id: u64) -> Result<usize> {
        let Object::Foreign(foreign) = self.get(value)? else {
            return Err(Diagnostic::new("TypeError", "expected foreign object"));
        };
        if foreign.adapter_id != adapter_id {
            return Err(Diagnostic::new(
                "TypeError",
                "foreign object belongs to a different adapter",
            ));
        }
        Ok(foreign.payload())
    }
    pub(crate) fn class_invalidation_targets(&self, owner: Value) -> Result<(usize, Vec<usize>)> {
        let owner_location = self.location(owner).ok_or_else(|| invalid_value(owner))?;
        let Object::Class(owner_class) = &self.objects[owner_location].object else {
            return Err(Diagnostic::new("TypeError", "expected a Tonic class"));
        };
        owner_class
            .version
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("ResourceError", "class version exhausted"))?;
        let mut dependents_to_invalidate = Vec::new();
        for dependent in &owner_class.dependents {
            let Some(location) = self.location(*dependent) else {
                continue;
            };
            let Object::Class(class) = &self.objects[location].object else {
                continue;
            };
            class
                .version
                .checked_add(1)
                .ok_or_else(|| Diagnostic::new("ResourceError", "class version exhausted"))?;
            dependents_to_invalidate.push(location);
        }
        Ok((owner_location, dependents_to_invalidate))
    }
    pub(crate) fn apply_class_invalidation(&mut self, owner_location: usize, dependents: &[usize]) {
        let Object::Class(owner) = &mut self.objects[owner_location].object else {
            unreachable!("validated class invalidation owner changed kind")
        };
        owner.version += 1;
        for location in dependents {
            let Object::Class(class) = &mut self.objects[*location].object else {
                unreachable!("validated class invalidation target changed kind")
            };
            class.version += 1;
        }
    }
    pub fn live_objects(&self) -> usize {
        self.objects.len()
    }
    pub fn int(&mut self, n: BigInt) -> Result<Value> {
        if let Some(value) = n.to_i64().and_then(Value::int) {
            Ok(value)
        } else {
            self.alloc(Object::Int(n))
        }
    }
    pub fn i64(&mut self, n: i64) -> Result<Value> {
        if let Some(v) = Value::int(n) {
            Ok(v)
        } else {
            self.int(BigInt::from(n))
        }
    }
    pub fn integer(&self, v: Value) -> Result<BigInt> {
        let v = self.native_value(v);
        if let Some(n) = v.integer() {
            return Ok(n.into());
        }
        match self.get(v)? {
            Object::Int(n) => Ok(n.clone()),
            _ => Err(Diagnostic::new("TypeError", "expected integer")),
        }
    }
    pub fn to_i64(&self, v: Value) -> Result<i64> {
        if let Some(n) = v.integer() {
            return Ok(n);
        }
        self.integer(v)?
            .to_i64()
            .ok_or_else(|| Diagnostic::new("OverflowError", "integer does not fit i64"))
    }
    pub fn float(&self, v: Value) -> Result<f64> {
        let v = self.native_value(v);
        if let Some(n) = v.integer() {
            return Ok(n as f64);
        }
        match self.get(v)? {
            Object::Int(n) => n.to_f64().filter(|n| n.is_finite()).ok_or_else(|| {
                Diagnostic::new("OverflowError", "integer too large to convert to float")
            }),
            Object::Float(n) => Ok(*n),
            _ => Err(Diagnostic::new("TypeError", "expected number")),
        }
    }
    pub fn is_integer(&self, v: Value) -> bool {
        let v = self.native_value(v);
        v.integer().is_some() || self.try_get(v).is_some_and(|o| matches!(o, Object::Int(_)))
    }
    pub fn is_float(&self, v: Value) -> bool {
        let v = self.native_value(v);
        self.try_get(v)
            .is_some_and(|o| matches!(o, Object::Float(_)))
    }
    pub fn truth(&self, v: Value) -> Result<bool> {
        let v = self.native_value(v);
        if let Some(n) = v.integer() {
            return Ok(n != 0);
        }
        if v == Value::NONE {
            return Ok(false);
        }
        if v == Value::NOT_IMPLEMENTED {
            return Err(Diagnostic::new(
                "TypeError",
                "NotImplemented should not be used in a boolean context",
            ));
        }
        Ok(match self.get(v)? {
            Object::Int(n) => !n.is_zero(),
            Object::Float(n) => *n != 0.0,
            Object::Str(s) => !s.is_empty(),
            Object::Tuple(v) | Object::List(v) => !v.is_empty(),
            Object::Buffer(buffer) => buffer.len() != 0,
            Object::Dict(dict) => !dict.entries.is_empty(),
            Object::MappingProxy { class } => self.class(*class)?.dictionary_len() != 0,
            Object::Range { start, stop, step } => {
                if *step > 0 {
                    start < stop
                } else {
                    start > stop
                }
            }
            _ => true,
        })
    }
    pub fn format(&self, v: Value, repr: bool) -> Result<String> {
        self.format_depth(v, repr, &mut Vec::new())
    }
    pub(crate) fn exception_message(&self, arguments: &[Value]) -> Result<String> {
        match arguments {
            [] => Ok(String::new()),
            [value] => self.format(*value, false),
            values => {
                let mut message = String::from("(");
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        message.push_str(", ");
                    }
                    message.push_str(&self.format(*value, true)?);
                }
                message.push(')');
                Ok(message)
            }
        }
    }
    fn format_depth(&self, v: Value, repr: bool, path: &mut Vec<Value>) -> Result<String> {
        if path.len() > 100 {
            return Err(Diagnostic::new(
                "RecursionError",
                "representation nesting limit",
            ));
        }
        if let Some(n) = v.as_int() {
            return Ok(n.to_string());
        }
        if let Some(b) = v.as_bool() {
            return Ok(if b { "True" } else { "False" }.into());
        }
        if v == Value::NONE {
            return Ok("None".into());
        }
        if v == Value::NOT_IMPLEMENTED {
            return Ok("NotImplemented".into());
        }
        Ok(match self.get(v)? {
            Object::Class(c) => format!("<class '{}'>", c.name),
            Object::Exception {
                class,
                message,
                arguments,
                ..
            } => {
                if repr {
                    let name = &self.class(*class)?.name;
                    let mut result = format!("{name}(");
                    for (index, value) in arguments.iter().enumerate() {
                        if index != 0 {
                            result.push_str(", ");
                        }
                        result.push_str(&self.format_depth(*value, true, path)?);
                    }
                    result.push(')');
                    result
                } else {
                    message.clone()
                }
            }
            Object::MappingProxy { class } => {
                if path.contains(&v) {
                    return Ok("mappingproxy({...})".into());
                }
                path.push(v);
                let class = self.class(*class)?;
                let entries = (0..class.dictionary_len())
                    .filter_map(|index| class.dictionary_entry(index))
                    .collect::<Vec<_>>();
                let mut result = String::from("mappingproxy({");
                for (index, (key, value)) in entries.into_iter().enumerate() {
                    if index > 0 {
                        result.push_str(", ");
                    }
                    match key {
                        ClassDictionaryKey::String(name) => result.push_str(&quote(&name)),
                        ClassDictionaryKey::Other(key) => {
                            result.push_str(&self.format_depth(key, true, path)?)
                        }
                    }
                    result.push_str(": ");
                    result.push_str(&self.format_depth(value, true, path)?);
                }
                path.pop();
                result.push_str("})");
                result
            }
            Object::Instance {
                class,
                native: Some(native),
                ..
            } => {
                let _ = class;
                self.format_depth(*native, repr, path)?
            }
            Object::Instance { class, .. } => format!("<{} instance>", self.class(*class)?.name),
            Object::Traceback { .. } => "<traceback object>".into(),
            Object::BoundMethod { .. } => "<bound method>".into(),
            Object::StaticMethod(_) => "<staticmethod>".into(),
            Object::ClassMethod(_) => "<classmethod>".into(),
            Object::Property { .. } => "<property>".into(),
            Object::PropertySetter(_) => "<property setter>".into(),
            Object::PropertyDeleter(_) => "<property deleter>".into(),
            Object::Super { .. } => "<super>".into(),
            Object::Buffer(buffer) => {
                format!("<buffer dtype=f64 shape={:?}>", buffer.view().shape())
            }
            Object::Foreign(foreign) => {
                format!("<foreign adapter={}>", foreign.adapter_id)
            }
            Object::Namespace(_) => {
                return Err(Diagnostic::new("BytecodeError", "class namespace escaped"))
            }
            Object::Int(n) => n.to_string(),
            Object::Float(n) => {
                if n.is_nan() {
                    "nan".into()
                } else {
                    let text = format!("{n:?}");
                    if let Some((mantissa, exponent)) = text.split_once('e') {
                        let exponent: i32 = exponent.parse().expect("Rust float exponent");
                        format!("{mantissa}e{exponent:+03}")
                    } else {
                        text
                    }
                }
            }
            Object::Str(s) => {
                if repr {
                    quote(s)
                } else {
                    s.clone()
                }
            }
            Object::Tuple(values) | Object::List(values) => {
                let tuple = matches!(self.get(v)?, Object::Tuple(_));
                if path.contains(&v) {
                    return Ok(if tuple { "(...)" } else { "[...]" }.into());
                }
                path.push(v);
                let mut s = String::from(if tuple { "(" } else { "[" });
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&self.format_depth(*value, true, path)?);
                }
                if tuple && values.len() == 1 {
                    s.push(',');
                }
                s.push(if tuple { ')' } else { ']' });
                path.pop();
                s
            }
            Object::Slice([start, stop, step]) => format!(
                "slice({}, {}, {})",
                self.format_depth(*start, true, path)?,
                self.format_depth(*stop, true, path)?,
                self.format_depth(*step, true, path)?
            ),
            Object::Function { .. } => "<function>".into(),
            Object::Dict(dict) => {
                if path.contains(&v) {
                    return Ok("{...}".into());
                }
                path.push(v);
                let mut result = String::from("{");
                for (i, (key, value)) in dict.entries.iter().enumerate() {
                    if i > 0 {
                        result.push_str(", ");
                    }
                    result.push_str(&self.format_depth(*key, true, path)?);
                    result.push_str(": ");
                    result.push_str(&self.format_depth(*value, true, path)?);
                }
                path.pop();
                result.push('}');
                result
            }
            Object::Cell(_) => {
                return Err(Diagnostic::new(
                    "BytecodeError",
                    "internal closure cell escaped",
                ))
            }
            Object::Builtin(_) | Object::Native(_) => "<native function>".into(),
            Object::Module(_) => "<module>".into(),
            Object::Range { start, stop, step } => {
                if *step == 1 {
                    format!("range({start}, {stop})")
                } else {
                    format!("range({start}, {stop}, {step})")
                }
            }
            Object::Iterator { .. }
            | Object::RangeIterator { .. }
            | Object::DictIterator { .. }
            | Object::MappingProxyIterator { .. } => "<iterator>".into(),
        })
    }
    pub fn trace_all(&self, mut visit: impl FnMut(Value)) {
        for object in &self.objects {
            object.object.trace(&mut visit);
        }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        for entry in &mut self.objects {
            if let Object::Foreign(foreign) = &mut entry.object {
                self.pending_foreign.push(foreign.take_finalizer());
            }
        }
        for mut pending in self.pending_foreign.drain(..) {
            let _ = pending.run();
        }
    }
}
fn quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

fn invalid_value(value: Value) -> Diagnostic {
    if value.heap_index().is_some() {
        Diagnostic::new("HandleError", "stale or invalid internal heap handle")
    } else {
        Diagnostic::new("TypeError", "expected a heap object")
    }
}
impl Object {
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Class(c) | Self::Namespace(c) => c.estimated_bytes(),
                Self::Instance { attributes, .. } => attributes.estimated_bytes(),
                Self::Str(s) => s.capacity(),
                Self::Exception {
                    message,
                    arguments,
                    attributes,
                    ..
                } => {
                    message.capacity()
                        + arguments.capacity() * std::mem::size_of::<Value>()
                        + attributes.estimated_bytes()
                }
                Self::Traceback { entries } => {
                    entries.capacity() * std::mem::size_of::<TracebackEntry>()
                        + entries
                            .iter()
                            .map(|entry| entry.function.capacity())
                            .sum::<usize>()
                }
                Self::Tuple(v) | Self::List(v) => v.capacity() * 8,
                Self::Buffer(buffer) => buffer.estimated_bytes(),
                Self::Foreign(foreign) => foreign.estimated_bytes(),
                Self::Int(n) => n.bits().div_ceil(8) as usize,
                Self::Module(m) => {
                    m.capacity() * std::mem::size_of::<(String, Value)>()
                        + m.iter().map(|(name, _)| name.capacity()).sum::<usize>()
                }
                Self::Function {
                    captures, defaults, ..
                } => (captures.capacity() + defaults.capacity()) * 8,
                Self::Dict(dict) => dict.estimated_bytes(),
                _ => 0,
            }
    }
}
