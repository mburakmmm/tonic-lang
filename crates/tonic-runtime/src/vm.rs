#[path = "calls.rs"]
pub(crate) mod calls;
use crate::classes::{DescriptorCall, DirectMethodKind};
use crate::{
    heap::{Builtin, Heap, Object},
    native::{
        fastmath_add, fastmath_array, fastmath_sum, Context, HandleTable, NativeCallable,
        NativeDef, NativeFn, PersistentHandle,
    },
    runtime_owner::RuntimeOwner,
    shapes::ShapeId,
    value::Value,
};
use calls::{Arguments, ExpandedArgs};
use num_bigint::BigInt;
use std::{collections::HashMap, io::Write, sync::Arc, thread::ThreadId};
use tonic_core::{
    ast::Constant,
    bytecode::{Op, Program, VerifiedProgram},
    diagnostic::{Diagnostic, Result},
};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub instructions: Option<u64>,
    pub frames: usize,
    pub registers: usize,
    pub arguments: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            instructions: None,
            frames: 1024,
            registers: 1_048_576,
            arguments: 65_535,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub instructions: u64,
    pub calls: u64,
    pub native_calls: u64,
    pub callback_calls: u64,
    pub buffer_exports: u64,
    pub buffer_copies: u64,
    pub foreign_wrapper_creations: u64,
    pub foreign_trace_calls: u64,
    pub foreign_destructor_calls: u64,
    pub foreign_destructor_panics: u64,
    pub backedges: u64,
    pub heap_allocations: u64,
    pub estimated_heap_bytes: usize,
    pub peak_registers: usize,
    pub gc_collections: u64,
    pub gc_minor_collections: u64,
    pub gc_major_collections: u64,
    pub gc_reclaimed: u64,
    pub gc_promoted: u64,
    pub gc_moved: u64,
    pub gc_pause_ns: u128,
    pub gc_max_pause_ns: u128,
    /// Allocated heap entries at the snapshot, including garbage awaiting GC.
    pub live_objects: usize,
    pub peak_heap_bytes: usize,
    pub jit_compile_attempts: u64,
    pub jit_compiled: u64,
    pub jit_calls: u64,
    pub jit_returns: u64,
    pub jit_deopts: u64,
    pub jit_despecialized: u64,
    pub jit_deferred: u64,
    pub jit_fallbacks: u64,
    pub jit_unprofitable: u64,
    pub jit_compile_ns: u128,
    pub jit_code_bytes: usize,
    pub jit_helper_calls: u64,
    pub jit_safepoints: u64,
    pub jit_gc_collections: u64,
    pub jit_runtime_errors: u64,
    pub jit_side_exits: u64,
    pub jit_resumes: u64,
    pub jit_direct_call_sites: u64,
    pub jit_direct_calls: u64,
    pub jit_direct_method_sites: u64,
    pub jit_backedge_polls: u64,
    pub jit_osr_entries: u64,
    pub quickened: u64,
    pub quickened_misses: u64,
    pub call_quickened: u64,
    pub call_cache_misses: u64,
    pub call_pic_promotions: u64,
    pub attr_quickened: u64,
    pub attr_cache_misses: u64,
    pub attr_pic_promotions: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePhase {
    Running,
    ShuttingDown,
    Finalizing,
    Dead,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    #[default]
    Interpreter,
    Jit,
}
struct Frame {
    namespace: Option<Value>,
    action: ReturnAction,
    code: usize,
    ip: usize,
    base: usize,
    destination: Option<usize>,
    cell_base: usize,
    callable: Option<Value>,
    jit_attempted: bool,
    jit_resume: bool,
    jit_expanded_resume_depth: Option<usize>,
}
enum JitEntry {
    Untried,
    Unsupported,
    Compiled {
        function: Box<tonic_jit::CompiledFunction>,
        deopts: u8,
    },
}

struct JitRuntime<'a> {
    heap: &'a mut Heap,
    handles: &'a mut HandleTable,
    stats: &'a mut Stats,
    roots: &'a [Value],
    globals: &'a [Value],
    symbols: &'a [String],
    gc_interval: Option<u64>,
    minor_collections: &'a mut u8,
}
impl JitRuntime<'_> {
    fn safepoint(
        &mut self,
        registers: &[u64],
    ) -> std::result::Result<(), tonic_jit::RuntimeFailure> {
        self.stats.jit_helper_calls += 1;
        self.stats.jit_safepoints += 1;
        if self.gc_interval.is_some_and(|interval| {
            self.heap.allocations - self.heap.last_collection_allocations >= interval.max(1)
        }) {
            let started = std::time::Instant::now();
            let mut roots = self.roots.to_vec();
            roots.extend(registers.iter().copied().map(Value::from_jit));
            let trace_calls = self
                .heap
                .refresh_foreign_references(self.handles)
                .map_err(runtime_failure)?;
            self.stats.foreign_trace_calls += trace_calls;
            let collection = if *self.minor_collections >= MINORS_PER_MAJOR - 1 {
                *self.minor_collections = 0;
                self.heap.collect(roots)
            } else {
                *self.minor_collections += 1;
                self.heap.collect_young(roots)
            }
            .map_err(runtime_failure)?;
            let (destructors, panics) = self.heap.drain_foreign_finalizers(self.handles);
            self.stats.foreign_destructor_calls += destructors;
            self.stats.foreign_destructor_panics += panics;
            record_collection(self.stats, self.heap, collection, started);
            self.stats.jit_gc_collections += 1;
        }
        Ok(())
    }
}
impl tonic_jit::Runtime for JitRuntime<'_> {
    fn binary(
        &mut self,
        op: tonic_jit::RuntimeOp,
        left: u64,
        right: u64,
        registers: &[u64],
    ) -> std::result::Result<u64, tonic_jit::RuntimeFailure> {
        self.safepoint(registers)?;
        let op = match op {
            tonic_jit::RuntimeOp::Div => Op::Div,
            tonic_jit::RuntimeOp::Add => Op::Add,
            tonic_jit::RuntimeOp::Sub => Op::Sub,
            tonic_jit::RuntimeOp::Mul => Op::Mul,
            tonic_jit::RuntimeOp::FloorDiv => Op::FloorDiv,
            tonic_jit::RuntimeOp::Mod => Op::Mod,
            tonic_jit::RuntimeOp::Eq => Op::Eq,
            tonic_jit::RuntimeOp::Ne => Op::Ne,
            tonic_jit::RuntimeOp::Lt => Op::Lt,
            tonic_jit::RuntimeOp::Le => Op::Le,
            tonic_jit::RuntimeOp::Gt => Op::Gt,
            tonic_jit::RuntimeOp::Ge => Op::Ge,
            tonic_jit::RuntimeOp::InplaceAdd => {
                return self
                    .heap
                    .inplace_add(Value::from_jit(left), Value::from_jit(right))
                    .map(Value::raw)
                    .map_err(runtime_failure);
            }
            tonic_jit::RuntimeOp::LoadGlobal
            | tonic_jit::RuntimeOp::LoadMethod
            | tonic_jit::RuntimeOp::LoadSequenceItem
            | tonic_jit::RuntimeOp::LoadMappingItem
            | tonic_jit::RuntimeOp::BuildTuple
            | tonic_jit::RuntimeOp::BuildDict
            | tonic_jit::RuntimeOp::DictSetSymbol
            | tonic_jit::RuntimeOp::UnboxFloat
            | tonic_jit::RuntimeOp::BoxFloat
            | tonic_jit::RuntimeOp::Poll => {
                return Err(tonic_jit::RuntimeFailure::new(
                    "JitError",
                    "invalid binary runtime operation",
                ))
            }
        };
        if matches!(op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge) {
            self.heap
                .compare(op, Value::from_jit(left), Value::from_jit(right))
                .map(Value::raw)
                .map_err(runtime_failure)
        } else {
            self.heap
                .binary(op, Value::from_jit(left), Value::from_jit(right))
                .map(Value::raw)
                .map_err(runtime_failure)
        }
    }

    fn load_global(
        &mut self,
        symbol: u32,
        _registers: &[u64],
    ) -> std::result::Result<u64, tonic_jit::RuntimeFailure> {
        self.stats.jit_helper_calls += 1;
        let index = symbol as usize;
        let value = self.globals.get(index).copied().ok_or_else(|| {
            tonic_jit::RuntimeFailure::new("JitError", "global symbol index is out of bounds")
        })?;
        if value == Value::UNBOUND {
            let name = self.symbols.get(index).map(String::as_str).unwrap_or("?");
            return Err(tonic_jit::RuntimeFailure::new(
                "NameError",
                format!("name '{name}' is not defined"),
            ));
        }
        Ok(value.raw())
    }

    fn load_method(
        &mut self,
        owner: u64,
        selector: u64,
        _registers: &[u64],
    ) -> std::result::Result<tonic_jit::MethodLookup, tonic_jit::RuntimeFailure> {
        self.stats.jit_helper_calls += 1;
        let symbol = selector as u32;
        let expected = match selector >> 32 {
            0 => DirectMethodKind::Static,
            1 => DirectMethodKind::Instance,
            2 => DirectMethodKind::Class,
            _ => {
                return Err(tonic_jit::RuntimeFailure::new(
                    "JitError",
                    "invalid method binding selector",
                ))
            }
        };
        let name = self.symbols.get(symbol as usize).ok_or_else(|| {
            tonic_jit::RuntimeFailure::new("JitError", "method symbol index is out of bounds")
        })?;
        let lookup = self
            .heap
            .direct_method(Value::from_jit(owner), name)
            .filter(|(_, kind, _)| *kind == expected);
        Ok(match lookup {
            Some((function, _, receiver)) => tonic_jit::MethodLookup {
                function: function.raw(),
                receiver: receiver.unwrap_or(Value::UNBOUND).raw(),
            },
            None => tonic_jit::MethodLookup {
                function: Value::UNBOUND.raw(),
                receiver: Value::UNBOUND.raw(),
            },
        })
    }

    fn load_sequence_item(
        &mut self,
        owner: u64,
        index: u32,
        length: u32,
        _registers: &[u64],
    ) -> std::result::Result<Option<u64>, tonic_jit::RuntimeFailure> {
        self.stats.jit_helper_calls += 1;
        let object = self
            .heap
            .get(Value::from_jit(owner))
            .map_err(runtime_failure)?;
        let values = match object {
            Object::List(values) | Object::Tuple(values) => values,
            _ => return Ok(None),
        };
        if values.len() != length as usize {
            return Ok(None);
        }
        Ok(values.get(index as usize).copied().map(Value::raw))
    }

    fn load_mapping_item(
        &mut self,
        owner: u64,
        symbol: u32,
        key_count: u32,
        _registers: &[u64],
    ) -> std::result::Result<Option<u64>, tonic_jit::RuntimeFailure> {
        self.stats.jit_helper_calls += 1;
        let name = self.symbols.get(symbol as usize).ok_or_else(|| {
            tonic_jit::RuntimeFailure::new("JitError", "mapping symbol index is out of bounds")
        })?;
        let Object::Dict(dict) = self
            .heap
            .get(Value::from_jit(owner))
            .map_err(runtime_failure)?
        else {
            return Ok(None);
        };
        if dict.entries.len() != key_count as usize {
            return Ok(None);
        }
        for (key, value) in &dict.entries {
            if matches!(self.heap.get(*key), Ok(Object::Str(key)) if key == name) {
                return Ok(Some(value.raw()));
            }
        }
        Ok(None)
    }

    fn build_tuple(
        &mut self,
        first: u32,
        count: u32,
        registers: &[u64],
    ) -> std::result::Result<u64, tonic_jit::RuntimeFailure> {
        self.safepoint(registers)?;
        let first = first as usize;
        let end = first.checked_add(count as usize).ok_or_else(|| {
            tonic_jit::RuntimeFailure::new("JitError", "variadic tuple range overflow")
        })?;
        let values = registers.get(first..end).ok_or_else(|| {
            tonic_jit::RuntimeFailure::new("JitError", "variadic tuple range is out of bounds")
        })?;
        self.heap
            .alloc(Object::Tuple(
                values.iter().copied().map(Value::from_jit).collect(),
            ))
            .map(Value::raw)
            .map_err(runtime_failure)
    }

    fn build_dict(
        &mut self,
        registers: &[u64],
    ) -> std::result::Result<u64, tonic_jit::RuntimeFailure> {
        self.safepoint(registers)?;
        self.heap
            .alloc(Object::Dict(Default::default()))
            .map(Value::raw)
            .map_err(runtime_failure)
    }

    fn dict_set_symbol(
        &mut self,
        owner: u64,
        symbol: u32,
        value_register: u32,
        registers: &[u64],
    ) -> std::result::Result<u64, tonic_jit::RuntimeFailure> {
        self.safepoint(registers)?;
        let name = self.symbols.get(symbol as usize).ok_or_else(|| {
            tonic_jit::RuntimeFailure::new("JitError", "keyword symbol index is out of bounds")
        })?;
        let value = registers
            .get(value_register as usize)
            .copied()
            .ok_or_else(|| {
                tonic_jit::RuntimeFailure::new(
                    "JitError",
                    "keyword value register is out of bounds",
                )
            })?;
        let owner = Value::from_jit(owner);
        let key = self
            .heap
            .alloc(Object::Str(name.clone()))
            .map_err(runtime_failure)?;
        self.heap
            .dict_set(owner, key, Value::from_jit(value))
            .map_err(runtime_failure)?;
        Ok(owner.raw())
    }

    fn unbox_float(
        &mut self,
        value: u64,
        _registers: &[u64],
    ) -> std::result::Result<Option<u64>, tonic_jit::RuntimeFailure> {
        self.stats.jit_helper_calls += 1;
        match self.heap.get(Value::from_jit(value)) {
            Ok(Object::Float(value)) => Ok(Some(value.to_bits())),
            Ok(_) => Ok(None),
            Err(_) => Ok(None),
        }
    }

    fn box_float(
        &mut self,
        bits: u64,
        registers: &[u64],
    ) -> std::result::Result<u64, tonic_jit::RuntimeFailure> {
        self.safepoint(registers)?;
        self.heap
            .alloc(Object::Float(f64::from_bits(bits)))
            .map(Value::raw)
            .map_err(runtime_failure)
    }

    fn poll(&mut self, registers: &[u64]) -> std::result::Result<u64, tonic_jit::RuntimeFailure> {
        self.stats.jit_backedge_polls += 1;
        self.safepoint(registers)?;
        Ok(0)
    }
}

fn runtime_failure(error: Diagnostic) -> tonic_jit::RuntimeFailure {
    tonic_jit::RuntimeFailure::new(error.kind, error.message)
}

fn record_collection(
    stats: &mut Stats,
    heap: &Heap,
    collection: crate::CollectionStats,
    started: std::time::Instant,
) {
    stats.gc_collections += 1;
    if collection.major {
        stats.gc_major_collections += 1;
    } else {
        stats.gc_minor_collections += 1;
    }
    stats.gc_reclaimed += collection.reclaimed as u64;
    stats.gc_promoted += collection.promoted as u64;
    stats.gc_moved += collection.moved as u64;
    stats.live_objects = collection.survivors;
    stats.estimated_heap_bytes = collection.live_bytes;
    stats.peak_heap_bytes = heap.peak_bytes;
    let pause = started.elapsed().as_nanos();
    stats.gc_pause_ns += pause;
    stats.gc_max_pause_ns = stats.gc_max_pause_ns.max(pause);
}
const JIT_DEOPT_LIMIT: u8 = 8;
const DEFAULT_JIT_THRESHOLD: u32 = 8;
const DEFAULT_JIT_MIN_INSTRUCTIONS: usize = 7;
const DEFAULT_JIT_OSR_THRESHOLD: u32 = 64;
const QUICKEN_THRESHOLD: u8 = 8;
const MINORS_PER_MAJOR: u8 = 32;
#[derive(Clone, Copy, Default)]
enum AdaptiveState {
    #[default]
    Generic,
    ObservedInt(u8),
    IntBinary,
    ObservedCall {
        callee: Value,
        code: u16,
        count: u8,
    },
    TonicCall {
        callee: Value,
        code: u16,
    },
    TonicCallPic(u32),
    ObservedAttr {
        class: Value,
        shape: ShapeId,
        slot: u16,
        epoch: u64,
        count: u8,
    },
    AttrSlot {
        class: Value,
        shape: ShapeId,
        slot: u16,
        epoch: u64,
    },
    AttrSlotPic(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CallCacheEntry {
    callee: Value,
    code: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CallPic {
    first: CallCacheEntry,
    second: CallCacheEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AttrCacheEntry {
    class: Value,
    shape: ShapeId,
    slot: u16,
    epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AttrPic {
    first: AttrCacheEntry,
    second: AttrCacheEntry,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MethodProfile {
    function: Value,
    kind: DirectMethodKind,
    count: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SequenceProfile {
    length: u16,
    count: u8,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct MappingProfile {
    keys: Vec<tonic_core::ast::SymbolId>,
    count: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExpandedCallProfile {
    callee: Value,
    code: u16,
    count: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FloatCallProfile {
    callee: Value,
    count: u8,
}
enum ReturnAction {
    Value,
    Length,
    Truth {
        protocol: TruthProtocol,
        action: TruthAction,
    },
    Initializer(Value),
    New {
        class: Value,
        arguments: ExpandedArgs,
    },
    Class(Value),
    SetNames {
        class: Value,
        pending: Vec<SetNameCall>,
    },
    Setter,
}
#[derive(Clone, Copy)]
enum TruthProtocol {
    Bool,
    Length,
}
#[derive(Clone, Copy)]
enum TruthAction {
    Not,
    Jump {
        when: bool,
        target: usize,
        pc: usize,
        original: Value,
    },
}
struct SetNameCall {
    call: DescriptorCall,
    name: Value,
}
impl ReturnAction {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        match self {
            Self::Value | Self::Length | Self::Setter => {}
            Self::Truth {
                action: TruthAction::Jump { original, .. },
                ..
            } => visit(*original),
            Self::Truth {
                action: TruthAction::Not,
                ..
            } => {}
            Self::Initializer(v) | Self::Class(v) => visit(*v),
            Self::New { class, arguments } => {
                visit(*class);
                arguments.trace(visit);
            }
            Self::SetNames { class, pending } => {
                visit(*class);
                for item in pending {
                    visit(item.call.callable);
                    if let Some(receiver) = item.call.receiver {
                        visit(receiver);
                    }
                    visit(item.name);
                }
            }
        }
    }
    fn root_count(&self) -> usize {
        let mut count = 0;
        self.trace(|_| count += 1);
        count
    }
}
/// Single-threaded interpreter instance. Handles may only be used with this VM.
/// Repeated run calls start fresh module globals; persistent native roots survive.
/// Collection occurs between instructions; native Context scopes cannot collect.
pub struct Vm {
    object_class: Value,
    pub(crate) execution: u64,
    phase: RuntimePhase,
    attached_thread: Option<ThreadId>,
    active_program: Option<Arc<Program>>,
    pub(crate) runtime_owner: Arc<RuntimeOwner>,
    pub(crate) heap: Heap,
    pub(crate) handles: HandleTable,
    natives: Vec<NativeDef>,
    modules: HashMap<String, Value>,
    builtins: Vec<(String, Value)>,
    globals: Vec<Value>,
    constants: Vec<Vec<Value>>,
    registers: Vec<Value>,
    cells: Vec<Value>,
    arguments: Vec<ExpandedArgs>,
    frames: Vec<Frame>,
    pub limits: Limits,
    pub stats: Stats,
    /// Allocation interval for scheduled minor/major collection; None disables automatic GC.
    pub gc_interval: Option<u64>,
    minor_collections: u8,
    pub execution_mode: ExecutionMode,
    /// Enables interpreter opcode specialization. Disable only for measurement.
    pub adaptive_specialization: bool,
    /// Function-entry count required before compiling a non-loop leaf function.
    pub jit_threshold: u32,
    /// Straight-line functions below this size stay in the adaptive interpreter.
    pub jit_min_instructions: usize,
    /// Interpreted backedges required before entering native code mid-loop.
    pub jit_osr_threshold: u32,
    /// Enables profile-backed exact-callee leaf inlining. Disable only for A/B measurement.
    pub jit_direct_call_inlining: bool,
    jit_cache: Vec<JitEntry>,
    jit_hotness: Vec<u32>,
    jit_registers: Vec<u64>,
    jit_globals: Vec<u64>,
    jit_roots: Vec<Value>,
    adaptive_sites: Vec<Vec<AdaptiveState>>,
    call_pics: Vec<CallPic>,
    attr_pics: Vec<AttrPic>,
    method_profiles: Vec<Vec<Option<MethodProfile>>>,
    sequence_profiles: Vec<Vec<Option<SequenceProfile>>>,
    mapping_profiles: Vec<Vec<Option<MappingProfile>>>,
    expanded_call_profiles: Vec<Vec<Option<ExpandedCallProfile>>>,
    float_call_profiles: Vec<Vec<Option<FloatCallProfile>>>,
}
impl Vm {
    pub fn new() -> Result<Self> {
        let mut vm = Self {
            object_class: Value::UNBOUND,
            execution: 0,
            phase: RuntimePhase::Running,
            attached_thread: Some(std::thread::current().id()),
            active_program: None,
            runtime_owner: Arc::new(RuntimeOwner::new()?),
            heap: Heap::default(),
            handles: HandleTable::default(),
            natives: Vec::new(),
            modules: HashMap::new(),
            builtins: Vec::new(),
            globals: Vec::new(),
            constants: Vec::new(),
            registers: Vec::new(),
            cells: Vec::new(),
            arguments: Vec::new(),
            frames: Vec::new(),
            limits: Limits::default(),
            stats: Stats::default(),
            gc_interval: Some(1024),
            minor_collections: 0,
            execution_mode: ExecutionMode::Interpreter,
            adaptive_specialization: true,
            jit_threshold: DEFAULT_JIT_THRESHOLD,
            jit_min_instructions: DEFAULT_JIT_MIN_INSTRUCTIONS,
            jit_osr_threshold: DEFAULT_JIT_OSR_THRESHOLD,
            jit_direct_call_inlining: true,
            jit_cache: Vec::new(),
            jit_hotness: Vec::new(),
            jit_registers: Vec::new(),
            jit_globals: Vec::new(),
            jit_roots: Vec::new(),
            adaptive_sites: Vec::new(),
            call_pics: Vec::new(),
            attr_pics: Vec::new(),
            method_profiles: Vec::new(),
            sequence_profiles: Vec::new(),
            mapping_profiles: Vec::new(),
            expanded_call_profiles: Vec::new(),
            float_call_profiles: Vec::new(),
        };
        for (name, builtin) in [
            ("print", Builtin::Print),
            ("range", Builtin::Range),
            ("len", Builtin::Len),
            ("abs", Builtin::Abs),
            ("isinstance", Builtin::IsInstance),
            ("issubclass", Builtin::IsSubclass),
            ("getattr", Builtin::GetAttr),
            ("setattr", Builtin::SetAttr),
            ("hasattr", Builtin::HasAttr),
            ("staticmethod", Builtin::StaticMethod),
            ("classmethod", Builtin::ClassMethod),
            ("property", Builtin::Property),
            ("super", Builtin::Super),
        ] {
            let v = vm.heap.alloc(Object::Builtin(builtin))?;
            vm.builtins.push((name.into(), v));
        }
        vm.register_native("fastmath", "add", 2, fastmath_add)?;
        vm.register_native("fastmath", "array", 1, fastmath_array)?;
        vm.register_native("fastmath", "sum", 1, fastmath_sum)?;
        let object_new = vm.heap.alloc(Object::Builtin(Builtin::ObjectNew))?;
        vm.object_class = vm.heap.root_object_class(object_new)?;
        vm.builtins.push(("object".into(), vm.object_class));
        Ok(vm)
    }
    fn ensure_attached(&self) -> Result<()> {
        match self.attached_thread {
            Some(thread) if thread == std::thread::current().id() => Ok(()),
            Some(_) => Err(Diagnostic::new(
                "ThreadError",
                "runtime is attached to another thread",
            )),
            None => Err(Diagnostic::new(
                "ThreadError",
                "current thread is not attached to the runtime",
            )),
        }
    }
    fn ensure_running(&self) -> Result<()> {
        if self.phase == RuntimePhase::Running {
            self.ensure_attached()
        } else {
            Err(Diagnostic::new(
                "RuntimeError",
                format!("runtime is not running ({:?})", self.phase),
            ))
        }
    }
    pub fn phase(&self) -> RuntimePhase {
        self.phase
    }
    pub fn attach_current_thread(&mut self) -> Result<()> {
        if self.phase != RuntimePhase::Running {
            return Err(Diagnostic::new(
                "RuntimeError",
                "cannot attach to a runtime that is shutting down",
            ));
        }
        if !self.frames.is_empty() {
            return Err(Diagnostic::new(
                "RuntimeError",
                "cannot attach while guest code is active",
            ));
        }
        let current = std::thread::current().id();
        match self.attached_thread {
            None => {
                self.attached_thread = Some(current);
                Ok(())
            }
            Some(thread) if thread == current => Ok(()),
            Some(_) => Err(Diagnostic::new(
                "ThreadError",
                "runtime is attached to another thread",
            )),
        }
    }
    pub fn detach_current_thread(&mut self) -> Result<()> {
        self.ensure_running()?;
        if !self.frames.is_empty() {
            return Err(Diagnostic::new(
                "RuntimeError",
                "cannot detach while guest code is active",
            ));
        }
        self.attached_thread = None;
        Ok(())
    }
    pub fn context(&mut self) -> Result<Context<'_>> {
        self.ensure_running()?;
        self.drain_deferred_persistent_releases()?;
        Ok(Context::new(self))
    }

    pub(crate) fn drain_deferred_persistent_releases(&mut self) -> Result<()> {
        for raw in self.runtime_owner.take_persistent_releases() {
            let handle = PersistentHandle::from_raw(raw);
            self.handles.release_persistent(&handle).map_err(|error| {
                Diagnostic::new(
                    error.kind,
                    format!("invalid deferred persistent release: {}", error.message),
                )
            })?;
        }
        Ok(())
    }
    pub fn call_persistent(
        &mut self,
        callable: &PersistentHandle,
        arguments: &[&PersistentHandle],
        output: &mut dyn Write,
    ) -> Result<PersistentHandle> {
        self.ensure_running()?;
        if !self.frames.is_empty() {
            return Err(Diagnostic::new(
                "RuntimeError",
                "nested callback entry is not available while guest code is active",
            ));
        }
        let program = self.active_program.clone().ok_or_else(|| {
            Diagnostic::new("RuntimeError", "runtime has no active module for callbacks")
        })?;
        let callable = self.handles.resolve_persistent(callable)?;
        let values = arguments
            .iter()
            .map(|argument| self.handles.resolve_persistent(argument))
            .collect::<Result<Vec<_>>>()?;
        if values.len() > self.limits.arguments {
            return Err(Diagnostic::new(
                "ResourceError",
                "too many callback arguments",
            ));
        }
        let count = values.len();
        self.registers.clear();
        self.cells.clear();
        self.arguments.clear();
        self.frames.clear();
        self.registers.push(Value::UNBOUND);
        let first = self.registers.len();
        self.registers.extend(values);
        self.stats.callback_calls += 1;
        let result = self
            .invoke_target(
                &program,
                callable,
                0,
                Arguments::Direct {
                    receiver: None,
                    first,
                    count,
                    keywords: &[],
                },
                output,
            )
            .and_then(|_| self.execute(&program, output))
            .and_then(|_| self.read(0));
        let result = result.map_err(|mut error| {
            for frame in self.frames.iter().rev() {
                let code = &program.code[frame.code];
                let pc = frame.ip.saturating_sub(1);
                let span = code.spans[pc];
                if error.span.is_none() {
                    error.span = Some(span);
                }
                error.trace.push((code.name.clone(), span));
            }
            error
        });
        self.frames.clear();
        self.registers.clear();
        self.cells.clear();
        self.arguments.clear();
        self.handles.persist_value(result?)
    }
    pub(crate) fn reenter_from_native(
        &mut self,
        program: &Program,
        callable: Value,
        arguments: &[Value],
        output: &mut dyn Write,
    ) -> Result<Value> {
        if arguments.len() > self.limits.arguments {
            return Err(Diagnostic::new(
                "ResourceError",
                "too many callback arguments",
            ));
        }
        let register_len = self.registers.len();
        let cell_len = self.cells.len();
        let argument_depth = self.arguments.len();
        let frame_depth = self.frames.len();
        let required = 1usize
            .checked_add(arguments.len())
            .and_then(|count| register_len.checked_add(count))
            .ok_or_else(|| Diagnostic::new("ResourceError", "callback register overflow"))?;
        if required > self.limits.registers {
            return Err(Diagnostic::new(
                "ResourceError",
                "callback register limit exceeded",
            ));
        }
        let destination = register_len;
        self.registers.push(Value::UNBOUND);
        let first = self.registers.len();
        self.registers.extend_from_slice(arguments);
        self.stats.callback_calls += 1;
        let result = self
            .invoke_target(
                program,
                callable,
                destination,
                Arguments::Direct {
                    receiver: None,
                    first,
                    count: arguments.len(),
                    keywords: &[],
                },
                output,
            )
            .and_then(|_| self.execute_until_depth(program, output, frame_depth))
            .and_then(|_| self.read(destination));
        self.frames.truncate(frame_depth);
        self.registers.truncate(register_len);
        self.cells.truncate(cell_len);
        self.arguments.truncate(argument_depth);
        result
    }
    pub(crate) fn reenter_from_native_expanded(
        &mut self,
        program: &Program,
        callable: Value,
        arguments: ExpandedArgs,
        output: &mut dyn Write,
    ) -> Result<Value> {
        if arguments.count() > self.limits.arguments {
            return Err(Diagnostic::new(
                "ResourceError",
                "too many callback arguments",
            ));
        }
        let register_len = self.registers.len();
        let cell_len = self.cells.len();
        let argument_depth = self.arguments.len();
        let frame_depth = self.frames.len();
        let required = register_len
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("ResourceError", "callback register overflow"))?;
        if required > self.limits.registers {
            return Err(Diagnostic::new(
                "ResourceError",
                "callback register limit exceeded",
            ));
        }
        let destination = register_len;
        self.registers.push(Value::UNBOUND);
        self.stats.callback_calls += 1;
        let result = self
            .invoke_target(
                program,
                callable,
                destination,
                Arguments::Expanded(arguments),
                output,
            )
            .and_then(|_| self.execute_until_depth(program, output, frame_depth))
            .and_then(|_| self.read(destination));
        self.frames.truncate(frame_depth);
        self.registers.truncate(register_len);
        self.cells.truncate(cell_len);
        self.arguments.truncate(argument_depth);
        result
    }
    pub fn begin_shutdown(&mut self) -> Result<()> {
        self.ensure_running()?;
        if !self.frames.is_empty() {
            return Err(Diagnostic::new(
                "RuntimeError",
                "cannot begin shutdown while guest code is active",
            ));
        }
        self.drain_deferred_persistent_releases()?;
        self.phase = RuntimePhase::ShuttingDown;
        Ok(())
    }
    pub fn finalize_shutdown(&mut self) -> Result<crate::CollectionStats> {
        self.ensure_attached()?;
        if self.phase != RuntimePhase::ShuttingDown {
            return Err(Diagnostic::new(
                "RuntimeError",
                "runtime must be shutting down before finalization",
            ));
        }
        self.phase = RuntimePhase::Finalizing;
        self.frames.clear();
        self.registers.clear();
        self.cells.clear();
        self.arguments.clear();
        self.globals.clear();
        self.constants.clear();
        self.jit_globals.clear();
        self.jit_registers.clear();
        self.jit_roots.clear();
        self.jit_cache.clear();
        self.active_program = None;
        self.modules.clear();
        self.builtins.clear();
        self.natives.clear();
        let collection = self.heap.collect(Vec::new())?;
        let (destructors, panics) = self.heap.drain_foreign_finalizers(&mut self.handles);
        self.stats.foreign_destructor_calls += destructors;
        self.stats.foreign_destructor_panics += panics;
        self.drain_deferred_persistent_releases()?;
        self.handles.invalidate_all();
        self.stats.live_objects = collection.survivors;
        self.stats.estimated_heap_bytes = collection.live_bytes;
        Ok(collection)
    }
    pub fn complete_shutdown(&mut self) -> Result<()> {
        self.ensure_attached()?;
        if self.phase != RuntimePhase::Finalizing {
            return Err(Diagnostic::new(
                "RuntimeError",
                "runtime must finish finalization before becoming dead",
            ));
        }
        self.runtime_owner.mark_dead();
        self.phase = RuntimePhase::Dead;
        self.attached_thread = None;
        Ok(())
    }
    pub fn shutdown(&mut self) -> Result<crate::CollectionStats> {
        self.begin_shutdown()?;
        let collection = self.finalize_shutdown()?;
        self.complete_shutdown()?;
        Ok(collection)
    }
    pub fn active_handles(&self) -> usize {
        self.handles.active()
    }
    /// Allocated heap entries; unreachable entries may await the next collection.
    pub fn live_objects(&self) -> usize {
        self.heap.live_objects()
    }
    pub fn shape_count(&self) -> usize {
        self.heap.shapes.len()
    }
    fn append_gc_roots(
        &self,
        excluded_registers: Option<std::ops::Range<usize>>,
        roots: &mut Vec<Value>,
    ) {
        // Exact Value roots, not conservative scanning of the host stack.
        // All initialized VM slots are retained; CFG liveness trimming is future work.
        if let Some(excluded) = excluded_registers {
            roots.extend_from_slice(&self.registers[..excluded.start]);
            roots.extend_from_slice(&self.registers[excluded.end..]);
        } else {
            roots.extend_from_slice(&self.registers);
        }
        roots.extend_from_slice(&self.cells);
        roots.extend_from_slice(&self.globals);
        for constants in &self.constants {
            roots.extend_from_slice(constants);
        }
        roots.extend(self.modules.values().copied());
        roots.extend(self.builtins.iter().map(|(_, v)| *v));
        roots.extend(self.frames.iter().filter_map(|frame| frame.callable));
        roots.extend(self.frames.iter().filter_map(|frame| frame.namespace));
        for frame in &self.frames {
            frame.action.trace(|value| roots.push(value));
        }
        for args in &self.arguments {
            args.trace(|value| roots.push(value));
        }
        self.handles.roots(|value| roots.push(value));
    }
    pub fn collect_garbage(&mut self) -> Result<crate::CollectionStats> {
        self.ensure_running()?;
        self.drain_deferred_persistent_releases()?;
        let start = std::time::Instant::now();
        let mut roots = Vec::new();
        self.append_gc_roots(None, &mut roots);
        self.stats.foreign_trace_calls +=
            self.heap.refresh_foreign_references(&mut self.handles)?;
        let stats = self.heap.collect(roots)?;
        let (destructors, panics) = self.heap.drain_foreign_finalizers(&mut self.handles);
        self.stats.foreign_destructor_calls += destructors;
        self.stats.foreign_destructor_panics += panics;
        self.drain_deferred_persistent_releases()?;
        self.minor_collections = 0;
        record_collection(&mut self.stats, &self.heap, stats, start);
        Ok(stats)
    }
    fn collect_automatic(&mut self) -> Result<crate::CollectionStats> {
        let start = std::time::Instant::now();
        let mut roots = Vec::new();
        self.append_gc_roots(None, &mut roots);
        self.stats.foreign_trace_calls +=
            self.heap.refresh_foreign_references(&mut self.handles)?;
        let stats = if self.minor_collections >= MINORS_PER_MAJOR - 1 {
            self.minor_collections = 0;
            self.heap.collect(roots)?
        } else {
            self.minor_collections += 1;
            self.heap.collect_young(roots)?
        };
        let (destructors, panics) = self.heap.drain_foreign_finalizers(&mut self.handles);
        self.stats.foreign_destructor_calls += destructors;
        self.stats.foreign_destructor_panics += panics;
        self.drain_deferred_persistent_releases()?;
        record_collection(&mut self.stats, &self.heap, stats, start);
        Ok(stats)
    }
    pub fn register_native(
        &mut self,
        module: &str,
        name: &str,
        arity: usize,
        function: NativeFn,
    ) -> Result<()> {
        self.ensure_running()?;
        let value = if let Some(v) = self.modules.get(module) {
            *v
        } else {
            let v = self.heap.alloc(Object::Module(Vec::new()))?;
            self.modules.insert(module.into(), v);
            v
        };
        if let Object::Module(m) = self.heap.get(value)? {
            if m.iter().any(|(n, _)| n == name) {
                return Err(Diagnostic::new(
                    "ImportError",
                    "native function already registered",
                ));
            }
        }
        let id = self.natives.len();
        self.natives.push(NativeDef {
            arity,
            function: NativeCallable::Rust(function),
        });
        let callable = self.heap.alloc(Object::Native(id))?;
        self.heap.add_module_member(value, name, callable)?;
        Ok(())
    }
    pub fn register_c_native(
        &mut self,
        module: &str,
        name: &str,
        arity: usize,
        function: crate::c_api::CNativeFn,
        abi_version: u32,
        required_capabilities: u64,
    ) -> Result<()> {
        self.ensure_running()?;
        crate::c_api::negotiate_api(abi_version, 0, required_capabilities)?;
        let value = if let Some(value) = self.modules.get(module) {
            *value
        } else {
            let value = self.heap.alloc(Object::Module(Vec::new()))?;
            self.modules.insert(module.into(), value);
            value
        };
        if let Object::Module(members) = self.heap.get(value)? {
            if members.iter().any(|(member, _)| member == name) {
                return Err(Diagnostic::new(
                    "ImportError",
                    "native function already registered",
                ));
            }
        }
        let id = self.natives.len();
        self.natives.push(NativeDef {
            arity,
            function: NativeCallable::C(function),
        });
        let callable = self.heap.alloc(Object::Native(id))?;
        self.heap.add_module_member(value, name, callable)?;
        Ok(())
    }
    pub fn initialize_c_extension(
        &mut self,
        init: crate::c_api::CExtensionInitFn,
        abi_version: u32,
        minimum_struct_size: u32,
        required_capabilities: u64,
    ) -> Result<()> {
        let api =
            crate::c_api::negotiate_api(abi_version, minimum_struct_size, required_capabilities)?;
        let mut context = self.context()?;
        crate::c_api::invoke_extension_init(&mut context, init, api)
    }
    /// Diagnostic hook enumerating explicit root slots and all stored managed edges.
    /// It does not collect or assert reachability; garbage may await a safepoint.
    pub fn root_and_edge_counts(&self) -> (usize, usize) {
        let mut roots = self.registers.len()
            + self.cells.len()
            + self.globals.len()
            + self.constants.iter().map(Vec::len).sum::<usize>()
            + self.modules.len()
            + self.builtins.len()
            + self
                .frames
                .iter()
                .filter(|frame| frame.callable.is_some())
                .count();
        roots += self.frames.iter().filter(|f| f.namespace.is_some()).count()
            + self
                .frames
                .iter()
                .map(|f| f.action.root_count())
                .sum::<usize>();
        for args in &self.arguments {
            roots += args.count()
                + usize::from(args.receiver.is_some())
                + args.invalid_keywords.len()
                + usize::from(args.deferred_star.is_some());
        }
        self.handles.roots(|_| roots += 1);
        let mut edges = 0;
        self.heap.trace_all(|_| edges += 1);
        (roots, edges)
    }
    pub fn run(&mut self, verified: &VerifiedProgram, output: &mut dyn Write) -> Result<()> {
        self.ensure_running()?;
        self.drain_deferred_persistent_releases()?;
        if !self.frames.is_empty() {
            return Err(Diagnostic::new(
                "RuntimeError",
                "nested Vm::run is not allowed; use a persistent callback",
            ));
        }
        self.execution = self
            .execution
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("RuntimeError", "execution identity space exhausted"))?;
        let program = verified.program();
        self.active_program = Some(Arc::new(program.clone()));
        self.stats = Stats::default();
        self.registers.clear();
        self.cells.clear();
        self.arguments.clear();
        self.frames.clear();
        self.constants.clear();
        self.globals.clear();
        self.jit_cache = (0..program.code.len()).map(|_| JitEntry::Untried).collect();
        self.jit_hotness = vec![0; program.code.len()];
        self.jit_registers.clear();
        self.jit_globals.clear();
        self.jit_roots.clear();
        self.adaptive_sites = program
            .code
            .iter()
            .map(|code| vec![AdaptiveState::Generic; code.instructions.len()])
            .collect();
        self.call_pics.clear();
        self.attr_pics.clear();
        self.method_profiles = program
            .code
            .iter()
            .map(|code| vec![None; code.instructions.len()])
            .collect();
        self.sequence_profiles = program
            .code
            .iter()
            .map(|code| vec![None; code.instructions.len()])
            .collect();
        self.mapping_profiles = program
            .code
            .iter()
            .map(|code| vec![None; code.instructions.len()])
            .collect();
        self.expanded_call_profiles = program
            .code
            .iter()
            .map(|code| vec![None; code.instructions.len()])
            .collect();
        self.float_call_profiles = program
            .code
            .iter()
            .map(|code| vec![None; code.instructions.len()])
            .collect();
        let allocations = self.heap.allocations;
        let buffer_exports = self.heap.buffer_exports;
        let buffer_copies = self.heap.buffer_copies;
        let foreign_wrapper_creations = self.heap.foreign_wrapper_creations;
        let foreign_trace_calls = self.heap.foreign_trace_calls;
        let foreign_destructor_calls = self.heap.foreign_destructor_calls;
        let foreign_destructor_panics = self.heap.foreign_destructor_panics;
        for name in &program.symbols {
            self.globals.push(
                self.builtins
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| *v)
                    .unwrap_or(Value::UNBOUND),
            );
        }
        self.jit_globals
            .extend(self.globals.iter().copied().map(Value::raw));
        for code in &program.code {
            let mut constants = Vec::with_capacity(code.constants.len());
            for c in &code.constants {
                constants.push(match c {
                    Constant::None => Value::NONE,
                    Constant::Bool(b) => Value::bool(*b),
                    Constant::Int(s) => self.heap.int(s.parse::<BigInt>().map_err(|_| {
                        Diagnostic::new("BytecodeError", "invalid integer constant")
                    })?)?,
                    Constant::Float(n) => self.heap.alloc(Object::Float(*n))?,
                    Constant::Str(s) => self.heap.alloc(Object::Str(s.clone()))?,
                });
            }
            self.constants.push(constants);
        }
        let result = self
            .enter_frame(
                program,
                0,
                None,
                Arguments::Direct {
                    receiver: None,
                    first: 0,
                    count: 0,
                    keywords: &[],
                },
                None,
            )
            .and_then(|_| self.execute(program, output));
        self.stats.heap_allocations = self.heap.allocations - allocations;
        self.stats.buffer_exports = self.heap.buffer_exports - buffer_exports;
        self.stats.buffer_copies = self.heap.buffer_copies - buffer_copies;
        self.stats.foreign_wrapper_creations =
            self.heap.foreign_wrapper_creations - foreign_wrapper_creations;
        self.stats.foreign_trace_calls = self.heap.foreign_trace_calls - foreign_trace_calls;
        self.stats.foreign_destructor_calls =
            self.heap.foreign_destructor_calls - foreign_destructor_calls;
        self.stats.foreign_destructor_panics =
            self.heap.foreign_destructor_panics - foreign_destructor_panics;
        self.stats.estimated_heap_bytes = self.heap.bytes;
        self.stats.peak_heap_bytes = self.heap.peak_bytes;
        self.stats.live_objects = self.heap.live_objects();
        let result = result.map_err(|mut e| {
            for frame in self.frames.iter().rev() {
                let code = &program.code[frame.code];
                let pc = frame.ip.saturating_sub(1);
                let span = code.spans[pc];
                if e.span.is_none() {
                    e.span = Some(span);
                }
                e.trace.push((code.name.clone(), span));
            }
            e
        });
        // Error propagation must not retain stale frame/root state for later runs.
        self.frames.clear();
        self.registers.clear();
        self.cells.clear();
        self.arguments.clear();
        result
    }
    fn read(&self, index: usize) -> Result<Value> {
        let v = self.registers[index];
        if v == Value::UNBOUND {
            Err(Diagnostic::new(
                "UnboundLocalError",
                "local variable referenced before assignment",
            ))
        } else {
            Ok(v)
        }
    }
    fn execute(&mut self, p: &Program, output: &mut dyn Write) -> Result<()> {
        self.execute_until_depth(p, output, 0)
    }
    pub(crate) fn execute_until_depth(
        &mut self,
        p: &Program,
        output: &mut dyn Write,
        depth: usize,
    ) -> Result<()> {
        while self.frames.len() > depth {
            if self.gc_interval.is_some_and(|interval| {
                self.heap.allocations - self.heap.last_collection_allocations >= interval.max(1)
            }) {
                self.collect_automatic()?;
            }
            if self.execution_mode == ExecutionMode::Jit
                && self.frames.last().is_some_and(|frame| {
                    frame.jit_resume || (frame.ip == 0 && !frame.jit_attempted)
                })
                && self.try_jit(p, output)?
            {
                continue;
            }
            let frame = self.frames.last_mut().expect("active frame");
            if self
                .limits
                .instructions
                .is_some_and(|limit| self.stats.instructions >= limit)
            {
                return Err(Diagnostic::new(
                    "ResourceError",
                    "instruction budget exhausted",
                ));
            }
            let code_id = frame.code;
            let pc = frame.ip;
            let base = frame.base;
            let cell_base = frame.cell_base;
            frame.ip += 1;
            let code = &p.code[code_id];
            let i = code.instructions[pc];
            let op = Op::try_from(i.opcode)?;
            self.stats.instructions += 1;
            let a = base + i.a as usize;
            let b = base + i.b as usize;
            let c = base + i.c as usize;
            match op {
                Op::LoadCell => {
                    let value = self.heap.cell(self.cells[cell_base + i.b as usize])?;
                    if value == Value::UNBOUND {
                        return Err(Diagnostic::new(
                            if (i.b as usize) < code.cell_locals.len() {
                                "UnboundLocalError"
                            } else {
                                "NameError"
                            },
                            "captured variable referenced before assignment",
                        ));
                    }
                    self.registers[a] = value;
                }
                Op::StoreCell => self
                    .heap
                    .store_cell(self.cells[cell_base + i.b as usize], self.read(a)?)?,
                Op::Const => self.registers[a] = self.constants[code_id][i.b as usize],
                Op::Move => self.registers[a] = self.read(b)?,
                Op::LoadGlobal => {
                    let v = self.globals[i.b as usize];
                    if v == Value::UNBOUND {
                        return Err(Diagnostic::new(
                            "NameError",
                            format!("name '{}' is not defined", p.symbols[i.b as usize]),
                        ));
                    }
                    self.registers[a] = v;
                }
                Op::StoreGlobal => {
                    let value = self.read(a)?;
                    self.globals[i.b as usize] = value;
                    self.jit_globals[i.b as usize] = value.raw();
                }
                Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul => {
                    let left = self.read(b)?;
                    let right = self.read(c)?;
                    self.registers[a] = if self.adaptive_specialization {
                        self.adaptive_binary(code_id, pc, op, left, right)?
                    } else if op == Op::InplaceAdd {
                        self.heap.inplace_add(left, right)?
                    } else {
                        self.heap.binary(op, left, right)?
                    };
                }
                Op::FloorDiv | Op::Mod | Op::Div => {
                    self.registers[a] = self.heap.binary(op, self.read(b)?, self.read(c)?)?
                }
                Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                    self.registers[a] = self.heap.compare(op, self.read(b)?, self.read(c)?)?
                }
                Op::Neg | Op::Pos => self.registers[a] = self.heap.unary(op, self.read(b)?)?,
                Op::Not => {
                    let value = self.read(b)?;
                    self.invoke_truth(p, value, a, TruthAction::Not, output)?;
                }
                Op::Jump => self.jump(i.a as usize, pc),
                Op::JumpFalse | Op::JumpTrue => {
                    let value = self.read(a)?;
                    self.invoke_truth(
                        p,
                        value,
                        a,
                        TruthAction::Jump {
                            when: op == Op::JumpTrue,
                            target: i.b as usize,
                            pc,
                            original: value,
                        },
                        output,
                    )?;
                }
                Op::Function => {
                    let site = &code.functions[i.b as usize];
                    let captures = site
                        .captures
                        .iter()
                        .map(|c| self.cells[cell_base + *c as usize])
                        .collect();
                    self.registers[a] = self.heap.alloc(Object::Function {
                        code: site.code,
                        execution: self.execution,
                        captures,
                        defaults: site
                            .defaults
                            .iter()
                            .map(|r| self.read(base + *r as usize))
                            .collect::<Result<_>>()?,
                    })?
                }
                Op::Return => {
                    let mut value = self.read(a)?;
                    let frame = self.frames.pop().expect("active frame");
                    let mut set_names = None;
                    let mut finish_new = None;
                    let mut finish_truth = None;
                    match frame.action {
                        ReturnAction::Value => {}
                        ReturnAction::Length => value = self.validate_length(value)?,
                        ReturnAction::Truth { protocol, action } => {
                            finish_truth = Some((protocol, action));
                        }
                        ReturnAction::Initializer(instance) => {
                            if value != Value::NONE {
                                return Err(Diagnostic::new(
                                    "TypeError",
                                    "__init__ must return None",
                                ));
                            }
                            value = instance;
                        }
                        ReturnAction::New { class, arguments } => {
                            finish_new = Some((class, arguments));
                        }
                        ReturnAction::Class(namespace) => {
                            self.heap.finish_class(namespace)?;
                            let metadata = &p.code[frame.code];
                            if let Some((cell, _)) =
                                metadata.cell_locals.iter().enumerate().find(|(_, local)| {
                                    p.symbols[metadata.locals[**local as usize].0 as usize]
                                        == "__class__"
                                })
                            {
                                self.heap
                                    .store_cell(self.cells[frame.cell_base + cell], namespace)?;
                            }
                            value = namespace;
                            let mut pending = self
                                .heap
                                .descriptor_set_names(namespace)?
                                .into_iter()
                                .map(|(call, name)| SetNameCall { call, name })
                                .collect::<Vec<_>>();
                            pending.reverse();
                            set_names = Some((namespace, pending));
                        }
                        ReturnAction::SetNames { class, pending } => {
                            value = class;
                            set_names = Some((class, pending));
                        }
                        ReturnAction::Setter => value = Value::NONE,
                    }
                    self.registers.truncate(frame.base);
                    self.cells.truncate(frame.cell_base);
                    if let Some(dest) = frame.destination {
                        self.registers[dest] = value;
                        if let Some((protocol, action)) = finish_truth {
                            self.finish_truth(dest, value, protocol, action)?;
                        } else if let Some((class, arguments)) = finish_new {
                            self.finish_new(p, dest, class, value, arguments, output)?;
                        } else if let Some((class, pending)) = set_names {
                            self.invoke_set_names(p, dest, class, pending, output)?;
                        }
                    } else if set_names.is_some() || finish_new.is_some() || finish_truth.is_some()
                    {
                        return Err(Diagnostic::new(
                            "BytecodeError",
                            "continuation has no destination",
                        ));
                    }
                }
                Op::Call => self.execute_call(p, code_id, pc, output)?,
                Op::Class => {
                    let function = self.read(b)?;
                    let (body, execution) = match self.heap.get(function)? {
                        Object::Function {
                            code, execution, ..
                        } => (*code as usize, *execution),
                        _ => {
                            return Err(Diagnostic::new(
                                "BytecodeError",
                                "expected class body function",
                            ))
                        }
                    };
                    if execution != self.execution || !p.code[body].class_body {
                        return Err(Diagnostic::new("BytecodeError", "invalid class body"));
                    }
                    let site = &code.calls[i.c as usize];
                    let mut bases = Vec::new();
                    for r in 0..site.count {
                        bases.push(self.read(base + (site.first + r) as usize)?);
                    }
                    if bases.is_empty() {
                        bases.push(self.object_class);
                    }
                    let namespace = self.heap.namespace(&p.code[body].name, bases)?;
                    self.enter_frame(
                        p,
                        body,
                        Some(a),
                        Arguments::Direct {
                            first: 0,
                            count: 0,
                            keywords: &[],
                            receiver: None,
                        },
                        Some(function),
                    )?;
                    let frame = self.frames.last_mut().expect("class frame");
                    frame.namespace = Some(namespace);
                    frame.action = ReturnAction::Class(namespace);
                }
                Op::LoadName | Op::ClassDeref => {
                    let namespace = frame.namespace.ok_or_else(|| {
                        Diagnostic::new("BytecodeError", "class frame has no namespace")
                    })?;
                    let name = &p.symbols[i.b as usize];
                    let value = if let Some(value) = self.heap.namespace_get(namespace, name)? {
                        value
                    } else if op == Op::ClassDeref {
                        self.heap.cell(self.cells[cell_base + i.c as usize])?
                    } else {
                        self.globals[i.b as usize]
                    };
                    if value == Value::UNBOUND {
                        return Err(Diagnostic::new(
                            "NameError",
                            format!("name '{name}' is not defined"),
                        ));
                    }
                    self.registers[a] = value;
                }
                Op::StoreName => {
                    let namespace = frame.namespace.ok_or_else(|| {
                        Diagnostic::new("BytecodeError", "class frame has no namespace")
                    })?;
                    self.heap
                        .namespace_set(namespace, &p.symbols[i.b as usize], self.read(a)?)?;
                }
                Op::SetAttr => {
                    let owner = self.read(a)?;
                    let value = self.read(b)?;
                    if let Some(setter) =
                        self.heap.property_setter(owner, &p.symbols[i.c as usize])?
                    {
                        let depth = self.frames.len();
                        self.invoke(
                            p,
                            setter,
                            a,
                            Arguments::Direct {
                                receiver: Some(owner),
                                first: b,
                                count: 1,
                                keywords: &[],
                            },
                            output,
                        )?;
                        if self.frames.len() > depth {
                            self.frames
                                .last_mut()
                                .expect("property setter frame")
                                .action = ReturnAction::Setter;
                        } else {
                            self.registers[a] = Value::NONE;
                        }
                    } else if let Some(setter) = self
                        .heap
                        .descriptor_setter(owner, &p.symbols[i.c as usize])?
                    {
                        let depth = self.frames.len();
                        self.invoke(
                            p,
                            setter.callable,
                            a,
                            Arguments::Inline {
                                receiver: setter.receiver,
                                positional: [owner, value],
                                count: 2,
                            },
                            output,
                        )?;
                        if self.frames.len() > depth {
                            self.frames
                                .last_mut()
                                .expect("descriptor setter frame")
                                .action = ReturnAction::Setter;
                        } else {
                            self.registers[a] = Value::NONE;
                        }
                    } else {
                        self.heap.set_attr(owner, &p.symbols[i.c as usize], value)?;
                    }
                }
                Op::DelAttr => {
                    let owner = self.read(a)?;
                    let name = &p.symbols[i.b as usize];
                    if let Some(deleter) = self.heap.property_deleter(owner, name)? {
                        let depth = self.frames.len();
                        self.invoke(
                            p,
                            deleter,
                            a,
                            Arguments::Direct {
                                receiver: Some(owner),
                                first: 0,
                                count: 0,
                                keywords: &[],
                            },
                            output,
                        )?;
                        if self.frames.len() > depth {
                            self.frames
                                .last_mut()
                                .expect("property deleter frame")
                                .action = ReturnAction::Setter;
                        } else {
                            self.registers[a] = Value::NONE;
                        }
                    } else if let Some(deleter) = self.heap.descriptor_deleter(owner, name)? {
                        let depth = self.frames.len();
                        self.invoke(
                            p,
                            deleter.callable,
                            a,
                            Arguments::Inline {
                                receiver: deleter.receiver,
                                positional: [owner, Value::UNBOUND],
                                count: 1,
                            },
                            output,
                        )?;
                        if self.frames.len() > depth {
                            self.frames
                                .last_mut()
                                .expect("descriptor deleter frame")
                                .action = ReturnAction::Setter;
                        } else {
                            self.registers[a] = Value::NONE;
                        }
                    } else {
                        self.heap.del_attr(owner, name)?;
                    }
                }
                Op::BeginArgs => self.arguments.push(ExpandedArgs::default()),
                Op::ArgPos | Op::ArgStar | Op::ArgNamed | Op::ArgMapping => {
                    let value = self.read(a)?;
                    if self.adaptive_specialization && op == Op::ArgStar {
                        let length = match self.heap.get(value) {
                            Ok(Object::List(values) | Object::Tuple(values)) => {
                                u16::try_from(values.len()).ok()
                            }
                            _ => None,
                        };
                        self.observe_sequence(code_id, pc, length);
                    }
                    if self.adaptive_specialization && op == Op::ArgMapping {
                        self.observe_mapping(p, code_id, pc, value);
                    }
                    self.append_argument(p, op, value, i.b, i.c)?
                }
                Op::CallExpanded => {
                    let callee = self.read(b)?;
                    if self.adaptive_specialization {
                        self.observe_expanded_call(p, code_id, pc, callee);
                    }
                    if let Some(value) = self
                        .arguments
                        .last_mut()
                        .expect("verified argument stack")
                        .deferred_star
                        .take()
                    {
                        self.append_argument(p, Op::ArgStar, value, 0, 0)?;
                    }
                    let args = self.arguments.pop().expect("verified argument stack");
                    let resume = self
                        .frames
                        .last()
                        .and_then(|frame| frame.jit_expanded_resume_depth)
                        == Some(self.arguments.len());
                    if resume {
                        let frame = self.frames.last_mut().expect("expanded-call frame");
                        frame.jit_expanded_resume_depth = None;
                        frame.jit_resume = true;
                    }
                    self.invoke(p, callee, a, Arguments::Expanded(args), output)?;
                }
                Op::Dict => {
                    self.registers[a] = self.heap.alloc(Object::Dict(Default::default()))?
                }
                Op::SetItem => self
                    .heap
                    .set_item(self.read(a)?, self.read(b)?, self.read(c)?)?,
                Op::DictMerge => self.heap.dict_merge(self.read(a)?, self.read(b)?)?,
                Op::Tuple | Op::List => {
                    let end = b + i.c as usize;
                    for n in b..end {
                        self.read(n)?;
                    }
                    let values = self.registers[b..end].to_vec();
                    self.registers[a] = self.heap.alloc(if op == Op::Tuple {
                        Object::Tuple(values)
                    } else {
                        Object::List(values)
                    })?;
                }
                Op::Item => self.registers[a] = self.heap.item(self.read(b)?, self.read(c)?)?,
                Op::Slice => {
                    let values = [self.read(b)?, self.read(b + 1)?, self.read(b + 2)?];
                    self.registers[a] = self.heap.alloc(Object::Slice(values))?;
                }
                Op::Unpack => {
                    let source = self.read(b)?;
                    let count = i.c as usize;
                    // Generic iterable unpack validates length before assigning targets.
                    // Values stay in the caller register window; no temporary guest tuple.
                    let iterator = self.heap.iterator(source)?;
                    for n in 0..count {
                        let value = self.heap.next(iterator)?.ok_or_else(|| {
                            Diagnostic::new("ValueError", "not enough values to unpack")
                        })?;
                        self.registers[a + n] = value;
                    }
                    if self.heap.next(iterator)?.is_some() {
                        return Err(Diagnostic::new("ValueError", "too many values to unpack"));
                    }
                }
                Op::Iter => self.registers[a] = self.heap.iterator(self.read(b)?)?,
                Op::Next => {
                    if let Some(value) = self.heap.next(self.read(b)?)? {
                        self.registers[a] = value;
                    } else {
                        self.jump(i.c as usize, pc);
                    }
                }
                Op::Import => {
                    let name = &p.symbols[i.b as usize];
                    self.registers[a] = *self.modules.get(name).ok_or_else(|| {
                        Diagnostic::new(
                            "ModuleNotFoundError",
                            format!("no registered native module '{name}'"),
                        )
                    })?;
                }
                Op::Attr => {
                    let object = self.read(b)?;
                    let name = &p.symbols[i.c as usize];
                    let method_candidate = (self.adaptive_specialization
                        && self.jit_direct_call_inlining)
                        .then(|| self.heap.direct_method(object, name))
                        .flatten();
                    let mut missed_monomorphic = None;
                    if self.adaptive_specialization {
                        match self.adaptive_sites[code_id][pc] {
                            AdaptiveState::AttrSlot {
                                class,
                                shape,
                                slot,
                                epoch,
                            } => {
                                let cached = AttrCacheEntry {
                                    class,
                                    shape,
                                    slot,
                                    epoch,
                                };
                                if let Some(value) = self.cached_attr(object, cached) {
                                    self.registers[a] = value;
                                    continue;
                                }
                                self.stats.attr_cache_misses += 1;
                                missed_monomorphic = Some(cached);
                                self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
                            }
                            AdaptiveState::AttrSlotPic(index) => {
                                let pic = self.attr_pics[index as usize];
                                if let Some(value) = self
                                    .cached_attr(object, pic.first)
                                    .or_else(|| self.cached_attr(object, pic.second))
                                {
                                    self.registers[a] = value;
                                    continue;
                                }
                                self.stats.attr_cache_misses += 1;
                                self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
                            }
                            _ => {}
                        }
                    }
                    let mut cache = None;
                    if let Some(access) = self.heap.super_getter(object, name)? {
                        match access {
                            crate::classes::DescriptorAccess::Value(value) => {
                                self.registers[a] = value
                            }
                            crate::classes::DescriptorAccess::Call {
                                callable,
                                receiver,
                                positional,
                                count,
                            } => self.invoke(
                                p,
                                callable,
                                a,
                                Arguments::Inline {
                                    receiver,
                                    positional,
                                    count,
                                },
                                output,
                            )?,
                        }
                    } else if let Some(getter) = self.heap.property_getter(object, name)? {
                        self.invoke(
                            p,
                            getter,
                            a,
                            Arguments::Direct {
                                receiver: Some(object),
                                first: 0,
                                count: 0,
                                keywords: &[],
                            },
                            output,
                        )?;
                    } else if let Some(access) = self.heap.descriptor_getter(object, name)? {
                        match access {
                            crate::classes::DescriptorAccess::Value(value) => {
                                self.registers[a] = value
                            }
                            crate::classes::DescriptorAccess::Call {
                                callable,
                                receiver,
                                positional,
                                count,
                            } => self.invoke(
                                p,
                                callable,
                                a,
                                Arguments::Inline {
                                    receiver,
                                    positional,
                                    count,
                                },
                                output,
                            )?,
                        }
                    } else {
                        self.registers[a] = self.heap.attr(object, name)?;
                        cache = self.heap.instance_slot_cache(object, name);
                    }
                    if let Some((function, kind, _)) = method_candidate {
                        self.observe_method(code_id, pc, function, kind);
                    }
                    if self.adaptive_specialization {
                        if let Some((class, shape, slot, epoch)) = cache {
                            if let Ok(slot) = u16::try_from(slot) {
                                let current = AttrCacheEntry {
                                    class,
                                    shape,
                                    slot,
                                    epoch,
                                };
                                if let Some(previous) =
                                    missed_monomorphic.filter(|previous| *previous != current)
                                {
                                    self.install_attr_pic(code_id, pc, previous, current);
                                } else {
                                    self.observe_instance_attr(
                                        code_id, pc, class, shape, slot, epoch,
                                    );
                                }
                            }
                        } else {
                            self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn jump(&mut self, target: usize, pc: usize) {
        if target <= pc {
            self.stats.backedges += 1;
            let code = self.frames.last().expect("active frame").code;
            if self.execution_mode == ExecutionMode::Jit
                && self.limits.instructions.is_none()
                && code != 0
                && matches!(self.jit_cache.get(code), Some(JitEntry::Untried))
            {
                self.jit_hotness[code] = self.jit_hotness[code].saturating_add(1);
                if self.jit_hotness[code] >= self.jit_osr_threshold.max(1) {
                    self.frames.last_mut().expect("active frame").jit_resume = true;
                }
            }
        }
        self.frames.last_mut().expect("active frame").ip = target;
    }
    #[inline(always)]
    fn adaptive_binary(
        &mut self,
        code_id: usize,
        pc: usize,
        op: Op,
        left: Value,
        right: Value,
    ) -> Result<Value> {
        let fast = immediate_binary(op, left, right);
        if matches!(self.adaptive_sites[code_id][pc], AdaptiveState::IntBinary) {
            if let Some(result) = fast {
                return Ok(result);
            }
            self.stats.quickened_misses += 1;
            self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
        }
        let result = if op == Op::InplaceAdd {
            self.heap.inplace_add(left, right)?
        } else {
            self.heap.binary(op, left, right)?
        };
        if fast == Some(result) {
            let observations = match self.adaptive_sites[code_id][pc] {
                AdaptiveState::Generic => 1,
                AdaptiveState::ObservedInt(count) => count.saturating_add(1),
                AdaptiveState::IntBinary => unreachable!("handled above"),
                AdaptiveState::ObservedCall { .. }
                | AdaptiveState::TonicCall { .. }
                | AdaptiveState::TonicCallPic(_)
                | AdaptiveState::ObservedAttr { .. }
                | AdaptiveState::AttrSlot { .. }
                | AdaptiveState::AttrSlotPic(_) => unreachable!("non-binary adaptive state"),
            };
            if observations >= QUICKEN_THRESHOLD {
                self.adaptive_sites[code_id][pc] = AdaptiveState::IntBinary;
                self.stats.quickened += 1;
            } else {
                self.adaptive_sites[code_id][pc] = AdaptiveState::ObservedInt(observations);
            }
        } else {
            self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
        }
        Ok(result)
    }
    fn execute_call(
        &mut self,
        program: &Program,
        code_id: usize,
        pc: usize,
        output: &mut dyn Write,
    ) -> Result<()> {
        let frame = self.frames.last().expect("active call frame");
        let base = frame.base;
        let instruction = program.code[code_id].instructions[pc];
        let destination = base + instruction.a as usize;
        let callee = self.read(base + instruction.b as usize)?;
        let site = &program.code[code_id].calls[instruction.c as usize];
        let first = base + site.first as usize;
        let count = site.count as usize;
        for register in first..first + count + site.keywords.len() {
            self.read(register)?;
        }
        if self.execution_mode == ExecutionMode::Jit && self.adaptive_specialization {
            self.observe_float_call(code_id, pc, callee, first, count + site.keywords.len());
        }
        let mut missed_monomorphic = None;
        if self.adaptive_specialization {
            let target = match self.adaptive_sites[code_id][pc] {
                AdaptiveState::TonicCall {
                    callee: cached,
                    code: target,
                } if cached == callee => Some(target),
                AdaptiveState::TonicCall {
                    callee: cached,
                    code: target,
                } => {
                    self.stats.call_cache_misses += 1;
                    missed_monomorphic = Some(CallCacheEntry {
                        callee: cached,
                        code: target,
                    });
                    self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
                    None
                }
                AdaptiveState::TonicCallPic(index) => {
                    let pic = self.call_pics[index as usize];
                    if pic.first.callee == callee {
                        Some(pic.first.code)
                    } else if pic.second.callee == callee {
                        Some(pic.second.code)
                    } else {
                        self.stats.call_cache_misses += 1;
                        self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
                        None
                    }
                }
                _ => None,
            };
            if let Some(target) = target {
                self.stats.calls += 1;
                self.enter_simple_frame(
                    program,
                    target as usize,
                    destination,
                    first,
                    site,
                    callee,
                )?;
                return Ok(());
            }
        }
        let candidate = self
            .adaptive_specialization
            .then(|| self.simple_call_target(program, callee, site))
            .flatten();
        let depth = self.frames.len();
        self.invoke(
            program,
            callee,
            destination,
            Arguments::Direct {
                receiver: None,
                first,
                count,
                keywords: &site.keywords,
            },
            output,
        )?;
        if self.adaptive_specialization
            && self.frames.len() > depth
            && self.frames.last().is_some_and(|frame| frame.code != 0)
        {
            self.observe_expanded_call(program, code_id, pc, callee);
        }
        if let Some(target) = candidate {
            if self.frames.len() > depth
                && self.frames.last().is_some_and(|frame| frame.code == target)
            {
                let current = CallCacheEntry {
                    callee,
                    code: target as u16,
                };
                if let Some(previous) = missed_monomorphic.filter(|previous| *previous != current) {
                    self.install_call_pic(code_id, pc, previous, current);
                } else {
                    self.observe_simple_call(code_id, pc, callee, target as u16);
                }
            }
        } else if self.adaptive_specialization {
            self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
        }
        Ok(())
    }
    fn simple_call_target(
        &self,
        program: &Program,
        callee: Value,
        site: &tonic_core::bytecode::CallSite,
    ) -> Option<usize> {
        self.direct_call_target(program, callee, site, 0, false)
    }

    fn direct_call_target(
        &self,
        program: &Program,
        callee: Value,
        site: &tonic_core::bytecode::CallSite,
        positional_prefix: usize,
        materialize_variadics: bool,
    ) -> Option<usize> {
        let Object::Function {
            code,
            execution,
            captures,
            defaults,
        } = self.heap.get(callee).ok()?
        else {
            return None;
        };
        if *execution != self.execution || !captures.is_empty() {
            return None;
        }
        let code = *code as usize;
        let metadata = &program.code[code];
        let signature = &metadata.signature;
        let named = usize::from(signature.positional) + usize::from(signature.keyword_only);
        let positional_bound = positional_prefix
            + usize::from(site.count)
                .min(usize::from(signature.positional).saturating_sub(positional_prefix));
        if (!materialize_variadics && !tonic_jit::unobserved_variadic_parameters(metadata))
            || defaults.len() != signature.defaults.len()
            || positional_prefix + usize::from(site.count) > usize::from(signature.positional)
                && (!materialize_variadics || signature.vararg.is_none())
            || !metadata.cell_locals.is_empty()
            || !metadata.free_vars.is_empty()
            || metadata.class_body
        {
            return None;
        }
        for (index, keyword) in site.keywords.iter().copied().enumerate() {
            if let Some(slot) = (usize::from(signature.posonly)..named)
                .find(|slot| metadata.locals[*slot] == keyword)
            {
                if slot < positional_bound
                    || site.keywords[..index]
                        .iter()
                        .any(|previous| *previous == keyword)
                {
                    return None;
                }
            } else if !materialize_variadics || signature.kwarg.is_none() {
                return None;
            }
        }
        let bound = |slot: usize| {
            slot < positional_bound
                || slot >= usize::from(signature.posonly)
                    && site
                        .keywords
                        .iter()
                        .any(|keyword| metadata.locals[slot] == *keyword)
                || signature
                    .defaults
                    .iter()
                    .any(|default| usize::from(*default) == slot)
        };
        (0..named).all(bound).then_some(code)
    }

    fn jit_direct_arguments(
        &self,
        program: &Program,
        callee: Value,
        site: &tonic_core::bytecode::CallSite,
        target: usize,
        implicit_receiver: bool,
    ) -> Option<Vec<tonic_jit::DirectArgument>> {
        let Object::Function { defaults, .. } = self.heap.get(callee).ok()? else {
            return None;
        };
        let metadata = &program.code[target];
        let signature = &metadata.signature;
        let named = usize::from(signature.positional) + usize::from(signature.keyword_only);
        let prefix = usize::from(implicit_receiver);
        let positional_bound = prefix
            + usize::from(site.count).min(usize::from(signature.positional).saturating_sub(prefix));
        let named_arguments: Vec<_> = (0..named)
            .map(|slot| {
                if slot == 0 && implicit_receiver {
                    return Some(tonic_jit::DirectArgument::MethodReceiver);
                }
                if slot >= prefix && slot < positional_bound {
                    return u16::try_from(slot - prefix)
                        .ok()
                        .map(tonic_jit::DirectArgument::Caller);
                }
                if let Some(keyword) = (slot >= usize::from(signature.posonly))
                    .then(|| {
                        site.keywords
                            .iter()
                            .position(|keyword| metadata.locals[slot] == *keyword)
                    })
                    .flatten()
                {
                    return site
                        .count
                        .checked_add(u16::try_from(keyword).ok()?)
                        .map(tonic_jit::DirectArgument::Caller);
                }
                let default = signature
                    .defaults
                    .iter()
                    .position(|default| usize::from(*default) == slot)?;
                Some(tonic_jit::DirectArgument::Default(defaults[default].raw()))
            })
            .collect::<Option<_>>()?;
        if tonic_jit::unobserved_variadic_parameters(metadata) {
            return Some(named_arguments);
        }
        if implicit_receiver {
            return None;
        }
        let mut arguments = named_arguments.into_iter().map(Some).collect::<Vec<_>>();
        arguments.resize(metadata.params as usize, None);
        if let Some(slot) = signature.vararg {
            let explicit = usize::from(signature.positional);
            let count = usize::from(site.count).saturating_sub(explicit);
            let first = usize::from(site.first).checked_add(explicit)?;
            arguments[slot as usize] = Some(tonic_jit::DirectArgument::VariadicTuple {
                first: u16::try_from(first).ok()?,
                count: u16::try_from(count).ok()?,
            });
        }
        if let Some(slot) = signature.kwarg {
            let mut items = Vec::new();
            for (index, symbol) in site.keywords.iter().copied().enumerate() {
                if (usize::from(signature.posonly)..named)
                    .any(|named_slot| metadata.locals[named_slot] == symbol)
                {
                    continue;
                }
                let register = usize::from(site.first)
                    .checked_add(usize::from(site.count))?
                    .checked_add(index)?;
                items.push((symbol.0, u16::try_from(register).ok()?));
            }
            arguments[slot as usize] = Some(tonic_jit::DirectArgument::VariadicDict { items });
        }
        arguments.into_iter().collect()
    }
    fn jit_direct_calls<'a>(
        &self,
        program: &'a Program,
        code_id: usize,
    ) -> Vec<tonic_jit::DirectCall<'a>> {
        if !self.adaptive_specialization || !self.jit_direct_call_inlining {
            return Vec::new();
        }
        let mut calls: Vec<_> = program.code[code_id]
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(pc, instruction)| {
                if Op::try_from(instruction.opcode) != Ok(Op::Call) {
                    return None;
                }
                let (callee, target_code) = match self.adaptive_sites[code_id][pc] {
                    AdaptiveState::TonicCall { callee, code } => (callee, code),
                    _ => {
                        let profile = self.expanded_call_profiles[code_id][pc]
                            .filter(|profile| profile.count >= QUICKEN_THRESHOLD)?;
                        (profile.callee, profile.code)
                    }
                };
                let site = &program.code[code_id].calls[instruction.c as usize];
                let target = self.direct_call_target(program, callee, site, 0, true)?;
                if target != target_code as usize
                    || !tonic_jit::is_direct_call_inlineable(&program.code[target])
                {
                    return None;
                }
                let arguments = self.jit_direct_arguments(program, callee, site, target, false)?;
                let float = self.float_call_profiles[code_id][pc].is_some_and(|profile| {
                    profile.callee == callee && profile.count >= QUICKEN_THRESHOLD
                }) && arguments
                    .iter()
                    .all(|argument| matches!(argument, tonic_jit::DirectArgument::Caller(_)))
                    && tonic_jit::is_direct_float_leaf_inlineable(&program.code[target]);
                Some(tonic_jit::DirectCall {
                    pc,
                    callee: callee.raw(),
                    target: &program.code[target],
                    arguments,
                    method_attr_pc: None,
                    method_binding: None,
                    expanded_begin_pc: None,
                    float,
                })
            })
            .collect();
        let code = &program.code[code_id];
        let mut argument_starts = Vec::new();
        for (pc, instruction) in code.instructions.iter().copied().enumerate() {
            match Op::try_from(instruction.opcode) {
                Ok(Op::BeginArgs) => argument_starts.push(pc),
                Ok(Op::CallExpanded) => {
                    let Some(begin_pc) = argument_starts.pop() else {
                        continue;
                    };
                    if !argument_starts.is_empty()
                        || code.instructions[begin_pc + 1..pc].iter().any(|between| {
                            !matches!(
                                Op::try_from(between.opcode),
                                Ok(Op::Const
                                    | Op::Move
                                    | Op::ArgPos
                                    | Op::ArgStar
                                    | Op::ArgNamed
                                    | Op::ArgMapping)
                            )
                        })
                    {
                        continue;
                    }
                    let mut positional = Vec::new();
                    let mut keywords = Vec::new();
                    let mut valid = true;
                    for (offset, argument) in code.instructions[begin_pc + 1..pc].iter().enumerate()
                    {
                        let argument_pc = begin_pc + 1 + offset;
                        match Op::try_from(argument.opcode) {
                            Ok(Op::ArgPos) => {
                                positional.push(tonic_jit::DirectArgument::Register(argument.a));
                            }
                            Ok(Op::ArgStar) => {
                                let Some(profile) = self.sequence_profiles[code_id][argument_pc]
                                    .filter(|profile| profile.count >= QUICKEN_THRESHOLD)
                                else {
                                    valid = false;
                                    break;
                                };
                                let length = profile.length;
                                for index in 0..length {
                                    positional.push(tonic_jit::DirectArgument::SequenceItem {
                                        register: argument.a,
                                        index,
                                        length,
                                    });
                                }
                            }
                            Ok(Op::ArgNamed) => keywords.push((
                                tonic_core::ast::SymbolId(argument.b),
                                tonic_jit::DirectArgument::Register(argument.a),
                            )),
                            Ok(Op::ArgMapping) => {
                                let Some(profile) = self.mapping_profiles[code_id][argument_pc]
                                    .as_ref()
                                    .filter(|profile| {
                                        profile.count >= QUICKEN_THRESHOLD
                                            && !profile.keys.is_empty()
                                    })
                                else {
                                    valid = false;
                                    break;
                                };
                                let Ok(key_count) = u16::try_from(profile.keys.len()) else {
                                    valid = false;
                                    break;
                                };
                                for symbol in profile.keys.iter().copied() {
                                    keywords.push((
                                        symbol,
                                        tonic_jit::DirectArgument::MappingItem {
                                            register: argument.a,
                                            symbol: symbol.0,
                                            key_count,
                                        },
                                    ));
                                }
                            }
                            Ok(Op::Const | Op::Move) => {}
                            _ => {
                                valid = false;
                                break;
                            }
                        }
                    }
                    if !valid {
                        continue;
                    }
                    let Some(profile) = self.expanded_call_profiles[code_id][pc]
                        .filter(|profile| profile.count >= QUICKEN_THRESHOLD)
                    else {
                        continue;
                    };
                    let callee = profile.callee;
                    let target = usize::from(profile.code);
                    let target_code = &program.code[target];
                    let defaults = match self.heap.get(callee) {
                        Ok(Object::Function {
                            code,
                            execution,
                            captures,
                            defaults,
                        }) if usize::from(*code) == target
                            && *execution == self.execution
                            && captures.is_empty() =>
                        {
                            defaults
                        }
                        _ => continue,
                    };
                    let signature = &target_code.signature;
                    let named =
                        usize::from(signature.positional) + usize::from(signature.keyword_only);
                    if positional.len() > usize::from(signature.positional)
                        || !target_code.cell_locals.is_empty()
                        || !target_code.free_vars.is_empty()
                        || target_code.class_body
                        || !tonic_jit::unobserved_variadic_parameters(target_code)
                        || !tonic_jit::is_direct_call_inlineable(target_code)
                    {
                        continue;
                    }
                    let mut arguments = vec![None; named];
                    for (slot, argument) in positional.into_iter().enumerate() {
                        arguments[slot] = Some(argument);
                    }
                    for (name, argument) in keywords {
                        let Some(slot) = (usize::from(signature.posonly)..named)
                            .find(|slot| target_code.locals[*slot] == name)
                        else {
                            valid = false;
                            break;
                        };
                        if arguments[slot].replace(argument).is_some() {
                            valid = false;
                            break;
                        }
                    }
                    if !valid {
                        continue;
                    }
                    for (default_index, slot) in signature.defaults.iter().copied().enumerate() {
                        let slot = usize::from(slot);
                        if arguments[slot].is_none() {
                            let Some(default) = defaults.get(default_index) else {
                                valid = false;
                                break;
                            };
                            arguments[slot] =
                                Some(tonic_jit::DirectArgument::Default(default.raw()));
                        }
                    }
                    if !valid || arguments.iter().any(Option::is_none) {
                        continue;
                    }
                    let arguments = arguments.into_iter().flatten().collect();
                    calls.push(tonic_jit::DirectCall {
                        pc,
                        callee: callee.raw(),
                        target: target_code,
                        arguments,
                        method_attr_pc: None,
                        method_binding: None,
                        expanded_begin_pc: Some(begin_pc),
                        float: false,
                    });
                }
                _ => {}
            }
        }
        for (attr_pc, profile) in self.method_profiles[code_id].iter().copied().enumerate() {
            let Some(profile) = profile.filter(|profile| profile.count >= QUICKEN_THRESHOLD) else {
                continue;
            };
            let Some(call_pc) = method_call_pc(code, attr_pc) else {
                continue;
            };
            // A staticmethod also appears as an exact function at CALL. Prefer
            // the fused ATTR profile so the caller remains compilable and the
            // descriptor binding kind is guarded.
            calls.retain(|call| call.pc != call_pc);
            let call = code.instructions[call_pc];
            let site = &code.calls[call.c as usize];
            let implicit_receiver = !matches!(profile.kind, DirectMethodKind::Static);
            let Some(target) = self.direct_call_target(
                program,
                profile.function,
                site,
                usize::from(implicit_receiver),
                false,
            ) else {
                continue;
            };
            if !tonic_jit::is_direct_call_inlineable(&program.code[target]) {
                continue;
            }
            let Some(arguments) = self.jit_direct_arguments(
                program,
                profile.function,
                site,
                target,
                implicit_receiver,
            ) else {
                continue;
            };
            calls.push(tonic_jit::DirectCall {
                pc: call_pc,
                callee: profile.function.raw(),
                target: &program.code[target],
                arguments,
                method_attr_pc: Some(attr_pc),
                method_binding: Some(match profile.kind {
                    DirectMethodKind::Static => tonic_jit::MethodBinding::Static,
                    DirectMethodKind::Instance => tonic_jit::MethodBinding::Instance,
                    DirectMethodKind::Class => tonic_jit::MethodBinding::Class,
                }),
                expanded_begin_pc: None,
                float: false,
            });
        }
        calls
    }
    fn jit_direct_call_profile_pending(&self, program: &Program, code_id: usize) -> bool {
        if !self.jit_direct_call_inlining {
            return false;
        }
        let function_pending =
            program.code[code_id]
                .instructions
                .iter()
                .enumerate()
                .any(|(pc, instruction)| {
                    if Op::try_from(instruction.opcode) != Ok(Op::Call) {
                        return false;
                    }
                    let AdaptiveState::ObservedCall {
                        callee,
                        code,
                        count,
                    } = self.adaptive_sites[code_id][pc]
                    else {
                        return false;
                    };
                    if count < QUICKEN_THRESHOLD - 1 {
                        return false;
                    }
                    let site = &program.code[code_id].calls[instruction.c as usize];
                    self.simple_call_target(program, callee, site)
                        .is_some_and(|target| {
                            target == code as usize
                                && tonic_jit::is_direct_call_inlineable(&program.code[target])
                        })
                });
        let expanded_pending = self.expanded_call_profiles[code_id]
            .iter()
            .flatten()
            .any(|profile| profile.count == QUICKEN_THRESHOLD - 1);
        function_pending
            || expanded_pending
            || self.method_profiles[code_id]
                .iter()
                .copied()
                .enumerate()
                .any(|(attr_pc, profile)| {
                    let Some(profile) =
                        profile.filter(|profile| profile.count >= QUICKEN_THRESHOLD - 1)
                    else {
                        return false;
                    };
                    let Some(call_pc) = method_call_pc(&program.code[code_id], attr_pc) else {
                        return false;
                    };
                    let call = program.code[code_id].instructions[call_pc];
                    let site = &program.code[code_id].calls[call.c as usize];
                    self.direct_call_target(
                        program,
                        profile.function,
                        site,
                        usize::from(!matches!(profile.kind, DirectMethodKind::Static)),
                        false,
                    )
                    .is_some_and(|target| {
                        tonic_jit::is_direct_call_inlineable(&program.code[target])
                    })
                })
    }
    fn observe_simple_call(&mut self, code_id: usize, pc: usize, callee: Value, code: u16) {
        let count = match self.adaptive_sites[code_id][pc] {
            AdaptiveState::Generic => 1,
            AdaptiveState::ObservedCall {
                callee: observed,
                code: observed_code,
                count,
            } if observed == callee && observed_code == code => count.saturating_add(1),
            AdaptiveState::ObservedCall { .. } => 1,
            AdaptiveState::TonicCall { .. } | AdaptiveState::TonicCallPic(_) => return,
            AdaptiveState::ObservedInt(_)
            | AdaptiveState::IntBinary
            | AdaptiveState::ObservedAttr { .. }
            | AdaptiveState::AttrSlot { .. }
            | AdaptiveState::AttrSlotPic(_) => unreachable!("non-call adaptive state"),
        };
        if count >= QUICKEN_THRESHOLD {
            self.adaptive_sites[code_id][pc] = AdaptiveState::TonicCall { callee, code };
            self.stats.call_quickened += 1;
        } else {
            self.adaptive_sites[code_id][pc] = AdaptiveState::ObservedCall {
                callee,
                code,
                count,
            };
        }
    }
    fn install_call_pic(
        &mut self,
        code_id: usize,
        pc: usize,
        first: CallCacheEntry,
        second: CallCacheEntry,
    ) {
        let capacity = self
            .adaptive_sites
            .iter()
            .fold(0usize, |total, sites| total.saturating_add(sites.len()));
        if self.call_pics.len() >= capacity {
            self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
            return;
        }
        let Ok(index) = u32::try_from(self.call_pics.len()) else {
            self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
            return;
        };
        self.call_pics.push(CallPic { first, second });
        self.adaptive_sites[code_id][pc] = AdaptiveState::TonicCallPic(index);
        self.stats.call_quickened += 1;
        self.stats.call_pic_promotions += 1;
    }
    fn observe_method(
        &mut self,
        code_id: usize,
        pc: usize,
        function: Value,
        kind: DirectMethodKind,
    ) {
        let count = match self.method_profiles[code_id][pc] {
            Some(observed) if observed.function == function && observed.kind == kind => {
                observed.count.saturating_add(1)
            }
            _ => 1,
        };
        self.method_profiles[code_id][pc] = Some(MethodProfile {
            function,
            kind,
            count,
        });
    }
    fn observe_sequence(&mut self, code_id: usize, pc: usize, length: Option<u16>) {
        let Some(length) = length else {
            self.sequence_profiles[code_id][pc] = None;
            return;
        };
        let count = match self.sequence_profiles[code_id][pc] {
            Some(observed) if observed.length == length => observed.count.saturating_add(1),
            _ => 1,
        };
        self.sequence_profiles[code_id][pc] = Some(SequenceProfile { length, count });
    }
    fn observe_mapping(&mut self, program: &Program, code_id: usize, pc: usize, owner: Value) {
        let keys = match self.heap.get(owner) {
            Ok(Object::Dict(dict)) if u16::try_from(dict.entries.len()).is_ok() => {
                let mut keys = Vec::with_capacity(dict.entries.len());
                for (key, _) in &dict.entries {
                    let Ok(Object::Str(name)) = self.heap.get(*key) else {
                        self.mapping_profiles[code_id][pc] = None;
                        return;
                    };
                    let Some(symbol) = program
                        .symbols
                        .iter()
                        .position(|candidate| candidate == name)
                        .and_then(|index| u16::try_from(index).ok())
                        .map(tonic_core::ast::SymbolId)
                    else {
                        self.mapping_profiles[code_id][pc] = None;
                        return;
                    };
                    keys.push(symbol);
                }
                keys
            }
            _ => {
                self.mapping_profiles[code_id][pc] = None;
                return;
            }
        };
        let count = match &self.mapping_profiles[code_id][pc] {
            Some(observed) if observed.keys == keys => observed.count.saturating_add(1),
            _ => 1,
        };
        self.mapping_profiles[code_id][pc] = Some(MappingProfile { keys, count });
    }
    fn observe_expanded_call(
        &mut self,
        program: &Program,
        code_id: usize,
        pc: usize,
        callee: Value,
    ) {
        let target = match self.heap.get(callee) {
            Ok(Object::Function {
                code,
                execution,
                captures,
                ..
            }) if *execution == self.execution && captures.is_empty() => *code,
            _ => {
                self.expanded_call_profiles[code_id][pc] = None;
                return;
            }
        };
        if usize::from(target) >= program.code.len() {
            self.expanded_call_profiles[code_id][pc] = None;
            return;
        }
        let count = match self.expanded_call_profiles[code_id][pc] {
            Some(observed) if observed.callee == callee && observed.code == target => {
                observed.count.saturating_add(1)
            }
            _ => 1,
        };
        self.expanded_call_profiles[code_id][pc] = Some(ExpandedCallProfile {
            callee,
            code: target,
            count,
        });
    }
    fn observe_float_call(
        &mut self,
        code_id: usize,
        pc: usize,
        callee: Value,
        first: usize,
        count: usize,
    ) {
        let all_float = count > 0
            && self.registers[first..first + count]
                .iter()
                .all(|value| matches!(self.heap.get(*value), Ok(Object::Float(_))));
        if !all_float {
            self.float_call_profiles[code_id][pc] = None;
            return;
        }
        let count = match self.float_call_profiles[code_id][pc] {
            Some(profile) if profile.callee == callee => profile.count.saturating_add(1),
            _ => 1,
        };
        self.float_call_profiles[code_id][pc] = Some(FloatCallProfile { callee, count });
    }
    fn cached_attr(&self, object: Value, cache: AttrCacheEntry) -> Option<Value> {
        self.heap.cached_instance_slot(
            object,
            cache.class,
            cache.shape,
            usize::from(cache.slot),
            cache.epoch,
        )
    }
    fn observe_instance_attr(
        &mut self,
        code_id: usize,
        pc: usize,
        class: Value,
        shape: ShapeId,
        slot: u16,
        epoch: u64,
    ) {
        let count = match self.adaptive_sites[code_id][pc] {
            AdaptiveState::Generic => 1,
            AdaptiveState::ObservedAttr {
                class: observed_class,
                shape: observed_shape,
                slot: observed_slot,
                epoch: observed_epoch,
                count,
            } if observed_class == class
                && observed_shape == shape
                && observed_slot == slot
                && observed_epoch == epoch =>
            {
                count.saturating_add(1)
            }
            AdaptiveState::ObservedAttr { .. } => 1,
            AdaptiveState::AttrSlot { .. } | AdaptiveState::AttrSlotPic(_) => return,
            AdaptiveState::ObservedInt(_)
            | AdaptiveState::IntBinary
            | AdaptiveState::ObservedCall { .. }
            | AdaptiveState::TonicCall { .. }
            | AdaptiveState::TonicCallPic(_) => unreachable!("non-attribute adaptive state"),
        };
        if count >= QUICKEN_THRESHOLD {
            self.adaptive_sites[code_id][pc] = AdaptiveState::AttrSlot {
                class,
                shape,
                slot,
                epoch,
            };
            self.stats.attr_quickened += 1;
        } else {
            self.adaptive_sites[code_id][pc] = AdaptiveState::ObservedAttr {
                class,
                shape,
                slot,
                epoch,
                count,
            };
        }
    }
    fn install_attr_pic(
        &mut self,
        code_id: usize,
        pc: usize,
        first: AttrCacheEntry,
        second: AttrCacheEntry,
    ) {
        let capacity = self
            .adaptive_sites
            .iter()
            .fold(0usize, |total, sites| total.saturating_add(sites.len()));
        if self.attr_pics.len() >= capacity {
            self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
            return;
        }
        let Ok(index) = u32::try_from(self.attr_pics.len()) else {
            self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
            return;
        };
        self.attr_pics.push(AttrPic { first, second });
        self.adaptive_sites[code_id][pc] = AdaptiveState::AttrSlotPic(index);
        self.stats.attr_quickened += 1;
        self.stats.attr_pic_promotions += 1;
    }
    fn enter_simple_frame(
        &mut self,
        program: &Program,
        code: usize,
        destination: usize,
        first: usize,
        site: &tonic_core::bytecode::CallSite,
        callable: Value,
    ) -> Result<()> {
        if self.frames.len() >= self.limits.frames {
            return Err(Diagnostic::new(
                "RecursionError",
                "maximum call depth exceeded",
            ));
        }
        let base = self.registers.len();
        let metadata = &program.code[code];
        let register_count = metadata.registers as usize;
        let end = base
            .checked_add(register_count)
            .ok_or_else(|| Diagnostic::new("MemoryError", "register size overflow"))?;
        if end > self.limits.registers {
            return Err(Diagnostic::new("MemoryError", "register budget exceeded"));
        }
        self.registers.resize(end, Value::UNBOUND);
        for offset in 0..usize::from(site.count) {
            self.registers[base + offset] = self.registers[first + offset];
        }
        let named = usize::from(metadata.signature.positional)
            + usize::from(metadata.signature.keyword_only);
        for (index, keyword) in site.keywords.iter().copied().enumerate() {
            let slot = (usize::from(metadata.signature.posonly)..named)
                .find(|slot| metadata.locals[*slot] == keyword)
                .expect("validated specialized keyword binding");
            self.registers[base + slot] = self.registers[first + usize::from(site.count) + index];
        }
        let Object::Function { defaults, .. } = self.heap.get(callable)? else {
            unreachable!("specialized call guard requires an exact function")
        };
        for (slot, value) in metadata.signature.defaults.iter().zip(defaults) {
            let register = base + usize::from(*slot);
            if self.registers[register] == Value::UNBOUND {
                self.registers[register] = *value;
            }
        }
        self.frames.push(Frame {
            namespace: None,
            action: ReturnAction::Value,
            code,
            ip: 0,
            base,
            destination: Some(destination),
            cell_base: self.cells.len(),
            callable: Some(callable),
            jit_attempted: matches!(self.jit_cache.get(code), Some(JitEntry::Unsupported)),
            jit_resume: false,
            jit_expanded_resume_depth: None,
        });
        self.stats.peak_registers = self.stats.peak_registers.max(end);
        Ok(())
    }
    fn try_jit(&mut self, program: &Program, output: &mut dyn Write) -> Result<bool> {
        if self.execution_mode != ExecutionMode::Jit || self.limits.instructions.is_some() {
            return Ok(false);
        }
        let frame = self.frames.last_mut().expect("active frame");
        let resuming = frame.jit_resume;
        if frame.code == 0 || (!resuming && (frame.ip != 0 || frame.jit_attempted)) {
            return Ok(false);
        }
        frame.jit_resume = false;
        if !resuming {
            frame.jit_attempted = true;
        }
        let code_id = frame.code;
        let base = frame.base;
        let start_pc = frame.ip;
        let osr_entry = resuming && matches!(self.jit_cache[code_id], JitEntry::Untried);
        if resuming && !osr_entry {
            self.stats.jit_resumes += 1;
        }
        if matches!(self.jit_cache[code_id], JitEntry::Unsupported) {
            self.stats.jit_fallbacks += 1;
            return Ok(false);
        }
        let register_count = program.code[code_id].registers as usize;
        if matches!(self.jit_cache[code_id], JitEntry::Untried) {
            let loop_candidate = has_backedge(&program.code[code_id]);
            let float_leaf = !loop_candidate
                && program.code[code_id].params > 0
                && tonic_jit::is_direct_float_leaf_inlineable(&program.code[code_id])
                && self.registers[base..base + program.code[code_id].params as usize]
                    .iter()
                    .all(|value| matches!(self.heap.get(*value), Ok(Object::Float(_))));
            if float_leaf {
                self.stats.jit_unprofitable += 1;
                self.jit_cache[code_id] = JitEntry::Unsupported;
                return Ok(false);
            }
            let runtime_calls = jit_runtime_call_count(&program.code[code_id]);
            let profitable_size = if self.jit_min_instructions == 0 {
                0
            } else {
                self.jit_min_instructions
                    .saturating_add(runtime_calls.saturating_mul(4))
            };
            if !loop_candidate && program.code[code_id].instructions.len() < profitable_size {
                self.stats.jit_unprofitable += 1;
                self.jit_cache[code_id] = JitEntry::Unsupported;
                return Ok(false);
            }
            if !osr_entry {
                self.jit_hotness[code_id] = self.jit_hotness[code_id].saturating_add(1);
                let threshold = if loop_candidate {
                    self.jit_osr_threshold.max(1)
                } else {
                    self.jit_threshold.max(1)
                };
                if self.jit_hotness[code_id] < threshold
                    || self.jit_direct_call_profile_pending(program, code_id)
                {
                    self.stats.jit_deferred += 1;
                    return Ok(false);
                }
            }
            self.stats.jit_compile_attempts += 1;
            let direct_calls = self.jit_direct_calls(program, code_id);
            let exact_float_parameters = (0..program.code[code_id].params)
                .filter(|register| {
                    matches!(
                        self.heap.get(self.registers[base + *register as usize]),
                        Ok(Object::Float(_))
                    )
                })
                .collect::<Vec<_>>();
            let materialized_constants = program.code[code_id]
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, instruction)| Op::try_from(instruction.opcode) == Ok(Op::Const))
                .map(|(pc, instruction)| tonic_jit::MaterializedConstant {
                    pc,
                    value: self.constants[code_id][instruction.b as usize].raw(),
                })
                .collect::<Vec<_>>();
            match tonic_jit::compile_with_execution_profile(
                &program.code[code_id],
                &direct_calls,
                &materialized_constants,
                &exact_float_parameters,
            ) {
                Ok(compiled) => {
                    let metadata = compiled.metadata();
                    self.stats.jit_compiled += 1;
                    self.stats.jit_compile_ns += metadata.compile_time.as_nanos();
                    self.stats.jit_code_bytes += metadata.code_bytes;
                    self.stats.jit_direct_call_sites += metadata.direct_call_sites as u64;
                    self.stats.jit_direct_method_sites += metadata.direct_method_sites as u64;
                    self.jit_cache[code_id] = JitEntry::Compiled {
                        function: Box::new(compiled),
                        deopts: 0,
                    };
                    if osr_entry {
                        self.stats.jit_osr_entries += 1;
                    }
                }
                Err(tonic_jit::Error::Unsupported(_)) => {
                    self.stats.jit_fallbacks += 1;
                    self.jit_cache[code_id] = JitEntry::Unsupported;
                    return Ok(false);
                }
                Err(error) => return Err(Diagnostic::new("JitError", error.to_string())),
            }
        }
        let end = base + register_count;
        self.jit_registers.clear();
        self.jit_registers
            .extend(self.registers[base..end].iter().map(|value| value.raw()));
        let (root_count, safepoints) = match &self.jit_cache[code_id] {
            JitEntry::Compiled { function, .. } => {
                let metadata = function.metadata();
                (metadata.root_count, metadata.safepoints)
            }
            JitEntry::Untried | JitEntry::Unsupported => {
                self.stats.jit_fallbacks += 1;
                return Ok(false);
            }
        };
        self.jit_registers.resize(root_count, Value::UNBOUND.raw());
        let direct_call_sites = match &self.jit_cache[code_id] {
            JitEntry::Compiled { function, .. } => function.metadata().direct_call_sites,
            JitEntry::Untried | JitEntry::Unsupported => 0,
        };
        if direct_call_sites > 0 && self.frames.len() >= self.limits.frames {
            self.stats.jit_fallbacks += 1;
            return Ok(false);
        }
        let mut roots = std::mem::take(&mut self.jit_roots);
        roots.clear();
        if safepoints > 0 {
            self.append_gc_roots(Some(base..end), &mut roots);
        }
        self.stats.jit_calls += 1;
        let mut direct_calls = 0;
        let result = {
            let JitEntry::Compiled { function, .. } = &self.jit_cache[code_id] else {
                unreachable!("compiled entry checked immediately above")
            };
            let mut runtime = JitRuntime {
                heap: &mut self.heap,
                handles: &mut self.handles,
                stats: &mut self.stats,
                roots: &roots,
                globals: &self.globals,
                symbols: &program.symbols,
                gc_interval: self.gc_interval,
                minor_collections: &mut self.minor_collections,
            };
            function.run_from_with_globals_counted(
                &mut self.jit_registers,
                &self.jit_globals,
                start_pc,
                &mut runtime,
                &mut direct_calls,
            )
        };
        self.stats.calls = self.stats.calls.saturating_add(direct_calls);
        self.stats.jit_direct_calls = self.stats.jit_direct_calls.saturating_add(direct_calls);
        self.jit_roots = roots;
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(tonic_jit::Error::Runtime { pc, failure }) => {
                self.stats.jit_runtime_errors += 1;
                self.frames.last_mut().expect("JIT frame").ip = pc + 1;
                return Err(Diagnostic::new(failure.kind, failure.message));
            }
            Err(error) => return Err(Diagnostic::new("JitError", error.to_string())),
        };
        for (slot, raw) in self.registers[base..end]
            .iter_mut()
            .zip(self.jit_registers[..register_count].iter().copied())
        {
            *slot = Value::from_jit(raw);
        }
        match outcome {
            tonic_jit::Outcome::Returned { pc, .. } => {
                self.stats.jit_returns += 1;
                self.frames.last_mut().expect("JIT frame").ip = pc;
            }
            tonic_jit::Outcome::Deopt { pc } => {
                self.stats.jit_deopts += 1;
                self.frames.last_mut().expect("JIT frame").ip = pc;
                let JitEntry::Compiled { deopts, .. } = &mut self.jit_cache[code_id] else {
                    unreachable!("compiled entry ran immediately above")
                };
                *deopts += 1;
                if *deopts >= JIT_DEOPT_LIMIT {
                    self.jit_cache[code_id] = JitEntry::Unsupported;
                    self.stats.jit_despecialized += 1;
                }
            }
            tonic_jit::Outcome::SideExit { pc } => {
                self.stats.jit_side_exits += 1;
                let op = Op::try_from(program.code[code_id].instructions[pc].opcode)?;
                if op == Op::Call {
                    let frame = self.frames.last_mut().expect("JIT frame");
                    frame.ip = pc + 1;
                    frame.jit_resume = true;
                    self.execute_call(program, code_id, pc, output)?;
                    return Ok(true);
                }
                if op == Op::BeginArgs {
                    let depth = self.arguments.len();
                    let frame = self.frames.last_mut().expect("JIT frame");
                    frame.ip = pc;
                    frame.jit_resume = false;
                    frame.jit_expanded_resume_depth = Some(depth);
                    return Ok(false);
                }
                let frame = self.frames.last_mut().expect("JIT frame");
                // Re-run the exact generic instruction in the interpreter.
                // The main loop has already decided not to call `try_jit` again
                // in this iteration, so leaving `jit_resume` set is safe even
                // when the instruction suspends into a child VM frame.
                frame.ip = pc;
                frame.jit_resume = true;
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn invoke_set_names(
        &mut self,
        p: &Program,
        destination: usize,
        class: Value,
        mut pending: Vec<SetNameCall>,
        output: &mut dyn Write,
    ) -> Result<()> {
        while let Some(item) = pending.pop() {
            let depth = self.frames.len();
            self.invoke(
                p,
                item.call.callable,
                destination,
                Arguments::Inline {
                    receiver: item.call.receiver,
                    positional: [class, item.name],
                    count: 2,
                },
                output,
            )?;
            if self.frames.len() > depth {
                self.frames
                    .last_mut()
                    .expect("set_name callback frame")
                    .action = ReturnAction::SetNames { class, pending };
                return Ok(());
            }
            self.registers[destination] = class;
        }
        Ok(())
    }
}

impl Drop for Vm {
    fn drop(&mut self) {
        self.runtime_owner.mark_dead();
    }
}

fn has_backedge(code: &tonic_core::bytecode::CodeObject) -> bool {
    code.instructions
        .iter()
        .enumerate()
        .any(|(pc, instruction)| {
            let Ok(op) = Op::try_from(instruction.opcode) else {
                return false;
            };
            match op {
                Op::Jump => usize::from(instruction.a) <= pc,
                Op::JumpFalse | Op::JumpTrue => usize::from(instruction.b) <= pc,
                _ => false,
            }
        })
}

fn method_call_pc(code: &tonic_core::bytecode::CodeObject, attr_pc: usize) -> Option<usize> {
    let attr = *code.instructions.get(attr_pc)?;
    if Op::try_from(attr.opcode) != Ok(Op::Attr) {
        return None;
    }
    if attr.a == attr.b {
        return None;
    }
    for (pc, instruction) in code.instructions.iter().enumerate().skip(attr_pc + 1) {
        let op = Op::try_from(instruction.opcode).ok()?;
        if op == Op::Call && instruction.b == attr.a {
            return Some(pc);
        }
        if !matches!(op, Op::Const | Op::Move)
            || instruction.a == attr.a
            || instruction.a == attr.b
            || (op == Op::Move && instruction.b == attr.a)
        {
            return None;
        }
    }
    None
}

fn jit_runtime_call_count(code: &tonic_core::bytecode::CodeObject) -> usize {
    code.instructions
        .iter()
        .filter(|instruction| {
            matches!(
                Op::try_from(instruction.opcode),
                Ok(Op::Div | Op::LoadGlobal)
            )
        })
        .count()
}

#[inline(always)]
fn immediate_binary(op: Op, left: Value, right: Value) -> Option<Value> {
    let left = left.integer()?;
    let right = right.integer()?;
    let result = match op {
        Op::Add | Op::InplaceAdd => left.checked_add(right),
        Op::Sub => left.checked_sub(right),
        Op::Mul => left.checked_mul(right),
        _ => None,
    }?;
    Value::int(result)
}
