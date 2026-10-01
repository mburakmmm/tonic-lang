#[path = "gc.rs"]
mod gc;
use crate::{classes::ClassDictionaryKey, value::Value};
pub use gc::CollectionStats;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::collections::HashSet;
use tonic_core::diagnostic::{Diagnostic, Result, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TracebackEntry {
    pub function: String,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneratorState {
    Created,
    Suspended,
    Running,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneratorKind {
    Generator,
    Coroutine,
    AsyncGenerator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncGeneratorOperation {
    Send(Value),
    Throw {
        exception: Value,
        traceback: Option<Value>,
    },
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncGeneratorAwaitState {
    Created,
    Running,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncState {
    Pending,
    Finished(Value),
    Failed(Value),
    Cancelled(Value),
}

#[derive(Debug)]
pub(crate) struct AsyncFuture {
    pub class: Value,
    pub state: AsyncState,
    pub waiters: Vec<Value>,
    pub callbacks: Vec<Value>,
    pub due_tick: Option<u64>,
    pub timer_result: Value,
}

#[derive(Debug)]
pub(crate) struct AsyncTask {
    pub class: Value,
    pub coroutine: Value,
    pub state: AsyncState,
    pub waiters: Vec<Value>,
    pub callbacks: Vec<Value>,
    pub waiting_on: Option<Value>,
    pub cancel_requested: bool,
}

#[derive(Debug)]
pub(crate) struct GeneratorFrame {
    pub class: Value,
    pub kind: GeneratorKind,
    pub execution: u64,
    pub code: u16,
    pub ip: usize,
    pub registers: Vec<Value>,
    pub cells: Vec<Value>,
    pub exception_stack: Vec<Value>,
    pub resume_register: Option<u16>,
    pub yield_from: Option<Value>,
    pub async_driver: Option<Value>,
    pub return_value: Value,
    pub state: GeneratorState,
}

pub(crate) struct ResumedGenerator {
    pub code: u16,
    pub ip: usize,
    pub registers: Vec<Value>,
    pub cells: Vec<Value>,
    pub exception_stack: Vec<Value>,
    pub resume_register: Option<u16>,
    pub yield_from: Option<Value>,
}

pub(crate) struct SuspendedGenerator {
    pub ip: usize,
    pub registers: Vec<Value>,
    pub cells: Vec<Value>,
    pub exception_stack: Vec<Value>,
    pub resume_register: u16,
    pub yield_from: Option<Value>,
}

impl GeneratorFrame {
    fn payload_bytes(&self) -> usize {
        (self.registers.capacity() + self.cells.capacity() + self.exception_stack.capacity())
            * std::mem::size_of::<Value>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Print,
    Len,
    Iter,
    Next,
    GeneratorIter,
    GeneratorNext,
    GeneratorSend,
    GeneratorThrow,
    GeneratorClose,
    CoroutineAwait,
    AsyncGeneratorIter,
    AsyncGeneratorNext,
    AsyncGeneratorSend,
    AsyncGeneratorThrow,
    AsyncGeneratorClose,
    AsyncGeneratorAwait,
    AsyncGeneratorAwaitNext,
    AsyncGeneratorAwaitSend,
    AsyncGeneratorAwaitThrow,
    AsyncGeneratorAwaitClose,
    AsyncioRun,
    AsyncioCreateTask,
    AsyncioCurrentTask,
    AsyncioGetRunningLoop,
    AsyncioSleep,
    AsyncioFutureNew,
    AsyncioFutureAwait,
    AsyncioFutureNext,
    AsyncioFutureSend,
    AsyncioFutureDone,
    AsyncioFutureCancelled,
    AsyncioFutureCancel,
    AsyncioFutureResult,
    AsyncioFutureException,
    AsyncioFutureSetResult,
    AsyncioFutureSetException,
    AsyncioFutureAddDoneCallback,
    AsyncioTaskDone,
    AsyncioTaskCancelled,
    AsyncioTaskCancel,
    AsyncioTaskResult,
    AsyncioTaskException,
    AsyncioTaskAddDoneCallback,
    Hash,
    Abs,
    DivMod,
    Pow,
    Round,
    Repr,
    Ascii,
    Format,
    Sum,
    IntRound,
    FloatRound,
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
    ObjectInit,
    ObjectHash,
    IntHash,
    FloatHash,
    StrHash,
    TupleHash,
    RangeHash,
    IntNew,
    BoolNew,
    FloatNew,
    StrNew,
    ListNew,
    ListInit,
    TupleNew,
    DictNew,
    DictInit,
    RangeNew,
    ObjectGetAttribute,
    ObjectSetAttr,
    ObjectDelAttr,
    TypeNew,
    TypeGetAttribute,
    TypeSetAttr,
    TypeDelAttr,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeParameterKind {
    TypeVar,
    ParamSpec,
    TypeVarTuple,
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
    Set(crate::dict::Dict),
    Exception {
        class: Value,
        message: String,
        arguments: Vec<Value>,
        stop_iteration_value: Option<Value>,
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
        annotations: Option<Value>,
        type_params: Option<Value>,
    },
    TypeParam {
        name: String,
        kind: TypeParameterKind,
        bound: Option<Value>,
    },
    TypeAlias {
        name: String,
        type_params: Value,
        value: Value,
    },
    GenericAlias {
        origin: Value,
        args: Value,
    },
    Generator(GeneratorFrame),
    CoroutineIterator {
        class: Value,
        coroutine: Value,
    },
    AsyncGeneratorAwaitable {
        class: Value,
        generator: Value,
        operation: AsyncGeneratorOperation,
        state: AsyncGeneratorAwaitState,
    },
    AsyncFuture(AsyncFuture),
    AsyncTask(AsyncTask),
    AsyncFutureIterator {
        class: Value,
        source: Value,
    },
    AsyncEventLoop {
        class: Value,
        generation: u64,
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
            Self::Instance { class, .. }
            | Self::Exception { class, .. }
            | Self::Generator(GeneratorFrame { class, .. })
            | Self::CoroutineIterator { class, .. }
            | Self::AsyncGeneratorAwaitable { class, .. }
            | Self::AsyncFuture(AsyncFuture { class, .. })
            | Self::AsyncTask(AsyncTask { class, .. })
            | Self::AsyncFutureIterator { class, .. }
            | Self::AsyncEventLoop { class, .. } => Some(*class),
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
                stop_iteration_value,
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
                    .chain(stop_iteration_value)
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
                captures,
                defaults,
                annotations,
                type_params,
                ..
            } => {
                captures
                    .iter()
                    .chain(defaults)
                    .copied()
                    .for_each(&mut visit);
                annotations.iter().copied().for_each(&mut visit);
                type_params.iter().copied().for_each(visit);
            }
            Self::TypeParam { bound, .. } => bound.iter().copied().for_each(visit),
            Self::TypeAlias {
                type_params, value, ..
            } => {
                visit(*type_params);
                visit(*value);
            }
            Self::GenericAlias { origin, args } => {
                visit(*origin);
                visit(*args);
            }
            Self::Generator(frame) => {
                visit(frame.class);
                visit(frame.return_value);
                frame
                    .registers
                    .iter()
                    .chain(&frame.cells)
                    .chain(&frame.exception_stack)
                    .chain(&frame.yield_from)
                    .chain(&frame.async_driver)
                    .copied()
                    .for_each(visit);
            }
            Self::CoroutineIterator { class, coroutine } => {
                visit(*class);
                visit(*coroutine);
            }
            Self::AsyncGeneratorAwaitable {
                class,
                generator,
                operation,
                ..
            } => {
                visit(*class);
                visit(*generator);
                match operation {
                    AsyncGeneratorOperation::Send(value) => visit(*value),
                    AsyncGeneratorOperation::Throw {
                        exception,
                        traceback,
                    } => {
                        visit(*exception);
                        traceback.iter().copied().for_each(visit);
                    }
                    AsyncGeneratorOperation::Close => {}
                }
            }
            Self::AsyncFuture(future) => {
                visit(future.class);
                visit(future.timer_result);
                match future.state {
                    AsyncState::Pending => {}
                    AsyncState::Finished(value)
                    | AsyncState::Failed(value)
                    | AsyncState::Cancelled(value) => visit(value),
                }
                future.waiters.iter().copied().for_each(&mut visit);
                future.callbacks.iter().copied().for_each(visit);
            }
            Self::AsyncTask(task) => {
                visit(task.class);
                visit(task.coroutine);
                match task.state {
                    AsyncState::Pending => {}
                    AsyncState::Finished(value)
                    | AsyncState::Failed(value)
                    | AsyncState::Cancelled(value) => visit(value),
                }
                task.waiters.iter().copied().for_each(&mut visit);
                task.callbacks.iter().copied().for_each(&mut visit);
                task.waiting_on.iter().copied().for_each(visit);
            }
            Self::AsyncFutureIterator { class, source } => {
                visit(*class);
                visit(*source);
            }
            Self::AsyncEventLoop { class, .. } => visit(*class),
            Self::Dict(dict) => {
                for (key, value) in &dict.entries {
                    visit(*key);
                    visit(*value);
                }
            }
            Self::Set(set) => set.entries.iter().for_each(|(value, _)| visit(*value)),
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
    pending_generators: Vec<Value>,
    pending_finalizers: Vec<Value>,
    finalized_objects: Vec<Value>,
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

    pub(crate) fn has_pending_generator_finalizers(&self) -> bool {
        !self.pending_generators.is_empty()
    }

    pub(crate) fn pop_generator_finalizer(&mut self) -> Option<Value> {
        self.pending_generators.pop()
    }

    pub(crate) fn has_pending_object_finalizers(&self) -> bool {
        !self.pending_finalizers.is_empty()
    }

    pub(crate) fn pop_object_finalizer(&mut self) -> Option<Value> {
        self.pending_finalizers.pop()
    }

    /// Queue every currently allocated object with a user-visible `__del__`.
    /// Shutdown has no guest roots, so this is intentionally explicit rather
    /// than relying on reachability discovery during a later collection.
    pub(crate) fn queue_all_object_finalizers(&mut self) -> Result<()> {
        let candidates = self
            .objects
            .iter()
            .filter_map(|entry| {
                let value = Value::heap(entry.slot, self.slots[entry.slot as usize].generation);
                matches!(
                    entry.object,
                    Object::Instance { .. } | Object::Exception { .. }
                )
                .then_some(value)
            })
            .collect::<Vec<_>>();
        for value in candidates {
            self.queue_object_finalizer(value)?;
        }
        Ok(())
    }

    fn queue_object_finalizer(&mut self, value: Value) -> Result<bool> {
        if self.finalized_objects.contains(&value) || self.pending_finalizers.contains(&value) {
            return Ok(false);
        }
        if self.special_method_call(value, "__del__")?.is_none() {
            return Ok(false);
        }
        self.finalized_objects.push(value);
        self.pending_finalizers.push(value);
        Ok(true)
    }

    pub(crate) fn queue_all_suspended_generators(&mut self) {
        for entry in &self.objects {
            if matches!(
                &entry.object,
                Object::Generator(GeneratorFrame {
                    state: GeneratorState::Suspended,
                    ..
                })
            ) {
                let generation = self.slots[entry.slot as usize].generation;
                let value = Value::heap(entry.slot, generation);
                if !self.pending_generators.contains(&value) {
                    self.pending_generators.push(value);
                }
            }
        }
    }
    /// Owner-aware mutation boundary for the native module registry.
    pub fn add_module_member(&mut self, owner: Value, name: &str, value: Value) -> Result<()> {
        self.write_barrier(owner, value);
        let Object::Module(members) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected module"));
        };
        if let Some((_, current)) = members.iter_mut().find(|(member, _)| member == name) {
            *current = value;
            return Ok(());
        }
        let before = members.capacity() * std::mem::size_of::<(String, Value)>();
        let name = name.to_owned();
        let name_bytes = name.capacity();
        members.push((name, value));
        self.bytes +=
            members.capacity() * std::mem::size_of::<(String, Value)>() - before + name_bytes;
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }
    pub fn delete_module_member(&mut self, owner: Value, name: &str) -> Result<()> {
        let Object::Module(members) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "expected module"));
        };
        let Some(index) = members.iter().position(|(member, _)| member == name) else {
            return Err(Diagnostic::new(
                "AttributeError",
                format!("module has no attribute '{name}'"),
            ));
        };
        let (removed, _) = members.remove(index);
        self.bytes = self.bytes.saturating_sub(removed.capacity());
        Ok(())
    }
    pub fn reset_module_members(&mut self, owner: Value) -> Result<()> {
        let removed_bytes = {
            let Object::Module(members) = self.get_mut(owner)? else {
                return Err(Diagnostic::new("TypeError", "expected module"));
            };
            let mut removed_bytes = 0;
            members.retain(|(name, _)| {
                let retain = matches!(name.as_str(), "__name__" | "__file__");
                if !retain {
                    removed_bytes += name.capacity();
                }
                retain
            });
            removed_bytes
        };
        self.bytes = self.bytes.saturating_sub(removed_bytes);
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

    pub(crate) fn set_exception_traceback(
        &mut self,
        owner: Value,
        traceback: Option<Value>,
    ) -> Result<()> {
        if let Some(traceback) = traceback {
            self.write_barrier(owner, traceback);
        }
        let Object::Exception {
            traceback: slot, ..
        } = self.get_mut(owner)?
        else {
            return Err(Diagnostic::new("TypeError", "expected exception instance"));
        };
        *slot = traceback;
        Ok(())
    }

    pub(crate) fn stop_iteration_value(&self, owner: Value) -> Option<Value> {
        match self.try_get(owner) {
            Some(Object::Exception {
                stop_iteration_value: Some(value),
                ..
            }) => Some(*value),
            _ => None,
        }
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
    pub(crate) fn replace_list(&mut self, owner: Value, values: Vec<Value>) -> Result<()> {
        let owner = self.native_value(owner);
        if !matches!(self.get(owner)?, Object::List(_)) {
            return Err(Diagnostic::new("TypeError", "expected list"));
        }
        values
            .iter()
            .copied()
            .for_each(|value| self.write_barrier(owner, value));
        let before = self.get(owner)?.estimated_bytes();
        let Object::List(current) = self.get_mut(owner)? else {
            unreachable!("validated list changed kind")
        };
        *current = values;
        let after = self.get(owner)?.estimated_bytes();
        if after >= before {
            self.bytes += after - before;
        } else {
            self.bytes -= before - after;
        }
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

    pub(crate) fn is_generator(&self, value: Value) -> bool {
        matches!(
            self.try_get(value),
            Some(Object::Generator(GeneratorFrame {
                kind: GeneratorKind::Generator,
                ..
            }))
        )
    }

    pub(crate) fn is_coroutine(&self, value: Value) -> bool {
        matches!(
            self.try_get(value),
            Some(Object::Generator(GeneratorFrame {
                kind: GeneratorKind::Coroutine,
                ..
            }))
        )
    }

    pub(crate) fn is_async_generator(&self, value: Value) -> bool {
        matches!(
            self.try_get(value),
            Some(Object::Generator(GeneratorFrame {
                kind: GeneratorKind::AsyncGenerator,
                ..
            }))
        )
    }

    pub(crate) fn is_resumable(&self, value: Value) -> bool {
        matches!(
            self.try_get(value),
            Some(Object::Generator(GeneratorFrame {
                kind: GeneratorKind::Generator | GeneratorKind::Coroutine,
                ..
            }))
        )
    }

    pub(crate) fn coroutine_iterator_source(&self, value: Value) -> Option<Value> {
        match self.try_get(value) {
            Some(Object::CoroutineIterator { coroutine, .. }) => Some(*coroutine),
            _ => None,
        }
    }

    pub(crate) fn async_generator_awaitable(
        &self,
        value: Value,
    ) -> Option<(Value, AsyncGeneratorOperation, AsyncGeneratorAwaitState)> {
        match self.try_get(value) {
            Some(Object::AsyncGeneratorAwaitable {
                generator,
                operation,
                state,
                ..
            }) => Some((*generator, *operation, *state)),
            _ => None,
        }
    }

    pub(crate) fn set_async_generator_await_state(
        &mut self,
        owner: Value,
        state: AsyncGeneratorAwaitState,
    ) -> Result<()> {
        let Object::AsyncGeneratorAwaitable { state: current, .. } = self.get_mut(owner)? else {
            return Err(Diagnostic::new(
                "TypeError",
                "object is not an async-generator awaitable",
            ));
        };
        *current = state;
        Ok(())
    }

    pub(crate) fn claim_async_generator_driver(
        &mut self,
        generator: Value,
        awaitable: Value,
    ) -> Result<()> {
        self.write_barrier(generator, awaitable);
        let Object::Generator(frame) = self.get_mut(generator)? else {
            return Err(Diagnostic::new(
                "TypeError",
                "object is not an async generator",
            ));
        };
        if frame.kind != GeneratorKind::AsyncGenerator {
            return Err(Diagnostic::new(
                "TypeError",
                "object is not an async generator",
            ));
        }
        if frame
            .async_driver
            .is_some_and(|current| current != awaitable)
        {
            return Err(Diagnostic::new(
                "RuntimeError",
                "asynchronous generator is already running",
            ));
        }
        frame.async_driver = Some(awaitable);
        Ok(())
    }

    pub(crate) fn release_async_generator_driver(
        &mut self,
        generator: Value,
        awaitable: Value,
    ) -> Result<()> {
        let Object::Generator(frame) = self.get_mut(generator)? else {
            return Err(Diagnostic::new(
                "TypeError",
                "object is not an async generator",
            ));
        };
        if frame.async_driver == Some(awaitable) {
            frame.async_driver = None;
        }
        Ok(())
    }

    pub(crate) fn async_state(&self, value: Value) -> Option<AsyncState> {
        match self.try_get(value) {
            Some(Object::AsyncFuture(future)) => Some(future.state),
            Some(Object::AsyncTask(task)) => Some(task.state),
            _ => None,
        }
    }

    pub(crate) fn async_future_iterator_source(&self, value: Value) -> Option<Value> {
        match self.try_get(value) {
            Some(Object::AsyncFutureIterator { source, .. }) => Some(*source),
            _ => None,
        }
    }

    pub(crate) fn async_future_timer(&self, value: Value) -> Option<(u64, Value)> {
        match self.try_get(value) {
            Some(Object::AsyncFuture(future)) if future.state == AsyncState::Pending => {
                future.due_tick.map(|tick| (tick, future.timer_result))
            }
            _ => None,
        }
    }

    pub(crate) fn async_task_coroutine(&self, value: Value) -> Option<Value> {
        match self.try_get(value) {
            Some(Object::AsyncTask(task)) => Some(task.coroutine),
            _ => None,
        }
    }

    pub(crate) fn request_task_cancel(&mut self, value: Value) -> Result<(bool, Option<Value>)> {
        let Object::AsyncTask(task) = self.get_mut(value)? else {
            return Err(Diagnostic::new("TypeError", "object is not a task"));
        };
        if task.state != AsyncState::Pending || task.cancel_requested {
            return Ok((false, None));
        }
        task.cancel_requested = true;
        let waiting_on = task.waiting_on.take();
        Ok((true, waiting_on))
    }

    pub(crate) fn clear_task_cancel_request(&mut self, value: Value) -> Result<()> {
        let Object::AsyncTask(task) = self.get_mut(value)? else {
            return Err(Diagnostic::new("TypeError", "object is not a task"));
        };
        task.cancel_requested = false;
        Ok(())
    }

    pub(crate) fn remove_async_waiter(&mut self, source: Value, task: Value) -> Result<()> {
        let before = self.get(source)?.estimated_bytes();
        match self.get_mut(source)? {
            Object::AsyncFuture(future) => future.waiters.retain(|waiter| *waiter != task),
            Object::AsyncTask(source_task) => source_task.waiters.retain(|waiter| *waiter != task),
            _ => return Err(Diagnostic::new("TypeError", "object is not a future")),
        }
        let after = self.get(source)?.estimated_bytes();
        self.bytes = self.bytes.saturating_sub(before.saturating_sub(after));
        Ok(())
    }

    pub(crate) fn add_async_waiter(&mut self, source: Value, task: Value) -> Result<()> {
        self.write_barrier(source, task);
        let before = self.get(source)?.estimated_bytes();
        match self.get_mut(source)? {
            Object::AsyncFuture(future) if future.state == AsyncState::Pending => {
                if !future.waiters.contains(&task) {
                    future.waiters.push(task);
                }
            }
            Object::AsyncTask(source_task) if source_task.state == AsyncState::Pending => {
                if !source_task.waiters.contains(&task) {
                    source_task.waiters.push(task);
                }
            }
            Object::AsyncFuture(_) | Object::AsyncTask(_) => {}
            _ => return Err(Diagnostic::new("TypeError", "object is not awaitable")),
        }
        let after = self.get(source)?.estimated_bytes();
        self.bytes += after.saturating_sub(before);
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }

    pub(crate) fn add_async_callback(&mut self, source: Value, callback: Value) -> Result<bool> {
        self.write_barrier(source, callback);
        let before = self.get(source)?.estimated_bytes();
        let pending = match self.get_mut(source)? {
            Object::AsyncFuture(future) => {
                if future.state == AsyncState::Pending {
                    future.callbacks.push(callback);
                    true
                } else {
                    false
                }
            }
            Object::AsyncTask(task) => {
                if task.state == AsyncState::Pending {
                    task.callbacks.push(callback);
                    true
                } else {
                    false
                }
            }
            _ => return Err(Diagnostic::new("TypeError", "object is not a future")),
        };
        let after = self.get(source)?.estimated_bytes();
        self.bytes += after.saturating_sub(before);
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(pending)
    }

    pub(crate) fn complete_async(
        &mut self,
        source: Value,
        state: AsyncState,
    ) -> Result<(Vec<Value>, Vec<Value>)> {
        if state == AsyncState::Pending {
            return Err(Diagnostic::new(
                "RuntimeError",
                "future completion state must be terminal",
            ));
        }
        match state {
            AsyncState::Finished(value)
            | AsyncState::Failed(value)
            | AsyncState::Cancelled(value) => self.write_barrier(source, value),
            AsyncState::Pending => unreachable!(),
        }
        let before = self.get(source)?.estimated_bytes();
        let (waiters, callbacks) = match self.get_mut(source)? {
            Object::AsyncFuture(future) => {
                if future.state != AsyncState::Pending {
                    return Err(Diagnostic::new(
                        "InvalidStateError",
                        "future is already done",
                    ));
                }
                future.state = state;
                future.due_tick = None;
                (
                    std::mem::take(&mut future.waiters),
                    std::mem::take(&mut future.callbacks),
                )
            }
            Object::AsyncTask(task) => {
                if task.state != AsyncState::Pending {
                    return Err(Diagnostic::new("InvalidStateError", "task is already done"));
                }
                task.state = state;
                task.waiting_on = None;
                (
                    std::mem::take(&mut task.waiters),
                    std::mem::take(&mut task.callbacks),
                )
            }
            _ => return Err(Diagnostic::new("TypeError", "object is not a future")),
        };
        let after = self.get(source)?.estimated_bytes();
        self.bytes = self.bytes.saturating_sub(before.saturating_sub(after));
        Ok((waiters, callbacks))
    }

    pub(crate) fn set_task_waiting(&mut self, task: Value, source: Option<Value>) -> Result<()> {
        if let Some(source) = source {
            self.write_barrier(task, source);
        }
        let Object::AsyncTask(task) = self.get_mut(task)? else {
            return Err(Diagnostic::new("TypeError", "object is not a task"));
        };
        task.waiting_on = source;
        Ok(())
    }

    pub(crate) fn generator_state(&self, value: Value) -> Option<GeneratorState> {
        match self.try_get(value) {
            Some(Object::Generator(frame)) => Some(frame.state),
            _ => None,
        }
    }

    pub(crate) fn generator_kind(&self, value: Value) -> Option<GeneratorKind> {
        match self.try_get(value) {
            Some(Object::Generator(frame)) => Some(frame.kind),
            _ => None,
        }
    }

    pub(crate) fn resume_generator(
        &mut self,
        owner: Value,
        execution: u64,
    ) -> Result<ResumedGenerator> {
        let before = self.get(owner)?.estimated_bytes();
        let (code, ip, registers, cells, exception_stack, resume_register, yield_from) = {
            let Object::Generator(frame) = self.get_mut(owner)? else {
                return Err(Diagnostic::new("TypeError", "object is not a generator"));
            };
            if frame.execution != execution {
                return Err(Diagnostic::new(
                    "RuntimeError",
                    "generator belongs to a previous module execution",
                ));
            }
            match frame.state {
                GeneratorState::Completed => {
                    return Err(Diagnostic::new("StopIteration", String::new()))
                }
                GeneratorState::Running => {
                    return Err(Diagnostic::new("ValueError", "generator already executing"))
                }
                GeneratorState::Created | GeneratorState::Suspended => {}
            }
            frame.state = GeneratorState::Running;
            (
                frame.code,
                frame.ip,
                std::mem::take(&mut frame.registers),
                std::mem::take(&mut frame.cells),
                std::mem::take(&mut frame.exception_stack),
                frame.resume_register.take(),
                frame.yield_from.take(),
            )
        };
        let after = self.get(owner)?.estimated_bytes();
        self.bytes = self.bytes.saturating_sub(before.saturating_sub(after));
        Ok(ResumedGenerator {
            code,
            ip,
            registers,
            cells,
            exception_stack,
            resume_register,
            yield_from,
        })
    }

    pub(crate) fn suspend_generator(
        &mut self,
        owner: Value,
        suspended: SuspendedGenerator,
    ) -> Result<()> {
        let SuspendedGenerator {
            ip,
            registers,
            cells,
            exception_stack,
            resume_register,
            yield_from,
        } = suspended;
        for value in registers
            .iter()
            .chain(&cells)
            .chain(&exception_stack)
            .chain(&yield_from)
            .copied()
        {
            self.write_barrier(owner, value);
        }
        let before = self.get(owner)?.estimated_bytes();
        {
            let Object::Generator(frame) = self.get_mut(owner)? else {
                return Err(Diagnostic::new("TypeError", "object is not a generator"));
            };
            if frame.state != GeneratorState::Running {
                return Err(Diagnostic::new("RuntimeError", "generator is not running"));
            }
            frame.ip = ip;
            frame.registers = registers;
            frame.cells = cells;
            frame.exception_stack = exception_stack;
            frame.resume_register = Some(resume_register);
            frame.yield_from = yield_from;
            frame.state = GeneratorState::Suspended;
        }
        let after = self.get(owner)?.estimated_bytes();
        self.bytes += after.saturating_sub(before);
        self.peak_bytes = self.peak_bytes.max(self.bytes);
        Ok(())
    }

    pub(crate) fn complete_generator(&mut self, owner: Value) -> Result<()> {
        self.complete_generator_with_value(owner, Value::NONE)
    }

    pub(crate) fn complete_generator_with_value(
        &mut self,
        owner: Value,
        value: Value,
    ) -> Result<()> {
        self.write_barrier(owner, value);
        let before = self.get(owner)?.estimated_bytes();
        let Object::Generator(frame) = self.get_mut(owner)? else {
            return Err(Diagnostic::new("TypeError", "object is not a generator"));
        };
        frame.registers = Vec::new();
        frame.cells = Vec::new();
        frame.exception_stack = Vec::new();
        frame.yield_from = None;
        frame.async_driver = None;
        frame.return_value = value;
        frame.state = GeneratorState::Completed;
        let after = self.get(owner)?.estimated_bytes();
        self.bytes = self.bytes.saturating_sub(before.saturating_sub(after));
        Ok(())
    }

    pub(crate) fn generator_return_value(&self, owner: Value) -> Option<Value> {
        match self.try_get(owner) {
            Some(Object::Generator(frame)) if frame.state == GeneratorState::Completed => {
                Some(frame.return_value)
            }
            _ => None,
        }
    }

    pub(crate) fn generator_yield_from(&self, owner: Value) -> Option<Value> {
        match self.try_get(owner) {
            Some(Object::Generator(frame)) if frame.state == GeneratorState::Suspended => {
                frame.yield_from
            }
            _ => None,
        }
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
            Object::Dict(dict) | Object::Set(dict) => !dict.entries.is_empty(),
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
    pub fn format_spec(&self, value: Value, spec: &str) -> Result<String> {
        if spec.is_empty() {
            return self.format(value, false);
        }
        let native = self.native_value(value);
        if let Some(integer) = native.integer() {
            return format_integer(&BigInt::from(integer), spec);
        }
        match self.get(native)? {
            Object::Int(integer) => format_integer(integer, spec),
            Object::Float(float) => format_float(*float, spec),
            Object::Str(string) => format_string(string, spec),
            _ => Err(Diagnostic::new(
                "TypeError",
                format!("unsupported format string '{spec}'"),
            )),
        }
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
            Object::TypeParam { name, .. } | Object::TypeAlias { name, .. } => name.clone(),
            Object::GenericAlias { origin, args } => {
                let origin = match self.get(*origin)? {
                    Object::Class(class) => class.name.clone(),
                    Object::TypeAlias { name, .. } => name.clone(),
                    _ => self.format_depth(*origin, true, path)?,
                };
                let Object::Tuple(arguments) = self.get(*args)? else {
                    return Err(Diagnostic::new(
                        "BytecodeError",
                        "invalid generic alias args",
                    ));
                };
                let mut text = format!("{origin}[");
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        text.push_str(", ");
                    }
                    match self.get(*argument) {
                        Ok(Object::Class(class)) => text.push_str(&class.name),
                        _ => text.push_str(&self.format_depth(*argument, true, path)?),
                    }
                }
                text.push(']');
                text
            }
            Object::Generator(frame) => match frame.kind {
                GeneratorKind::Generator => "<generator object>".into(),
                GeneratorKind::Coroutine => "<coroutine object>".into(),
                GeneratorKind::AsyncGenerator => "<async_generator object>".into(),
            },
            Object::CoroutineIterator { .. } => "<coroutine_wrapper object>".into(),
            Object::AsyncGeneratorAwaitable { .. } => "<async_generator_awaitable object>".into(),
            Object::AsyncFuture(future) => match future.state {
                AsyncState::Pending => "<Future pending>".into(),
                AsyncState::Finished(_) => "<Future finished>".into(),
                AsyncState::Failed(_) => "<Future finished exception>".into(),
                AsyncState::Cancelled(_) => "<Future cancelled>".into(),
            },
            Object::AsyncTask(task) => match task.state {
                AsyncState::Pending => "<Task pending>".into(),
                AsyncState::Finished(_) => "<Task finished>".into(),
                AsyncState::Failed(_) => "<Task finished exception>".into(),
                AsyncState::Cancelled(_) => "<Task cancelled>".into(),
            },
            Object::AsyncFutureIterator { .. } => "<_asyncio_future_iter object>".into(),
            Object::AsyncEventLoop { generation, .. } => {
                format!("<EventLoop running generation={generation}>")
            }
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
            Object::Set(set) => {
                if set.entries.is_empty() {
                    "set()".into()
                } else {
                    let mut result = String::from("{");
                    for (index, (value, _)) in set.entries.iter().enumerate() {
                        if index > 0 {
                            result.push_str(", ");
                        }
                        result.push_str(&self.format_depth(*value, true, path)?);
                    }
                    result.push('}');
                    result
                }
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

#[derive(Clone, Debug)]
struct FormatSpec {
    fill: char,
    explicit_fill: bool,
    align: Option<char>,
    sign: Option<char>,
    coerce_negative_zero: bool,
    alternate: bool,
    zero: bool,
    width: Option<usize>,
    grouping: Option<char>,
    precision: Option<usize>,
    kind: Option<char>,
}

fn parse_format_spec(spec: &str) -> Result<FormatSpec> {
    let chars: Vec<char> = spec.chars().collect();
    let mut at = 0;
    let mut parsed = FormatSpec {
        fill: ' ',
        explicit_fill: false,
        align: None,
        sign: None,
        coerce_negative_zero: false,
        alternate: false,
        zero: false,
        width: None,
        grouping: None,
        precision: None,
        kind: None,
    };
    if chars.get(1).is_some_and(|c| "<>=^".contains(*c)) {
        parsed.fill = chars[0];
        parsed.explicit_fill = true;
        parsed.align = Some(chars[1]);
        at = 2;
    } else if chars.first().is_some_and(|c| "<>=^".contains(*c)) {
        parsed.align = Some(chars[0]);
        at = 1;
    }
    if chars.get(at).is_some_and(|c| "+- ".contains(*c)) {
        parsed.sign = Some(chars[at]);
        at += 1;
    }
    if chars.get(at) == Some(&'z') {
        parsed.coerce_negative_zero = true;
        at += 1;
    }
    if chars.get(at) == Some(&'#') {
        parsed.alternate = true;
        at += 1;
    }
    if chars.get(at) == Some(&'0') {
        parsed.zero = true;
        at += 1;
    }
    let width_start = at;
    while chars.get(at).is_some_and(char::is_ascii_digit) {
        at += 1;
    }
    if at > width_start {
        parsed.width = Some(
            chars[width_start..at]
                .iter()
                .collect::<String>()
                .parse()
                .map_err(|_| Diagnostic::new("ValueError", "format width is too large"))?,
        );
    }
    if chars.get(at).is_some_and(|c| matches!(c, ',' | '_')) {
        parsed.grouping = Some(chars[at]);
        at += 1;
    }
    if chars.get(at) == Some(&'.') {
        at += 1;
        let precision_start = at;
        while chars.get(at).is_some_and(char::is_ascii_digit) {
            at += 1;
        }
        if at == precision_start {
            return Err(Diagnostic::new("ValueError", "missing format precision"));
        }
        parsed.precision = Some(
            chars[precision_start..at]
                .iter()
                .collect::<String>()
                .parse()
                .map_err(|_| Diagnostic::new("ValueError", "format precision is too large"))?,
        );
    }
    if at < chars.len() {
        parsed.kind = Some(chars[at]);
        at += 1;
    }
    if at != chars.len() {
        return Err(Diagnostic::new(
            "ValueError",
            format!("invalid format specifier '{spec}'"),
        ));
    }
    Ok(parsed)
}

fn apply_width(mut text: String, spec: &FormatSpec, numeric_prefix: usize) -> String {
    let Some(width) = spec.width else {
        return text;
    };
    let length = text.chars().count();
    if length >= width {
        return text;
    }
    let padding = width - length;
    let align = spec.align.unwrap_or(if spec.zero { '=' } else { '>' });
    let fill = if spec.zero && !spec.explicit_fill {
        '0'
    } else {
        spec.fill
    };
    match align {
        '<' => text.extend(std::iter::repeat_n(fill, padding)),
        '^' => {
            let left = padding / 2;
            let right = padding - left;
            text = format!(
                "{}{}{}",
                std::iter::repeat_n(fill, left).collect::<String>(),
                text,
                std::iter::repeat_n(fill, right).collect::<String>()
            );
        }
        '=' => {
            let split = numeric_prefix.min(text.len());
            text = format!(
                "{}{}{}",
                &text[..split],
                std::iter::repeat_n(fill, padding).collect::<String>(),
                &text[split..]
            );
        }
        _ => {
            text = format!(
                "{}{}",
                std::iter::repeat_n(fill, padding).collect::<String>(),
                text
            );
        }
    }
    text
}

fn grouped(mut digits: String, separator: Option<char>, group: usize) -> String {
    let Some(separator) = separator else {
        return digits;
    };
    let mut result = String::with_capacity(digits.len() + digits.len() / group);
    let first = digits.len() % group;
    if first != 0 {
        result.push_str(&digits[..first]);
        if digits.len() > first {
            result.push(separator);
        }
    }
    digits.drain(..first);
    for (index, chunk) in digits.as_bytes().chunks(group).enumerate() {
        if index > 0 {
            result.push(separator);
        }
        result.push_str(std::str::from_utf8(chunk).expect("ASCII digits"));
    }
    result
}

fn format_integer(value: &BigInt, source: &str) -> Result<String> {
    let spec = parse_format_spec(source)?;
    if matches!(spec.kind, Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%')) {
        let value = value.to_f64().ok_or_else(|| {
            Diagnostic::new("OverflowError", "integer is too large for floating format")
        })?;
        return format_float(value, source);
    }
    if spec.coerce_negative_zero {
        return Err(Diagnostic::new(
            "ValueError",
            "negative zero coercion is not allowed in integer format specifier",
        ));
    }
    if spec.precision.is_some() {
        return Err(Diagnostic::new(
            "ValueError",
            "precision not allowed in integer format specifier",
        ));
    }
    let kind = spec.kind.unwrap_or('d');
    let (radix, prefix, upper, group) = match kind {
        'd' | 'n' => (10, "", false, 3),
        'b' => (2, "0b", false, 4),
        'o' => (8, "0o", false, 4),
        'x' => (16, "0x", false, 4),
        'X' => (16, "0X", true, 4),
        'c' => {
            if spec.sign.is_some() || spec.alternate || spec.zero || spec.grouping.is_some() {
                return Err(Diagnostic::new(
                    "ValueError",
                    "invalid integer character format",
                ));
            }
            let code = value
                .to_u32()
                .and_then(char::from_u32)
                .ok_or_else(|| Diagnostic::new("OverflowError", "%c arg not in range"))?;
            return Ok(apply_width(code.to_string(), &spec, 0));
        }
        _ => {
            return Err(Diagnostic::new(
                "ValueError",
                format!("unknown format code '{kind}' for integer"),
            ))
        }
    };
    if spec.grouping == Some(',') && radix != 10 {
        return Err(Diagnostic::new(
            "ValueError",
            "comma grouping is not allowed for non-decimal integers",
        ));
    }
    let mut digits = value.abs().to_str_radix(radix);
    if upper {
        digits.make_ascii_uppercase();
    }
    digits = grouped(digits, spec.grouping, group);
    let sign = if value.is_negative() {
        "-"
    } else {
        match spec.sign {
            Some('+') => "+",
            Some(' ') => " ",
            _ => "",
        }
    };
    let prefix = if spec.alternate { prefix } else { "" };
    let text = format!("{sign}{prefix}{digits}");
    Ok(apply_width(text, &spec, sign.len() + prefix.len()))
}

fn format_float(value: f64, source: &str) -> Result<String> {
    let spec = parse_format_spec(source)?;
    let magnitude = value.abs();
    let kind = spec.kind;
    let precision = spec.precision.unwrap_or(6);
    let mut body = if magnitude.is_nan() {
        "nan".to_owned()
    } else if magnitude.is_infinite() {
        "inf".to_owned()
    } else {
        match kind {
            Some('f' | 'F') => {
                alternate_decimal(format!("{magnitude:.precision$}"), spec.alternate)
            }
            Some('e' | 'E') => normalize_exponent(&alternate_decimal(
                format!("{magnitude:.precision$e}"),
                spec.alternate,
            )),
            Some('%') => format!(
                "{}%",
                alternate_decimal(format!("{:.precision$}", magnitude * 100.0), spec.alternate)
            ),
            Some('g' | 'G' | 'n') => general_float(magnitude, precision, spec.alternate),
            None if spec.precision.is_some() => general_float(magnitude, precision, spec.alternate),
            None => normalize_exponent(&format!("{magnitude:?}")),
            _ => {
                return Err(Diagnostic::new(
                    "ValueError",
                    format!(
                        "unknown format code '{}' for float",
                        kind.expect("unknown explicit kind")
                    ),
                ))
            }
        }
    };
    if matches!(kind, Some('E' | 'F' | 'G')) {
        body.make_ascii_uppercase();
    }
    if let Some(separator) = spec.grouping {
        if let Some(dot) = body.find('.') {
            let integer = grouped(body[..dot].to_owned(), Some(separator), 3);
            body = format!("{integer}{}", &body[dot..]);
        } else if !body.contains('e') && !body.contains('E') {
            body = grouped(body, Some(separator), 3);
        }
    }
    let negative = value.is_sign_negative() && !(spec.coerce_negative_zero && magnitude == 0.0);
    let sign = if negative {
        "-"
    } else {
        match spec.sign {
            Some('+') => "+",
            Some(' ') => " ",
            _ => "",
        }
    };
    let text = format!("{sign}{body}");
    Ok(apply_width(text, &spec, sign.len()))
}

fn alternate_decimal(mut text: String, alternate: bool) -> String {
    if !alternate || text.contains('.') {
        return text;
    }
    if let Some(exponent) = text.find(['e', 'E']) {
        text.insert(exponent, '.');
    } else {
        text.push('.');
    }
    text
}

fn trim_float_zeros(text: &str) -> String {
    let exponent = text.find(['e', 'E']).unwrap_or(text.len());
    let (mantissa, suffix) = text.split_at(exponent);
    let mut mantissa = mantissa.to_owned();
    if mantissa.contains('.') {
        while mantissa.ends_with('0') {
            mantissa.pop();
        }
        if mantissa.ends_with('.') {
            mantissa.pop();
        }
    }
    format!("{mantissa}{suffix}")
}

fn normalize_exponent(text: &str) -> String {
    let Some(position) = text.find(['e', 'E']) else {
        return text.to_owned();
    };
    let marker = text.as_bytes()[position] as char;
    let Ok(exponent) = text[position + 1..].parse::<i32>() else {
        return text.to_owned();
    };
    format!("{}{marker}{exponent:+03}", &text[..position])
}

fn general_float(value: f64, precision: usize, alternate: bool) -> String {
    let precision = precision.max(1);
    if value == 0.0 {
        let fixed = format!("{value:.digits$}", digits = precision.saturating_sub(1));
        return if alternate {
            alternate_decimal(fixed, true)
        } else {
            trim_float_zeros(&fixed)
        };
    }
    let scientific = format!("{value:.digits$e}", digits = precision.saturating_sub(1));
    let exponent = scientific
        .split_once('e')
        .and_then(|(_, exponent)| exponent.parse::<i32>().ok())
        .unwrap_or(0);
    let mut text = if exponent < -4 || exponent >= precision as i32 {
        scientific
    } else {
        let decimals = (precision as i32 - 1 - exponent).max(0) as usize;
        format!("{value:.decimals$}")
    };
    if !alternate {
        text = trim_float_zeros(&text);
    } else {
        text = alternate_decimal(text, true);
    }
    normalize_exponent(&text)
}

fn format_string(value: &str, source: &str) -> Result<String> {
    let spec = parse_format_spec(source)?;
    if spec.sign.is_some()
        || spec.alternate
        || spec.coerce_negative_zero
        || spec.grouping.is_some()
        || !matches!(spec.kind, None | Some('s'))
        || spec.align == Some('=')
    {
        return Err(Diagnostic::new(
            "ValueError",
            "invalid string format specifier",
        ));
    }
    let mut text = match spec.precision {
        Some(limit) => value.chars().take(limit).collect(),
        None => value.to_owned(),
    };
    let string_spec = FormatSpec {
        align: Some(spec.align.unwrap_or('<')),
        ..spec
    };
    text = apply_width(text, &string_spec, 0);
    Ok(text)
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
                Self::TypeParam { name, .. } | Self::TypeAlias { name, .. } => name.capacity(),
                Self::Generator(frame) => frame.payload_bytes(),
                Self::AsyncFuture(future) => {
                    (future.waiters.capacity() + future.callbacks.capacity())
                        * std::mem::size_of::<Value>()
                }
                Self::AsyncTask(task) => {
                    (task.waiters.capacity() + task.callbacks.capacity())
                        * std::mem::size_of::<Value>()
                }
                Self::Dict(dict) | Self::Set(dict) => dict.estimated_bytes(),
                _ => 0,
            }
    }
}
