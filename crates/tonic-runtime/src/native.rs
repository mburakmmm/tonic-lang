//! Safe Rust native boundary. This is not a stable binary/C ABI.
use crate::{
    buffer::{Buffer, ExportParts, F64BufferView},
    heap::{Builtin, Object},
    value::Value,
    vm::{calls::ExpandedArgs, Vm},
};
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;
use std::{
    io::Write,
    sync::atomic::{AtomicU32, Ordering},
    sync::Arc,
};
use tonic_core::{
    bytecode::Program,
    diagnostic::{Diagnostic, Result},
};

/// No raw constructor, Value encoding, address, or mutable table entry is exposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Handle(u64);
#[derive(Debug)]
pub struct PersistentHandle(Handle);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueKind {
    None,
    Bool,
    Int,
    Float,
    Str,
    Foreign,
    List,
    Tuple,
    Dict,
    Other,
}
struct Entry {
    token: u32,
    value: Option<Value>,
    kind: HandleKind,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum HandleKind {
    Local,
    Persistent,
    BufferOwner,
    ForeignReference,
}
#[derive(Default)]
pub(crate) struct HandleTable {
    entries: Vec<Entry>,
    free: Vec<u32>,
}
// Unique tokens prevent stale and cross-runtime aliasing. Never wrap/reuse tokens.
static NEXT_TOKEN: AtomicU32 = AtomicU32::new(1);
fn invalid() -> Diagnostic {
    Diagnostic::new("HandleError", "stale, released, or foreign-runtime handle")
}
impl HandleTable {
    fn insert(&mut self, value: Value, kind: HandleKind) -> Result<Handle> {
        let token = NEXT_TOKEN
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Diagnostic::new("HandleError", "handle token space exhausted"))?;
        let slot = if let Some(i) = self.free.pop() {
            i
        } else {
            let i = u32::try_from(self.entries.len())
                .map_err(|_| Diagnostic::new("HandleError", "handle table exhausted"))?;
            self.entries.push(Entry {
                token: 0,
                value: None,
                kind: HandleKind::Local,
            });
            i
        };
        self.entries[slot as usize] = Entry {
            token,
            value: Some(value),
            kind,
        };
        Ok(Handle(((token as u64) << 32) | slot as u64))
    }
    fn entry(&self, h: Handle) -> Result<&Entry> {
        let e = self.entries.get(h.0 as u32 as usize).ok_or_else(invalid)?;
        if e.token != (h.0 >> 32) as u32 || e.value.is_none() {
            return Err(invalid());
        }
        Ok(e)
    }
    pub(crate) fn resolve(&self, h: Handle) -> Result<Value> {
        let entry = self.entry(h)?;
        if !matches!(entry.kind, HandleKind::Local | HandleKind::Persistent) {
            return Err(invalid());
        }
        entry.value.ok_or_else(invalid)
    }
    fn release(&mut self, h: Handle, kind: HandleKind) -> Result<()> {
        if self.entry(h)?.kind != kind {
            return Err(invalid());
        }
        let slot = h.0 as u32;
        self.entries[slot as usize].value = None;
        self.free.push(slot);
        Ok(())
    }
    pub fn roots(&self, mut visit: impl FnMut(Value)) {
        for e in &self.entries {
            if e.kind != HandleKind::ForeignReference {
                if let Some(v) = e.value {
                    visit(v);
                }
            }
        }
    }
    pub fn active(&self) -> usize {
        self.entries.iter().filter(|e| e.value.is_some()).count()
    }
    pub(crate) fn resolve_persistent(&self, handle: &PersistentHandle) -> Result<Value> {
        let entry = self.entry(handle.0)?;
        if entry.kind != HandleKind::Persistent {
            return Err(invalid());
        }
        entry.value.ok_or_else(invalid)
    }
    pub(crate) fn persist_value(&mut self, value: Value) -> Result<PersistentHandle> {
        Ok(PersistentHandle(
            self.insert(value, HandleKind::Persistent)?,
        ))
    }
    pub(crate) fn release_persistent(&mut self, handle: &PersistentHandle) -> Result<()> {
        self.release(handle.0, HandleKind::Persistent)
    }
    pub(crate) fn insert_foreign_reference(&mut self, value: Value) -> Result<Handle> {
        self.insert(value, HandleKind::ForeignReference)
    }
    pub(crate) fn resolve_foreign_reference(&self, handle: Handle) -> Result<Value> {
        let entry = self.entry(handle)?;
        if entry.kind != HandleKind::ForeignReference {
            return Err(invalid());
        }
        entry.value.ok_or_else(invalid)
    }
    pub(crate) fn promote_foreign_reference(&mut self, handle: Handle) -> Result<PersistentHandle> {
        let value = self.resolve_foreign_reference(handle)?;
        self.persist_value(value)
    }
    pub(crate) fn release_foreign_reference(&mut self, handle: Handle) -> Result<()> {
        self.release(handle, HandleKind::ForeignReference)
    }
    pub(crate) fn invalidate_all(&mut self) {
        self.free.clear();
        for (index, entry) in self.entries.iter_mut().enumerate() {
            entry.value = None;
            if let Ok(index) = u32::try_from(index) {
                self.free.push(index);
            }
        }
    }
}
impl Handle {
    pub(crate) fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    pub(crate) fn raw(self) -> u64 {
        self.0
    }
}
impl PersistentHandle {
    pub(crate) fn from_raw(raw: u64) -> Self {
        Self(Handle::from_raw(raw))
    }
    pub(crate) fn raw(&self) -> u64 {
        self.0.raw()
    }
}
/// One local scope. Dropping it invalidates all local handles, including errors.
/// Rust borrows prevent heap mutation while a borrowed string view is live.
/// Collection occurs at VM instruction boundaries or via `Vm::collect_garbage`,
/// never inside this borrowed scope. Allocations remain rooted until it closes.
/// Active native-call reentry is not available. `Vm::call_persistent` reenters
/// an idle, attached runtime with a separate root/frame scope.
pub struct Context<'a> {
    vm: &'a mut Vm,
    program: Option<&'a Program>,
    output: Option<&'a mut dyn Write>,
    locals: Vec<Handle>,
}
impl<'a> Context<'a> {
    pub(crate) fn new(vm: &'a mut Vm) -> Self {
        Self {
            vm,
            program: None,
            output: None,
            locals: Vec::new(),
        }
    }
    pub(crate) fn for_native_call(
        vm: &'a mut Vm,
        program: &'a Program,
        output: &'a mut dyn Write,
    ) -> Self {
        Self {
            vm,
            program: Some(program),
            output: Some(output),
            locals: Vec::new(),
        }
    }
    pub(crate) fn local(&mut self, value: Value) -> Result<Handle> {
        let h = self.vm.handles.insert(value, HandleKind::Local)?;
        self.locals.push(h);
        Ok(h)
    }
    pub(crate) fn resolve(&self, h: Handle) -> Result<Value> {
        self.vm.handles.resolve(h)
    }
    pub fn none(&mut self) -> Result<Handle> {
        self.local(Value::NONE)
    }
    /// Borrow a registered native module into this call's local handle scope.
    pub fn native_module(&mut self, name: &str) -> Result<Handle> {
        let module = self.vm.native_module_value(name)?;
        self.local(module)
    }
    /// Borrow an original built-in binding into this call's local handle scope.
    pub fn builtin(&mut self, name: &str) -> Result<Handle> {
        let value = self.vm.named_builtin(name)?;
        self.local(value)
    }
    /// Store one adapter-owned global as an explicit per-runtime root.
    pub fn native_global_store(
        &mut self,
        module: u64,
        slot: u32,
        value: Option<Handle>,
    ) -> Result<()> {
        let key = (module, slot);
        if let Some(value) = value {
            let value = self.resolve(value)?;
            self.vm.native_globals.insert(key, value);
        } else {
            self.vm.native_globals.remove(&key);
        }
        Ok(())
    }

    /// Borrow one adapter-owned per-runtime global into this local scope.
    pub fn native_global_load(&mut self, module: u64, slot: u32) -> Result<Option<Handle>> {
        let Some(value) = self.vm.native_globals.get(&(module, slot)).copied() else {
            return Ok(None);
        };
        self.local(value).map(Some)
    }

    /// Store a precise extension field edge and run the generational barrier.
    pub fn native_field_store(
        &mut self,
        owner: Handle,
        field: u64,
        value: Option<Handle>,
    ) -> Result<()> {
        let owner = self.resolve(owner)?;
        let value = value.map(|value| self.resolve(value)).transpose()?;
        self.vm.heap.store_external_field(owner, field, value)
    }

    /// Borrow a precise extension field edge into this local scope.
    pub fn native_field_load(&mut self, owner: Handle, field: u64) -> Result<Option<Handle>> {
        let owner = self.resolve(owner)?;
        let Some(value) = self.vm.heap.load_external_field(owner, field)? else {
            return Ok(None);
        };
        self.local(value).map(Some)
    }
    pub(crate) fn value_kind(&self, handle: Handle) -> Result<ValueKind> {
        let value = self.resolve(handle)?;
        if value == Value::NONE {
            return Ok(ValueKind::None);
        }
        if value == Value::NOT_IMPLEMENTED || value == Value::ELLIPSIS {
            return Ok(ValueKind::Other);
        }
        if value.as_bool().is_some() {
            return Ok(ValueKind::Bool);
        }
        if value.as_int().is_some() {
            return Ok(ValueKind::Int);
        }
        let native = self.vm.heap.native_value(value);
        if native.as_bool().is_some() {
            return Ok(ValueKind::Bool);
        }
        if native.as_int().is_some() {
            return Ok(ValueKind::Int);
        }
        Ok(match self.vm.heap.get(native)? {
            Object::Int(_) => ValueKind::Int,
            Object::Float(_) => ValueKind::Float,
            Object::Str(_) => ValueKind::Str,
            Object::Foreign(_) => ValueKind::Foreign,
            Object::List(_) => ValueKind::List,
            Object::Tuple(_) => ValueKind::Tuple,
            Object::Dict(_) => ValueKind::Dict,
            _ => ValueKind::Other,
        })
    }
    pub(crate) fn is_identical(&self, left: Handle, right: Handle) -> Result<bool> {
        Ok(self.resolve(left)? == self.resolve(right)?)
    }
    pub fn from_bool(&mut self, value: bool) -> Result<Handle> {
        self.local(Value::bool(value))
    }
    pub fn to_bool(&self, handle: Handle) -> Result<bool> {
        self.resolve(handle)?
            .as_bool()
            .ok_or_else(|| Diagnostic::new("TypeError", "expected bool"))
    }
    pub fn from_i64(&mut self, n: i64) -> Result<Handle> {
        let v = self.vm.heap.i64(n)?;
        self.local(v)
    }
    pub fn from_u64(&mut self, value: u64) -> Result<Handle> {
        let value = self.vm.heap.int(BigInt::from(value))?;
        self.local(value)
    }
    pub fn to_i64(&self, h: Handle) -> Result<i64> {
        self.vm.heap.to_i64(self.resolve(h)?)
    }
    fn integer(&self, handle: Handle) -> Result<BigInt> {
        self.vm.heap.integer(self.resolve(handle)?)
    }
    pub fn to_i32(&self, handle: Handle) -> Result<i32> {
        self.integer(handle)?
            .to_i32()
            .ok_or_else(|| Diagnostic::new("OverflowError", "integer does not fit i32"))
    }
    pub fn to_u32(&self, handle: Handle) -> Result<u32> {
        self.integer(handle)?
            .to_u32()
            .ok_or_else(|| Diagnostic::new("OverflowError", "integer does not fit u32"))
    }
    pub fn to_u64(&self, handle: Handle) -> Result<u64> {
        self.integer(handle)?
            .to_u64()
            .ok_or_else(|| Diagnostic::new("OverflowError", "integer does not fit u64"))
    }
    pub fn to_isize(&self, handle: Handle) -> Result<isize> {
        self.integer(handle)?
            .to_isize()
            .ok_or_else(|| Diagnostic::new("OverflowError", "integer does not fit isize"))
    }
    pub fn to_usize(&self, handle: Handle) -> Result<usize> {
        self.integer(handle)?
            .to_usize()
            .ok_or_else(|| Diagnostic::new("OverflowError", "integer does not fit usize"))
    }
    fn integer_mask(&self, handle: Handle, bits: usize) -> Result<BigInt> {
        let modulus = BigInt::from(1_u8) << bits;
        let mut value = self.integer(handle)? % &modulus;
        if value.sign() == Sign::Minus {
            value += modulus;
        }
        Ok(value)
    }
    pub fn to_u32_mask(&self, handle: Handle) -> Result<u32> {
        self.integer_mask(handle, u32::BITS as usize)?
            .to_u32()
            .ok_or_else(|| Diagnostic::new("RuntimeError", "u32 mask conversion failed"))
    }
    pub fn to_u64_mask(&self, handle: Handle) -> Result<u64> {
        self.integer_mask(handle, u64::BITS as usize)?
            .to_u64()
            .ok_or_else(|| Diagnostic::new("RuntimeError", "u64 mask conversion failed"))
    }
    pub fn integer_to_f64(&self, handle: Handle) -> Result<f64> {
        self.integer(handle)?
            .to_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| Diagnostic::new("OverflowError", "integer too large for float"))
    }
    pub fn class_name(&self, handle: Handle) -> Result<&str> {
        let value = self.resolve(handle)?;
        Ok(&self.vm.heap.class(value)?.name)
    }
    pub(crate) fn int_decimal(&self, handle: Handle) -> Result<String> {
        Ok(self.vm.heap.integer(self.resolve(handle)?)?.to_string())
    }
    pub(crate) fn int_from_decimal(&mut self, decimal: &str) -> Result<Handle> {
        let integer = decimal
            .parse::<num_bigint::BigInt>()
            .map_err(|_| Diagnostic::new("ValueError", "invalid decimal integer"))?;
        let value = self.vm.heap.int(integer)?;
        self.local(value)
    }
    pub fn from_f64(&mut self, n: f64) -> Result<Handle> {
        let v = self.vm.heap.alloc(Object::Float(n))?;
        self.local(v)
    }
    pub fn to_f64(&self, h: Handle) -> Result<f64> {
        self.vm.heap.float(self.resolve(h)?)
    }
    pub fn call(&mut self, callable: Handle, arguments: &[Handle]) -> Result<Handle> {
        let callable = self.resolve(callable)?;
        let arguments = arguments
            .iter()
            .map(|argument| self.resolve(*argument))
            .collect::<Result<Vec<_>>>()?;
        let value = {
            let program = self.program.ok_or_else(|| {
                Diagnostic::new(
                    "RuntimeError",
                    "Tonic callback requires an active native call context",
                )
            })?;
            let output = self.output.as_deref_mut().ok_or_else(|| {
                Diagnostic::new("RuntimeError", "native callback has no output context")
            })?;
            self.vm
                .reenter_from_native(program, callable, &arguments, output)?
        };
        self.local(value)
    }
    pub fn call_with_keywords(
        &mut self,
        callable: Handle,
        arguments: &[Handle],
        keywords: Handle,
    ) -> Result<Handle> {
        let callable = self.resolve(callable)?;
        let positional = arguments
            .iter()
            .map(|argument| self.resolve(*argument))
            .collect::<Result<Vec<_>>>()?;
        let keywords = self.resolve(keywords)?;
        let entries = match self.vm.heap.get(self.vm.heap.native_value(keywords))? {
            Object::Dict(dict) => dict.entries.clone(),
            _ => return Err(Diagnostic::new("TypeError", "keywords must be a dict")),
        };
        let keywords = entries
            .into_iter()
            .map(|(name, value)| match self.vm.heap.get(name)? {
                Object::Str(name) => Ok((name.clone(), value)),
                _ => Err(Diagnostic::new("TypeError", "keywords must be strings")),
            })
            .collect::<Result<Vec<_>>>()?;
        let value = {
            let program = self.program.ok_or_else(|| {
                Diagnostic::new(
                    "RuntimeError",
                    "Tonic callback requires an active native call context",
                )
            })?;
            let output = self.output.as_deref_mut().ok_or_else(|| {
                Diagnostic::new("RuntimeError", "native callback has no output context")
            })?;
            self.vm.reenter_from_native_expanded(
                program,
                callable,
                ExpandedArgs {
                    positional,
                    keywords,
                    ..ExpandedArgs::default()
                },
                output,
            )?
        };
        self.local(value)
    }
    pub fn call_with_named(
        &mut self,
        callable: Handle,
        arguments: &[Handle],
        keywords: &[(String, Handle)],
    ) -> Result<Handle> {
        let callable = self.resolve(callable)?;
        let positional = arguments
            .iter()
            .map(|argument| self.resolve(*argument))
            .collect::<Result<Vec<_>>>()?;
        let keywords = keywords
            .iter()
            .map(|(name, value)| Ok((name.clone(), self.resolve(*value)?)))
            .collect::<Result<Vec<_>>>()?;
        let value = {
            let program = self.program.ok_or_else(|| {
                Diagnostic::new(
                    "RuntimeError",
                    "Tonic callback requires an active native call context",
                )
            })?;
            let output = self.output.as_deref_mut().ok_or_else(|| {
                Diagnostic::new("RuntimeError", "native callback has no output context")
            })?;
            self.vm.reenter_from_native_expanded(
                program,
                callable,
                ExpandedArgs {
                    positional,
                    keywords,
                    ..ExpandedArgs::default()
                },
                output,
            )?
        };
        self.local(value)
    }
    pub fn is_callable(&self, handle: Handle) -> Result<bool> {
        let value = self.resolve(handle)?;
        match self.vm.heap.get(value)? {
            Object::Function { .. }
            | Object::Builtin(_)
            | Object::Native(_)
            | Object::BoundMethod { .. }
            | Object::StaticMethod(_)
            | Object::PropertySetter(_)
            | Object::PropertyDeleter(_)
            | Object::Class(_) => return Ok(true),
            Object::GenericAlias { origin, .. } => {
                return Ok(matches!(self.vm.heap.get(*origin), Ok(Object::Class(_))));
            }
            object if object.instance_class().is_some() => {}
            _ => return Ok(false),
        }
        Ok(self
            .vm
            .heap
            .special_method_call(value, "__call__")?
            .is_some())
    }
    fn call_values(&mut self, callable: Value, arguments: &[Value]) -> Result<Value> {
        let program = self.program.ok_or_else(|| {
            Diagnostic::new(
                "RuntimeError",
                "Tonic callback requires an active native call context",
            )
        })?;
        let output = self.output.as_deref_mut().ok_or_else(|| {
            Diagnostic::new("RuntimeError", "native callback has no output context")
        })?;
        self.vm
            .reenter_from_native(program, callable, arguments, output)
    }
    pub fn get_attr(&mut self, owner: Handle, name: &str) -> Result<Handle> {
        let owner = self.resolve(owner)?;
        let name = self.vm.heap.alloc(Object::Str(name.to_owned()))?;
        let callable = self.vm.builtin_callable(Builtin::GetAttr)?;
        let value = self.call_values(callable, &[owner, name])?;
        self.local(value)
    }
    pub fn set_attr(&mut self, owner: Handle, name: &str, value: Handle) -> Result<()> {
        let owner = self.resolve(owner)?;
        let value = self.resolve(value)?;
        let name = self.vm.heap.alloc(Object::Str(name.to_owned()))?;
        let callable = self.vm.builtin_callable(Builtin::SetAttr)?;
        let _ = self.call_values(callable, &[owner, name, value])?;
        Ok(())
    }
    pub fn repr(&mut self, value: Handle) -> Result<Handle> {
        let value = self.resolve(value)?;
        let callable = self.vm.builtin_callable(Builtin::Repr)?;
        let result = self.call_values(callable, &[value])?;
        self.local(result)
    }
    pub fn str_value(&mut self, value: Handle) -> Result<Handle> {
        let value = self.resolve(value)?;
        let callable = self.vm.named_builtin("str")?;
        let result = self.call_values(callable, &[value])?;
        self.local(result)
    }
    pub(crate) fn add(&mut self, left: Handle, right: Handle) -> Result<Handle> {
        let left = self.resolve(left)?;
        let right = self.resolve(right)?;
        let value = self
            .vm
            .heap
            .binary(tonic_core::bytecode::Op::Add, left, right)?;
        self.local(value)
    }
    pub fn from_f64_buffer(
        &mut self,
        values: &[f64],
        shape: &[usize],
        writable: bool,
    ) -> Result<Handle> {
        let buffer = Buffer::f64(values, shape, writable)?;
        self.vm.heap.buffer_copies += 1;
        let value = self.vm.heap.alloc(Object::Buffer(buffer))?;
        self.local(value)
    }
    pub fn f64_buffer_from_sequence(&mut self, sequence: Handle) -> Result<Handle> {
        let sequence = self.resolve(sequence)?;
        let values = match self.vm.heap.get(sequence)? {
            Object::List(values) | Object::Tuple(values) => values
                .iter()
                .map(|value| self.vm.heap.float(*value))
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(Diagnostic::new("TypeError", "expected list or tuple")),
        };
        self.from_f64_buffer(&values, &[values.len()], false)
    }
    pub fn f64_buffer(&mut self, buffer: Handle) -> Result<F64BufferView<'_>> {
        let buffer = self.resolve(buffer)?;
        if !matches!(self.vm.heap.get(buffer)?, Object::Buffer(_)) {
            return Err(Diagnostic::new("TypeError", "expected f64 buffer"));
        }
        self.vm.heap.buffer_exports += 1;
        match self.vm.heap.get(buffer)? {
            Object::Buffer(buffer) => Ok(buffer.view()),
            _ => unreachable!("type checked above"),
        }
    }
    pub(crate) fn create_foreign_reference(&mut self, value: Handle) -> Result<Handle> {
        let value = self.resolve(value)?;
        self.vm.handles.insert_foreign_reference(value)
    }
    pub(crate) fn release_foreign_reference(&mut self, reference: Handle) -> Result<()> {
        self.vm.handles.release_foreign_reference(reference)
    }
    pub(crate) fn borrow_foreign_reference(&mut self, reference: Handle) -> Result<Handle> {
        let value = self.vm.handles.resolve_foreign_reference(reference)?;
        self.local(value)
    }
    pub(crate) fn create_foreign(
        &mut self,
        payload: usize,
        spec: crate::foreign::ForeignSpec,
    ) -> Result<Handle> {
        let traced = crate::foreign::trace_handles(spec, payload, &mut self.vm.handles)?;
        self.vm.heap.foreign_trace_calls += u64::from(spec.trace.is_some());
        self.vm.stats.foreign_trace_calls += u64::from(spec.trace.is_some());
        let references = traced
            .owned
            .iter()
            .chain(&traced.borrowed)
            .copied()
            .map(|handle| self.vm.handles.resolve_foreign_reference(handle))
            .collect::<Result<Vec<_>>>()?;
        let foreign = crate::foreign::ForeignObject::new(
            payload,
            spec,
            traced.owned.clone(),
            traced.borrowed,
            references,
        );
        let value = match self.vm.heap.alloc(Object::Foreign(foreign)) {
            Ok(value) => value,
            Err(error) => {
                for handle in traced.owned {
                    let _ = self.vm.handles.release_foreign_reference(handle);
                }
                return Err(error);
            }
        };
        self.vm.heap.foreign_wrapper_creations += 1;
        self.vm.stats.foreign_wrapper_creations += 1;
        self.local(value)
    }
    pub(crate) fn foreign_payload(&self, foreign: Handle, adapter_id: u64) -> Result<usize> {
        self.vm
            .heap
            .foreign_payload(self.resolve(foreign)?, adapter_id)
    }
    pub(crate) fn export_f64_buffer(
        &mut self,
        buffer: Handle,
        writable: bool,
    ) -> Result<(Handle, ExportParts)> {
        let value = self.resolve(buffer)?;
        let Object::Buffer(buffer) = self.vm.heap.get(value)? else {
            return Err(Diagnostic::new("TypeError", "expected f64 buffer"));
        };
        if writable && !buffer.view().is_writable() {
            return Err(Diagnostic::new("BufferError", "buffer is read-only"));
        }
        let parts = buffer.export();
        let owner = self.vm.handles.insert(value, HandleKind::BufferOwner)?;
        self.locals.push(owner);
        self.vm.heap.buffer_exports += 1;
        Ok((owner, parts))
    }
    pub(crate) fn release_buffer_owner(&mut self, handle: Handle) -> Result<()> {
        self.vm.handles.release(handle, HandleKind::BufferOwner)
    }
    fn sum_f64_values(&mut self, value: Handle) -> Result<f64> {
        let value = self.resolve(value)?;
        if matches!(self.vm.heap.get(value)?, Object::Buffer(_)) {
            self.vm.heap.buffer_exports += 1;
            let Object::Buffer(buffer) = self.vm.heap.get(value)? else {
                unreachable!("type checked above")
            };
            return Ok(buffer.view().as_slice().iter().sum());
        }
        match self.vm.heap.get(value)? {
            Object::List(values) | Object::Tuple(values) => values
                .iter()
                .try_fold(0.0, |total, value| Ok(total + self.vm.heap.float(*value)?)),
            _ => Err(Diagnostic::new(
                "TypeError",
                "fastmath.sum expects an f64 buffer, list, or tuple",
            )),
        }
    }
    pub fn from_str(&mut self, s: &str) -> Result<Handle> {
        let v = self.vm.heap.alloc(Object::Str(s.to_owned()))?;
        self.local(v)
    }
    pub fn as_str(&self, h: Handle) -> Result<&str> {
        let value = self.resolve(h)?;
        match self.vm.heap.get(self.vm.heap.native_value(value))? {
            Object::Str(s) => Ok(s),
            _ => Err(Diagnostic::new("TypeError", "expected string")),
        }
    }
    pub fn sequence_len(&self, handle: Handle) -> Result<usize> {
        let value = self.resolve(handle)?;
        match self.vm.heap.get(self.vm.heap.native_value(value))? {
            Object::List(values) | Object::Tuple(values) => Ok(values.len()),
            _ => Err(Diagnostic::new("TypeError", "expected list or tuple")),
        }
    }
    pub fn sequence_get(&mut self, handle: Handle, index: usize) -> Result<Handle> {
        let value = self.resolve(handle)?;
        let item = match self.vm.heap.get(self.vm.heap.native_value(value))? {
            Object::List(values) | Object::Tuple(values) => values
                .get(index)
                .copied()
                .ok_or_else(|| Diagnostic::new("IndexError", "sequence index out of range"))?,
            _ => return Err(Diagnostic::new("TypeError", "expected list or tuple")),
        };
        self.local(item)
    }
    pub fn dict_len(&self, handle: Handle) -> Result<usize> {
        let value = self.resolve(handle)?;
        match self.vm.heap.get(self.vm.heap.native_value(value))? {
            Object::Dict(dict) => Ok(dict.entries.len()),
            _ => Err(Diagnostic::new("TypeError", "expected dict")),
        }
    }
    pub fn dict_entry(&mut self, handle: Handle, index: usize) -> Result<(Handle, Handle)> {
        let value = self.resolve(handle)?;
        let (key, value) = match self.vm.heap.get(self.vm.heap.native_value(value))? {
            Object::Dict(dict) => dict
                .entries
                .get(index)
                .copied()
                .ok_or_else(|| Diagnostic::new("IndexError", "dict index out of range"))?,
            _ => return Err(Diagnostic::new("TypeError", "expected dict")),
        };
        Ok((self.local(key)?, self.local(value)?))
    }
    pub fn is_list(&self, handle: Handle) -> Result<bool> {
        Ok(self.value_kind(handle)? == ValueKind::List)
    }
    pub fn is_tuple(&self, handle: Handle) -> Result<bool> {
        Ok(self.value_kind(handle)? == ValueKind::Tuple)
    }
    pub fn is_dict(&self, handle: Handle) -> Result<bool> {
        Ok(self.value_kind(handle)? == ValueKind::Dict)
    }
    pub fn new_list(&mut self) -> Result<Handle> {
        let value = self.vm.heap.alloc(Object::List(Vec::new()))?;
        self.local(value)
    }
    pub fn list_append(&mut self, list: Handle, item: Handle) -> Result<()> {
        let list = self.resolve(list)?;
        let item = self.resolve(item)?;
        self.vm.heap.append_list(list, item)
    }
    pub fn new_tuple(&mut self, items: &[Handle]) -> Result<Handle> {
        let items = items
            .iter()
            .map(|item| self.resolve(*item))
            .collect::<Result<Vec<_>>>()?;
        let value = self.vm.heap.alloc(Object::Tuple(items))?;
        self.local(value)
    }
    pub fn new_dict(&mut self) -> Result<Handle> {
        let value = self
            .vm
            .heap
            .alloc(Object::Dict(crate::dict::Dict::default()))?;
        self.local(value)
    }
    pub fn dict_set(&mut self, dict: Handle, key: Handle, value: Handle) -> Result<()> {
        let dict = self.resolve(dict)?;
        let key = self.resolve(key)?;
        let value = self.resolve(value)?;
        self.vm.heap.dict_set(dict, key, value)
    }
    pub fn get_item(&mut self, owner: Handle, key: Handle) -> Result<Handle> {
        let owner = self.resolve(owner)?;
        let key = self.resolve(key)?;
        let value = if let Some(call) = self.vm.heap.special_method_call(owner, "__getitem__")? {
            let mut arguments = Vec::with_capacity(1 + usize::from(call.receiver.is_some()));
            arguments.extend(call.receiver);
            arguments.push(key);
            self.call_values(call.callable, &arguments)?
        } else {
            self.vm.heap.item(owner, key)?
        };
        self.local(value)
    }
    pub fn set_item(&mut self, owner: Handle, key: Handle, value: Handle) -> Result<()> {
        let owner = self.resolve(owner)?;
        let key = self.resolve(key)?;
        let value = self.resolve(value)?;
        if let Some(call) = self.vm.heap.special_method_call(owner, "__setitem__")? {
            let mut arguments = Vec::with_capacity(2 + usize::from(call.receiver.is_some()));
            arguments.extend(call.receiver);
            arguments.extend([key, value]);
            let _ = self.call_values(call.callable, &arguments)?;
        } else {
            self.vm.heap.set_item(owner, key, value)?;
        }
        Ok(())
    }
    pub fn delete_item(&mut self, owner: Handle, key: Handle) -> Result<()> {
        let owner = self.resolve(owner)?;
        let key = self.resolve(key)?;
        if let Some(call) = self.vm.heap.special_method_call(owner, "__delitem__")? {
            let mut arguments = Vec::with_capacity(1 + usize::from(call.receiver.is_some()));
            arguments.extend(call.receiver);
            arguments.push(key);
            let _ = self.call_values(call.callable, &arguments)?;
        } else {
            self.vm.heap.delete_item(owner, key)?;
        }
        Ok(())
    }
    pub fn length(&mut self, owner: Handle) -> Result<isize> {
        let owner = self.resolve(owner)?;
        let value = if let Some(call) = self.vm.heap.special_method_call(owner, "__len__")? {
            let arguments = call.receiver.into_iter().collect::<Vec<_>>();
            self.call_values(call.callable, &arguments)?
        } else {
            self.vm.heap.length(owner)?
        };
        let value = self.vm.heap.to_i64(value)?;
        if value < 0 {
            return Err(Diagnostic::new(
                "ValueError",
                "__len__() should return >= 0",
            ));
        }
        isize::try_from(value)
            .map_err(|_| Diagnostic::new("OverflowError", "length does not fit HPy_ssize_t"))
    }
    pub fn contains(&mut self, container: Handle, key: Handle) -> Result<bool> {
        let container = self.resolve(container)?;
        let key = self.resolve(key)?;
        if let Some(call) = self
            .vm
            .heap
            .special_method_call(container, "__contains__")?
        {
            let mut arguments = Vec::with_capacity(1 + usize::from(call.receiver.is_some()));
            arguments.extend(call.receiver);
            arguments.push(key);
            let result = self.call_values(call.callable, &arguments)?;
            return self.vm.heap.truth(result);
        }
        let storage = self.vm.heap.native_value(container);
        match self.vm.heap.get(storage)? {
            Object::Dict(_) | Object::Set(_) => Ok(self.vm.heap.dict_get(storage, key)?.is_some()),
            Object::List(values) | Object::Tuple(values) => {
                let values = values.clone();
                values.into_iter().try_fold(false, |found, value| {
                    Ok(found || self.vm.heap.equal(value, key, 0)?)
                })
            }
            Object::Str(value) => {
                let value = value.clone();
                let Object::Str(key) = self.vm.heap.get(self.vm.heap.native_value(key))? else {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "string membership requires a string key",
                    ));
                };
                Ok(value.contains(key))
            }
            _ => Err(Diagnostic::new(
                "TypeError",
                "object does not support membership",
            )),
        }
    }
    pub fn persist(&mut self, h: Handle) -> Result<PersistentHandle> {
        let v = self.resolve(h)?;
        self.vm.handles.persist_value(v)
    }
    pub fn borrow_persistent(&mut self, h: &PersistentHandle) -> Result<Handle> {
        let v = self.vm.handles.resolve(h.0)?;
        self.local(v)
    }
    /// Takes a borrow so a wrong-runtime release does not lose the owning token.
    pub fn release_persistent(&mut self, h: &PersistentHandle) -> Result<()> {
        self.vm.handles.release_persistent(h)
    }
    pub(crate) fn runtime_owner(&self) -> std::sync::Arc<crate::runtime_owner::RuntimeOwner> {
        self.vm.runtime_owner.clone()
    }
    pub(crate) fn owns_runtime(&self, owner: &crate::runtime_owner::RuntimeOwner) -> bool {
        self.vm.runtime_owner.id() == owner.id()
    }
    pub(crate) fn execution_id(&self) -> u64 {
        self.vm.execution
    }
    pub(crate) fn drain_deferred_persistent_releases(&mut self) -> Result<()> {
        self.vm.drain_deferred_persistent_releases()
    }
}
impl Drop for Context<'_> {
    fn drop(&mut self) {
        for h in self.locals.drain(..) {
            let kind = self.vm.handles.entry(h).map(|entry| entry.kind);
            if let Ok(kind @ (HandleKind::Local | HandleKind::BufferOwner)) = kind {
                let _ = self.vm.handles.release(h, kind);
            }
        }
    }
}
pub type NativeFn = fn(&mut Context<'_>, &[Handle]) -> Result<Handle>;
/// VM-owned native callback with immutable extension state.
///
/// Stateful callbacks are isolated to the native boundary. The VM clones the
/// `Arc` before borrowing its execution state, so a callback may safely use the
/// ordinary [`Context`] API without aliasing the native registry.
pub type StatefulNativeFn =
    dyn Fn(&mut Context<'_>, &[Handle]) -> Result<Handle> + Send + Sync + 'static;
/// VM-owned native callback that receives both positional and named arguments.
pub type StatefulKeywordNativeFn = dyn Fn(&mut Context<'_>, &[Handle], &[(String, Handle)]) -> Result<Handle>
    + Send
    + Sync
    + 'static;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatefulNativeSignature {
    Exact(usize),
    VarArgs,
    Keywords,
}

#[derive(Clone)]
pub(crate) enum NativeCallable {
    Rust(NativeFn),
    C(crate::c_api::CNativeFn),
    Stateful(Arc<StatefulNativeFn>),
    StatefulKeywords(Arc<StatefulKeywordNativeFn>),
}
#[derive(Clone)]
pub(crate) struct NativeDef {
    pub signature: StatefulNativeSignature,
    pub function: NativeCallable,
}

pub(crate) fn fastmath_add(ctx: &mut Context<'_>, args: &[Handle]) -> Result<Handle> {
    let a = ctx.to_i64(args[0])?;
    let b = ctx.to_i64(args[1])?;
    let n = a
        .checked_add(b)
        .ok_or_else(|| Diagnostic::new("OverflowError", "fastmath.add accepts an i64 result"))?;
    ctx.from_i64(n)
}

pub(crate) fn fastmath_array(ctx: &mut Context<'_>, args: &[Handle]) -> Result<Handle> {
    ctx.f64_buffer_from_sequence(args[0])
}

pub(crate) fn fastmath_sum(ctx: &mut Context<'_>, args: &[Handle]) -> Result<Handle> {
    let total = ctx.sum_f64_values(args[0])?;
    ctx.from_f64(total)
}
