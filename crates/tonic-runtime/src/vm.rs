#[path = "calls.rs"]
pub(crate) mod calls;
use crate::classes::{DescriptorCall, DirectMethodKind};
use crate::{
    heap::{
        Builtin, GeneratorFrame, GeneratorState, Heap, Object, SuspendedGenerator, TracebackEntry,
    },
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
    pub jit_code_budget_rejections: u64,
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
    argument_base: usize,
    pending_class_base: usize,
    callable: Option<Value>,
    generator: Option<Value>,
    yield_from: Option<Value>,
    injected_exception: Option<Value>,
    exception_stack: Vec<Value>,
    jit_attempted: bool,
    jit_resume: bool,
    jit_expanded_resume_depth: Option<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceModuleState {
    Uninitialized,
    Initializing,
    Loaded,
}
struct SourceModuleRuntime {
    object: Option<Value>,
    state: SourceModuleState,
    version: u64,
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
    runtime_owner: &'a Arc<RuntimeOwner>,
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
            let trace_calls = self
                .heap
                .refresh_foreign_references(self.handles)
                .map_err(runtime_failure)?;
            self.stats.foreign_trace_calls += trace_calls;
            drain_deferred_handle_releases(self.runtime_owner, self.handles)
                .map_err(runtime_failure)?;
            let mut roots = self.roots.to_vec();
            roots.extend(registers.iter().copied().map(Value::from_jit));
            self.handles.roots(|value| roots.push(value));
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

fn drain_deferred_handle_releases(owner: &RuntimeOwner, handles: &mut HandleTable) -> Result<()> {
    for raw in owner.take_persistent_releases() {
        let handle = PersistentHandle::from_raw(raw);
        handles.release_persistent(&handle).map_err(|error| {
            Diagnostic::new(
                error.kind,
                format!("invalid deferred persistent release: {}", error.message),
            )
        })?;
    }
    for raw in owner.take_foreign_reference_releases() {
        let handle = crate::native::Handle::from_raw(raw);
        handles.release_foreign_reference(handle).map_err(|error| {
            Diagnostic::new(
                error.kind,
                format!(
                    "invalid deferred foreign reference release: {}",
                    error.message
                ),
            )
        })?;
    }
    Ok(())
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
const DEFAULT_JIT_MAX_CODE_BYTES: usize = 64 * 1024 * 1024;
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
    Iterator,
    Next(Option<Value>),
    Close,
    IteratorNext {
        target: usize,
        pc: usize,
    },
    YieldFrom {
        target: usize,
        pc: usize,
        iterator: Value,
    },
    YieldFromClose {
        exception: Value,
    },
    GeneratorThrowInit {
        generator: Value,
        traceback: Option<Value>,
        instance: Value,
    },
    CollectIterableStart(IterableCollectionKind),
    CollectIterableNext(IterableCollection),
    ExpandIterableStart(Option<usize>),
    ExpandIterableNext(ArgumentExpansion),
    DictIterableStart(DictConstructionStart),
    DictIterableNext(DictConstruction),
    DictPairStart(DictConstruction),
    DictPairIteratorStart(DictConstruction),
    DictPairNext(DictPairConstruction),
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
    ClassPrepare(ClassBuild),
    ClassBody(ClassBody),
    MetaclassNew(ClassHookState),
    MetaclassInit(Value),
    SetNames {
        class: Value,
        pending: Vec<SetNameCall>,
        after: Option<ClassHookState>,
    },
    NamespaceLookup {
        target: usize,
        symbol: u16,
        cell: Option<Value>,
    },
    AttributeGet(AttributeGet),
    BinaryProtocol(BinaryProtocol),
    UnaryProtocol(UnaryProtocol),
    NumericConversion(NumericConversion),
    IndexConversion(IndexConversion),
    Hash(HashAction),
    Import(usize),
    Setter,
}
#[derive(Clone)]
pub(super) enum HashAction {
    Return,
    Dict(DictOperationStart),
    Sequence {
        values: Vec<Value>,
        next: usize,
        accumulator: i64,
        depth: usize,
        outer: Box<HashAction>,
    },
}
#[derive(Clone)]
pub(super) enum DictOperationKind {
    Get,
    Set(Value),
    SetAndContinue {
        value: Value,
        state: Box<DictConstruction>,
    },
    SetAndMerge {
        value: Value,
        state: Box<DictMergeState>,
    },
    Delete,
    CompareValue {
        expected: Value,
        action: Box<EqualityAction>,
    },
}
#[derive(Clone)]
pub(super) struct DictOperationStart {
    owner: Value,
    key: Value,
    kind: DictOperationKind,
}
#[derive(Clone)]
pub(super) struct DictOperation {
    start: DictOperationStart,
    hash: i64,
    version: u64,
    candidates: Vec<usize>,
    next: usize,
    comparison_depth: usize,
}
#[derive(Clone)]
pub(super) enum DictMergeFinish {
    Preserve,
    Construction(DictConstructionStart),
}
#[derive(Clone)]
pub(super) struct DictMergeState {
    target: Value,
    entries: Vec<(Value, Value)>,
    next: usize,
    finish: DictMergeFinish,
}
#[derive(Clone, Copy)]
pub(super) struct BinaryCandidate {
    call: DescriptorCall,
    argument: Value,
    negate: bool,
}
#[derive(Clone)]
pub(super) struct BinaryProtocol {
    op: Op,
    left: Value,
    right: Value,
    candidates: Vec<BinaryCandidate>,
    next: usize,
    negate_result: bool,
    completion: BinaryCompletion,
}
#[derive(Clone)]
pub(super) enum BinaryCompletion {
    Value,
    Equality(EqualityAction),
}
#[derive(Clone)]
pub(super) enum EqualityAction {
    Return {
        negate: bool,
    },
    Sequence {
        left: Vec<Value>,
        right: Vec<Value>,
        next: usize,
        depth: usize,
        outer: Box<EqualityAction>,
    },
    DictCandidate {
        state: DictOperation,
        index: usize,
    },
    DictEntries {
        left: Value,
        right: Value,
        entries: Vec<(Value, Value)>,
        next: usize,
        left_version: u64,
        right_version: u64,
        depth: usize,
        outer: Box<EqualityAction>,
    },
    SequenceOrder(SequenceOrder),
}
#[derive(Clone)]
pub(super) struct SequenceOrder {
    left: Vec<Value>,
    right: Vec<Value>,
    index: usize,
    op: Op,
    depth: usize,
}
#[derive(Clone, Copy)]
pub(super) enum UnaryProtocolKind {
    Neg,
    Pos,
    Abs,
    Invert,
}
#[derive(Clone, Copy)]
pub(super) struct UnaryProtocol {
    kind: UnaryProtocolKind,
    value: Value,
}
#[derive(Clone, Copy)]
pub(super) enum NumericConversionKind {
    Int,
    Float,
    IndexToInt,
    IndexToFloat,
}
#[derive(Clone)]
pub(super) struct NumericConversion {
    kind: NumericConversionKind,
    finish: Option<NativeSubclassFinish>,
}
#[derive(Clone)]
pub(super) struct IndexConversion {
    continuation: IndexContinuation,
}
#[derive(Clone)]
pub(super) enum IndexContinuation {
    Length,
    Truth(TruthAction),
    Range(RangeConstruction),
    IntBase(IntBaseConversion),
    GetItem { owner: Value },
    SetItem { owner: Value, value: Value },
    DelItem { owner: Value },
    Slice(SliceConversion),
}
#[derive(Clone)]
pub(super) struct RangeConstruction {
    values: [Value; 3],
    count: usize,
    next: usize,
}
#[derive(Clone)]
pub(super) struct IntBaseConversion {
    argument: Value,
    finish: Option<NativeSubclassFinish>,
}
#[derive(Clone)]
pub(super) struct SliceConversion {
    owner: Value,
    components: [Value; 3],
    next: usize,
}
#[derive(Clone)]
pub(super) struct AttributeGet {
    owner: Value,
    name: String,
    missing: AttributeMissing,
    phase: AttributePhase,
}
#[derive(Clone, Copy)]
pub(super) enum AttributeMissing {
    Raise,
    Default(Value),
    HasAttr,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AttributePhase {
    Primary,
    Fallback,
}
#[derive(Clone)]
pub(super) enum IterableCollectionKind {
    List,
    Tuple,
    NativeList(NativeSubclassFinish),
    NativeTuple(NativeSubclassFinish),
    ListInit { owner: Value, result: Value },
    Unpack { first: usize, count: usize },
}
#[derive(Clone)]
pub(super) struct NativeSubclassFinish {
    class: Value,
    initialize: Option<ExpandedArgs>,
}
#[derive(Clone)]
pub(super) struct IterableCollection {
    kind: IterableCollectionKind,
    iterator: Value,
    items: Vec<Value>,
}
#[derive(Clone, Copy)]
pub(super) struct ArgumentExpansion {
    iterator: Value,
    resume_pc: Option<usize>,
}
#[derive(Clone)]
pub(super) struct DictConstructionStart {
    result: Value,
    keywords: Vec<(Value, Value)>,
    native: Option<NativeSubclassFinish>,
    return_value: Option<Value>,
}
#[derive(Clone)]
pub(super) struct DictConstruction {
    start: DictConstructionStart,
    iterator: Value,
    index: usize,
}
#[derive(Clone)]
pub(super) struct DictPairConstruction {
    outer: DictConstruction,
    iterator: Value,
    items: Vec<Value>,
}
#[derive(Clone, Copy)]
enum TruthProtocol {
    Bool,
    Length,
}
#[derive(Clone)]
pub(super) enum TruthAction {
    Not,
    Return,
    Jump {
        when: bool,
        target: usize,
        pc: usize,
        original: Value,
    },
    Equality(EqualityAction),
}
struct SetNameCall {
    call: DescriptorCall,
    name: Value,
}
struct ClassBuild {
    function: Value,
    bases: Vec<Value>,
    declared_bases: Vec<Value>,
    metaclass: Value,
    qualname: String,
}
struct ClassBody {
    namespace: Value,
    declared_bases: Vec<Value>,
    metaclass: Value,
    qualname: String,
}
#[derive(Clone)]
struct ClassHookState {
    namespace: Value,
    mapping: Value,
    metaclass: Value,
    name: Value,
    bases: Value,
    class_cell: Option<Value>,
}
struct PendingClass {
    state: ClassHookState,
    finalized: bool,
}
#[derive(Clone)]
struct RuntimeTypes {
    none: Value,
    not_implemented: Value,
    int: Value,
    bool_: Value,
    float: Value,
    str_: Value,
    list: Value,
    tuple: Value,
    dict: Value,
    range: Value,
    function: Value,
    generator: Value,
    base_exception: Value,
    exception: Value,
    type_error: Value,
    value_error: Value,
    runtime_error: Value,
    stop_iteration: Value,
    generator_exit: Value,
    other_exceptions: Vec<(String, Value)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimeTypeKind {
    Int,
    Bool,
    Float,
    Str,
    List,
    Tuple,
    Dict,
    Range,
    Exception,
}
impl RuntimeTypes {
    fn unbound() -> Self {
        Self {
            none: Value::UNBOUND,
            not_implemented: Value::UNBOUND,
            int: Value::UNBOUND,
            bool_: Value::UNBOUND,
            float: Value::UNBOUND,
            str_: Value::UNBOUND,
            list: Value::UNBOUND,
            tuple: Value::UNBOUND,
            dict: Value::UNBOUND,
            range: Value::UNBOUND,
            function: Value::UNBOUND,
            generator: Value::UNBOUND,
            base_exception: Value::UNBOUND,
            exception: Value::UNBOUND,
            type_error: Value::UNBOUND,
            value_error: Value::UNBOUND,
            runtime_error: Value::UNBOUND,
            stop_iteration: Value::UNBOUND,
            generator_exit: Value::UNBOUND,
            other_exceptions: Vec::new(),
        }
    }
    fn roots(&self) -> Vec<Value> {
        let mut roots = vec![
            self.none,
            self.not_implemented,
            self.int,
            self.bool_,
            self.float,
            self.str_,
            self.list,
            self.tuple,
            self.dict,
            self.range,
            self.function,
            self.generator,
            self.base_exception,
            self.exception,
            self.type_error,
            self.value_error,
            self.runtime_error,
            self.stop_iteration,
            self.generator_exit,
        ];
        roots.extend(self.other_exceptions.iter().map(|(_, value)| *value));
        roots
    }
}
impl ClassHookState {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        visit(self.namespace);
        visit(self.mapping);
        visit(self.metaclass);
        visit(self.name);
        visit(self.bases);
        self.class_cell.iter().copied().for_each(visit);
    }
}
impl DictConstructionStart {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        visit(self.result);
        self.keywords.iter().for_each(|(key, value)| {
            visit(*key);
            visit(*value);
        });
        if let Some(native) = &self.native {
            native.trace(&mut visit);
        }
        self.return_value.iter().copied().for_each(visit);
    }
}
impl DictConstruction {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        self.start.trace(&mut visit);
        visit(self.iterator);
    }
}
impl DictPairConstruction {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        self.outer.trace(&mut visit);
        visit(self.iterator);
        self.items.iter().copied().for_each(visit);
    }
}
impl ReturnAction {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        match self {
            Self::Value
            | Self::Iterator
            | Self::IteratorNext { .. }
            | Self::Close
            | Self::ExpandIterableStart(_)
            | Self::Length
            | Self::Import(_)
            | Self::NamespaceLookup { cell: None, .. }
            | Self::Setter => {}
            Self::YieldFrom { iterator, .. } => visit(*iterator),
            Self::YieldFromClose { exception } => visit(*exception),
            Self::GeneratorThrowInit {
                generator,
                traceback,
                instance,
            } => {
                visit(*generator);
                traceback.iter().copied().for_each(&mut visit);
                visit(*instance);
            }
            Self::Next(default) => default.iter().copied().for_each(visit),
            Self::CollectIterableStart(kind) => kind.trace(visit),
            Self::AttributeGet(state) => {
                visit(state.owner);
                if let AttributeMissing::Default(value) = state.missing {
                    visit(value);
                }
            }
            Self::BinaryProtocol(state) => {
                visit(state.left);
                visit(state.right);
                for candidate in &state.candidates[state.next..] {
                    visit(candidate.call.callable);
                    candidate.call.receiver.iter().copied().for_each(&mut visit);
                    visit(candidate.argument);
                }
                state.completion.trace(&mut visit);
            }
            Self::UnaryProtocol(state) => visit(state.value),
            Self::NumericConversion(state) => {
                if let Some(finish) = &state.finish {
                    finish.trace(visit);
                }
            }
            Self::IndexConversion(state) => state.continuation.trace(visit),
            Self::Hash(action) => action.trace(visit),
            Self::NamespaceLookup {
                cell: Some(cell), ..
            } => visit(*cell),
            Self::CollectIterableNext(state) => {
                state.kind.trace(&mut visit);
                visit(state.iterator);
                state.items.iter().copied().for_each(visit);
            }
            Self::ExpandIterableNext(state) => visit(state.iterator),
            Self::DictIterableStart(state) => state.trace(visit),
            Self::DictIterableNext(state)
            | Self::DictPairStart(state)
            | Self::DictPairIteratorStart(state) => state.trace(visit),
            Self::DictPairNext(state) => state.trace(visit),
            Self::Truth { action, .. } => action.trace(visit),
            Self::Initializer(v) | Self::MetaclassInit(v) => visit(*v),
            Self::New { class, arguments } => {
                visit(*class);
                arguments.trace(visit);
            }
            Self::ClassPrepare(build) => {
                visit(build.function);
                build.bases.iter().copied().for_each(&mut visit);
                build.declared_bases.iter().copied().for_each(&mut visit);
                visit(build.metaclass);
            }
            Self::ClassBody(body) => {
                visit(body.namespace);
                body.declared_bases.iter().copied().for_each(&mut visit);
                visit(body.metaclass);
            }
            Self::MetaclassNew(state) => state.trace(visit),
            Self::SetNames {
                class,
                pending,
                after,
            } => {
                visit(*class);
                for item in pending {
                    visit(item.call.callable);
                    if let Some(receiver) = item.call.receiver {
                        visit(receiver);
                    }
                    visit(item.name);
                }
                if let Some(after) = after {
                    after.trace(visit);
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
impl HashAction {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        match self {
            Self::Return => {}
            Self::Dict(state) => state.trace(visit),
            Self::Sequence { values, outer, .. } => {
                values.iter().copied().for_each(&mut visit);
                outer.trace(visit);
            }
        }
    }
}
impl DictOperationStart {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        visit(self.owner);
        visit(self.key);
        match &self.kind {
            DictOperationKind::Get | DictOperationKind::Delete => {}
            DictOperationKind::Set(value) => visit(*value),
            DictOperationKind::SetAndContinue { value, state } => {
                visit(*value);
                state.trace(visit);
            }
            DictOperationKind::SetAndMerge { value, state } => {
                visit(*value);
                state.trace(visit);
            }
            DictOperationKind::CompareValue { expected, action } => {
                visit(*expected);
                action.trace(visit);
            }
        }
    }
}
impl DictOperation {
    fn trace(&self, visit: impl FnMut(Value)) {
        self.start.trace(visit);
    }
}
impl DictMergeState {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        visit(self.target);
        self.entries.iter().for_each(|(key, value)| {
            visit(*key);
            visit(*value);
        });
        if let DictMergeFinish::Construction(state) = &self.finish {
            state.trace(visit);
        }
    }
}
impl EqualityAction {
    fn comparison_depth(&self) -> usize {
        match self {
            Self::Return { .. } => 0,
            Self::Sequence { depth, .. } => *depth,
            Self::DictCandidate { state, .. } => state.comparison_depth,
            Self::DictEntries { depth, .. } => *depth,
            Self::SequenceOrder(state) => state.depth,
        }
    }

    fn trace(&self, mut visit: impl FnMut(Value)) {
        match self {
            Self::Return { .. } => {}
            Self::Sequence {
                left, right, outer, ..
            } => {
                left.iter().chain(right).copied().for_each(&mut visit);
                outer.trace(visit);
            }
            Self::DictCandidate { state, .. } => state.trace(visit),
            Self::DictEntries {
                left,
                right,
                entries,
                outer,
                ..
            } => {
                visit(*left);
                visit(*right);
                entries.iter().for_each(|(key, value)| {
                    visit(*key);
                    visit(*value);
                });
                outer.trace(visit);
            }
            Self::SequenceOrder(state) => {
                state
                    .left
                    .iter()
                    .chain(&state.right)
                    .copied()
                    .for_each(visit);
            }
        }
    }
}
impl BinaryCompletion {
    fn trace(&self, visit: impl FnMut(Value)) {
        if let Self::Equality(action) = self {
            action.trace(visit);
        }
    }
}
impl TruthAction {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        match self {
            Self::Not | Self::Return => {}
            Self::Jump { original, .. } => visit(*original),
            Self::Equality(action) => action.trace(visit),
        }
    }
}
impl NativeSubclassFinish {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        visit(self.class);
        if let Some(arguments) = &self.initialize {
            arguments.trace(visit);
        }
    }
}
impl IndexContinuation {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        match self {
            Self::Length => {}
            Self::Truth(action) => action.trace(visit),
            Self::Range(state) => state.values[..state.count].iter().copied().for_each(visit),
            Self::IntBase(state) => {
                visit(state.argument);
                if let Some(finish) = &state.finish {
                    finish.trace(visit);
                }
            }
            Self::GetItem { owner } | Self::DelItem { owner } => visit(*owner),
            Self::SetItem { owner, value } => {
                visit(*owner);
                visit(*value);
            }
            Self::Slice(state) => {
                visit(state.owner);
                state.components.iter().copied().for_each(visit);
            }
        }
    }
}
impl IterableCollectionKind {
    fn trace(&self, mut visit: impl FnMut(Value)) {
        match self {
            Self::NativeList(state) | Self::NativeTuple(state) => state.trace(visit),
            Self::ListInit { owner, result } => {
                visit(*owner);
                visit(*result);
            }
            Self::List | Self::Tuple | Self::Unpack { .. } => {}
        }
    }
}
/// Single-threaded interpreter instance. Handles may only be used with this VM.
/// Repeated run calls start fresh module globals; persistent native roots survive.
/// Collection occurs between instructions; native Context scopes cannot collect.
pub struct Vm {
    object_class: Value,
    type_class: Value,
    runtime_types: RuntimeTypes,
    pub(crate) execution: u64,
    phase: RuntimePhase,
    attached_thread: Option<ThreadId>,
    active_program: Option<Arc<Program>>,
    pub(crate) runtime_owner: Arc<RuntimeOwner>,
    pub(crate) heap: Heap,
    pub(crate) handles: HandleTable,
    natives: Vec<NativeDef>,
    modules: HashMap<String, Value>,
    source_modules: Vec<SourceModuleRuntime>,
    source_module_names: HashMap<String, usize>,
    global_owners: Vec<Option<usize>>,
    global_defined: Vec<bool>,
    builtins: Vec<(String, Value)>,
    globals: Vec<Value>,
    constants: Vec<Vec<Value>>,
    registers: Vec<Value>,
    cells: Vec<Value>,
    arguments: Vec<ExpandedArgs>,
    frames: Vec<Frame>,
    pending_classes: Vec<PendingClass>,
    pending_exception: Option<Value>,
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
    /// Maximum native code bytes accepted during one module execution.
    pub jit_max_code_bytes: usize,
    jit_code_budget_used: usize,
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
            type_class: Value::UNBOUND,
            runtime_types: RuntimeTypes::unbound(),
            execution: 0,
            phase: RuntimePhase::Running,
            attached_thread: Some(std::thread::current().id()),
            active_program: None,
            runtime_owner: Arc::new(RuntimeOwner::new()?),
            heap: Heap::default(),
            handles: HandleTable::default(),
            natives: Vec::new(),
            modules: HashMap::new(),
            source_modules: Vec::new(),
            source_module_names: HashMap::new(),
            global_owners: Vec::new(),
            global_defined: Vec::new(),
            builtins: Vec::new(),
            globals: Vec::new(),
            constants: Vec::new(),
            registers: Vec::new(),
            cells: Vec::new(),
            arguments: Vec::new(),
            frames: Vec::new(),
            pending_classes: Vec::new(),
            pending_exception: None,
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
            jit_max_code_bytes: DEFAULT_JIT_MAX_CODE_BYTES,
            jit_code_budget_used: 0,
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
            ("len", Builtin::Len),
            ("iter", Builtin::Iter),
            ("next", Builtin::Next),
            ("hash", Builtin::Hash),
            ("abs", Builtin::Abs),
            ("isinstance", Builtin::IsInstance),
            ("issubclass", Builtin::IsSubclass),
            ("getattr", Builtin::GetAttr),
            ("setattr", Builtin::SetAttr),
            ("delattr", Builtin::DelAttr),
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
        let object_init = vm.heap.alloc(Object::Builtin(Builtin::ObjectInit))?;
        let object_hash = vm.heap.alloc(Object::Builtin(Builtin::ObjectHash))?;
        let object_getattribute = vm
            .heap
            .alloc(Object::Builtin(Builtin::ObjectGetAttribute))?;
        let object_setattr = vm.heap.alloc(Object::Builtin(Builtin::ObjectSetAttr))?;
        let object_delattr = vm.heap.alloc(Object::Builtin(Builtin::ObjectDelAttr))?;
        let type_new = vm.heap.alloc(Object::Builtin(Builtin::TypeNew))?;
        let type_getattribute = vm.heap.alloc(Object::Builtin(Builtin::TypeGetAttribute))?;
        let type_setattr = vm.heap.alloc(Object::Builtin(Builtin::TypeSetAttr))?;
        let type_delattr = vm.heap.alloc(Object::Builtin(Builtin::TypeDelAttr))?;
        vm.object_class = vm.heap.root_object_class(
            object_new,
            object_init,
            object_hash,
            object_getattribute,
            object_setattr,
            object_delattr,
        )?;
        vm.type_class = vm.heap.root_type_class(
            vm.object_class,
            type_new,
            type_getattribute,
            type_setattr,
            type_delattr,
        )?;
        let int = vm
            .heap
            .builtin_class("int", vec![vm.object_class], vm.type_class)?;
        let base_exception =
            vm.heap
                .builtin_class("BaseException", vec![vm.object_class], vm.type_class)?;
        let exception = vm
            .heap
            .builtin_class("Exception", vec![base_exception], vm.type_class)?;
        let type_error = vm
            .heap
            .builtin_class("TypeError", vec![exception], vm.type_class)?;
        let value_error = vm
            .heap
            .builtin_class("ValueError", vec![exception], vm.type_class)?;
        let runtime_error =
            vm.heap
                .builtin_class("RuntimeError", vec![exception], vm.type_class)?;
        let stop_iteration =
            vm.heap
                .builtin_class("StopIteration", vec![exception], vm.type_class)?;
        let generator_exit =
            vm.heap
                .builtin_class("GeneratorExit", vec![base_exception], vm.type_class)?;
        let arithmetic_error =
            vm.heap
                .builtin_class("ArithmeticError", vec![exception], vm.type_class)?;
        let lookup_error = vm
            .heap
            .builtin_class("LookupError", vec![exception], vm.type_class)?;
        let name_error = vm
            .heap
            .builtin_class("NameError", vec![exception], vm.type_class)?;
        let import_error = vm
            .heap
            .builtin_class("ImportError", vec![exception], vm.type_class)?;
        let other_exceptions = vec![
            ("ArithmeticError".into(), arithmetic_error),
            (
                "OverflowError".into(),
                vm.heap
                    .builtin_class("OverflowError", vec![arithmetic_error], vm.type_class)?,
            ),
            (
                "ZeroDivisionError".into(),
                vm.heap.builtin_class(
                    "ZeroDivisionError",
                    vec![arithmetic_error],
                    vm.type_class,
                )?,
            ),
            ("LookupError".into(), lookup_error),
            (
                "IndexError".into(),
                vm.heap
                    .builtin_class("IndexError", vec![lookup_error], vm.type_class)?,
            ),
            (
                "KeyError".into(),
                vm.heap
                    .builtin_class("KeyError", vec![lookup_error], vm.type_class)?,
            ),
            ("NameError".into(), name_error),
            (
                "UnboundLocalError".into(),
                vm.heap
                    .builtin_class("UnboundLocalError", vec![name_error], vm.type_class)?,
            ),
            (
                "AttributeError".into(),
                vm.heap
                    .builtin_class("AttributeError", vec![exception], vm.type_class)?,
            ),
            ("ImportError".into(), import_error),
            (
                "ModuleNotFoundError".into(),
                vm.heap
                    .builtin_class("ModuleNotFoundError", vec![import_error], vm.type_class)?,
            ),
            (
                "RecursionError".into(),
                vm.heap
                    .builtin_class("RecursionError", vec![runtime_error], vm.type_class)?,
            ),
            (
                "OSError".into(),
                vm.heap
                    .builtin_class("OSError", vec![exception], vm.type_class)?,
            ),
            (
                "MemoryError".into(),
                vm.heap
                    .builtin_class("MemoryError", vec![exception], vm.type_class)?,
            ),
        ];
        vm.runtime_types = RuntimeTypes {
            none: vm
                .heap
                .builtin_class("NoneType", vec![vm.object_class], vm.type_class)?,
            not_implemented: vm.heap.builtin_class(
                "NotImplementedType",
                vec![vm.object_class],
                vm.type_class,
            )?,
            int,
            bool_: vm.heap.builtin_class("bool", vec![int], vm.type_class)?,
            float: vm
                .heap
                .builtin_class("float", vec![vm.object_class], vm.type_class)?,
            str_: vm
                .heap
                .builtin_class("str", vec![vm.object_class], vm.type_class)?,
            list: vm
                .heap
                .builtin_class("list", vec![vm.object_class], vm.type_class)?,
            tuple: vm
                .heap
                .builtin_class("tuple", vec![vm.object_class], vm.type_class)?,
            dict: vm
                .heap
                .builtin_class("dict", vec![vm.object_class], vm.type_class)?,
            range: vm
                .heap
                .builtin_class("range", vec![vm.object_class], vm.type_class)?,
            function: vm
                .heap
                .builtin_class("function", vec![vm.object_class], vm.type_class)?,
            generator: vm
                .heap
                .builtin_class("generator", vec![vm.object_class], vm.type_class)?,
            base_exception,
            exception,
            type_error,
            value_error,
            runtime_error,
            stop_iteration,
            generator_exit,
            other_exceptions,
        };
        for (class, name, builtin) in [
            (vm.runtime_types.int, "__new__", Builtin::IntNew),
            (vm.runtime_types.bool_, "__new__", Builtin::BoolNew),
            (vm.runtime_types.float, "__new__", Builtin::FloatNew),
            (vm.runtime_types.str_, "__new__", Builtin::StrNew),
            (vm.runtime_types.list, "__new__", Builtin::ListNew),
            (vm.runtime_types.list, "__init__", Builtin::ListInit),
            (vm.runtime_types.tuple, "__new__", Builtin::TupleNew),
            (vm.runtime_types.dict, "__new__", Builtin::DictNew),
            (vm.runtime_types.dict, "__init__", Builtin::DictInit),
            (vm.runtime_types.range, "__new__", Builtin::RangeNew),
            (
                vm.runtime_types.generator,
                "__iter__",
                Builtin::GeneratorIter,
            ),
            (
                vm.runtime_types.generator,
                "__next__",
                Builtin::GeneratorNext,
            ),
            (vm.runtime_types.generator, "send", Builtin::GeneratorSend),
            (vm.runtime_types.generator, "throw", Builtin::GeneratorThrow),
            (vm.runtime_types.generator, "close", Builtin::GeneratorClose),
        ] {
            let value = vm.heap.alloc(Object::Builtin(builtin))?;
            vm.heap.set_attr(class, name, value)?;
        }
        for (class, builtin) in [
            (vm.runtime_types.int, Builtin::IntHash),
            (vm.runtime_types.bool_, Builtin::IntHash),
            (vm.runtime_types.float, Builtin::FloatHash),
            (vm.runtime_types.str_, Builtin::StrHash),
            (vm.runtime_types.tuple, Builtin::TupleHash),
            (vm.runtime_types.range, Builtin::RangeHash),
        ] {
            let value = vm.heap.alloc(Object::Builtin(builtin))?;
            vm.heap.set_attr(class, "__hash__", value)?;
        }
        for class in [vm.runtime_types.list, vm.runtime_types.dict] {
            vm.heap.set_attr(class, "__hash__", Value::NONE)?;
        }
        vm.builtins.push(("object".into(), vm.object_class));
        vm.builtins.push(("type".into(), vm.type_class));
        for (name, value) in [
            ("int", vm.runtime_types.int),
            ("bool", vm.runtime_types.bool_),
            ("float", vm.runtime_types.float),
            ("str", vm.runtime_types.str_),
            ("list", vm.runtime_types.list),
            ("tuple", vm.runtime_types.tuple),
            ("dict", vm.runtime_types.dict),
            ("range", vm.runtime_types.range),
            ("NotImplemented", Value::NOT_IMPLEMENTED),
            ("BaseException", vm.runtime_types.base_exception),
            ("Exception", vm.runtime_types.exception),
            ("TypeError", vm.runtime_types.type_error),
            ("ValueError", vm.runtime_types.value_error),
            ("RuntimeError", vm.runtime_types.runtime_error),
            ("StopIteration", vm.runtime_types.stop_iteration),
            ("GeneratorExit", vm.runtime_types.generator_exit),
        ] {
            vm.builtins.push((name.into(), value));
        }
        vm.builtins
            .extend(vm.runtime_types.other_exceptions.iter().cloned());
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
        drain_deferred_handle_releases(&self.runtime_owner, &mut self.handles)
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
        self.pending_classes.clear();
        self.pending_exception = None;
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
                    error.filename = Some(module_filename(&program, frame.code).to_owned());
                }
                error.trace.push((code.name.clone(), span));
            }
            error
        });
        self.frames.clear();
        self.registers.clear();
        self.cells.clear();
        self.arguments.clear();
        self.pending_classes.clear();
        self.pending_exception = None;
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
        let pending_class_depth = self.pending_classes.len();
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
        self.pending_classes.truncate(pending_class_depth);
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
        let pending_class_depth = self.pending_classes.len();
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
        self.pending_classes.truncate(pending_class_depth);
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
        self.pending_classes.clear();
        self.pending_exception = None;
        self.globals.clear();
        self.constants.clear();
        self.jit_globals.clear();
        self.jit_registers.clear();
        self.jit_roots.clear();
        self.jit_cache.clear();
        self.active_program = None;
        self.source_modules.clear();
        self.source_module_names.clear();
        self.global_owners.clear();
        self.global_defined.clear();
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
        include_handles: bool,
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
        roots.extend(
            self.source_modules
                .iter()
                .filter_map(|module| module.object),
        );
        roots.extend(self.builtins.iter().map(|(_, v)| *v));
        roots.extend(self.runtime_types.roots());
        roots.extend(self.pending_exception);
        roots.extend(self.frames.iter().filter_map(|frame| frame.callable));
        roots.extend(self.frames.iter().filter_map(|frame| frame.namespace));
        roots.extend(self.frames.iter().filter_map(|frame| frame.yield_from));
        roots.extend(
            self.frames
                .iter()
                .filter_map(|frame| frame.injected_exception),
        );
        roots.extend(
            self.frames
                .iter()
                .flat_map(|frame| frame.exception_stack.iter().copied()),
        );
        for frame in &self.frames {
            frame.action.trace(|value| roots.push(value));
        }
        for args in &self.arguments {
            args.trace(|value| roots.push(value));
        }
        for pending in &self.pending_classes {
            pending.state.trace(|value| roots.push(value));
        }
        if include_handles {
            self.handles.roots(|value| roots.push(value));
        }
    }
    pub fn collect_garbage(&mut self) -> Result<crate::CollectionStats> {
        self.ensure_running()?;
        self.drain_deferred_persistent_releases()?;
        let start = std::time::Instant::now();
        self.stats.foreign_trace_calls +=
            self.heap.refresh_foreign_references(&mut self.handles)?;
        self.drain_deferred_persistent_releases()?;
        let mut roots = Vec::new();
        self.append_gc_roots(None, true, &mut roots);
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
        self.stats.foreign_trace_calls +=
            self.heap.refresh_foreign_references(&mut self.handles)?;
        self.drain_deferred_persistent_releases()?;
        let mut roots = Vec::new();
        self.append_gc_roots(None, true, &mut roots);
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
            + self
                .source_modules
                .iter()
                .filter(|module| module.object.is_some())
                .count()
            + self.builtins.len()
            + self.runtime_types.roots().len()
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
        roots += self
            .pending_classes
            .iter()
            .map(|pending| {
                let mut count = 0;
                pending.state.trace(|_| count += 1);
                count
            })
            .sum::<usize>();
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
        self.pending_classes.clear();
        self.pending_exception = None;
        self.constants.clear();
        self.globals.clear();
        self.source_modules.clear();
        self.source_module_names.clear();
        self.global_owners.clear();
        self.global_defined.clear();
        self.jit_cache = (0..program.code.len()).map(|_| JitEntry::Untried).collect();
        self.jit_code_budget_used = 0;
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
        self.global_owners.resize(program.symbols.len(), None);
        self.global_defined.resize(program.symbols.len(), false);
        for (module_id, module) in program.modules.iter().enumerate() {
            if self.modules.contains_key(&module.name) {
                return Err(Diagnostic::new(
                    "ImportError",
                    format!(
                        "source module '{}' conflicts with a native module",
                        module.name
                    ),
                ));
            }
            self.source_module_names
                .insert(module.name.clone(), module_id);
            self.source_modules.push(SourceModuleRuntime {
                object: None,
                state: if module_id == 0 {
                    SourceModuleState::Initializing
                } else {
                    SourceModuleState::Uninitialized
                },
                version: 0,
            });
            for symbol in &module.globals {
                let index = usize::from(symbol.0);
                self.global_owners[index] = Some(module_id);
                match program.symbols[index].as_str() {
                    "__name__" => {
                        self.globals[index] = self.heap.alloc(Object::Str(module.name.clone()))?;
                        self.global_defined[index] = true;
                    }
                    "__file__" => {
                        self.globals[index] =
                            self.heap.alloc(Object::Str(module.filename.clone()))?;
                        self.global_defined[index] = true;
                    }
                    _ => {}
                }
            }
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
        if result.is_ok() {
            if let Some(module) = self.source_modules.first_mut() {
                module.state = SourceModuleState::Loaded;
            }
        }
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
                    e.filename = Some(module_filename(program, frame.code).to_owned());
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
        self.pending_classes.clear();
        self.pending_exception = None;
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
    fn store_global(&mut self, program: &Program, index: usize, value: Value) -> Result<()> {
        self.globals[index] = value;
        self.jit_globals[index] = value.raw();
        self.global_defined[index] = true;
        if let Some(module_id) = self.global_owners[index] {
            let name = &program.symbols[index];
            let module = &mut self.source_modules[module_id];
            if let Some(object) = module.object {
                self.heap.add_module_member(object, name, value)?;
            }
            module.version = module.version.checked_add(1).ok_or_else(|| {
                Diagnostic::new("RuntimeError", "module global version exhausted")
            })?;
        }
        Ok(())
    }
    fn clear_global(&mut self, program: &Program, index: usize) -> Result<()> {
        self.globals[index] = Value::UNBOUND;
        self.jit_globals[index] = Value::UNBOUND.raw();
        self.global_defined[index] = false;
        if let Some(module_id) = self.global_owners[index] {
            let name = &program.symbols[index];
            let module = &mut self.source_modules[module_id];
            if let Some(object) = module.object {
                if self.heap.attr(object, name).is_ok() {
                    self.heap.delete_module_member(object, name)?;
                }
            }
            module.version = module.version.checked_add(1).ok_or_else(|| {
                Diagnostic::new("RuntimeError", "module global version exhausted")
            })?;
        }
        Ok(())
    }
    pub fn module_version(&self, name: &str) -> Option<u64> {
        self.source_module_names
            .get(name)
            .map(|module| self.source_modules[*module].version)
    }
    pub(super) fn store_module_attribute(
        &mut self,
        program: &Program,
        owner: Value,
        name: &str,
        value: Value,
    ) -> Result<bool> {
        let Some(module_id) = self
            .source_modules
            .iter()
            .position(|module| module.object == Some(owner))
        else {
            return Ok(false);
        };
        let slot = program.modules[module_id]
            .globals
            .iter()
            .map(|symbol| usize::from(symbol.0))
            .find(|index| program.symbols[*index] == name);
        if let Some(index) = slot {
            self.store_global(program, index, value)?;
        } else {
            let module = &mut self.source_modules[module_id];
            self.heap.add_module_member(owner, name, value)?;
            module.version = module.version.checked_add(1).ok_or_else(|| {
                Diagnostic::new("RuntimeError", "module global version exhausted")
            })?;
        }
        Ok(true)
    }
    pub(super) fn delete_module_attribute(
        &mut self,
        program: &Program,
        owner: Value,
        name: &str,
    ) -> Result<bool> {
        let Some(module_id) = self
            .source_modules
            .iter()
            .position(|module| module.object == Some(owner))
        else {
            return Ok(false);
        };
        let slot = program.modules[module_id]
            .globals
            .iter()
            .map(|symbol| usize::from(symbol.0))
            .find(|index| program.symbols[*index] == name);
        if let Some(index) = slot {
            self.clear_global(program, index)?;
        } else {
            self.heap.delete_module_member(owner, name)?;
            let module = &mut self.source_modules[module_id];
            module.version = module.version.checked_add(1).ok_or_else(|| {
                Diagnostic::new("RuntimeError", "module global version exhausted")
            })?;
        }
        Ok(true)
    }
    fn ensure_source_module_object(
        &mut self,
        program: &Program,
        module_id: usize,
    ) -> Result<Value> {
        if let Some(object) = self.source_modules[module_id].object {
            return Ok(object);
        }
        let metadata = &program.modules[module_id];
        let object = self.heap.alloc(Object::Module(Vec::new()))?;
        // Root the object before allocating metadata strings or copying members.
        self.source_modules[module_id].object = Some(object);
        let name_slot = metadata
            .globals
            .iter()
            .map(|symbol| usize::from(symbol.0))
            .find(|index| program.symbols[*index] == "__name__");
        let file_slot = metadata
            .globals
            .iter()
            .map(|symbol| usize::from(symbol.0))
            .find(|index| program.symbols[*index] == "__file__");
        let name = name_slot
            .map(|index| self.globals[index])
            .filter(|value| *value != Value::UNBOUND)
            .map_or_else(|| self.heap.alloc(Object::Str(metadata.name.clone())), Ok)?;
        self.heap.add_module_member(object, "__name__", name)?;
        let filename = file_slot
            .map(|index| self.globals[index])
            .filter(|value| *value != Value::UNBOUND)
            .map_or_else(
                || self.heap.alloc(Object::Str(metadata.filename.clone())),
                Ok,
            )?;
        self.heap.add_module_member(object, "__file__", filename)?;
        for symbol in &metadata.globals {
            let index = usize::from(symbol.0);
            let value = self.globals[index];
            let name = &program.symbols[index];
            if self.global_defined[index]
                && value != Value::UNBOUND
                && !matches!(name.as_str(), "__name__" | "__file__")
            {
                self.heap.add_module_member(object, name, value)?;
            }
        }
        Ok(object)
    }
    fn invoke_import(&mut self, program: &Program, name: &str, destination: usize) -> Result<()> {
        if let Some(module_id) = self.source_module_names.get(name).copied() {
            let object = self.ensure_source_module_object(program, module_id)?;
            let module = &self.source_modules[module_id];
            if matches!(
                module.state,
                SourceModuleState::Initializing | SourceModuleState::Loaded
            ) {
                self.attach_imported_module(program, name, object)?;
                self.registers[destination] = object;
                return Ok(());
            }
            self.source_modules[module_id].state = SourceModuleState::Initializing;
            let code = usize::from(program.modules[module_id].code);
            let result = self.enter_frame(
                program,
                code,
                Some(destination),
                Arguments::Direct {
                    receiver: None,
                    first: 0,
                    count: 0,
                    keywords: &[],
                },
                None,
            );
            if result.is_err() {
                self.source_modules[module_id].state = SourceModuleState::Uninitialized;
                return result;
            }
            self.frames.last_mut().expect("import module frame").action =
                ReturnAction::Import(module_id);
            return Ok(());
        }
        let object = *self.modules.get(name).ok_or_else(|| {
            Diagnostic::new("ModuleNotFoundError", format!("no module named '{name}'"))
        })?;
        self.attach_imported_module(program, name, object)?;
        self.registers[destination] = object;
        Ok(())
    }
    fn invoke_import_from(
        &mut self,
        program: &Program,
        module: Value,
        name: &str,
        destination: usize,
    ) -> Result<()> {
        match self.heap.attr(module, name) {
            Ok(value) => {
                self.registers[destination] = value;
                return Ok(());
            }
            Err(error) if error.kind == "AttributeError" => {}
            Err(error) => return Err(error),
        }
        let base = self
            .source_modules
            .iter()
            .position(|source| source.object == Some(module))
            .map(|module_id| program.modules[module_id].name.as_str())
            .or_else(|| {
                self.modules
                    .iter()
                    .find_map(|(candidate, value)| (*value == module).then_some(candidate.as_str()))
            })
            .ok_or_else(|| Diagnostic::new("ImportError", "import source is not a module"))?;
        let child = format!("{base}.{name}");
        if !self.source_module_names.contains_key(&child) && !self.modules.contains_key(&child) {
            return Err(Diagnostic::new(
                "ImportError",
                format!("cannot import name '{name}' from '{base}'"),
            ));
        }
        self.invoke_import(program, &child, destination)
    }
    fn attach_imported_module(
        &mut self,
        program: &Program,
        name: &str,
        object: Value,
    ) -> Result<()> {
        let Some((parent_name, child_name)) = name.rsplit_once('.') else {
            return Ok(());
        };
        let (parent, source_parent) =
            if let Some(module_id) = self.source_module_names.get(parent_name).copied() {
                (
                    self.ensure_source_module_object(program, module_id)?,
                    Some(module_id),
                )
            } else if let Some(parent) = self.modules.get(parent_name).copied() {
                (parent, None)
            } else {
                return Ok(());
            };
        if self.heap.attr(parent, child_name).ok() == Some(object) {
            return Ok(());
        }
        self.heap.add_module_member(parent, child_name, object)?;
        if let Some(module_id) = source_parent {
            let module = &mut self.source_modules[module_id];
            module.version = module.version.checked_add(1).ok_or_else(|| {
                Diagnostic::new("RuntimeError", "module global version exhausted")
            })?;
            if let Some(index) = program.modules[module_id]
                .globals
                .iter()
                .map(|symbol| usize::from(symbol.0))
                .find(|index| program.symbols[*index] == child_name)
            {
                self.globals[index] = object;
                self.jit_globals[index] = object.raw();
                self.global_defined[index] = true;
            }
        }
        Ok(())
    }
    fn reset_import_frames(&mut self, program: &Program, start: usize) -> Result<()> {
        let mut modules = self.frames[start..]
            .iter()
            .filter_map(|frame| match frame.action {
                ReturnAction::Import(module_id) => Some(module_id),
                _ => None,
            })
            .collect::<Vec<_>>();
        modules.sort_unstable();
        modules.dedup();
        for module_id in modules.into_iter().rev() {
            self.reset_source_module(program, module_id)?;
        }
        Ok(())
    }
    fn reset_source_module(&mut self, program: &Program, module_id: usize) -> Result<()> {
        let metadata = &program.modules[module_id];
        let object = self.source_modules[module_id].object;
        if let (Some(object), Some((parent_name, child_name))) =
            (object, metadata.name.rsplit_once('.'))
        {
            let parent = self
                .source_module_names
                .get(parent_name)
                .and_then(|parent_id| self.source_modules[*parent_id].object)
                .or_else(|| self.modules.get(parent_name).copied());
            if let Some(parent) = parent {
                if self.heap.attr(parent, child_name).ok() == Some(object) {
                    self.heap.delete_module_member(parent, child_name)?;
                    if let Some(parent_id) = self.source_module_names.get(parent_name).copied() {
                        if let Some(index) = program.modules[parent_id]
                            .globals
                            .iter()
                            .map(|symbol| usize::from(symbol.0))
                            .find(|index| program.symbols[*index] == child_name)
                        {
                            self.globals[index] = Value::UNBOUND;
                            self.jit_globals[index] = Value::UNBOUND.raw();
                            self.global_defined[index] = false;
                        }
                        self.source_modules[parent_id].version = self.source_modules[parent_id]
                            .version
                            .checked_add(1)
                            .ok_or_else(|| {
                                Diagnostic::new("RuntimeError", "module global version exhausted")
                            })?;
                    }
                }
            }
        }
        if let Some(object) = object {
            self.heap.reset_module_members(object)?;
        }
        for symbol in &metadata.globals {
            let index = usize::from(symbol.0);
            self.globals[index] = match program.symbols[index].as_str() {
                "__name__" => match object {
                    Some(object) => self.heap.attr(object, "__name__")?,
                    None => self.heap.alloc(Object::Str(metadata.name.clone()))?,
                },
                "__file__" => match object {
                    Some(object) => self.heap.attr(object, "__file__")?,
                    None => self.heap.alloc(Object::Str(metadata.filename.clone()))?,
                },
                name => self
                    .builtins
                    .iter()
                    .find_map(|(builtin, value)| (builtin == name).then_some(*value))
                    .unwrap_or(Value::UNBOUND),
            };
            self.jit_globals[index] = self.globals[index].raw();
            self.global_defined[index] =
                matches!(program.symbols[index].as_str(), "__name__" | "__file__");
        }
        let module = &mut self.source_modules[module_id];
        module.state = SourceModuleState::Uninitialized;
        module.version = module
            .version
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("RuntimeError", "module global version exhausted"))?;
        Ok(())
    }
    pub(super) fn builtin_type_kind(&self, class: Value) -> Option<RuntimeTypeKind> {
        [
            (self.runtime_types.int, RuntimeTypeKind::Int),
            (self.runtime_types.bool_, RuntimeTypeKind::Bool),
            (self.runtime_types.float, RuntimeTypeKind::Float),
            (self.runtime_types.str_, RuntimeTypeKind::Str),
            (self.runtime_types.list, RuntimeTypeKind::List),
            (self.runtime_types.tuple, RuntimeTypeKind::Tuple),
            (self.runtime_types.dict, RuntimeTypeKind::Dict),
            (self.runtime_types.range, RuntimeTypeKind::Range),
            (
                self.runtime_types.base_exception,
                RuntimeTypeKind::Exception,
            ),
            (self.runtime_types.exception, RuntimeTypeKind::Exception),
            (self.runtime_types.type_error, RuntimeTypeKind::Exception),
            (self.runtime_types.value_error, RuntimeTypeKind::Exception),
            (self.runtime_types.runtime_error, RuntimeTypeKind::Exception),
            (
                self.runtime_types.stop_iteration,
                RuntimeTypeKind::Exception,
            ),
        ]
        .into_iter()
        .find_map(|(candidate, kind)| (candidate == class).then_some(kind))
        .or_else(|| {
            self.runtime_types
                .other_exceptions
                .iter()
                .any(|(_, candidate)| *candidate == class)
                .then_some(RuntimeTypeKind::Exception)
        })
    }
    pub(super) fn builtin_subclass_kind(&self, class: Value) -> Option<RuntimeTypeKind> {
        let mro = &self.heap.class(class).ok()?.mro;
        [
            (self.runtime_types.int, RuntimeTypeKind::Int),
            (self.runtime_types.float, RuntimeTypeKind::Float),
            (self.runtime_types.str_, RuntimeTypeKind::Str),
            (self.runtime_types.list, RuntimeTypeKind::List),
            (self.runtime_types.tuple, RuntimeTypeKind::Tuple),
            (self.runtime_types.dict, RuntimeTypeKind::Dict),
        ]
        .into_iter()
        .find_map(|(candidate, kind)| mro.contains(&candidate).then_some(kind))
    }
    fn validate_builtin_subclass_bases(&self, bases: &[Value]) -> Result<()> {
        let mut storage = None;
        for base in bases {
            // Invalid non-class bases are diagnosed by ordinary class
            // finalization after the body has run, preserving Python's
            // observable class-body evaluation order.
            let Ok(class) = self.heap.class(*base) else {
                continue;
            };
            let inherits = |target| *base == target || class.mro.contains(&target);
            if inherits(self.runtime_types.bool_) || inherits(self.runtime_types.range) {
                return Err(Diagnostic::new(
                    "TypeError",
                    format!("type '{}' is not an acceptable base type", class.name),
                ));
            }
            let kind = [
                (self.runtime_types.int, RuntimeTypeKind::Int),
                (self.runtime_types.float, RuntimeTypeKind::Float),
                (self.runtime_types.str_, RuntimeTypeKind::Str),
                (self.runtime_types.list, RuntimeTypeKind::List),
                (self.runtime_types.tuple, RuntimeTypeKind::Tuple),
                (self.runtime_types.dict, RuntimeTypeKind::Dict),
            ]
            .into_iter()
            .find_map(|(target, kind)| inherits(target).then_some(kind));
            if let Some(kind) = kind {
                if storage.is_some_and(|existing| existing != kind) {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "multiple bases have incompatible native instance layouts",
                    ));
                }
                storage = Some(kind);
            }
        }
        Ok(())
    }
    pub(super) fn runtime_class(&self, value: Value) -> Result<Value> {
        if value.as_bool().is_some() {
            return Ok(self.runtime_types.bool_);
        }
        if value == Value::NONE {
            return Ok(self.runtime_types.none);
        }
        if value == Value::NOT_IMPLEMENTED {
            return Ok(self.runtime_types.not_implemented);
        }
        if value.integer().is_some() {
            return Ok(self.runtime_types.int);
        }
        Ok(match self.heap.get(value)? {
            Object::Class(class) if class.metaclass != Value::UNBOUND => class.metaclass,
            Object::Instance { class, .. } => *class,
            Object::Int(_) => self.runtime_types.int,
            Object::Float(_) => self.runtime_types.float,
            Object::Str(_) => self.runtime_types.str_,
            Object::List(_) => self.runtime_types.list,
            Object::Tuple(_) => self.runtime_types.tuple,
            Object::Dict(_) => self.runtime_types.dict,
            Object::Range { .. } => self.runtime_types.range,
            Object::Function { .. } => self.runtime_types.function,
            Object::Generator(frame) => frame.class,
            Object::Exception { class, .. } => *class,
            _ => self.object_class,
        })
    }
    fn exception_diagnostic(&self, value: Value) -> Result<Diagnostic> {
        let (class, message) = match self.heap.get(value) {
            Ok(Object::Exception { class, message, .. }) => (*class, message.clone()),
            Ok(Object::Instance { class, .. })
                if self.instance_check(value, self.runtime_types.base_exception, false, 0)? =>
            {
                (*class, String::new())
            }
            Ok(Object::Class(_))
                if self.instance_check(value, self.runtime_types.base_exception, true, 0)? =>
            {
                (value, String::new())
            }
            _ => {
                return Ok(Diagnostic::new(
                    "TypeError",
                    "exceptions must derive from BaseException",
                ))
            }
        };
        Ok(Diagnostic::new(
            self.heap.class(class)?.name.clone(),
            message,
        ))
    }
    fn normalize_raised_exception(&mut self, value: Value) -> Result<Value> {
        match self.heap.get(value) {
            Ok(Object::Exception { .. }) => return Ok(value),
            Ok(Object::Instance { .. })
                if self.instance_check(value, self.runtime_types.base_exception, false, 0)? =>
            {
                return Ok(value)
            }
            Ok(Object::Class(_))
                if self.instance_check(value, self.runtime_types.base_exception, true, 0)? =>
            {
                let stop_iteration_value = self
                    .instance_check(value, self.runtime_types.stop_iteration, true, 0)?
                    .then_some(Value::NONE);
                if self.builtin_type_kind(value) == Some(RuntimeTypeKind::Exception) {
                    return self.heap.alloc(Object::Exception {
                        class: value,
                        message: String::new(),
                        arguments: Vec::new(),
                        stop_iteration_value,
                        attributes: Default::default(),
                        cause: None,
                        context: None,
                        suppress_context: false,
                        traceback: None,
                    });
                }
                return self.heap.alloc(Object::Exception {
                    class: value,
                    message: String::new(),
                    arguments: Vec::new(),
                    stop_iteration_value,
                    attributes: Default::default(),
                    cause: None,
                    context: None,
                    suppress_context: false,
                    traceback: None,
                });
            }
            _ => {}
        }
        Err(Diagnostic::new(
            "TypeError",
            "exceptions must derive from BaseException",
        ))
    }
    fn validate_iterator(&self, value: Value) -> Result<()> {
        if self.heap.is_iterator(value)
            || self.heap.is_generator(value)
            || self.heap.special_method_call(value, "__next__")?.is_some()
        {
            Ok(())
        } else {
            Err(Diagnostic::new(
                "TypeError",
                "__iter__ returned a non-iterator",
            ))
        }
    }

    fn freeze_generator_call(&mut self, program: &Program, destination: usize) -> Result<()> {
        let frame = self.frames.pop().ok_or_else(|| {
            Diagnostic::new("BytecodeError", "generator call did not create a frame")
        })?;
        let metadata = &program.code[frame.code];
        if !metadata.generator || frame.generator.is_some() {
            return Err(Diagnostic::new(
                "BytecodeError",
                "invalid generator creation frame",
            ));
        }
        let registers = self.registers.split_off(frame.base);
        let cells = self.cells.split_off(frame.cell_base);
        self.arguments.truncate(frame.argument_base);
        self.pending_classes.truncate(frame.pending_class_base);
        let generator = self.heap.alloc(Object::Generator(GeneratorFrame {
            class: self.runtime_types.generator,
            execution: self.execution,
            code: frame.code as u16,
            ip: 0,
            registers,
            cells,
            exception_stack: frame.exception_stack,
            resume_register: None,
            yield_from: None,
            return_value: Value::NONE,
            state: GeneratorState::Created,
        }))?;
        self.registers[destination] = generator;
        Ok(())
    }

    fn resume_generator(
        &mut self,
        program: &Program,
        generator: Value,
        destination: usize,
        sent: Value,
        action: ReturnAction,
    ) -> Result<()> {
        if self.frames.len() >= self.limits.frames {
            return Err(Diagnostic::new(
                "RecursionError",
                "maximum call depth exceeded",
            ));
        }
        match self.heap.generator_state(generator) {
            Some(GeneratorState::Completed) => {
                return Err(Diagnostic::new("StopIteration", String::new()))
            }
            Some(GeneratorState::Running) => {
                return Err(Diagnostic::new("ValueError", "generator already executing"))
            }
            Some(GeneratorState::Created) if sent != Value::NONE => {
                return Err(Diagnostic::new(
                    "TypeError",
                    "cannot send non-None value to a just-started generator",
                ))
            }
            Some(GeneratorState::Created | GeneratorState::Suspended) => {}
            None => return Err(Diagnostic::new("TypeError", "object is not a generator")),
        }
        let (code, register_count, cell_count) = match self.heap.get(generator)? {
            Object::Generator(frame) => (
                usize::from(frame.code),
                frame.registers.len(),
                frame.cells.len(),
            ),
            _ => return Err(Diagnostic::new("TypeError", "object is not a generator")),
        };
        if code >= program.code.len()
            || !program.code[code].generator
            || register_count != usize::from(program.code[code].registers)
            || cell_count
                != program.code[code].cell_locals.len() + program.code[code].free_vars.len()
        {
            return Err(Diagnostic::new(
                "BytecodeError",
                "invalid suspended generator frame",
            ));
        }
        let base = self.registers.len();
        let end = base
            .checked_add(register_count)
            .ok_or_else(|| Diagnostic::new("MemoryError", "register size overflow"))?;
        if end > self.limits.registers {
            return Err(Diagnostic::new("MemoryError", "register budget exceeded"));
        }
        let mut resumed = self.heap.resume_generator(generator, self.execution)?;
        debug_assert_eq!(usize::from(resumed.code), code);
        if let Some(register) = resumed.resume_register {
            resumed.registers[usize::from(register)] = sent;
        }
        let cell_base = self.cells.len();
        self.registers.extend(resumed.registers);
        self.cells.extend(resumed.cells);
        self.frames.push(Frame {
            namespace: None,
            action,
            code,
            ip: resumed.ip,
            base,
            destination: Some(destination),
            cell_base,
            argument_base: self.arguments.len(),
            pending_class_base: self.pending_classes.len(),
            callable: None,
            generator: Some(generator),
            yield_from: resumed.yield_from,
            injected_exception: None,
            exception_stack: resumed.exception_stack,
            jit_attempted: true,
            jit_resume: false,
            jit_expanded_resume_depth: None,
        });
        self.stats.peak_registers = self.stats.peak_registers.max(end);
        Ok(())
    }

    fn suspend_active_generator(&mut self, program: &Program) -> Result<()> {
        let frame = self.frames.last().ok_or_else(|| {
            Diagnostic::new("BytecodeError", "yield has no active generator frame")
        })?;
        let generator = frame
            .generator
            .ok_or_else(|| Diagnostic::new("BytecodeError", "yield outside resumed generator"))?;
        let metadata = &program.code[frame.code];
        let registers =
            self.registers[frame.base..frame.base + usize::from(metadata.registers)].to_vec();
        let cells = self.cells[frame.cell_base
            ..frame.cell_base + metadata.cell_locals.len() + metadata.free_vars.len()]
            .to_vec();
        self.heap.suspend_generator(
            generator,
            SuspendedGenerator {
                ip: frame.ip,
                registers,
                cells,
                exception_stack: frame.exception_stack.clone(),
                resume_register: program.code[frame.code].instructions[frame.ip - 1].a,
                yield_from: frame.yield_from,
            },
        )
    }
    fn exception_from_diagnostic(&mut self, error: &Diagnostic) -> Result<Value> {
        let class = match error.kind.as_str() {
            "TypeError" => self.runtime_types.type_error,
            "ValueError" => self.runtime_types.value_error,
            "StopIteration" => self.runtime_types.stop_iteration,
            "GeneratorExit" => self.runtime_types.generator_exit,
            "RuntimeError" => self.runtime_types.runtime_error,
            name => self
                .runtime_types
                .other_exceptions
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, value)| *value)
                .unwrap_or(self.runtime_types.runtime_error),
        };
        let message = error.message.clone();
        let stop_iteration = class == self.runtime_types.stop_iteration;
        let arguments = if stop_iteration && message.is_empty() {
            Vec::new()
        } else {
            vec![self.heap.alloc(Object::Str(message.clone()))?]
        };
        let stop_iteration_value =
            stop_iteration.then_some(arguments.first().copied().unwrap_or(Value::NONE));
        self.heap.alloc(Object::Exception {
            class,
            message,
            arguments,
            stop_iteration_value,
            attributes: Default::default(),
            cause: None,
            context: None,
            suppress_context: false,
            traceback: None,
        })
    }

    fn generator_stop_iteration(&mut self, value: Value) -> Result<Diagnostic> {
        let arguments = if value == Value::NONE {
            Vec::new()
        } else {
            vec![value]
        };
        let message = self.heap.exception_message(&arguments)?;
        let exception = self.heap.alloc(Object::Exception {
            class: self.runtime_types.stop_iteration,
            message: message.clone(),
            arguments,
            stop_iteration_value: Some(value),
            attributes: Default::default(),
            cause: None,
            context: None,
            suppress_context: false,
            traceback: None,
        })?;
        self.pending_exception = Some(exception);
        Ok(Diagnostic::new("StopIteration", message))
    }
    fn dispatch_exception(
        &mut self,
        program: &Program,
        output: &mut dyn Write,
        error: &Diagnostic,
        minimum_depth: usize,
        current_code: usize,
        current_pc: usize,
    ) -> Result<bool> {
        let exception = match self.pending_exception.take() {
            Some(value) => value,
            None => self.exception_from_diagnostic(error)?,
        };
        let active_context = self
            .frames
            .iter()
            .rev()
            .find_map(|frame| frame.exception_stack.last().copied())
            .filter(|active| *active != exception);
        if let Some(context) = active_context {
            if matches!(self.heap.get(exception), Ok(Object::Exception { .. })) {
                self.heap.set_exception_context(exception, context)?;
            }
        }
        let last = self.frames.len().saturating_sub(1);
        let stop_iteration =
            self.instance_check(exception, self.runtime_types.stop_iteration, false, 0)?;
        let generator_exit =
            self.instance_check(exception, self.runtime_types.generator_exit, false, 0)?;
        let key_error_class = self
            .runtime_types
            .other_exceptions
            .iter()
            .find(|(name, _)| name == "KeyError")
            .map(|(_, class)| *class);
        let key_error = match key_error_class {
            Some(class) => self.instance_check(exception, class, false, 0)?,
            None => false,
        };
        let attribute_error_class = self
            .runtime_types
            .other_exceptions
            .iter()
            .find(|(name, _)| name == "AttributeError")
            .map(|(_, class)| *class);
        let attribute_error = match attribute_error_class {
            Some(class) => self.instance_check(exception, class, false, 0)?,
            None => false,
        };
        let mut selected = None;
        let mut traceback = Vec::new();
        for frame_index in (minimum_depth..self.frames.len()).rev() {
            let frame = &self.frames[frame_index];
            let fault_pc = if frame_index == last && frame.code == current_code {
                current_pc
            } else {
                frame.ip.saturating_sub(1)
            };
            let code = &program.code[frame.code];
            traceback.push(TracebackEntry {
                function: code.name.clone(),
                span: code.spans[fault_pc],
            });
            let completed_generator = frame.generator.is_some_and(|generator| {
                self.heap.generator_state(generator) == Some(GeneratorState::Completed)
            });
            let region = if completed_generator {
                None
            } else {
                code.exception_regions
                    .iter()
                    .filter(|region| {
                        usize::from(region.start) <= fault_pc && fault_pc < usize::from(region.end)
                    })
                    .min_by_key(|region| region.end - region.start)
                    .copied()
            };
            if let Some(region) = region {
                selected = Some((frame_index, region));
                break;
            }
            if stop_iteration && frame.generator.is_some() && !completed_generator {
                let generator = frame.generator.expect("checked generator frame");
                self.heap.complete_generator(generator)?;
                let converted = Diagnostic::new("RuntimeError", "generator raised StopIteration");
                let converted_exception = self.exception_from_diagnostic(&converted)?;
                self.heap
                    .set_exception_cause(converted_exception, Some(exception), true)?;
                self.pending_exception = Some(converted_exception);
                if self.dispatch_exception(
                    program,
                    output,
                    &converted,
                    minimum_depth,
                    current_code,
                    current_pc,
                )? {
                    return Ok(true);
                }
                return Err(converted);
            }
            if let Some(generator) = frame.generator {
                if !completed_generator {
                    self.heap.complete_generator(generator)?;
                }
            }
            if key_error {
                if let ReturnAction::NamespaceLookup {
                    target,
                    symbol,
                    cell,
                } = &frame.action
                {
                    let (target, symbol, cell) = (*target, *symbol, *cell);
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    let fallback = if let Some(cell) = cell {
                        self.heap.cell(cell)?
                    } else {
                        self.globals[usize::from(symbol)]
                    };
                    if fallback != Value::UNBOUND {
                        self.registers[target] = fallback;
                        return Ok(true);
                    }
                    let name = &program.symbols[usize::from(symbol)];
                    let error =
                        Diagnostic::new("NameError", format!("name '{name}' is not defined"));
                    let caller = self.frames.last().ok_or_else(|| {
                        Diagnostic::new("BytecodeError", "namespace lookup lost caller frame")
                    })?;
                    return self.dispatch_exception(
                        program,
                        output,
                        &error,
                        minimum_depth,
                        caller.code,
                        caller.ip.saturating_sub(1),
                    );
                }
            }
            if attribute_error {
                if let ReturnAction::AttributeGet(state) = &frame.action {
                    let state = state.clone();
                    let fallback = if state.phase == AttributePhase::Primary {
                        if matches!(self.heap.get(state.owner), Ok(Object::Class(_))) {
                            self.heap
                                .metaclass_method_call(state.owner, "__getattr__")?
                        } else {
                            self.heap.special_method_call(state.owner, "__getattr__")?
                        }
                    } else {
                        None
                    };
                    let consumable =
                        fallback.is_some() || !matches!(state.missing, AttributeMissing::Raise);
                    if consumable {
                        let destination = frame.destination.ok_or_else(|| {
                            Diagnostic::new(
                                "BytecodeError",
                                "attribute continuation has no destination",
                            )
                        })?;
                        let unwind = (
                            frame.base,
                            frame.cell_base,
                            frame.argument_base,
                            frame.pending_class_base,
                        );
                        self.frames.truncate(frame_index);
                        self.registers.truncate(unwind.0);
                        self.cells.truncate(unwind.1);
                        self.arguments.truncate(unwind.2);
                        self.pending_classes.truncate(unwind.3);
                        let result = self.continue_attribute_missing(
                            program,
                            destination,
                            state,
                            error.clone(),
                            output,
                        );
                        if let Err(next_error) = result {
                            let caller = self.frames.last().ok_or_else(|| {
                                Diagnostic::new(
                                    "BytecodeError",
                                    "attribute continuation lost caller frame",
                                )
                            })?;
                            return self.dispatch_exception(
                                program,
                                output,
                                &next_error,
                                minimum_depth,
                                caller.code,
                                caller.ip.saturating_sub(1),
                            );
                        }
                        return Ok(true);
                    }
                }
            }
            if stop_iteration || generator_exit {
                if let ReturnAction::YieldFromClose {
                    exception: outer_exception,
                } = frame.action
                {
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    let caller = self.frames.last().ok_or_else(|| {
                        Diagnostic::new("BytecodeError", "yield-from close lost outer frame")
                    })?;
                    let code = caller.code;
                    let pc = caller.ip.saturating_sub(1);
                    let diagnostic = self.exception_diagnostic(outer_exception)?;
                    self.pending_exception = Some(outer_exception);
                    if self.dispatch_exception(
                        program,
                        output,
                        &diagnostic,
                        minimum_depth,
                        code,
                        pc,
                    )? {
                        return Ok(true);
                    }
                    return Err(diagnostic);
                }
            }
            if (stop_iteration || generator_exit) && matches!(&frame.action, ReturnAction::Close) {
                let destination = frame.destination.ok_or_else(|| {
                    Diagnostic::new("BytecodeError", "close continuation has no destination")
                })?;
                let unwind = (
                    frame.base,
                    frame.cell_base,
                    frame.argument_base,
                    frame.pending_class_base,
                );
                self.frames.truncate(frame_index);
                self.registers.truncate(unwind.0);
                self.cells.truncate(unwind.1);
                self.arguments.truncate(unwind.2);
                self.pending_classes.truncate(unwind.3);
                self.registers[destination] = Value::NONE;
                return Ok(true);
            }
            if stop_iteration {
                if let ReturnAction::Next(Some(default)) = &frame.action {
                    let default = *default;
                    let destination = frame.destination.ok_or_else(|| {
                        Diagnostic::new("BytecodeError", "next continuation has no destination")
                    })?;
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    self.registers[destination] = default;
                    return Ok(true);
                }
                if let ReturnAction::DictIterableNext(state) = &frame.action {
                    let destination = frame.destination.ok_or_else(|| {
                        Diagnostic::new(
                            "BytecodeError",
                            "dict iterable continuation has no destination",
                        )
                    })?;
                    let state = state.clone();
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    if let Err(error) =
                        self.finish_dict_construction(program, destination, state.start, output)
                    {
                        let caller = self.frames.last().ok_or_else(|| {
                            Diagnostic::new("BytecodeError", "dict iterable lost caller frame")
                        })?;
                        return self.dispatch_exception(
                            program,
                            output,
                            &error,
                            minimum_depth,
                            caller.code,
                            caller.ip.saturating_sub(1),
                        );
                    }
                    return Ok(true);
                }
                if let ReturnAction::DictPairNext(state) = &frame.action {
                    let destination = frame.destination.ok_or_else(|| {
                        Diagnostic::new(
                            "BytecodeError",
                            "dict pair continuation has no destination",
                        )
                    })?;
                    let state = state.clone();
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    let result = self
                        .finish_dict_pair(program, destination, state, output)
                        .map(|_| ());
                    if let Err(error) = result {
                        let caller = self.frames.last().ok_or_else(|| {
                            Diagnostic::new("BytecodeError", "dict pair lost caller frame")
                        })?;
                        return self.dispatch_exception(
                            program,
                            output,
                            &error,
                            minimum_depth,
                            caller.code,
                            caller.ip.saturating_sub(1),
                        );
                    }
                    return Ok(true);
                }
                if let ReturnAction::CollectIterableNext(state) = &frame.action {
                    let destination = frame.destination.ok_or_else(|| {
                        Diagnostic::new(
                            "BytecodeError",
                            "iterable collection continuation has no destination",
                        )
                    })?;
                    let state = state.clone();
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    if let Err(error) = self.finish_iterable_collection(
                        program,
                        destination,
                        state.kind,
                        state.items,
                        output,
                    ) {
                        let caller = self.frames.last().ok_or_else(|| {
                            Diagnostic::new(
                                "BytecodeError",
                                "iterable collection lost caller frame",
                            )
                        })?;
                        return self.dispatch_exception(
                            program,
                            output,
                            &error,
                            minimum_depth,
                            caller.code,
                            caller.ip.saturating_sub(1),
                        );
                    }
                    return Ok(true);
                }
                if let ReturnAction::ExpandIterableNext(state) = frame.action {
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    if let Some(resume_pc) = state.resume_pc {
                        self.frames
                            .last_mut()
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    "BytecodeError",
                                    "argument expansion lost caller frame",
                                )
                            })?
                            .ip = resume_pc;
                    }
                    return Ok(true);
                }
                if let ReturnAction::IteratorNext { target, pc } = frame.action {
                    let destination = frame.destination.ok_or_else(|| {
                        Diagnostic::new("BytecodeError", "iterator continuation has no destination")
                    })?;
                    let return_value = frame
                        .generator
                        .and_then(|generator| self.heap.generator_return_value(generator))
                        .or_else(|| self.heap.stop_iteration_value(exception))
                        .unwrap_or(Value::NONE);
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                        target,
                        pc,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    self.registers[destination] = return_value;
                    self.jump(unwind.4, unwind.5);
                    return Ok(true);
                }
                if let ReturnAction::YieldFrom { target, pc, .. } = frame.action {
                    let destination = frame.destination.ok_or_else(|| {
                        Diagnostic::new(
                            "BytecodeError",
                            "yield-from continuation has no destination",
                        )
                    })?;
                    let return_value = self
                        .heap
                        .stop_iteration_value(exception)
                        .unwrap_or(Value::NONE);
                    let unwind = (
                        frame.base,
                        frame.cell_base,
                        frame.argument_base,
                        frame.pending_class_base,
                        target,
                        pc,
                    );
                    self.frames.truncate(frame_index);
                    self.registers.truncate(unwind.0);
                    self.cells.truncate(unwind.1);
                    self.arguments.truncate(unwind.2);
                    self.pending_classes.truncate(unwind.3);
                    self.frames
                        .last_mut()
                        .ok_or_else(|| {
                            Diagnostic::new(
                                "BytecodeError",
                                "yield-from completion lost outer frame",
                            )
                        })?
                        .yield_from = None;
                    self.registers[destination] = return_value;
                    self.jump(unwind.4, unwind.5);
                    return Ok(true);
                }
            }
        }
        traceback.reverse();
        if matches!(self.heap.get(exception), Ok(Object::Exception { .. })) {
            self.heap.record_exception_trace(exception, traceback)?;
        }
        let Some((frame_index, region)) = selected else {
            self.reset_import_frames(program, minimum_depth)?;
            self.pending_exception = Some(exception);
            return Ok(false);
        };
        self.reset_import_frames(program, frame_index + 1)?;
        self.frames.truncate(frame_index + 1);
        let frame = self.frames.last_mut().expect("selected exception frame");
        let code = &program.code[frame.code];
        self.registers
            .truncate(frame.base + usize::from(code.registers));
        self.cells
            .truncate(frame.cell_base + code.cell_locals.len() + code.free_vars.len());
        self.arguments.truncate(frame.argument_base);
        self.pending_classes.truncate(frame.pending_class_base);
        frame.ip = usize::from(region.target);
        frame.jit_resume = false;
        frame.jit_expanded_resume_depth = None;
        self.registers[frame.base + usize::from(region.exception)] = exception;
        Ok(true)
    }
    pub(super) fn instance_check(
        &self,
        value: Value,
        target: Value,
        subclass: bool,
        depth: usize,
    ) -> Result<bool> {
        if depth > 100 {
            return Err(Diagnostic::new("RecursionError", "classinfo nesting limit"));
        }
        if let Ok(Object::Tuple(types)) = self.heap.get(target) {
            for target in types {
                if self.instance_check(value, *target, subclass, depth + 1)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        self.heap.class(target)?;
        let actual = if subclass {
            self.heap.class(value)?;
            value
        } else {
            self.runtime_class(value)?
        };
        Ok(actual == target
            || self
                .heap
                .class(actual)
                .is_ok_and(|class| class.mro.contains(&target)))
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
            {
                match self.try_jit(p, output) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => {
                        let frame = self.frames.last().expect("active JIT frame");
                        let code = frame.code;
                        let pc = frame.ip.saturating_sub(1);
                        if self.dispatch_exception(p, output, &error, depth, code, pc)? {
                            continue;
                        }
                        return Err(error);
                    }
                }
            }
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
            let (code_id, pc, base, cell_base, namespace) = {
                let frame = self.frames.last_mut().expect("active frame");
                let state = (
                    frame.code,
                    frame.ip,
                    frame.base,
                    frame.cell_base,
                    frame.namespace,
                );
                frame.ip += 1;
                state
            };
            let code = &p.code[code_id];
            let i = code.instructions[pc];
            let op = Op::try_from(i.opcode)?;
            self.stats.instructions += 1;
            let a = base + i.a as usize;
            let b = base + i.b as usize;
            let c = base + i.c as usize;
            let step = (|| -> Result<()> {
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
                        self.store_global(p, i.b as usize, value)?;
                    }
                    Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul => {
                        let left = self.read(b)?;
                        let right = self.read(c)?;
                        if let Some(state) = self.binary_protocol(op, left, right)? {
                            self.continue_binary_protocol(p, a, state, output)?;
                        } else {
                            self.registers[a] = if self.adaptive_specialization {
                                self.adaptive_binary(code_id, pc, op, left, right)?
                            } else if op == Op::InplaceAdd {
                                self.heap.inplace_add(left, right)?
                            } else {
                                self.heap.binary(op, left, right)?
                            };
                        }
                    }
                    Op::FloorDiv
                    | Op::Mod
                    | Op::Div
                    | Op::Pow
                    | Op::BitOr
                    | Op::BitXor
                    | Op::BitAnd
                    | Op::LeftShift
                    | Op::RightShift
                    | Op::InplaceSub
                    | Op::InplaceMul
                    | Op::InplaceDiv
                    | Op::InplaceFloorDiv
                    | Op::InplaceMod
                    | Op::InplacePow
                    | Op::InplaceBitOr
                    | Op::InplaceBitXor
                    | Op::InplaceBitAnd
                    | Op::InplaceLeftShift
                    | Op::InplaceRightShift => {
                        let left = self.read(b)?;
                        let right = self.read(c)?;
                        if let Some(state) = self.binary_protocol(op, left, right)? {
                            self.continue_binary_protocol(p, a, state, output)?;
                        } else {
                            self.registers[a] =
                                self.heap.binary(base_binary_op(op), left, right)?;
                        }
                    }
                    Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                        let left = self.read(b)?;
                        let right = self.read(c)?;
                        if let Some(state) = self.binary_protocol(op, left, right)? {
                            self.continue_binary_protocol(p, a, state, output)?;
                        } else if matches!(op, Op::Eq | Op::Ne) {
                            self.invoke_equality(
                                p,
                                left,
                                right,
                                a,
                                EqualityAction::Return {
                                    negate: op == Op::Ne,
                                },
                                output,
                            )?;
                        } else {
                            self.invoke_order_comparison(p, left, right, a, (op, 0), output)?;
                        }
                    }
                    Op::Neg | Op::Pos | Op::Invert => {
                        let kind = match op {
                            Op::Neg => UnaryProtocolKind::Neg,
                            Op::Pos => UnaryProtocolKind::Pos,
                            Op::Invert => UnaryProtocolKind::Invert,
                            _ => unreachable!(),
                        };
                        let value = self.read(b)?;
                        self.invoke_unary_protocol(p, value, a, kind, output)?;
                    }
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
                    Op::Return | Op::Yield => {
                        let yielding = op == Op::Yield;
                        let mut value = self.read(if yielding { b } else { a })?;
                        if yielding {
                            self.registers[a] = Value::NONE;
                            self.suspend_active_generator(p)?;
                            if self.frames.last().is_some_and(|frame| {
                                matches!(
                                    &frame.action,
                                    ReturnAction::Close | ReturnAction::YieldFromClose { .. }
                                )
                            }) {
                                let generator = self
                                    .frames
                                    .last()
                                    .and_then(|frame| frame.generator)
                                    .expect("close action belongs to a generator");
                                self.heap.complete_generator(generator)?;
                                return Err(Diagnostic::new(
                                    "RuntimeError",
                                    "generator ignored GeneratorExit",
                                ));
                            }
                        } else if let Some(generator) =
                            self.frames.last().and_then(|frame| frame.generator)
                        {
                            self.heap.complete_generator_with_value(generator, value)?;
                            return Err(self.generator_stop_iteration(value)?);
                        }
                        let frame = self.frames.pop().expect("active frame");
                        let mut set_names = None;
                        let mut finish_new = None;
                        let mut finish_metaclass_new = None;
                        let mut finish_metaclass_init = None;
                        let mut finish_truth = None;
                        let mut class_prepare = None;
                        let mut class_body = None;
                        let mut iterable_start = None;
                        let mut iterable_next = None;
                        let mut expansion_start = None;
                        let mut expansion_next = None;
                        let mut dict_iterable_start = None;
                        let mut dict_iterable_next = None;
                        let mut dict_pair_start = None;
                        let mut dict_pair_iterator_start = None;
                        let mut dict_pair_next = None;
                        let mut binary_protocol = None;
                        let mut binary_result = None;
                        let mut unary_protocol = None;
                        let mut numeric_conversion = None;
                        let mut index_conversion = None;
                        let mut hash_action = None;
                        let mut length_result = None;
                        let mut yield_from_item = None;
                        let mut yield_from_close = None;
                        let mut generator_throw = None;
                        match frame.action {
                            ReturnAction::Value => {}
                            ReturnAction::Iterator => self.validate_iterator(value)?,
                            ReturnAction::Next(_) => {}
                            ReturnAction::Close => {}
                            ReturnAction::IteratorNext { .. } => {}
                            ReturnAction::YieldFrom { iterator, .. } => {
                                yield_from_item = Some(iterator)
                            }
                            ReturnAction::YieldFromClose { exception } => {
                                yield_from_close = Some(exception)
                            }
                            ReturnAction::GeneratorThrowInit {
                                generator,
                                traceback,
                                instance,
                            } => {
                                if value != Value::NONE {
                                    return Err(Diagnostic::new(
                                        "TypeError",
                                        "exception __init__ must return None",
                                    ));
                                }
                                value = instance;
                                generator_throw = Some((generator, traceback));
                            }
                            ReturnAction::CollectIterableStart(kind) => {
                                iterable_start = Some((kind, value));
                            }
                            ReturnAction::CollectIterableNext(mut state) => {
                                if let IterableCollectionKind::ListInit { owner, .. } = &state.kind
                                {
                                    self.heap.append_list(*owner, value)?;
                                } else {
                                    state.items.push(value);
                                }
                                iterable_next = Some(state);
                            }
                            ReturnAction::ExpandIterableStart(resume_pc) => {
                                expansion_start = Some((value, resume_pc));
                            }
                            ReturnAction::ExpandIterableNext(state) => {
                                expansion_next = Some((state, value));
                            }
                            ReturnAction::DictIterableStart(state) => {
                                dict_iterable_start = Some((state, value));
                            }
                            ReturnAction::DictIterableNext(state) => {
                                dict_iterable_next = Some((state, value));
                            }
                            ReturnAction::DictPairStart(state) => {
                                dict_pair_start = Some((state, value));
                            }
                            ReturnAction::DictPairIteratorStart(state) => {
                                dict_pair_iterator_start = Some((state, value));
                            }
                            ReturnAction::DictPairNext(mut state) => {
                                state.items.push(value);
                                dict_pair_next = Some(state);
                            }
                            ReturnAction::Length => length_result = Some(value),
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
                            ReturnAction::ClassPrepare(build) => {
                                class_prepare = Some((build, value));
                            }
                            ReturnAction::ClassBody(body) => {
                                let metadata = &p.code[frame.code];
                                let class_cell = metadata
                                    .cell_locals
                                    .iter()
                                    .enumerate()
                                    .find(|(_, local)| {
                                        p.symbols[metadata.locals[**local as usize].0 as usize]
                                            == "__class__"
                                    })
                                    .map(|(cell, _)| self.cells[frame.cell_base + cell]);
                                class_body = Some((body, class_cell));
                            }
                            ReturnAction::MetaclassNew(state) => {
                                finish_metaclass_new = Some((state, value));
                            }
                            ReturnAction::MetaclassInit(class) => {
                                if value != Value::NONE {
                                    return Err(Diagnostic::new(
                                        "TypeError",
                                        "metaclass __init__ must return None",
                                    ));
                                }
                                value = class;
                                finish_metaclass_init = Some(class);
                            }
                            ReturnAction::SetNames {
                                class,
                                pending,
                                after,
                            } => {
                                value = class;
                                set_names = Some((class, pending, after));
                            }
                            ReturnAction::NamespaceLookup { .. } => {}
                            ReturnAction::AttributeGet(state) => {
                                if matches!(state.missing, AttributeMissing::HasAttr) {
                                    value = Value::bool(true);
                                }
                            }
                            ReturnAction::BinaryProtocol(state) => {
                                if value == Value::NOT_IMPLEMENTED {
                                    binary_protocol = Some(state);
                                } else {
                                    binary_result = Some((state.negate_result, state.completion));
                                }
                            }
                            ReturnAction::UnaryProtocol(state) => {
                                if value == Value::NOT_IMPLEMENTED {
                                    unary_protocol = Some(state);
                                }
                            }
                            ReturnAction::NumericConversion(state) => {
                                numeric_conversion = Some(state);
                            }
                            ReturnAction::IndexConversion(state) => {
                                index_conversion = Some(state);
                            }
                            ReturnAction::Hash(action) => hash_action = Some(action),
                            ReturnAction::Import(module_id) => {
                                let name = p.modules[module_id].name.clone();
                                let object = self.source_modules[module_id]
                                    .object
                                    .expect("imported module object");
                                self.source_modules[module_id].state = SourceModuleState::Loaded;
                                self.attach_imported_module(p, &name, object)?;
                                value = object;
                            }
                            ReturnAction::Setter => value = Value::NONE,
                        }
                        self.registers.truncate(frame.base);
                        self.cells.truncate(frame.cell_base);
                        if let Some(iterator) = yield_from_item {
                            self.frames
                                .last_mut()
                                .ok_or_else(|| {
                                    Diagnostic::new(
                                        "BytecodeError",
                                        "yield-from continuation lost outer frame",
                                    )
                                })?
                                .yield_from = Some(iterator);
                        }
                        if let Some(exception) = yield_from_close {
                            self.pending_exception = Some(exception);
                            return Err(self.exception_diagnostic(exception)?);
                        }
                        if let Some((generator, traceback)) = generator_throw {
                            let destination = frame.destination.ok_or_else(|| {
                                Diagnostic::new(
                                    "BytecodeError",
                                    "generator throw constructor has no destination",
                                )
                            })?;
                            self.finish_generator_throw(
                                p,
                                generator,
                                destination,
                                value,
                                traceback,
                            )?;
                            return Ok(());
                        }
                        let continuation_required = set_names.is_some()
                            || finish_new.is_some()
                            || finish_metaclass_new.is_some()
                            || finish_metaclass_init.is_some()
                            || finish_truth.is_some()
                            || class_prepare.is_some()
                            || class_body.is_some()
                            || iterable_start.is_some()
                            || iterable_next.is_some()
                            || expansion_start.is_some()
                            || expansion_next.is_some()
                            || dict_iterable_start.is_some()
                            || dict_iterable_next.is_some()
                            || dict_pair_start.is_some()
                            || dict_pair_iterator_start.is_some()
                            || dict_pair_next.is_some()
                            || binary_protocol.is_some()
                            || binary_result.is_some()
                            || unary_protocol.is_some()
                            || numeric_conversion.is_some()
                            || index_conversion.is_some()
                            || hash_action.is_some()
                            || length_result.is_some();
                        if let Some(dest) = frame.destination {
                            self.registers[dest] = value;
                            if let Some((protocol, action)) = finish_truth {
                                self.finish_truth(p, dest, value, protocol, action, output)?;
                            } else if let Some(value) = length_result {
                                self.finish_length(p, dest, value, output)?;
                            } else if let Some((negate, completion)) = binary_result {
                                self.finish_binary_protocol_value(
                                    p, dest, value, negate, completion, output,
                                )?;
                            } else if let Some(state) = unary_protocol {
                                self.registers[dest] =
                                    self.unary_fallback(state.kind, state.value)?;
                            } else if let Some(state) = numeric_conversion {
                                self.finish_numeric_conversion(p, dest, value, state, output)?;
                            } else if let Some(state) = index_conversion {
                                self.finish_index_conversion(p, dest, value, state, output)?;
                            } else if let Some(action) = hash_action {
                                self.finish_hash_result(p, dest, value, action, output)?;
                            } else if let Some((class, arguments)) = finish_new {
                                self.finish_new(p, dest, class, value, arguments, output)?;
                            } else if let Some((build, mapping)) = class_prepare {
                                self.enter_class_body(p, dest, build, Some(mapping))?;
                            } else if let Some((body, class_cell)) = class_body {
                                self.complete_class_body(p, dest, body, class_cell, output)?;
                            } else if let Some((state, result)) = finish_metaclass_new {
                                self.finish_metaclass_new(p, dest, state, result, output)?;
                            } else if finish_metaclass_init.is_some() {
                                self.registers[dest] = value;
                            } else if let Some((class, pending, after)) = set_names {
                                self.invoke_set_names(p, dest, class, pending, after, output)?;
                            } else if let Some((kind, iterator)) = iterable_start {
                                self.validate_iterator(iterator)?;
                                self.continue_iterable_collection(
                                    p,
                                    dest,
                                    IterableCollection {
                                        kind,
                                        iterator,
                                        items: Vec::new(),
                                    },
                                    output,
                                )?;
                            } else if let Some(state) = iterable_next {
                                self.continue_iterable_collection(p, dest, state, output)?;
                            } else if let Some((iterator, resume_pc)) = expansion_start {
                                self.validate_iterator(iterator)?;
                                self.continue_argument_expansion(
                                    p,
                                    dest,
                                    ArgumentExpansion {
                                        iterator,
                                        resume_pc,
                                    },
                                    output,
                                )?;
                            } else if let Some((state, item)) = expansion_next {
                                self.push_expanded_positional(item)?;
                                self.continue_argument_expansion(p, dest, state, output)?;
                            } else if let Some((start, iterator)) = dict_iterable_start {
                                self.validate_iterator(iterator)?;
                                self.continue_dict_construction(
                                    p,
                                    dest,
                                    DictConstruction {
                                        start,
                                        iterator,
                                        index: 0,
                                    },
                                    output,
                                )?;
                            } else if let Some((state, item)) = dict_iterable_next {
                                if let Some(state) =
                                    self.invoke_dict_pair(p, dest, state, item, output)?
                                {
                                    self.continue_dict_construction(p, dest, state, output)?;
                                }
                            } else if let Some((outer, iterator)) = dict_pair_start {
                                self.validate_iterator(iterator)?;
                                if let Some(state) = self
                                    .invoke_dict_pair_iterator(p, dest, outer, iterator, output)?
                                {
                                    self.continue_dict_construction(p, dest, state, output)?;
                                }
                            } else if let Some((outer, iterator)) = dict_pair_iterator_start {
                                self.validate_iterator(iterator)?;
                                if let Some(state) = self.continue_dict_pair(
                                    p,
                                    dest,
                                    DictPairConstruction {
                                        outer,
                                        iterator,
                                        items: Vec::new(),
                                    },
                                    output,
                                )? {
                                    self.continue_dict_construction(p, dest, state, output)?;
                                }
                            } else if let Some(state) = dict_pair_next {
                                if let Some(state) =
                                    self.continue_dict_pair(p, dest, state, output)?
                                {
                                    self.continue_dict_construction(p, dest, state, output)?;
                                }
                            } else if let Some(state) = binary_protocol {
                                self.continue_binary_protocol(p, dest, state, output)?;
                            }
                        } else if continuation_required {
                            return Err(Diagnostic::new(
                                "BytecodeError",
                                "continuation has no destination",
                            ));
                        }
                    }
                    Op::Raise => {
                        if i.b == 1 {
                            let value = self
                                .frames
                                .iter()
                                .rev()
                                .find_map(|frame| frame.exception_stack.last().copied())
                                .ok_or_else(|| {
                                    Diagnostic::new(
                                        "RuntimeError",
                                        "no active exception to reraise",
                                    )
                                })?;
                            self.pending_exception = Some(value);
                            return Err(self.exception_diagnostic(value)?);
                        }
                        let value = self.normalize_raised_exception(self.read(a)?)?;
                        if i.b == 2 {
                            let source = self.read(c)?;
                            if source == Value::NONE {
                                self.heap.set_exception_cause(value, None, true)?;
                            } else {
                                let cause =
                                    self.normalize_raised_exception(source).map_err(|_| {
                                        Diagnostic::new(
                                            "TypeError",
                                            "exception causes must derive from BaseException",
                                        )
                                    })?;
                                self.heap.set_exception_cause(value, Some(cause), true)?;
                            }
                        }
                        self.pending_exception = Some(value);
                        return Err(self.exception_diagnostic(value)?);
                    }
                    Op::ExceptionMatch => {
                        let exception = self.read(b)?;
                        let class = self.read(c)?;
                        self.registers[a] =
                            Value::bool(self.instance_check(exception, class, false, 0)?);
                    }
                    Op::ClearException => {
                        self.frames
                            .last_mut()
                            .expect("active frame")
                            .exception_stack
                            .pop();
                    }
                    Op::PushException => {
                        let exception = self.read(a)?;
                        self.frames
                            .last_mut()
                            .expect("active frame")
                            .exception_stack
                            .push(exception);
                    }
                    Op::ClearBinding => match i.a {
                        0 => self.registers[base + usize::from(i.b)] = Value::UNBOUND,
                        1 => {
                            self.clear_global(p, usize::from(i.b))?;
                        }
                        2 => self
                            .heap
                            .store_cell(self.cells[cell_base + usize::from(i.b)], Value::UNBOUND)?,
                        3 => self.heap.namespace_delete(
                            namespace.ok_or_else(|| {
                                Diagnostic::new("BytecodeError", "class frame has no namespace")
                            })?,
                            &p.symbols[usize::from(i.b)],
                        )?,
                        _ => unreachable!("verified clear-binding kind"),
                    },
                    Op::ContextEnter => {
                        let manager = self.read(c)?;
                        let class_manager = matches!(self.heap.get(manager), Ok(Object::Class(_)));
                        let exit = if class_manager {
                            self.heap.metaclass_method_call(manager, "__exit__")?
                        } else {
                            self.heap.special_method_call(manager, "__exit__")?
                        }
                        .ok_or_else(|| {
                            Diagnostic::new(
                                "TypeError",
                                "object does not support the context manager protocol",
                            )
                        })?;
                        let enter = if class_manager {
                            self.heap.metaclass_method_call(manager, "__enter__")?
                        } else {
                            self.heap.special_method_call(manager, "__enter__")?
                        }
                        .ok_or_else(|| {
                            Diagnostic::new(
                                "TypeError",
                                "object does not support the context manager protocol",
                            )
                        })?;
                        self.registers[b] = self.heap.alloc(Object::Tuple(vec![
                            exit.callable,
                            exit.receiver.unwrap_or(Value::NONE),
                            Value::bool(exit.receiver.is_some()),
                        ]))?;
                        self.invoke(
                            p,
                            enter.callable,
                            a,
                            Arguments::Inline {
                                receiver: enter.receiver,
                                positional: [Value::UNBOUND; 3],
                                count: 0,
                            },
                            output,
                        )?;
                    }
                    Op::ContextExit => {
                        let token = self.read(b)?;
                        let exception = self.read(c)?;
                        let (callable, receiver) = match self.heap.get(token)? {
                            Object::Tuple(values) if values.len() == 3 => {
                                let bound = values[2].as_bool().ok_or_else(|| {
                                    Diagnostic::new("BytecodeError", "invalid context exit token")
                                })?;
                                (values[0], bound.then_some(values[1]))
                            }
                            _ => {
                                return Err(Diagnostic::new(
                                    "BytecodeError",
                                    "invalid context exit token",
                                ))
                            }
                        };
                        let mut positional = [Value::NONE; 3];
                        if exception != Value::NONE {
                            let traceback = self
                                .heap
                                .exception_traceback(exception)?
                                .unwrap_or(Value::NONE);
                            positional = [self.runtime_class(exception)?, exception, traceback];
                        }
                        self.invoke(
                            p,
                            callable,
                            a,
                            Arguments::Inline {
                                receiver,
                                positional,
                                count: 3,
                            },
                            output,
                        )?;
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
                        let declared_bases = bases.clone();
                        if bases.is_empty() {
                            bases.push(self.object_class);
                        }
                        let explicit_metaclass = site
                            .keywords
                            .first()
                            .map(|_| self.read(base + (site.first + site.count) as usize))
                            .transpose()?;
                        let metaclass = self.heap.select_metaclass(
                            explicit_metaclass,
                            &bases,
                            self.type_class,
                        )?;
                        let qualname = p.code[body].name.clone();
                        let build = ClassBuild {
                            function,
                            bases,
                            declared_bases,
                            metaclass,
                            qualname,
                        };
                        if self.heap.has_custom_metaclass_hook(
                            build.metaclass,
                            self.type_class,
                            "__prepare__",
                        )? {
                            let prepare = self.heap.attr(build.metaclass, "__prepare__")?;
                            let name = build.qualname.rsplit('.').next().unwrap_or(&build.qualname);
                            let name = self.heap.alloc(Object::Str(name.to_owned()))?;
                            let base_tuple = self
                                .heap
                                .alloc(Object::Tuple(build.declared_bases.clone()))?;
                            let depth = self.frames.len();
                            self.invoke(
                                p,
                                prepare,
                                a,
                                Arguments::Inline {
                                    receiver: None,
                                    positional: [name, base_tuple, Value::UNBOUND],
                                    count: 2,
                                },
                                output,
                            )?;
                            if self.frames.len() > depth {
                                self.frames.last_mut().expect("__prepare__ frame").action =
                                    ReturnAction::ClassPrepare(build);
                            } else {
                                let mapping = self.registers[a];
                                self.enter_class_body(p, a, build, Some(mapping))?;
                            }
                        } else {
                            self.enter_class_body(p, a, build, None)?;
                        }
                    }
                    Op::LoadName | Op::ClassDeref => {
                        let namespace = namespace.ok_or_else(|| {
                            Diagnostic::new("BytecodeError", "class frame has no namespace")
                        })?;
                        let name = &p.symbols[i.b as usize];
                        let custom_mapping = self
                            .heap
                            .namespace_backing_mapping(namespace)?
                            .filter(|mapping| {
                                !matches!(self.heap.get(*mapping), Ok(Object::Dict(_)))
                            });
                        if let Some(mapping) = custom_mapping {
                            let call = self
                                .heap
                                .special_method_call(mapping, "__getitem__")?
                                .ok_or_else(|| {
                                    Diagnostic::new(
                                        "TypeError",
                                        "class namespace mapping has no __getitem__",
                                    )
                                })?;
                            let key = self.heap.alloc(Object::Str(name.clone()))?;
                            let depth = self.frames.len();
                            self.invoke(
                                p,
                                call.callable,
                                a,
                                Arguments::Inline {
                                    receiver: call.receiver,
                                    positional: [key, Value::UNBOUND, Value::UNBOUND],
                                    count: 1,
                                },
                                output,
                            )?;
                            if self.frames.len() > depth {
                                self.frames
                                    .last_mut()
                                    .expect("namespace __getitem__ frame")
                                    .action = ReturnAction::NamespaceLookup {
                                    target: a,
                                    symbol: i.b,
                                    cell: (op == Op::ClassDeref)
                                        .then(|| self.cells[cell_base + i.c as usize]),
                                };
                            }
                        } else {
                            let value =
                                if let Some(value) = self.heap.namespace_get(namespace, name)? {
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
                    }
                    Op::StoreName => {
                        let namespace = namespace.ok_or_else(|| {
                            Diagnostic::new("BytecodeError", "class frame has no namespace")
                        })?;
                        let custom_mapping = self
                            .heap
                            .namespace_backing_mapping(namespace)?
                            .filter(|mapping| {
                                !matches!(self.heap.get(*mapping), Ok(Object::Dict(_)))
                            });
                        if let Some(mapping) = custom_mapping {
                            let value = self.read(a)?;
                            let call = self
                                .heap
                                .special_method_call(mapping, "__setitem__")?
                                .ok_or_else(|| {
                                    Diagnostic::new(
                                        "TypeError",
                                        "class namespace mapping has no __setitem__",
                                    )
                                })?;
                            let key = self
                                .heap
                                .alloc(Object::Str(p.symbols[i.b as usize].clone()))?;
                            let depth = self.frames.len();
                            self.invoke(
                                p,
                                call.callable,
                                a,
                                Arguments::Inline {
                                    receiver: call.receiver,
                                    positional: [key, value, Value::UNBOUND],
                                    count: 2,
                                },
                                output,
                            )?;
                            if self.frames.len() > depth {
                                self.frames
                                    .last_mut()
                                    .expect("namespace __setitem__ frame")
                                    .action = ReturnAction::Setter;
                            } else {
                                self.registers[a] = Value::NONE;
                            }
                        } else {
                            self.heap.namespace_set(
                                namespace,
                                &p.symbols[i.b as usize],
                                self.read(a)?,
                            )?;
                        }
                    }
                    Op::SetAttr => {
                        let owner = self.read(a)?;
                        let value = self.read(b)?;
                        let name = p.symbols[i.c as usize].clone();
                        self.invoke_attribute_set(p, owner, &name, value, a, output)?;
                    }
                    Op::DelAttr => {
                        let owner = self.read(a)?;
                        let name = p.symbols[i.b as usize].clone();
                        self.invoke_attribute_delete(p, owner, &name, a, output)?;
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
                        if op == Op::ArgStar && i.c != 1 {
                            self.expand_star_argument(p, a, value, None, output)?;
                        } else {
                            self.append_argument(p, op, value, i.b, i.c)?;
                        }
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
                            if self.expand_star_argument(p, a, value, Some(pc), output)? {
                                return Ok(());
                            }
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
                    Op::SetItem => {
                        let owner = self.read(a)?;
                        let key = self.read(b)?;
                        let value = self.read(c)?;
                        if let Some(call) = self.heap.special_method_call(owner, "__setitem__")? {
                            let depth = self.frames.len();
                            self.invoke(
                                p,
                                call.callable,
                                a,
                                Arguments::Inline {
                                    receiver: call.receiver,
                                    positional: [key, value, Value::UNBOUND],
                                    count: 2,
                                },
                                output,
                            )?;
                            if self.frames.len() > depth {
                                self.frames.last_mut().expect("__setitem__ frame").action =
                                    ReturnAction::Setter;
                            } else {
                                self.registers[a] = Value::NONE;
                            }
                        } else {
                            let storage = self.heap.native_value(owner);
                            if matches!(self.heap.get(storage)?, Object::Dict(_)) {
                                self.invoke_dict_operation(
                                    p,
                                    owner,
                                    key,
                                    a,
                                    DictOperationKind::Set(value),
                                    output,
                                )?;
                            } else if matches!(self.heap.get(storage)?, Object::List(_)) {
                                self.invoke_index_conversion(
                                    p,
                                    key,
                                    a,
                                    IndexContinuation::SetItem { owner, value },
                                    output,
                                )?;
                            } else {
                                self.heap.set_item(owner, key, value)?;
                            }
                        }
                    }
                    Op::DelItem => {
                        let owner = self.read(a)?;
                        let key = self.read(b)?;
                        if let Some(call) = self.heap.special_method_call(owner, "__delitem__")? {
                            let depth = self.frames.len();
                            self.invoke(
                                p,
                                call.callable,
                                a,
                                Arguments::Inline {
                                    receiver: call.receiver,
                                    positional: [key, Value::UNBOUND, Value::UNBOUND],
                                    count: 1,
                                },
                                output,
                            )?;
                            if self.frames.len() > depth {
                                self.frames.last_mut().expect("__delitem__ frame").action =
                                    ReturnAction::Setter;
                            } else {
                                self.registers[a] = Value::NONE;
                            }
                        } else {
                            let storage = self.heap.native_value(owner);
                            if matches!(self.heap.get(storage)?, Object::Dict(_)) {
                                self.invoke_dict_operation(
                                    p,
                                    owner,
                                    key,
                                    a,
                                    DictOperationKind::Delete,
                                    output,
                                )?;
                            } else if matches!(self.heap.get(storage)?, Object::List(_)) {
                                self.invoke_index_conversion(
                                    p,
                                    key,
                                    a,
                                    IndexContinuation::DelItem { owner },
                                    output,
                                )?;
                            } else {
                                self.heap.delete_item(owner, key)?;
                            }
                        }
                    }
                    Op::DictMerge => self.invoke_dict_merge(
                        p,
                        a,
                        self.read(a)?,
                        self.read(b)?,
                        DictMergeFinish::Preserve,
                        output,
                    )?,
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
                    Op::Item => {
                        let owner = self.read(b)?;
                        let key = self.read(c)?;
                        if let Some(call) = self.heap.special_method_call(owner, "__getitem__")? {
                            self.invoke(
                                p,
                                call.callable,
                                a,
                                Arguments::Inline {
                                    receiver: call.receiver,
                                    positional: [key, Value::UNBOUND, Value::UNBOUND],
                                    count: 1,
                                },
                                output,
                            )?;
                        } else {
                            let storage = self.heap.native_value(owner);
                            if matches!(self.heap.get(storage)?, Object::Dict(_)) {
                                self.invoke_dict_operation(
                                    p,
                                    owner,
                                    key,
                                    a,
                                    DictOperationKind::Get,
                                    output,
                                )?;
                                return Ok(());
                            }
                            let sequence = matches!(
                                self.heap.get(storage)?,
                                Object::Tuple(_)
                                    | Object::List(_)
                                    | Object::Str(_)
                                    | Object::Range { .. }
                            );
                            if sequence {
                                if let Ok(Object::Slice(components)) = self.heap.get(key) {
                                    if matches!(
                                        self.heap.get(storage)?,
                                        Object::Tuple(_) | Object::List(_) | Object::Str(_)
                                    ) {
                                        self.continue_slice_conversion(
                                            p,
                                            a,
                                            SliceConversion {
                                                owner,
                                                components: *components,
                                                next: 0,
                                            },
                                            output,
                                        )?;
                                    } else {
                                        self.registers[a] = self.heap.item(owner, key)?;
                                    }
                                } else {
                                    self.invoke_index_conversion(
                                        p,
                                        key,
                                        a,
                                        IndexContinuation::GetItem { owner },
                                        output,
                                    )?;
                                }
                            } else {
                                self.registers[a] = self.heap.item(owner, key)?;
                            }
                        }
                    }
                    Op::Slice => {
                        let values = [self.read(b)?, self.read(b + 1)?, self.read(b + 2)?];
                        self.registers[a] = self.heap.alloc(Object::Slice(values))?;
                    }
                    Op::Unpack => {
                        let source = self.read(b)?;
                        let count = i.c as usize;
                        self.invoke_iterable_collection(
                            p,
                            a,
                            IterableCollectionKind::Unpack { first: a, count },
                            source,
                            output,
                        )?;
                    }
                    Op::Iter => {
                        let source = self.read(b)?;
                        match self.heap.iterator(source) {
                            Ok(iterator) => self.registers[a] = iterator,
                            Err(error) if error.kind == "TypeError" => {
                                let Some(call) =
                                    self.heap.special_method_call(source, "__iter__")?
                                else {
                                    return Err(error);
                                };
                                let depth = self.frames.len();
                                self.invoke(
                                    p,
                                    call.callable,
                                    a,
                                    Arguments::Inline {
                                        receiver: call.receiver,
                                        positional: [Value::UNBOUND; 3],
                                        count: 0,
                                    },
                                    output,
                                )?;
                                if self.frames.len() > depth {
                                    self.frames.last_mut().expect("__iter__ frame").action =
                                        ReturnAction::Iterator;
                                } else {
                                    self.validate_iterator(self.registers[a])?;
                                }
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    Op::Next => {
                        let iterator = self.read(b)?;
                        if self.heap.is_generator(iterator) {
                            match self.resume_generator(
                                p,
                                iterator,
                                a,
                                Value::NONE,
                                ReturnAction::IteratorNext {
                                    target: i.c as usize,
                                    pc,
                                },
                            ) {
                                Ok(()) => {}
                                Err(error) if error.kind == "StopIteration" => {
                                    self.registers[a] = self
                                        .heap
                                        .generator_return_value(iterator)
                                        .unwrap_or(Value::NONE);
                                    self.jump(i.c as usize, pc);
                                }
                                Err(error) => return Err(error),
                            }
                        } else if self.heap.is_iterator(iterator) {
                            if let Some(value) = self.heap.next(iterator)? {
                                self.registers[a] = value;
                            } else {
                                self.registers[a] = Value::NONE;
                                self.jump(i.c as usize, pc);
                            }
                        } else if let Some(call) =
                            self.heap.special_method_call(iterator, "__next__")?
                        {
                            let depth = self.frames.len();
                            self.invoke(
                                p,
                                call.callable,
                                a,
                                Arguments::Inline {
                                    receiver: call.receiver,
                                    positional: [Value::UNBOUND; 3],
                                    count: 0,
                                },
                                output,
                            )?;
                            if self.frames.len() > depth {
                                self.frames.last_mut().expect("__next__ frame").action =
                                    ReturnAction::IteratorNext {
                                        target: i.c as usize,
                                        pc,
                                    };
                            }
                        } else {
                            return Err(Diagnostic::new("TypeError", "object is not an iterator"));
                        }
                    }
                    Op::YieldFrom => {
                        let iterator = self.read(b)?;
                        let sent = self.read(a)?;
                        let injected = self
                            .frames
                            .last_mut()
                            .expect("active yield-from frame")
                            .injected_exception
                            .take();
                        self.frames
                            .last_mut()
                            .expect("active yield-from frame")
                            .yield_from = None;
                        if let Some(exception) = injected {
                            let closing = self.instance_check(
                                exception,
                                self.runtime_types.generator_exit,
                                false,
                                0,
                            )?;
                            if closing {
                                if self.heap.is_generator(iterator) {
                                    self.resume_generator(
                                        p,
                                        iterator,
                                        a,
                                        Value::NONE,
                                        ReturnAction::YieldFromClose { exception },
                                    )?;
                                    let diagnostic = self.exception_diagnostic(exception)?;
                                    self.pending_exception = Some(exception);
                                    return Err(diagnostic);
                                }
                                if let Some(call) =
                                    self.heap.special_method_call(iterator, "close")?
                                {
                                    let depth = self.frames.len();
                                    self.invoke(
                                        p,
                                        call.callable,
                                        a,
                                        Arguments::Inline {
                                            receiver: call.receiver,
                                            positional: [Value::UNBOUND; 3],
                                            count: 0,
                                        },
                                        output,
                                    )?;
                                    if self.frames.len() > depth {
                                        self.frames
                                            .last_mut()
                                            .expect("delegate close frame")
                                            .action = ReturnAction::YieldFromClose { exception };
                                    } else {
                                        let diagnostic = self.exception_diagnostic(exception)?;
                                        self.pending_exception = Some(exception);
                                        return Err(diagnostic);
                                    }
                                } else {
                                    let diagnostic = self.exception_diagnostic(exception)?;
                                    self.pending_exception = Some(exception);
                                    return Err(diagnostic);
                                }
                            } else if self.heap.is_generator(iterator) {
                                self.resume_generator(
                                    p,
                                    iterator,
                                    a,
                                    Value::NONE,
                                    ReturnAction::YieldFrom {
                                        target: i.c as usize,
                                        pc,
                                        iterator,
                                    },
                                )?;
                                let diagnostic = self.exception_diagnostic(exception)?;
                                self.pending_exception = Some(exception);
                                return Err(diagnostic);
                            } else if let Some(call) =
                                self.heap.special_method_call(iterator, "throw")?
                            {
                                let depth = self.frames.len();
                                match self.invoke(
                                    p,
                                    call.callable,
                                    a,
                                    Arguments::Inline {
                                        receiver: call.receiver,
                                        positional: [exception, Value::UNBOUND, Value::UNBOUND],
                                        count: 1,
                                    },
                                    output,
                                ) {
                                    Ok(()) => {}
                                    Err(error) if error.kind == "StopIteration" => {
                                        let return_value = self
                                            .pending_exception
                                            .take()
                                            .and_then(|exception| {
                                                self.heap.stop_iteration_value(exception)
                                            })
                                            .unwrap_or(Value::NONE);
                                        self.registers[a] = return_value;
                                        self.jump(i.c as usize, pc);
                                    }
                                    Err(error) => return Err(error),
                                }
                                if self.frames.len() > depth {
                                    self.frames.last_mut().expect("delegate throw frame").action =
                                        ReturnAction::YieldFrom {
                                            target: i.c as usize,
                                            pc,
                                            iterator,
                                        };
                                } else if self.frames.last().is_some_and(|frame| frame.ip == pc + 1)
                                {
                                    self.frames
                                        .last_mut()
                                        .expect("active yield-from frame")
                                        .yield_from = Some(iterator);
                                }
                            } else {
                                let diagnostic = self.exception_diagnostic(exception)?;
                                self.pending_exception = Some(exception);
                                return Err(diagnostic);
                            }
                        } else if self.heap.is_generator(iterator) {
                            match self.resume_generator(
                                p,
                                iterator,
                                a,
                                sent,
                                ReturnAction::YieldFrom {
                                    target: i.c as usize,
                                    pc,
                                    iterator,
                                },
                            ) {
                                Ok(()) => {}
                                Err(error) if error.kind == "StopIteration" => {
                                    self.pending_exception = None;
                                    self.registers[a] = Value::NONE;
                                    self.jump(i.c as usize, pc);
                                }
                                Err(error) => return Err(error),
                            }
                        } else if sent == Value::NONE && self.heap.is_iterator(iterator) {
                            if let Some(value) = self.heap.next(iterator)? {
                                self.registers[a] = value;
                                self.frames
                                    .last_mut()
                                    .expect("active yield-from frame")
                                    .yield_from = Some(iterator);
                            } else {
                                self.registers[a] = Value::NONE;
                                self.jump(i.c as usize, pc);
                            }
                        } else {
                            let method = if sent == Value::NONE {
                                "__next__"
                            } else {
                                "send"
                            };
                            let Some(call) = self.heap.special_method_call(iterator, method)?
                            else {
                                return Err(Diagnostic::new(
                                    "AttributeError",
                                    format!("iterator has no '{method}' method"),
                                ));
                            };
                            let mut positional = [Value::UNBOUND; 3];
                            let count = if sent == Value::NONE {
                                0
                            } else {
                                positional[0] = sent;
                                1
                            };
                            let depth = self.frames.len();
                            match self.invoke(
                                p,
                                call.callable,
                                a,
                                Arguments::Inline {
                                    receiver: call.receiver,
                                    positional,
                                    count,
                                },
                                output,
                            ) {
                                Ok(()) => {}
                                Err(error) if error.kind == "StopIteration" => {
                                    let return_value = self
                                        .pending_exception
                                        .take()
                                        .and_then(|exception| {
                                            self.heap.stop_iteration_value(exception)
                                        })
                                        .unwrap_or(Value::NONE);
                                    self.registers[a] = return_value;
                                    self.jump(i.c as usize, pc);
                                }
                                Err(error) => return Err(error),
                            }
                            if self.frames.len() > depth {
                                self.frames.last_mut().expect("yield-from frame").action =
                                    ReturnAction::YieldFrom {
                                        target: i.c as usize,
                                        pc,
                                        iterator,
                                    };
                            } else if self.frames.last().is_some_and(|frame| frame.ip == pc + 1) {
                                self.frames
                                    .last_mut()
                                    .expect("active yield-from frame")
                                    .yield_from = Some(iterator);
                            }
                        }
                    }
                    Op::Import => {
                        let name = &p.symbols[i.b as usize];
                        self.invoke_import(p, name, a)?;
                    }
                    Op::ImportFrom => {
                        let module = self.read(b)?;
                        let name = &p.symbols[i.c as usize];
                        self.invoke_import_from(p, module, name, a)?;
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
                                        return Ok(());
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
                                        return Ok(());
                                    }
                                    self.stats.attr_cache_misses += 1;
                                    self.adaptive_sites[code_id][pc] = AdaptiveState::Generic;
                                }
                                _ => {}
                            }
                        }
                        self.invoke_attribute_get(
                            p,
                            object,
                            name,
                            a,
                            AttributeMissing::Raise,
                            output,
                        )?;
                        let cache = self.heap.instance_slot_cache(object, name);
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
                Ok(())
            })();
            if let Err(error) = step {
                if self.dispatch_exception(p, output, &error, depth, code_id, pc)? {
                    continue;
                }
                return Err(error);
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
            || metadata.generator
        {
            return None;
        }
        for (index, keyword) in site.keywords.iter().copied().enumerate() {
            if let Some(slot) = (usize::from(signature.posonly)..named)
                .find(|slot| metadata.locals[*slot] == keyword)
            {
                if slot < positional_bound || site.keywords[..index].contains(&keyword) {
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
            argument_base: self.arguments.len(),
            pending_class_base: self.pending_classes.len(),
            callable: Some(callable),
            generator: None,
            yield_from: None,
            injected_exception: None,
            exception_stack: Vec::new(),
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
        if program.code[frame.code].generator {
            frame.jit_attempted = true;
            return Ok(false);
        }
        let resuming = frame.jit_resume;
        if program
            .modules
            .iter()
            .any(|module| usize::from(module.code) == frame.code)
            || (!resuming && (frame.ip != 0 || frame.jit_attempted))
        {
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
                    let Some(next_code_bytes) = self
                        .jit_code_budget_used
                        .checked_add(metadata.code_bytes)
                        .filter(|total| *total <= self.jit_max_code_bytes)
                    else {
                        self.stats.jit_code_budget_rejections += 1;
                        self.stats.jit_fallbacks += 1;
                        self.jit_cache[code_id] = JitEntry::Unsupported;
                        return Ok(false);
                    };
                    self.jit_code_budget_used = next_code_bytes;
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
            self.append_gc_roots(Some(base..end), false, &mut roots);
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
                runtime_owner: &self.runtime_owner,
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
    fn enter_class_body(
        &mut self,
        program: &Program,
        destination: usize,
        build: ClassBuild,
        mapping: Option<Value>,
    ) -> Result<()> {
        let ClassBuild {
            function,
            bases,
            declared_bases,
            metaclass,
            qualname,
        } = build;
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
        if execution != self.execution || !program.code[body].class_body {
            return Err(Diagnostic::new("BytecodeError", "invalid class body"));
        }
        let namespace = if let Some(mapping) = mapping {
            self.heap
                .namespace_from_mapping(&qualname, bases, metaclass, mapping)?
        } else {
            self.heap
                .namespace_with_metaclass(&qualname, bases, metaclass)?
        };
        self.enter_frame(
            program,
            body,
            Some(destination),
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
        frame.action = ReturnAction::ClassBody(ClassBody {
            namespace,
            declared_bases,
            metaclass,
            qualname,
        });
        Ok(())
    }

    fn complete_class_body(
        &mut self,
        p: &Program,
        destination: usize,
        body: ClassBody,
        class_cell: Option<Value>,
        output: &mut dyn Write,
    ) -> Result<()> {
        let custom_new =
            self.heap
                .has_custom_metaclass_hook(body.metaclass, self.type_class, "__new__")?;
        let custom_init =
            self.heap
                .has_custom_metaclass_hook(body.metaclass, self.type_class, "__init__")?;
        if !custom_new && !custom_init {
            self.validate_builtin_subclass_bases(&body.declared_bases)?;
            self.heap.finish_class(body.namespace)?;
            if let Some(cell) = class_cell {
                self.heap.store_cell(cell, body.namespace)?;
            }
            let mut pending = self
                .heap
                .descriptor_set_names(body.namespace)?
                .into_iter()
                .map(|(call, name)| SetNameCall { call, name })
                .collect::<Vec<_>>();
            pending.reverse();
            return self.invoke_set_names(p, destination, body.namespace, pending, None, output);
        }
        let mapping = self.heap.namespace_mapping(body.namespace)?;
        let name = body.qualname.rsplit('.').next().unwrap_or(&body.qualname);
        let name = self.heap.alloc(Object::Str(name.to_owned()))?;
        let bases = self.heap.alloc(Object::Tuple(body.declared_bases))?;
        let state = ClassHookState {
            namespace: body.namespace,
            mapping,
            metaclass: body.metaclass,
            name,
            bases,
            class_cell,
        };
        if custom_new {
            let constructor = self
                .heap
                .class_lookup(state.metaclass, "__new__")?
                .ok_or_else(|| Diagnostic::new("TypeError", "metaclass has no __new__"))?;
            self.pending_classes.push(PendingClass {
                state: state.clone(),
                finalized: false,
            });
            let depth = self.frames.len();
            let result = self.invoke_target(
                p,
                constructor,
                destination,
                Arguments::Expanded(ExpandedArgs {
                    positional: vec![state.metaclass, state.name, state.bases, state.mapping],
                    ..ExpandedArgs::default()
                }),
                output,
            );
            if let Err(error) = result {
                self.pending_classes.pop();
                return Err(error);
            }
            if self.frames.len() > depth {
                self.frames
                    .last_mut()
                    .expect("metaclass __new__ frame")
                    .action = ReturnAction::MetaclassNew(state);
            } else {
                let result = self.registers[destination];
                self.finish_metaclass_new(p, destination, state, result, output)?;
            }
            return Ok(());
        }

        self.finalize_class(&state)?;
        let mut pending = self
            .heap
            .descriptor_set_names(state.namespace)?
            .into_iter()
            .map(|(call, name)| SetNameCall { call, name })
            .collect::<Vec<_>>();
        pending.reverse();
        self.invoke_set_names(
            p,
            destination,
            state.namespace,
            pending,
            Some(state),
            output,
        )
    }

    fn finalize_class(&mut self, state: &ClassHookState) -> Result<()> {
        let bases = match self.heap.get(state.namespace)? {
            Object::Namespace(class) => class.bases.clone(),
            Object::Class(class) => class.bases.clone(),
            _ => return Err(Diagnostic::new("BytecodeError", "missing class namespace")),
        };
        self.validate_builtin_subclass_bases(&bases)?;
        self.heap.finish_class(state.namespace)?;
        if let Some(cell) = state.class_cell {
            self.heap.store_cell(cell, state.namespace)?;
        }
        Ok(())
    }

    pub(super) fn invoke_dynamic_type(
        &mut self,
        p: &Program,
        destination: usize,
        name: Value,
        bases: Value,
        mapping: Value,
        output: &mut dyn Write,
    ) -> Result<()> {
        let name = match self.heap.get(name)? {
            Object::Str(name) => name.clone(),
            _ => return Err(Diagnostic::new("TypeError", "type name must be a string")),
        };
        let mut bases = match self.heap.get(bases)? {
            Object::Tuple(bases) => bases.clone(),
            _ => return Err(Diagnostic::new("TypeError", "type bases must be a tuple")),
        };
        if bases.is_empty() {
            bases.push(self.object_class);
        }
        self.validate_builtin_subclass_bases(&bases)?;
        let class =
            self.heap
                .namespace_from_type_mapping(&name, bases, self.type_class, mapping)?;
        self.heap.finish_class(class)?;
        let mut pending = self
            .heap
            .descriptor_set_names(class)?
            .into_iter()
            .map(|(call, name)| SetNameCall { call, name })
            .collect::<Vec<_>>();
        pending.reverse();
        self.invoke_set_names(p, destination, class, pending, None, output)
    }

    pub(super) fn invoke_type_new(
        &mut self,
        p: &Program,
        destination: usize,
        args: &Arguments<'_>,
        output: &mut dyn Write,
    ) -> Result<()> {
        if args.keyword_count() != 0 || args.count() != 4 {
            return Err(Diagnostic::new(
                "TypeError",
                "type.__new__ expects metaclass, name, bases, and namespace",
            ));
        }
        let supplied = [
            args.positional(&self.registers, 0),
            args.positional(&self.registers, 1),
            args.positional(&self.registers, 2),
            args.positional(&self.registers, 3),
        ];
        let index = self
            .pending_classes
            .iter()
            .rposition(|pending| {
                !pending.finalized
                    && pending.state.metaclass == supplied[0]
                    && pending.state.name == supplied[1]
                    && pending.state.bases == supplied[2]
                    && (pending.state.mapping == supplied[3]
                        || matches!(self.heap.try_get(supplied[3]), Some(Object::Dict(_))))
            })
            .ok_or_else(|| {
                Diagnostic::new(
                    "UnsupportedFeature",
                    "type.__new__ is currently available only inside an active metaclass __new__",
                )
            })?;
        let state = self.pending_classes[index].state.clone();
        if state.mapping != supplied[3] {
            self.heap
                .namespace_use_mapping(state.namespace, supplied[3])?;
        }
        self.finalize_class(&state)?;
        self.pending_classes[index].finalized = true;
        let mut pending = self
            .heap
            .descriptor_set_names(state.namespace)?
            .into_iter()
            .map(|(call, name)| SetNameCall { call, name })
            .collect::<Vec<_>>();
        pending.reverse();
        self.invoke_set_names(p, destination, state.namespace, pending, None, output)
    }

    fn finish_metaclass_new(
        &mut self,
        p: &Program,
        destination: usize,
        state: ClassHookState,
        result: Value,
        output: &mut dyn Write,
    ) -> Result<()> {
        let index = self
            .pending_classes
            .iter()
            .rposition(|pending| pending.state.namespace == state.namespace)
            .ok_or_else(|| Diagnostic::new("RuntimeError", "missing metaclass creation state"))?;
        self.pending_classes.remove(index);
        if !self.instance_check(result, state.metaclass, false, 0)? {
            self.registers[destination] = result;
            return Ok(());
        }
        self.invoke_metaclass_init(p, destination, result, state, output)
    }

    fn invoke_metaclass_init(
        &mut self,
        p: &Program,
        destination: usize,
        class: Value,
        state: ClassHookState,
        output: &mut dyn Write,
    ) -> Result<()> {
        if !self
            .heap
            .has_custom_metaclass_hook(state.metaclass, self.type_class, "__init__")?
        {
            self.registers[destination] = class;
            return Ok(());
        }
        let call = self
            .heap
            .metaclass_method_call(class, "__init__")?
            .ok_or_else(|| Diagnostic::new("TypeError", "metaclass has no __init__"))?;
        let depth = self.frames.len();
        self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Expanded(ExpandedArgs {
                receiver: call.receiver,
                positional: vec![state.name, state.bases, state.mapping],
                ..ExpandedArgs::default()
            }),
            output,
        )?;
        if self.frames.len() > depth {
            self.frames
                .last_mut()
                .expect("metaclass __init__ frame")
                .action = ReturnAction::MetaclassInit(class);
        } else if self.registers[destination] != Value::NONE {
            return Err(Diagnostic::new(
                "TypeError",
                "metaclass __init__ must return None",
            ));
        } else {
            self.registers[destination] = class;
        }
        Ok(())
    }

    fn invoke_set_names(
        &mut self,
        p: &Program,
        destination: usize,
        class: Value,
        mut pending: Vec<SetNameCall>,
        after: Option<ClassHookState>,
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
                    positional: [class, item.name, Value::UNBOUND],
                    count: 2,
                },
                output,
            )?;
            if self.frames.len() > depth {
                self.frames
                    .last_mut()
                    .expect("set_name callback frame")
                    .action = ReturnAction::SetNames {
                    class,
                    pending,
                    after,
                };
                return Ok(());
            }
            self.registers[destination] = class;
        }
        if let Some(after) = after {
            self.invoke_metaclass_init(p, destination, class, after, output)
        } else {
            self.registers[destination] = class;
            Ok(())
        }
    }
}

fn module_filename(program: &Program, code: usize) -> &str {
    program
        .modules
        .iter()
        .find(|module| {
            let start = usize::from(module.code);
            (start..start + usize::from(module.code_count)).contains(&code)
        })
        .map_or("<unknown>", |module| module.filename.as_str())
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

pub(super) fn base_binary_op(op: Op) -> Op {
    match op {
        Op::InplaceAdd => Op::Add,
        Op::InplaceSub => Op::Sub,
        Op::InplaceMul => Op::Mul,
        Op::InplaceDiv => Op::Div,
        Op::InplaceFloorDiv => Op::FloorDiv,
        Op::InplaceMod => Op::Mod,
        Op::InplacePow => Op::Pow,
        Op::InplaceBitOr => Op::BitOr,
        Op::InplaceBitXor => Op::BitXor,
        Op::InplaceBitAnd => Op::BitAnd,
        Op::InplaceLeftShift => Op::LeftShift,
        Op::InplaceRightShift => Op::RightShift,
        op => op,
    }
}
