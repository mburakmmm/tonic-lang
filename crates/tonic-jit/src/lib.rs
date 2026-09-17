//! Isolated Cranelift backend for verified Tonic register bytecode.
//!
//! This initial tier compiles leaf functions whose live operations stay inside
//! immediate signed integers, booleans, and `None`, plus explicit runtime
//! helpers for allocation-producing operations. Bound globals are read from a
//! runtime-owned materialized value slice; missing names use the diagnostic
//! helper. Guards return the exact bytecode PC with materialized registers so the runtime can resume in the generic
//! interpreter. Opaque logical value words cross the ABI; heap layouts and native
//! object addresses do not.

use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, InstBuilder, MemFlags, StackSlot, StackSlotData,
    StackSlotKind, UserFuncName, Value,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};
use std::{
    collections::VecDeque,
    ffi::c_void,
    fmt, mem,
    panic::{catch_unwind, AssertUnwindSafe},
    time::Duration,
};
use tonic_core::{
    ast::Constant,
    bytecode::{CodeObject, Op},
};

pub const CRANELIFT_VERSION: &str = cranelift_codegen::VERSION;
pub const VALUE_NONE: u64 = 4;
pub const VALUE_UNBOUND: u64 = 5;
const TAG_MASK: i64 = 7;
const INT_TAG: i64 = 1;
const VALUE_FALSE: i64 = 2;
const VALUE_TRUE: i64 = 3;
const MIN_INT: i64 = -(1_i64 << 60);
const MAX_INT: i64 = (1_i64 << 60) - 1;
const RETURN_FLAG: u64 = 1 << 63;
const ERROR_FLAG: u64 = 1 << 62;
const SIDE_EXIT_FLAG: u64 = 1 << 61;
const BACKEDGE_POLL_INTERVAL: i64 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RuntimeOp {
    Div = 0,
    LoadGlobal = 1,
    Poll = 2,
    Add = 3,
    InplaceAdd = 4,
    Sub = 5,
    Mul = 6,
    FloorDiv = 7,
    Mod = 8,
    LoadMethod = 9,
    LoadSequenceItem = 10,
    LoadMappingItem = 11,
    BuildTuple = 12,
    BuildDict = 13,
    DictSetSymbol = 14,
    UnboxFloat = 15,
    BoxFloat = 16,
    Eq = 17,
    Ne = 18,
    Lt = 19,
    Le = 20,
    Gt = 21,
    Ge = 22,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeFailure {
    pub kind: String,
    pub message: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodLookup {
    pub function: u64,
    pub receiver: u64,
}
impl RuntimeFailure {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

/// Runtime services callable by generated code. The register slice is the
/// precise set of materialized JIT roots at this safepoint.
pub trait Runtime {
    fn binary(
        &mut self,
        op: RuntimeOp,
        left: u64,
        right: u64,
        registers: &[u64],
    ) -> Result<u64, RuntimeFailure>;

    fn load_global(&mut self, symbol: u32, registers: &[u64]) -> Result<u64, RuntimeFailure>;

    fn load_method(
        &mut self,
        _owner: u64,
        _selector: u64,
        _registers: &[u64],
    ) -> Result<MethodLookup, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires method lookup services",
        ))
    }

    /// Return one item only for an exact built-in list/tuple with the profiled
    /// length. `None` is a guard miss rather than a guest exception.
    fn load_sequence_item(
        &mut self,
        _owner: u64,
        _index: u32,
        _length: u32,
        _registers: &[u64],
    ) -> Result<Option<u64>, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires sequence expansion services",
        ))
    }

    /// Return a current value only for an exact built-in dict with the
    /// profiled key count and requested interned string key.
    fn load_mapping_item(
        &mut self,
        _owner: u64,
        _symbol: u32,
        _key_count: u32,
        _registers: &[u64],
    ) -> Result<Option<u64>, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires mapping expansion services",
        ))
    }

    fn build_tuple(
        &mut self,
        _first: u32,
        _count: u32,
        _registers: &[u64],
    ) -> Result<u64, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires tuple materialization services",
        ))
    }

    fn build_dict(&mut self, _registers: &[u64]) -> Result<u64, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires dict materialization services",
        ))
    }

    fn dict_set_symbol(
        &mut self,
        _owner: u64,
        _symbol: u32,
        _value_register: u32,
        _registers: &[u64],
    ) -> Result<u64, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires keyword materialization services",
        ))
    }

    /// Return the raw IEEE-754 bits for an exact Tonic float. `None` is a
    /// guard miss and must deoptimize rather than become a guest exception.
    fn unbox_float(
        &mut self,
        _value: u64,
        _registers: &[u64],
    ) -> Result<Option<u64>, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires float unboxing services",
        ))
    }

    /// Allocate the guest-visible float returned by an otherwise-unboxed
    /// direct leaf.
    fn box_float(&mut self, _bits: u64, _registers: &[u64]) -> Result<u64, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires float boxing services",
        ))
    }

    fn poll(&mut self, registers: &[u64]) -> Result<u64, RuntimeFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    pub pc: usize,
    pub opcode: Option<Op>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unsupported(Unsupported),
    InvalidBytecode { pc: Option<usize>, message: String },
    Backend(String),
    RegisterCount { expected: usize, actual: usize },
    Runtime { pc: usize, failure: RuntimeFailure },
    ResumePc { pc: usize, instructions: usize },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(u) => write!(f, "unsupported JIT bytecode at {}: {}", u.pc, u.reason),
            Self::InvalidBytecode { pc, message } => match pc {
                Some(pc) => write!(f, "invalid JIT bytecode at {pc}: {message}"),
                None => write!(f, "invalid JIT bytecode: {message}"),
            },
            Self::Backend(message) => write!(f, "Cranelift backend error: {message}"),
            Self::RegisterCount { expected, actual } => {
                write!(f, "JIT expected {expected} registers, got {actual}")
            }
            Self::Runtime { failure, .. } => write!(f, "{}: {}", failure.kind, failure.message),
            Self::ResumePc { pc, instructions } => write!(
                f,
                "JIT resume PC {pc} is outside {instructions} compiled instructions"
            ),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Returned { value: u64, pc: usize },
    Deopt { pc: usize },
    SideExit { pc: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
    pub code_bytes: usize,
    pub compile_time: Duration,
    /// Guest-visible bytecode registers at the start of the root buffer.
    pub register_count: usize,
    /// Entire precise root buffer, including JIT-private dependency caches.
    pub root_count: usize,
    pub runtime_calls: usize,
    pub safepoints: usize,
    pub resumable: bool,
    pub instruction_count: usize,
    pub direct_call_sites: usize,
    pub direct_method_sites: usize,
}

/// Profile-backed exact-callee call that may be folded into a caller. The
/// target is Tonic bytecode rather than a native address, so compiled code never
/// persists pointers into the moving guest heap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectArgument {
    /// Zero-based offset in the caller call site's positional+keyword window.
    Caller(u16),
    /// Absolute caller virtual register used by an expanded call.
    Register(u16),
    /// Item from an exact built-in list/tuple guarded by its current length.
    SequenceItem {
        register: u16,
        index: u16,
        length: u16,
    },
    /// Value from an exact built-in dict guarded by its complete key count.
    MappingItem {
        register: u16,
        symbol: u16,
        key_count: u16,
    },
    /// Materialized `*args` tuple from a contiguous ordinary-call window.
    VariadicTuple { first: u16, count: u16 },
    /// Materialized `**kwargs` dict from `(symbol, absolute register)` pairs.
    VariadicDict { items: Vec<(u16, u16)> },
    /// Receiver captured by the fused ATTR helper in the hidden root tail.
    MethodReceiver,
    /// Exact function default captured when the callee profile was compiled.
    Default(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MethodBinding {
    Static = 0,
    Instance = 1,
    Class = 2,
}

#[derive(Clone)]
pub struct DirectCall<'a> {
    pub pc: usize,
    pub callee: u64,
    pub target: &'a CodeObject,
    /// Fully bound target parameter slots, in target slot order. The plan is
    /// produced by the runtime's ordinary signature rules and allocates no
    /// guest tuple or keyword dictionary when generated code executes it.
    pub arguments: Vec<DirectArgument>,
    /// `ATTR` PC whose plain instance-method result is consumed by this call.
    /// A miss deoptimizes to this PC so the interpreter can reconstruct the
    /// observable bound-method value before replaying the pure argument setup.
    pub method_attr_pc: Option<usize>,
    /// Exact descriptor binding behavior guarded by the method helper.
    pub method_binding: Option<MethodBinding>,
    /// Matching `BEGIN_ARGS` for a guarded sequence/named expansion.
    pub expanded_begin_pc: Option<usize>,
    /// Profile-backed exact-float leaf. Arguments are unboxed once, arithmetic
    /// stays in Cranelift F64 SSA, and only the guest-visible result is boxed.
    pub float: bool,
}

/// Runtime-materialized value for a non-immediate CONST instruction. The word
/// is an opaque logical handle rooted by the owning VM program, never a heap
/// address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializedConstant {
    pub pc: usize,
    pub value: u64,
}

/// Interpreter reconstruction metadata for one bytecode location. Registers
/// not listed in `unboxed_float_registers` are already materialized in the
/// precise root buffer at this point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeoptMap {
    pub pc: usize,
    pub register_count: usize,
    pub unboxed_float_registers: Vec<u16>,
}

struct FloatAnalysis {
    before: Vec<Option<Vec<bool>>>,
    slots: Vec<bool>,
    deopt_maps: Vec<DeoptMap>,
}

type RuntimeHelper = extern "C" fn(*mut c_void, *const u64, usize, u32, u64, u64, *mut u64) -> u32;
type Entry =
    extern "C" fn(*mut u64, *const u64, usize, *mut c_void, RuntimeHelper, *mut u64, usize) -> u64;

struct CallState<'a, R> {
    runtime: &'a mut R,
    failure: Option<RuntimeFailure>,
}

extern "C" fn runtime_helper<R: Runtime>(
    context: *mut c_void,
    registers: *const u64,
    register_count: usize,
    op: u32,
    left: u64,
    right: u64,
    output: *mut u64,
) -> u32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `run_with_runtime` passes a live `CallState<R>` and the
        // generated function calls this helper synchronously before returning.
        let state = unsafe { &mut *context.cast::<CallState<'_, R>>() };
        // SAFETY: the generated function forwards its writable root buffer and
        // the exact guest-register plus JIT-private-root count checked at entry.
        let roots = unsafe { std::slice::from_raw_parts(registers, register_count) };
        let op = match op {
            value if value == RuntimeOp::Div as u32 => RuntimeOp::Div,
            value if value == RuntimeOp::LoadGlobal as u32 => RuntimeOp::LoadGlobal,
            value if value == RuntimeOp::Poll as u32 => RuntimeOp::Poll,
            value if value == RuntimeOp::Add as u32 => RuntimeOp::Add,
            value if value == RuntimeOp::InplaceAdd as u32 => RuntimeOp::InplaceAdd,
            value if value == RuntimeOp::Sub as u32 => RuntimeOp::Sub,
            value if value == RuntimeOp::Mul as u32 => RuntimeOp::Mul,
            value if value == RuntimeOp::FloorDiv as u32 => RuntimeOp::FloorDiv,
            value if value == RuntimeOp::Mod as u32 => RuntimeOp::Mod,
            value if value == RuntimeOp::LoadMethod as u32 => RuntimeOp::LoadMethod,
            value if value == RuntimeOp::LoadSequenceItem as u32 => RuntimeOp::LoadSequenceItem,
            value if value == RuntimeOp::LoadMappingItem as u32 => RuntimeOp::LoadMappingItem,
            value if value == RuntimeOp::BuildTuple as u32 => RuntimeOp::BuildTuple,
            value if value == RuntimeOp::BuildDict as u32 => RuntimeOp::BuildDict,
            value if value == RuntimeOp::DictSetSymbol as u32 => RuntimeOp::DictSetSymbol,
            value if value == RuntimeOp::UnboxFloat as u32 => RuntimeOp::UnboxFloat,
            value if value == RuntimeOp::BoxFloat as u32 => RuntimeOp::BoxFloat,
            value if value == RuntimeOp::Eq as u32 => RuntimeOp::Eq,
            value if value == RuntimeOp::Ne as u32 => RuntimeOp::Ne,
            value if value == RuntimeOp::Lt as u32 => RuntimeOp::Lt,
            value if value == RuntimeOp::Le as u32 => RuntimeOp::Le,
            value if value == RuntimeOp::Gt as u32 => RuntimeOp::Gt,
            value if value == RuntimeOp::Ge as u32 => RuntimeOp::Ge,
            _ => {
                state.failure = Some(RuntimeFailure::new(
                    "JitError",
                    "generated code requested an unknown runtime operation",
                ));
                return 1;
            }
        };
        if op == RuntimeOp::LoadMethod {
            return match state.runtime.load_method(left, right, roots) {
                Ok(lookup) => {
                    // SAFETY: generated LoadMethod calls pass a writable
                    // two-word region inside the precise JIT root buffer.
                    unsafe {
                        output.write(lookup.function);
                        output.add(1).write(lookup.receiver);
                    }
                    0
                }
                Err(failure) => {
                    state.failure = Some(failure);
                    1
                }
            };
        }
        if op == RuntimeOp::LoadSequenceItem {
            let index = right as u32;
            let length = (right >> 32) as u32;
            return match state.runtime.load_sequence_item(left, index, length, roots) {
                Ok(value) => {
                    // SAFETY: generated code passes a destination inside its
                    // writable, verified precise-root buffer.
                    unsafe { output.write(value.unwrap_or(VALUE_UNBOUND)) };
                    0
                }
                Err(failure) => {
                    state.failure = Some(failure);
                    1
                }
            };
        }
        if op == RuntimeOp::LoadMappingItem {
            let symbol = right as u32;
            let key_count = (right >> 32) as u32;
            return match state
                .runtime
                .load_mapping_item(left, symbol, key_count, roots)
            {
                Ok(value) => {
                    // SAFETY: generated code passes a destination inside its
                    // writable, verified precise-root buffer.
                    unsafe { output.write(value.unwrap_or(VALUE_UNBOUND)) };
                    0
                }
                Err(failure) => {
                    state.failure = Some(failure);
                    1
                }
            };
        }
        if op == RuntimeOp::BuildTuple {
            return match state.runtime.build_tuple(left as u32, right as u32, roots) {
                Ok(value) => {
                    // SAFETY: generated code passes a destination inside its
                    // writable, verified precise-root buffer.
                    unsafe { output.write(value) };
                    0
                }
                Err(failure) => {
                    state.failure = Some(failure);
                    1
                }
            };
        }
        if op == RuntimeOp::BuildDict {
            return match state.runtime.build_dict(roots) {
                Ok(value) => {
                    // SAFETY: generated code passes a destination inside its
                    // writable, verified precise-root buffer.
                    unsafe { output.write(value) };
                    0
                }
                Err(failure) => {
                    state.failure = Some(failure);
                    1
                }
            };
        }
        if op == RuntimeOp::DictSetSymbol {
            let symbol = right as u32;
            let value_register = (right >> 32) as u32;
            return match state
                .runtime
                .dict_set_symbol(left, symbol, value_register, roots)
            {
                Ok(value) => {
                    // SAFETY: generated code passes a destination inside its
                    // writable, verified precise-root buffer.
                    unsafe { output.write(value) };
                    0
                }
                Err(failure) => {
                    state.failure = Some(failure);
                    1
                }
            };
        }
        if op == RuntimeOp::UnboxFloat {
            return match state.runtime.unbox_float(left, roots) {
                Ok(value) => {
                    // SAFETY: generated UnboxFloat calls pass a writable
                    // two-word stack slot. Float bits are not managed roots.
                    unsafe {
                        output.write(value.unwrap_or(0));
                        output.add(1).write(u64::from(value.is_some()));
                    }
                    0
                }
                Err(failure) => {
                    state.failure = Some(failure);
                    1
                }
            };
        }
        if op == RuntimeOp::BoxFloat {
            return match state.runtime.box_float(left, roots) {
                Ok(value) => {
                    // SAFETY: generated code passes a destination in its
                    // writable, verified precise-root buffer.
                    unsafe { output.write(value) };
                    0
                }
                Err(failure) => {
                    state.failure = Some(failure);
                    1
                }
            };
        }
        let result = match op {
            RuntimeOp::Div
            | RuntimeOp::Add
            | RuntimeOp::InplaceAdd
            | RuntimeOp::Sub
            | RuntimeOp::Mul
            | RuntimeOp::FloorDiv
            | RuntimeOp::Mod
            | RuntimeOp::Eq
            | RuntimeOp::Ne
            | RuntimeOp::Lt
            | RuntimeOp::Le
            | RuntimeOp::Gt
            | RuntimeOp::Ge => state.runtime.binary(op, left, right, roots),
            RuntimeOp::LoadGlobal => state.runtime.load_global(left as u32, roots),
            RuntimeOp::LoadMethod
            | RuntimeOp::LoadSequenceItem
            | RuntimeOp::LoadMappingItem
            | RuntimeOp::BuildTuple
            | RuntimeOp::BuildDict
            | RuntimeOp::DictSetSymbol
            | RuntimeOp::UnboxFloat
            | RuntimeOp::BoxFloat => unreachable!("handled above"),
            RuntimeOp::Poll => state.runtime.poll(roots),
        };
        match result {
            Ok(value) => {
                // SAFETY: generated code passes a destination inside the same
                // writable, verified register array.
                unsafe { output.write(value) };
                0
            }
            Err(failure) => {
                state.failure = Some(failure);
                1
            }
        }
    }));
    match result {
        Ok(status) => status,
        Err(_) => {
            // SAFETY: the context invariant is identical to the first access.
            let state = unsafe { &mut *context.cast::<CallState<'_, R>>() };
            state.failure = Some(RuntimeFailure::new("JitError", "runtime helper panicked"));
            2
        }
    }
}

struct UnavailableRuntime;
impl Runtime for UnavailableRuntime {
    fn binary(
        &mut self,
        _op: RuntimeOp,
        _left: u64,
        _right: u64,
        _registers: &[u64],
    ) -> Result<u64, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires runtime services",
        ))
    }

    fn load_global(&mut self, _symbol: u32, _registers: &[u64]) -> Result<u64, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires runtime services",
        ))
    }

    fn poll(&mut self, _registers: &[u64]) -> Result<u64, RuntimeFailure> {
        Err(RuntimeFailure::new(
            "JitError",
            "compiled function requires runtime services",
        ))
    }
}

pub struct CompiledFunction {
    // Executable memory belongs to the module and must outlive `entry`.
    _module: JITModule,
    entry: Entry,
    metadata: Metadata,
    deopt_maps: Vec<DeoptMap>,
}
impl CompiledFunction {
    pub fn metadata(&self) -> Metadata {
        self.metadata
    }

    pub fn deopt_maps(&self) -> &[DeoptMap] {
        &self.deopt_maps
    }

    pub fn run(&self, registers: &mut [u64]) -> Result<Outcome, Error> {
        self.run_from(registers, 0, &mut UnavailableRuntime)
    }

    pub fn run_with_runtime<R: Runtime>(
        &self,
        registers: &mut [u64],
        runtime: &mut R,
    ) -> Result<Outcome, Error> {
        self.run_from(registers, 0, runtime)
    }

    pub fn run_from<R: Runtime>(
        &self,
        registers: &mut [u64],
        start_pc: usize,
        runtime: &mut R,
    ) -> Result<Outcome, Error> {
        self.run_from_with_globals(registers, &[], start_pc, runtime)
    }

    pub fn run_from_with_globals<R: Runtime>(
        &self,
        registers: &mut [u64],
        globals: &[u64],
        start_pc: usize,
        runtime: &mut R,
    ) -> Result<Outcome, Error> {
        let mut direct_calls = 0;
        self.run_from_with_globals_counted(registers, globals, start_pc, runtime, &mut direct_calls)
    }

    pub fn run_from_with_globals_counted<R: Runtime>(
        &self,
        registers: &mut [u64],
        globals: &[u64],
        start_pc: usize,
        runtime: &mut R,
        direct_calls: &mut u64,
    ) -> Result<Outcome, Error> {
        if registers.len() != self.metadata.root_count {
            return Err(Error::RegisterCount {
                expected: self.metadata.root_count,
                actual: registers.len(),
            });
        }
        if start_pc >= self.metadata.instruction_count
            || (start_pc != 0 && !self.metadata.resumable)
        {
            return Err(Error::ResumePc {
                pc: start_pc,
                instructions: self.metadata.instruction_count,
            });
        }
        let mut state = CallState {
            runtime,
            failure: None,
        };
        let status = (self.entry)(
            registers.as_mut_ptr(),
            globals.as_ptr(),
            globals.len(),
            (&mut state as *mut CallState<'_, R>).cast(),
            runtime_helper::<R>,
            direct_calls,
            start_pc,
        );
        if status & RETURN_FLAG != 0 {
            Ok(Outcome::Returned {
                value: registers[0],
                pc: (status & !RETURN_FLAG) as usize,
            })
        } else if status & ERROR_FLAG != 0 {
            Err(Error::Runtime {
                pc: (status & !(RETURN_FLAG | ERROR_FLAG)) as usize,
                failure: state.failure.unwrap_or_else(|| {
                    RuntimeFailure::new("JitError", "runtime helper failed without a diagnostic")
                }),
            })
        } else if status & SIDE_EXIT_FLAG != 0 {
            Ok(Outcome::SideExit {
                pc: (status & !(RETURN_FLAG | ERROR_FLAG | SIDE_EXIT_FLAG)) as usize,
            })
        } else {
            Ok(Outcome::Deopt {
                pc: status as usize,
            })
        }
    }
}

pub fn compile(code: &CodeObject) -> Result<CompiledFunction, Error> {
    compile_with_direct_calls(code, &[])
}

pub fn compile_with_direct_calls(
    code: &CodeObject,
    direct_calls: &[DirectCall<'_>],
) -> Result<CompiledFunction, Error> {
    compile_with_direct_calls_and_constants(code, direct_calls, &[])
}

pub fn compile_with_direct_calls_and_constants(
    code: &CodeObject,
    direct_calls: &[DirectCall<'_>],
    materialized_constants: &[MaterializedConstant],
) -> Result<CompiledFunction, Error> {
    compile_with_execution_profile(code, direct_calls, materialized_constants, &[])
}

pub fn compile_with_execution_profile(
    code: &CodeObject,
    direct_calls: &[DirectCall<'_>],
    materialized_constants: &[MaterializedConstant],
    exact_float_parameters: &[u16],
) -> Result<CompiledFunction, Error> {
    validate_structural_safety(code)?;
    for direct in direct_calls {
        validate_structural_safety(direct.target)?;
    }
    if exact_float_parameters
        .iter()
        .enumerate()
        .any(|(index, register)| {
            *register >= code.registers || exact_float_parameters[..index].contains(register)
        })
    {
        return Err(Error::InvalidBytecode {
            pc: None,
            message: "invalid exact-float parameter profile".into(),
        });
    }
    validate_direct_calls(code, direct_calls)?;
    validate_materialized_constants(code, materialized_constants)?;
    validate_supported(code, direct_calls, materialized_constants)?;
    let float_analysis = analyze_float_execution(code, exact_float_parameters)?;
    const METHOD_CACHE_WORDS: usize = 4;
    let method_site_count = direct_calls
        .iter()
        .filter(|call| call.method_attr_pc.is_some())
        .count();
    let dynamic_argument_count = direct_calls
        .iter()
        .flat_map(|call| &call.arguments)
        .filter(|argument| {
            matches!(
                argument,
                DirectArgument::SequenceItem { .. }
                    | DirectArgument::MappingItem { .. }
                    | DirectArgument::VariadicTuple { .. }
                    | DirectArgument::VariadicDict { .. }
            )
        })
        .count()
        + direct_calls.iter().filter(|call| call.float).count();
    let root_count = method_site_count
        .checked_mul(METHOD_CACHE_WORDS)
        .and_then(|method_words| method_words.checked_add(dynamic_argument_count))
        .and_then(|hidden| (code.registers as usize).checked_add(hidden))
        .ok_or_else(|| Error::Backend("JIT root buffer size overflow".into()))?;
    if root_count > i32::MAX as usize / mem::size_of::<u64>() {
        return Err(Error::Backend(
            "JIT root buffer exceeds Cranelift addressable displacement".into(),
        ));
    }
    let backedges = backedge_count(code);
    let resumable = code.instructions.iter().any(|instruction| {
        matches!(
            Op::try_from(instruction.opcode),
            Ok(Op::Call | Op::Attr | Op::BeginArgs | Op::CallExpanded)
        )
    }) || backedges > 0;
    let started = std::time::Instant::now();
    let builder = JITBuilder::with_flags(
        &[("opt_level", "speed"), ("enable_verifier", "true")],
        default_libcall_names(),
    )
    .map_err(|error| Error::Backend(error.to_string()))?;
    let mut module = JITModule::new(builder);
    let pointer_type = module.target_config().pointer_type();
    let mut signature = module.make_signature();
    signature.params.push(AbiParam::new(pointer_type));
    signature.params.push(AbiParam::new(pointer_type));
    signature.params.push(AbiParam::new(pointer_type));
    signature.params.push(AbiParam::new(pointer_type));
    signature.params.push(AbiParam::new(pointer_type));
    signature.params.push(AbiParam::new(pointer_type));
    signature.params.push(AbiParam::new(pointer_type));
    signature.returns.push(AbiParam::new(types::I64));
    let mut runtime_signature = module.make_signature();
    runtime_signature.params.push(AbiParam::new(pointer_type));
    runtime_signature.params.push(AbiParam::new(pointer_type));
    runtime_signature.params.push(AbiParam::new(pointer_type));
    runtime_signature.params.push(AbiParam::new(types::I32));
    runtime_signature.params.push(AbiParam::new(types::I64));
    runtime_signature.params.push(AbiParam::new(types::I64));
    runtime_signature.params.push(AbiParam::new(pointer_type));
    runtime_signature.returns.push(AbiParam::new(types::I32));
    let function_id = module
        .declare_function("tonic_leaf", Linkage::Local, &signature)
        .map_err(|error| Error::Backend(error.to_string()))?;
    let mut context = module.make_context();
    context.func.signature = signature;
    context.func.name = UserFuncName::user(0, function_id.as_u32());
    let runtime_signature = context.func.import_signature(runtime_signature);
    let mut frontend = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut frontend);
        let poll_slot = (backedges > 0).then(|| {
            builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3))
        });
        let float_slots = float_analysis
            .as_ref()
            .map(|analysis| {
                analysis
                    .slots
                    .iter()
                    .map(|needed| {
                        needed.then(|| {
                            builder.create_sized_stack_slot(StackSlotData::new(
                                StackSlotKind::ExplicitSlot,
                                8,
                                3,
                            ))
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![None; code.registers as usize]);
        let float_scratch = float_analysis.as_ref().map(|_| {
            builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3))
        });
        let method_caches = direct_calls
            .iter()
            .filter_map(|call| call.method_attr_pc)
            .enumerate()
            .map(|(index, pc)| (pc, code.registers as usize + index * METHOD_CACHE_WORDS))
            .collect::<Vec<_>>();
        let mut next_argument_root =
            code.registers as usize + method_site_count * METHOD_CACHE_WORDS;
        let argument_roots = direct_calls
            .iter()
            .filter_map(|call| {
                let count = call
                    .arguments
                    .iter()
                    .filter(|argument| {
                        matches!(
                            argument,
                            DirectArgument::SequenceItem { .. }
                                | DirectArgument::MappingItem { .. }
                                | DirectArgument::VariadicTuple { .. }
                                | DirectArgument::VariadicDict { .. }
                        )
                    })
                    .count()
                    + usize::from(call.float);
                (count > 0).then(|| {
                    let base = next_argument_root;
                    next_argument_root += count;
                    (call.pc, base)
                })
            })
            .collect::<Vec<_>>();
        let entry = builder.create_block();
        let blocks = (0..code.instructions.len())
            .map(|_| builder.create_block())
            .collect::<Vec<_>>();
        builder.append_block_params_for_function_params(entry);
        for block in &blocks {
            builder.append_block_param(*block, pointer_type);
        }
        builder.switch_to_block(entry);
        let mut registers = builder.block_params(entry)[0];
        let globals = builder.block_params(entry)[1];
        let global_count = builder.block_params(entry)[2];
        let runtime_context = builder.block_params(entry)[3];
        let runtime_helper = builder.block_params(entry)[4];
        let direct_call_counter = builder.block_params(entry)[5];
        let start_pc = builder.block_params(entry)[6];
        if let Some(slot) = poll_slot {
            let interval = builder.ins().iconst(types::I64, BACKEDGE_POLL_INTERVAL);
            builder.ins().stack_store(interval, slot, 0);
        }
        if let Some(analysis) = &float_analysis {
            registers = emit_float_entry_initialization(
                &mut builder,
                registers,
                start_pc,
                analysis,
                &float_slots,
                float_scratch.expect("float analysis scratch slot"),
                runtime_signature,
                runtime_helper,
                runtime_context,
                root_count,
                pointer_type,
            );
        }
        for (_, base) in &method_caches {
            let unbound = builder.ins().iconst(types::I64, VALUE_UNBOUND as i64);
            store_word(&mut builder, registers, *base, unbound);
            store_word(&mut builder, registers, *base + 1, unbound);
            store_word(&mut builder, registers, *base + 2, unbound);
            let uninitialized = builder.ins().iconst(types::I64, VALUE_FALSE);
            store_word(&mut builder, registers, *base + 3, uninitialized);
        }
        for root in code.registers as usize + method_site_count * METHOD_CACHE_WORDS..root_count {
            let unbound = builder.ins().iconst(types::I64, VALUE_UNBOUND as i64);
            store_word(&mut builder, registers, root, unbound);
        }
        if resumable {
            let dispatch_blocks = (0..code.instructions.len())
                .map(|_| builder.create_block())
                .collect::<Vec<_>>();
            let invalid_pc = builder.create_block();
            let mut dispatch = Switch::new();
            for (pc, block) in dispatch_blocks.iter().copied().enumerate() {
                dispatch.set_entry(pc as u128, block);
            }
            dispatch.emit(&mut builder, start_pc, invalid_pc);
            builder.switch_to_block(invalid_pc);
            let invalid_status = builder.ins().iconst(types::I64, 0);
            builder.ins().return_(&[invalid_status]);
            for (dispatch_block, opcode_block) in
                dispatch_blocks.iter().copied().zip(blocks.iter().copied())
            {
                builder.switch_to_block(dispatch_block);
                builder.ins().jump(opcode_block, &[registers]);
            }
        } else {
            builder.ins().jump(blocks[0], &[registers]);
        }
        for (pc, instruction) in code.instructions.iter().enumerate() {
            builder.switch_to_block(blocks[pc]);
            let registers = builder.block_params(blocks[pc])[0];
            let op = Op::try_from(instruction.opcode).map_err(|error| {
                Error::Backend(format!("verified opcode could not be decoded: {error}"))
            })?;
            let float_state = float_analysis
                .as_ref()
                .and_then(|analysis| analysis.before[pc].as_ref());
            match op {
                Op::Const => {
                    if matches!(code.constants[instruction.b as usize], Constant::Float(_))
                        && float_state.is_some()
                    {
                        let Constant::Float(value) = code.constants[instruction.b as usize] else {
                            unreachable!()
                        };
                        let bits = builder.ins().iconst(types::I64, value.to_bits() as i64);
                        let value = builder.ins().bitcast(types::F64, MemFlags::new(), bits);
                        builder.ins().stack_store(
                            value,
                            float_slots[instruction.a as usize]
                                .expect("analyzed float constant slot"),
                            0,
                        );
                        fallthrough(&mut builder, &blocks, pc, registers);
                        continue;
                    }
                    let raw = encode_constant(&code.constants[instruction.b as usize])
                        .or_else(|| {
                            materialized_constants
                                .iter()
                                .find(|constant| constant.pc == pc)
                                .map(|constant| constant.value)
                        })
                        .expect("validated constant");
                    let value = builder.ins().iconst(types::I64, raw as i64);
                    store(&mut builder, registers, instruction.a, value);
                    fallthrough(&mut builder, &blocks, pc, registers);
                }
                Op::Move => {
                    if float_state.is_some_and(|state| state[instruction.b as usize]) {
                        let value = builder.ins().stack_load(
                            types::F64,
                            float_slots[instruction.b as usize]
                                .expect("analyzed float source slot"),
                            0,
                        );
                        builder.ins().stack_store(
                            value,
                            float_slots[instruction.a as usize]
                                .expect("analyzed float destination slot"),
                            0,
                        );
                        fallthrough(&mut builder, &blocks, pc, registers);
                        continue;
                    }
                    let value = load(&mut builder, registers, instruction.b);
                    store(&mut builder, registers, instruction.a, value);
                    fallthrough(&mut builder, &blocks, pc, registers);
                }
                Op::LoadGlobal => {
                    let direct = builder.create_block();
                    builder.append_block_param(direct, pointer_type);
                    let generic = builder.create_block();
                    builder.append_block_param(generic, pointer_type);
                    let symbol_index = builder.ins().iconst(pointer_type, i64::from(instruction.b));
                    let in_bounds =
                        builder
                            .ins()
                            .icmp(IntCC::UnsignedLessThan, symbol_index, global_count);
                    builder
                        .ins()
                        .brif(in_bounds, direct, &[registers], generic, &[registers]);

                    builder.switch_to_block(direct);
                    let direct_registers = builder.block_params(direct)[0];
                    let value = builder.ins().load(
                        types::I64,
                        MemFlags::trusted().with_readonly(),
                        globals,
                        offset(instruction.b),
                    );
                    let bound =
                        builder
                            .ins()
                            .icmp_imm(IntCC::NotEqual, value, VALUE_UNBOUND as i64);
                    let found = builder.create_block();
                    builder.append_block_param(found, pointer_type);
                    builder.append_block_param(found, types::I64);
                    builder.ins().brif(
                        bound,
                        found,
                        &[direct_registers, value],
                        generic,
                        &[direct_registers],
                    );

                    builder.switch_to_block(found);
                    let found_registers = builder.block_params(found)[0];
                    let found_value = builder.block_params(found)[1];
                    store(&mut builder, found_registers, instruction.a, found_value);
                    fallthrough(&mut builder, &blocks, pc, found_registers);

                    builder.switch_to_block(generic);
                    let registers = builder.block_params(generic)[0];
                    let register_count = builder.ins().iconst(pointer_type, root_count as i64);
                    let operation = builder
                        .ins()
                        .iconst(types::I32, i64::from(RuntimeOp::LoadGlobal as u32));
                    let symbol = builder.ins().iconst(types::I64, i64::from(instruction.b));
                    let unused = builder.ins().iconst(types::I64, 0);
                    let output = builder
                        .ins()
                        .iadd_imm(registers, i64::from(offset(instruction.a)));
                    let call = builder.ins().call_indirect(
                        runtime_signature,
                        runtime_helper,
                        &[
                            runtime_context,
                            registers,
                            register_count,
                            operation,
                            symbol,
                            unused,
                            output,
                        ],
                    );
                    let status = builder.inst_results(call)[0];
                    let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                    let registers =
                        runtime_guard(&mut builder, success, registers, pc, pointer_type);
                    fallthrough(&mut builder, &blocks, pc, registers);
                    continue;
                }
                Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul | Op::FloorDiv | Op::Mod => {
                    if float_state.is_some_and(|state| {
                        state[instruction.b as usize] && state[instruction.c as usize]
                    }) {
                        let left = builder.ins().stack_load(
                            types::F64,
                            float_slots[instruction.b as usize]
                                .expect("analyzed float source slot"),
                            0,
                        );
                        let right = builder.ins().stack_load(
                            types::F64,
                            float_slots[instruction.c as usize]
                                .expect("analyzed float source slot"),
                            0,
                        );
                        let result = match op {
                            Op::Add | Op::InplaceAdd => builder.ins().fadd(left, right),
                            Op::Sub => builder.ins().fsub(left, right),
                            Op::Mul => builder.ins().fmul(left, right),
                            _ => unreachable!("float analysis excludes floor/mod"),
                        };
                        builder.ins().stack_store(
                            result,
                            float_slots[instruction.a as usize]
                                .expect("analyzed float result slot"),
                            0,
                        );
                        fallthrough(&mut builder, &blocks, pc, registers);
                        continue;
                    }
                    if float_state.is_some() {
                        emit_generic_binary(
                            &mut builder,
                            registers,
                            instruction,
                            op,
                            &blocks,
                            pc,
                            pointer_type,
                            runtime_signature,
                            runtime_helper,
                            runtime_context,
                            root_count,
                        );
                        continue;
                    }
                    let left = load(&mut builder, registers, instruction.b);
                    let right = load(&mut builder, registers, instruction.c);
                    let condition = both_exact_int(&mut builder, left, right);
                    let registers = if backedges > 0 {
                        let integer = builder.create_block();
                        builder.append_block_param(integer, pointer_type);
                        let generic = builder.create_block();
                        builder.append_block_param(generic, pointer_type);
                        builder
                            .ins()
                            .brif(condition, integer, &[registers], generic, &[registers]);

                        builder.switch_to_block(generic);
                        let generic_registers = builder.block_params(generic)[0];
                        let generic_left = load(&mut builder, generic_registers, instruction.b);
                        let generic_right = load(&mut builder, generic_registers, instruction.c);
                        let register_count = builder.ins().iconst(pointer_type, root_count as i64);
                        let operation = builder
                            .ins()
                            .iconst(types::I32, i64::from(runtime_binary_op(op) as u32));
                        let output = builder
                            .ins()
                            .iadd_imm(generic_registers, i64::from(offset(instruction.a)));
                        let call = builder.ins().call_indirect(
                            runtime_signature,
                            runtime_helper,
                            &[
                                runtime_context,
                                generic_registers,
                                register_count,
                                operation,
                                generic_left,
                                generic_right,
                                output,
                            ],
                        );
                        let status = builder.inst_results(call)[0];
                        let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                        let generic_registers = runtime_guard(
                            &mut builder,
                            success,
                            generic_registers,
                            pc,
                            pointer_type,
                        );
                        fallthrough(&mut builder, &blocks, pc, generic_registers);

                        builder.switch_to_block(integer);
                        builder.block_params(integer)[0]
                    } else {
                        guard(&mut builder, condition, registers, pc, pointer_type)
                    };
                    let left_raw = load(&mut builder, registers, instruction.b);
                    let right_raw = load(&mut builder, registers, instruction.c);
                    let left = decode_int(&mut builder, left_raw);
                    let right = decode_int(&mut builder, right_raw);
                    let (result, valid) = match op {
                        Op::Sub => (builder.ins().isub(left, right), None),
                        Op::Add | Op::InplaceAdd => (builder.ins().iadd(left, right), None),
                        Op::Mul => {
                            let (result, overflow) = builder.ins().smul_overflow(left, right);
                            let valid = builder.ins().icmp_imm(IntCC::Equal, overflow, 0);
                            (result, Some(valid))
                        }
                        Op::FloorDiv | Op::Mod => {
                            let nonzero = builder.ins().icmp_imm(IntCC::NotEqual, right, 0);
                            let (registers, left, right) = guard_values(
                                &mut builder,
                                nonzero,
                                registers,
                                left,
                                right,
                                pc,
                                pointer_type,
                            );
                            let quotient = builder.ins().sdiv(left, right);
                            let remainder = builder.ins().srem(left, right);
                            let has_remainder =
                                builder.ins().icmp_imm(IntCC::NotEqual, remainder, 0);
                            let signs_differ = builder.ins().bxor(left, right);
                            let signs_differ =
                                builder
                                    .ins()
                                    .icmp_imm(IntCC::SignedLessThan, signs_differ, 0);
                            let adjust = builder.ins().band(has_remainder, signs_differ);
                            let result = if op == Op::FloorDiv {
                                let adjusted = builder.ins().iadd_imm(quotient, -1);
                                builder.ins().select(adjust, adjusted, quotient)
                            } else {
                                let adjusted = builder.ins().iadd(remainder, right);
                                builder.ins().select(adjust, adjusted, remainder)
                            };
                            let in_range = immediate_range(&mut builder, result);
                            let (registers, result) = guard_value(
                                &mut builder,
                                in_range,
                                registers,
                                result,
                                pc,
                                pointer_type,
                            );
                            let encoded = encode_int(&mut builder, result);
                            store(&mut builder, registers, instruction.a, encoded);
                            fallthrough(&mut builder, &blocks, pc, registers);
                            continue;
                        }
                        _ => unreachable!(),
                    };
                    let in_range = immediate_range(&mut builder, result);
                    let valid = valid
                        .map(|valid| builder.ins().band(valid, in_range))
                        .unwrap_or(in_range);
                    let (registers, result) =
                        guard_value(&mut builder, valid, registers, result, pc, pointer_type);
                    let encoded = encode_int(&mut builder, result);
                    store(&mut builder, registers, instruction.a, encoded);
                    fallthrough(&mut builder, &blocks, pc, registers);
                }
                Op::Div => {
                    let left = load(&mut builder, registers, instruction.b);
                    let right = load(&mut builder, registers, instruction.c);
                    let register_count = builder.ins().iconst(pointer_type, root_count as i64);
                    let operation = builder
                        .ins()
                        .iconst(types::I32, i64::from(RuntimeOp::Div as u32));
                    let output = builder
                        .ins()
                        .iadd_imm(registers, i64::from(offset(instruction.a)));
                    let call = builder.ins().call_indirect(
                        runtime_signature,
                        runtime_helper,
                        &[
                            runtime_context,
                            registers,
                            register_count,
                            operation,
                            left,
                            right,
                            output,
                        ],
                    );
                    let status = builder.inst_results(call)[0];
                    let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                    let registers =
                        runtime_guard(&mut builder, success, registers, pc, pointer_type);
                    fallthrough(&mut builder, &blocks, pc, registers);
                }
                Op::Neg | Op::Pos => {
                    let operand = load(&mut builder, registers, instruction.b);
                    let condition = exact_int(&mut builder, operand);
                    let registers = guard(&mut builder, condition, registers, pc, pointer_type);
                    let raw = load(&mut builder, registers, instruction.b);
                    let operand = decode_int(&mut builder, raw);
                    let result = if matches!(op, Op::Neg) {
                        builder.ins().ineg(operand)
                    } else {
                        operand
                    };
                    let in_range = immediate_range(&mut builder, result);
                    let (registers, result) =
                        guard_value(&mut builder, in_range, registers, result, pc, pointer_type);
                    let encoded = encode_int(&mut builder, result);
                    store(&mut builder, registers, instruction.a, encoded);
                    fallthrough(&mut builder, &blocks, pc, registers);
                }
                Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                    if float_state.is_some() {
                        emit_generic_binary(
                            &mut builder,
                            registers,
                            instruction,
                            op,
                            &blocks,
                            pc,
                            pointer_type,
                            runtime_signature,
                            runtime_helper,
                            runtime_context,
                            root_count,
                        );
                        continue;
                    }
                    let left = load(&mut builder, registers, instruction.b);
                    let right = load(&mut builder, registers, instruction.c);
                    let condition = both_exact_int(&mut builder, left, right);
                    let registers = guard(&mut builder, condition, registers, pc, pointer_type);
                    let left_raw = load(&mut builder, registers, instruction.b);
                    let right_raw = load(&mut builder, registers, instruction.c);
                    let left = decode_int(&mut builder, left_raw);
                    let right = decode_int(&mut builder, right_raw);
                    let condition = builder.ins().icmp(comparison(op), left, right);
                    let yes = builder.ins().iconst(types::I64, VALUE_TRUE);
                    let no = builder.ins().iconst(types::I64, VALUE_FALSE);
                    let result = builder.ins().select(condition, yes, no);
                    store(&mut builder, registers, instruction.a, result);
                    fallthrough(&mut builder, &blocks, pc, registers);
                }
                Op::Not => {
                    let raw = load(&mut builder, registers, instruction.b);
                    let (valid, truth) = immediate_truth(&mut builder, raw);
                    let (registers, truth) =
                        guard_value(&mut builder, valid, registers, truth, pc, pointer_type);
                    let yes = builder.ins().iconst(types::I64, VALUE_TRUE);
                    let no = builder.ins().iconst(types::I64, VALUE_FALSE);
                    let result = builder.ins().select(truth, no, yes);
                    store(&mut builder, registers, instruction.a, result);
                    fallthrough(&mut builder, &blocks, pc, registers);
                }
                Op::Jump => {
                    let target = instruction.a as usize;
                    if target <= pc {
                        emit_backedge_poll(
                            &mut builder,
                            registers,
                            blocks[target],
                            runtime_signature,
                            runtime_helper,
                            runtime_context,
                            root_count,
                            poll_slot.expect("backedge has poll slot"),
                            pc,
                            target,
                            pointer_type,
                            float_analysis
                                .as_ref()
                                .and_then(|analysis| analysis.before[target].as_deref()),
                            &float_slots,
                        );
                    } else {
                        builder.ins().jump(blocks[target], &[registers]);
                    }
                }
                Op::JumpFalse | Op::JumpTrue => {
                    let raw = load(&mut builder, registers, instruction.a);
                    let (valid, truth) = immediate_truth(&mut builder, raw);
                    let (registers, truth) =
                        guard_value(&mut builder, valid, registers, truth, pc, pointer_type);
                    let jump_when_true = matches!(op, Op::JumpTrue);
                    let (taken, other) = if jump_when_true {
                        (instruction.b as usize, pc + 1)
                    } else {
                        (pc + 1, instruction.b as usize)
                    };
                    let backedge = (instruction.b as usize <= pc).then(|| {
                        let block = builder.create_block();
                        builder.append_block_param(block, pointer_type);
                        block
                    });
                    let taken_block = if jump_when_true {
                        backedge.unwrap_or(blocks[taken])
                    } else {
                        blocks[taken]
                    };
                    let other_block = if jump_when_true {
                        blocks[other]
                    } else {
                        backedge.unwrap_or(blocks[other])
                    };
                    builder
                        .ins()
                        .brif(truth, taken_block, &[registers], other_block, &[registers]);
                    if let Some(backedge) = backedge {
                        builder.switch_to_block(backedge);
                        let registers = builder.block_params(backedge)[0];
                        emit_backedge_poll(
                            &mut builder,
                            registers,
                            blocks[instruction.b as usize],
                            runtime_signature,
                            runtime_helper,
                            runtime_context,
                            root_count,
                            poll_slot.expect("backedge has poll slot"),
                            pc,
                            instruction.b as usize,
                            pointer_type,
                            float_analysis.as_ref().and_then(|analysis| {
                                analysis.before[instruction.b as usize].as_deref()
                            }),
                            &float_slots,
                        );
                    }
                }
                Op::Call | Op::CallExpanded => {
                    if let Some(call) = direct_calls.iter().find(|call| call.pc == pc) {
                        emit_direct_call(
                            &mut builder,
                            code,
                            instruction,
                            call,
                            registers,
                            direct_call_counter,
                            &blocks,
                            pc,
                            pointer_type,
                            runtime_signature,
                            runtime_helper,
                            runtime_context,
                            root_count,
                            call.method_attr_pc.and_then(|attr_pc| {
                                method_caches
                                    .iter()
                                    .find(|(profile_pc, _)| *profile_pc == attr_pc)
                                    .map(|(_, base)| *base)
                            }),
                            argument_roots
                                .iter()
                                .find(|(call_pc, _)| *call_pc == pc)
                                .map(|(_, base)| *base),
                        );
                    } else {
                        side_exit(&mut builder, registers, pc);
                    }
                }
                Op::Attr => {
                    if let Some(call) = direct_calls
                        .iter()
                        .find(|call| call.method_attr_pc == Some(pc))
                    {
                        emit_method_load(
                            &mut builder,
                            instruction,
                            call,
                            registers,
                            runtime_signature,
                            runtime_helper,
                            runtime_context,
                            &blocks,
                            pc,
                            pointer_type,
                            root_count,
                            method_caches
                                .iter()
                                .find(|(profile_pc, _)| *profile_pc == pc)
                                .map(|(_, base)| *base)
                                .expect("validated method receiver slot"),
                        );
                    } else {
                        side_exit(&mut builder, registers, pc);
                    }
                }
                Op::BeginArgs | Op::ArgPos | Op::ArgStar | Op::ArgNamed | Op::ArgMapping => {
                    if direct_calls.iter().any(|call| {
                        call.expanded_begin_pc
                            .is_some_and(|begin| begin <= pc && pc < call.pc)
                    }) {
                        fallthrough(&mut builder, &blocks, pc, registers);
                    } else {
                        side_exit(&mut builder, registers, pc);
                    }
                }
                Op::Return => {
                    if float_state.is_some_and(|state| state[instruction.a as usize]) {
                        let value = builder.ins().stack_load(
                            types::F64,
                            float_slots[instruction.a as usize]
                                .expect("analyzed float return slot"),
                            0,
                        );
                        let bits = builder.ins().bitcast(types::I64, MemFlags::new(), value);
                        let root_count_value =
                            builder.ins().iconst(pointer_type, root_count as i64);
                        let operation = builder
                            .ins()
                            .iconst(types::I32, i64::from(RuntimeOp::BoxFloat as u32));
                        let zero = builder.ins().iconst(types::I64, 0);
                        let output = builder
                            .ins()
                            .iadd_imm(registers, i64::from(offset(instruction.a)));
                        let call = builder.ins().call_indirect(
                            runtime_signature,
                            runtime_helper,
                            &[
                                runtime_context,
                                registers,
                                root_count_value,
                                operation,
                                bits,
                                zero,
                                output,
                            ],
                        );
                        let status = builder.inst_results(call)[0];
                        let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                        let registers =
                            runtime_guard(&mut builder, success, registers, pc, pointer_type);
                        let boxed = load(&mut builder, registers, instruction.a);
                        store(&mut builder, registers, 0, boxed);
                        let status = builder
                            .ins()
                            .iconst(types::I64, (RETURN_FLAG | pc as u64) as i64);
                        builder.ins().return_(&[status]);
                        continue;
                    }
                    let result = load(&mut builder, registers, instruction.a);
                    store(&mut builder, registers, 0, result);
                    let status = builder
                        .ins()
                        .iconst(types::I64, (RETURN_FLAG | pc as u64) as i64);
                    builder.ins().return_(&[status]);
                }
                _ => unreachable!("validated supported opcode"),
            }
        }
        builder.seal_all_blocks();
        builder.finalize();
    }
    module
        .define_function(function_id, &mut context)
        .map_err(|error| Error::Backend(error.to_string()))?;
    let code_bytes = context
        .compiled_code()
        .map(|code| code.code_info().total_size as usize)
        .unwrap_or(0);
    module
        .finalize_definitions()
        .map_err(|error| Error::Backend(error.to_string()))?;
    let pointer = module.get_finalized_function(function_id);
    // SAFETY: `pointer` was finalized from the signature
    // `(registers, globals, global count, runtime context, runtime helper,
    // direct-call counter, start PC) -> i64`
    // immediately above. `CompiledFunction` retains the owning JITModule, and
    // `run` supplies a writable root buffer with every verified guest register
    // and JIT-private dependency-cache slot.
    let entry = unsafe { mem::transmute::<*const u8, Entry>(pointer) };
    Ok(CompiledFunction {
        _module: module,
        entry,
        deopt_maps: float_analysis
            .as_ref()
            .map(|analysis| analysis.deopt_maps.clone())
            .unwrap_or_default(),
        metadata: Metadata {
            code_bytes,
            compile_time: started.elapsed(),
            register_count: code.registers as usize,
            root_count,
            runtime_calls: code
                .instructions
                .iter()
                .filter(|instruction| {
                    matches!(
                        Op::try_from(instruction.opcode),
                        Ok(Op::Div | Op::LoadGlobal)
                    )
                })
                .count()
                + direct_calls
                    .iter()
                    .filter(|call| call.method_attr_pc.is_some())
                    .count()
                + direct_calls
                    .iter()
                    .flat_map(|call| &call.arguments)
                    .map(|argument| match argument {
                        DirectArgument::SequenceItem { .. }
                        | DirectArgument::MappingItem { .. }
                        | DirectArgument::VariadicTuple { .. } => 1,
                        DirectArgument::VariadicDict { items } => 1 + items.len(),
                        _ => 0,
                    })
                    .sum::<usize>()
                + direct_calls
                    .iter()
                    .filter(|call| call.float)
                    .map(|call| call.arguments.len() + 1)
                    .sum::<usize>()
                + if backedges > 0 {
                    code.instructions
                        .iter()
                        .filter(|instruction| {
                            matches!(
                                Op::try_from(instruction.opcode),
                                Ok(Op::Add
                                    | Op::InplaceAdd
                                    | Op::Sub
                                    | Op::Mul
                                    | Op::FloorDiv
                                    | Op::Mod)
                            )
                        })
                        .count()
                } else {
                    0
                }
                + backedges,
            safepoints: code
                .instructions
                .iter()
                .filter(|instruction| Op::try_from(instruction.opcode) == Ok(Op::Div))
                .count()
                + if backedges > 0 {
                    code.instructions
                        .iter()
                        .filter(|instruction| {
                            matches!(
                                Op::try_from(instruction.opcode),
                                Ok(Op::Add
                                    | Op::InplaceAdd
                                    | Op::Sub
                                    | Op::Mul
                                    | Op::FloorDiv
                                    | Op::Mod)
                            )
                        })
                        .count()
                } else {
                    0
                }
                + backedges
                + direct_calls.iter().filter(|call| call.float).count(),
            resumable,
            instruction_count: code.instructions.len(),
            direct_call_sites: direct_calls.len(),
            direct_method_sites: direct_calls
                .iter()
                .filter(|call| call.method_attr_pc.is_some())
                .count(),
        },
    })
}

/// Validate every operand that the JIT compiler may index before control reaches
/// Cranelift. The runtime normally supplies a `VerifiedProgram`, but the public
/// crate API accepts a `CodeObject`; keeping this check here makes malformed
/// embedder input a normal error instead of a host panic or unchecked access.
fn validate_structural_safety(code: &CodeObject) -> Result<(), Error> {
    let invalid = |pc, message: &str| Error::InvalidBytecode {
        pc,
        message: message.into(),
    };
    if code.instructions.is_empty() || code.registers == 0 || code.params > code.registers {
        return Err(invalid(None, "invalid code metadata"));
    }
    let register = |pc, value: u16| {
        if value < code.registers {
            Ok(())
        } else {
            Err(invalid(Some(pc), "register out of bounds"))
        }
    };
    let jump = |pc, target: u16| {
        if usize::from(target) < code.instructions.len() {
            Ok(())
        } else {
            Err(invalid(Some(pc), "jump out of bounds"))
        }
    };
    for region in &code.exception_regions {
        if region.start >= region.end
            || usize::from(region.end) > code.instructions.len()
            || usize::from(region.target) >= code.instructions.len()
            || region.exception >= code.registers
        {
            return Err(invalid(None, "invalid exception region"));
        }
    }
    for (index, left) in code.exception_regions.iter().enumerate() {
        for right in &code.exception_regions[index + 1..] {
            let overlaps = left.start < right.end && right.start < left.end;
            let nested = (left.start <= right.start && right.end <= left.end)
                || (right.start <= left.start && left.end <= right.end);
            if overlaps && !nested {
                return Err(invalid(None, "partially overlapping exception regions"));
            }
        }
    }
    for (pc, instruction) in code.instructions.iter().enumerate() {
        let op =
            Op::try_from(instruction.opcode).map_err(|_| invalid(Some(pc), "unknown opcode"))?;
        match op {
            Op::Const => {
                register(pc, instruction.a)?;
                if usize::from(instruction.b) >= code.constants.len() {
                    return Err(invalid(Some(pc), "constant out of bounds"));
                }
                if instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::Move | Op::Neg | Op::Pos | Op::Not => {
                register(pc, instruction.a)?;
                register(pc, instruction.b)?;
                if instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::LoadGlobal => {
                register(pc, instruction.a)?;
                if instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::Add
            | Op::InplaceAdd
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::FloorDiv
            | Op::Mod
            | Op::Eq
            | Op::Ne
            | Op::Lt
            | Op::Le
            | Op::Gt
            | Op::Ge => {
                register(pc, instruction.a)?;
                register(pc, instruction.b)?;
                register(pc, instruction.c)?;
            }
            Op::Jump => {
                jump(pc, instruction.a)?;
                if instruction.b != 0 || instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::JumpFalse | Op::JumpTrue => {
                register(pc, instruction.a)?;
                jump(pc, instruction.b)?;
                if instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::Call => {
                register(pc, instruction.a)?;
                register(pc, instruction.b)?;
                let Some(site) = code.calls.get(usize::from(instruction.c)) else {
                    return Err(invalid(Some(pc), "call site out of bounds"));
                };
                let width = usize::from(site.count)
                    .checked_add(site.keywords.len())
                    .ok_or_else(|| invalid(Some(pc), "call window overflow"))?;
                if usize::from(site.first)
                    .checked_add(width)
                    .is_none_or(|end| end > usize::from(code.registers))
                {
                    return Err(invalid(Some(pc), "call window out of bounds"));
                }
            }
            Op::Attr => {
                register(pc, instruction.a)?;
                register(pc, instruction.b)?;
            }
            Op::BeginArgs => {
                if instruction.a != 0 || instruction.b != 0 || instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::ArgStar => {
                register(pc, instruction.a)?;
                if instruction.b != 0 || instruction.c > 1 {
                    return Err(invalid(Some(pc), "invalid star argument operands"));
                }
            }
            Op::ArgPos | Op::ArgMapping => {
                register(pc, instruction.a)?;
                if instruction.b != 0 || instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::ArgNamed => {
                register(pc, instruction.a)?;
                if instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::CallExpanded => {
                register(pc, instruction.a)?;
                register(pc, instruction.b)?;
                if instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::Return => {
                register(pc, instruction.a)?;
                if instruction.b != 0 || instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::Raise => {
                if instruction.b > 2 || (instruction.b != 2 && instruction.c != 0) {
                    return Err(invalid(Some(pc), "invalid raise operand"));
                }
                match instruction.b {
                    0 => register(pc, instruction.a)?,
                    1 if instruction.a != 0 => {
                        return Err(invalid(Some(pc), "nonzero bare raise operand"));
                    }
                    1 => {}
                    2 => {
                        register(pc, instruction.a)?;
                        register(pc, instruction.c)?;
                    }
                    _ => unreachable!(),
                }
            }
            Op::ExceptionMatch => {
                register(pc, instruction.a)?;
                register(pc, instruction.b)?;
                register(pc, instruction.c)?;
            }
            Op::ClearException => {
                if instruction.a != 0 || instruction.b != 0 || instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::PushException => {
                register(pc, instruction.a)?;
                if instruction.b != 0 || instruction.c != 0 {
                    return Err(invalid(Some(pc), "nonzero reserved operand"));
                }
            }
            Op::ContextEnter | Op::ContextExit => {
                register(pc, instruction.a)?;
                register(pc, instruction.b)?;
                register(pc, instruction.c)?;
            }
            Op::ClearBinding => {
                if instruction.a > 3 || instruction.c != 0 {
                    return Err(invalid(Some(pc), "invalid clear-binding operand"));
                }
                if instruction.a == 0 {
                    register(pc, instruction.b)?;
                }
            }
            _ => {}
        }
        if pc + 1 == code.instructions.len() && !matches!(op, Op::Jump | Op::Return | Op::Raise) {
            return Err(invalid(Some(pc), "code can fall off end"));
        }
    }
    Ok(())
}

fn validate_direct_calls(code: &CodeObject, direct_calls: &[DirectCall<'_>]) -> Result<(), Error> {
    for (index, direct) in direct_calls.iter().enumerate() {
        if direct_calls[..index]
            .iter()
            .any(|previous| previous.pc == direct.pc)
        {
            return Err(Error::Backend(format!(
                "duplicate direct-call profile at bytecode PC {}",
                direct.pc
            )));
        }
        let Some(instruction) = code.instructions.get(direct.pc) else {
            return Err(Error::Backend(format!(
                "direct-call profile PC {} is outside the caller",
                direct.pc
            )));
        };
        let call_op = Op::try_from(instruction.opcode).map_err(|error| {
            Error::Backend(format!("verified opcode could not be decoded: {error}"))
        })?;
        if !matches!(call_op, Op::Call | Op::CallExpanded) {
            return Err(Error::Backend(format!(
                "direct-call profile PC {} is not CALL/CALL_EXPANDED",
                direct.pc
            )));
        }
        let signature = &direct.target.signature;
        let named = usize::from(signature.positional) + usize::from(signature.keyword_only);
        let supplied = if call_op == Op::Call {
            let site = &code.calls[instruction.c as usize];
            site.count as usize + site.keywords.len()
        } else {
            direct.arguments.len()
        };
        let omitted_unobserved =
            unobserved_variadic_parameters(direct.target) && direct.arguments.len() == named;
        let materialized = direct.arguments.len() == direct.target.params as usize;
        let variadic_layout_valid =
            !materialized
                || signature.vararg.is_none_or(|slot| {
                    matches!(
                        direct.arguments.get(slot as usize),
                        Some(DirectArgument::VariadicTuple { .. })
                    )
                }) && signature.kwarg.is_none_or(|slot| {
                    matches!(
                        direct.arguments.get(slot as usize),
                        Some(DirectArgument::VariadicDict { .. })
                    )
                }) && direct.arguments.iter().enumerate().all(
                    |(slot, argument)| match argument {
                        DirectArgument::VariadicTuple { .. } => {
                            signature.vararg == u16::try_from(slot).ok()
                        }
                        DirectArgument::VariadicDict { .. } => {
                            signature.kwarg == u16::try_from(slot).ok()
                        }
                        _ => true,
                    },
                );
        let float_layout_valid = !direct.float
            || call_op == Op::Call
                && direct.method_attr_pc.is_none()
                && direct.expanded_begin_pc.is_none()
                && signature.vararg.is_none()
                && signature.kwarg.is_none()
                && direct.arguments.len() == direct.target.params as usize
                && direct
                    .arguments
                    .iter()
                    .all(|argument| matches!(argument, DirectArgument::Caller(_)))
                && is_direct_float_leaf_inlineable(direct.target);
        if (!omitted_unobserved && !materialized)
            || !variadic_layout_valid
            || !float_layout_valid
            || direct.method_attr_pc.is_some() != direct.method_binding.is_some()
            || direct.method_attr_pc.is_some() && direct.expanded_begin_pc.is_some()
            || (call_op == Op::CallExpanded) != direct.expanded_begin_pc.is_some()
            || direct.arguments.iter().any(|argument| match argument {
                DirectArgument::Caller(offset) => {
                    call_op != Op::Call || usize::from(*offset) >= supplied
                }
                DirectArgument::Register(register) => {
                    call_op != Op::CallExpanded || *register >= code.registers
                }
                DirectArgument::SequenceItem {
                    register,
                    index,
                    length,
                } => {
                    call_op != Op::CallExpanded || *register >= code.registers || *index >= *length
                }
                DirectArgument::MappingItem {
                    register,
                    symbol: _,
                    key_count,
                } => call_op != Op::CallExpanded || *register >= code.registers || *key_count == 0,
                DirectArgument::VariadicTuple { first, count } => {
                    call_op != Op::Call
                        || first
                            .checked_add(*count)
                            .is_none_or(|end| end > code.registers)
                }
                DirectArgument::VariadicDict { items } => {
                    call_op != Op::Call
                        || items
                            .iter()
                            .any(|(_, register)| *register >= code.registers)
                        || items.iter().enumerate().any(|(index, (symbol, _))| {
                            items[..index]
                                .iter()
                                .any(|(previous, _)| previous == symbol)
                        })
                }
                DirectArgument::MethodReceiver => !matches!(
                    direct.method_binding,
                    Some(MethodBinding::Instance | MethodBinding::Class)
                ),
                DirectArgument::Default(value) => *value == VALUE_UNBOUND,
            })
            || !direct.target.cell_locals.is_empty()
            || !direct.target.free_vars.is_empty()
            || direct.target.class_body
            || !direct.float && !is_direct_call_inlineable(direct.target)
        {
            return Err(unsupported(
                direct.pc,
                Some(call_op),
                "direct-call target or argument binding is not an inlineable leaf",
            ));
        }
        if let Some(begin_pc) = direct.expanded_begin_pc {
            if begin_pc >= direct.pc
                || code
                    .instructions
                    .get(begin_pc)
                    .is_none_or(|begin| Op::try_from(begin.opcode) != Ok(Op::BeginArgs))
                || code.instructions[begin_pc + 1..direct.pc]
                    .iter()
                    .any(|between| {
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
                return Err(unsupported(
                    direct.pc,
                    Some(call_op),
                    "expanded direct-call segment is not a replayable flat expansion",
                ));
            }
        }
        if let Some(attr_pc) = direct.method_attr_pc {
            if direct_calls[..index]
                .iter()
                .any(|previous| previous.method_attr_pc == Some(attr_pc))
            {
                return Err(Error::Backend(format!(
                    "duplicate direct-method profile at bytecode PC {attr_pc}"
                )));
            }
            let Some(attr) = code.instructions.get(attr_pc) else {
                return Err(Error::Backend(format!(
                    "direct-method ATTR PC {attr_pc} is outside the caller"
                )));
            };
            if attr_pc >= direct.pc
                || Op::try_from(attr.opcode) != Ok(Op::Attr)
                || attr.a != instruction.b
                || attr.a == attr.b
                || if matches!(
                    direct.method_binding,
                    Some(MethodBinding::Instance | MethodBinding::Class)
                ) {
                    direct.arguments.first() != Some(&DirectArgument::MethodReceiver)
                        || direct.arguments[1..]
                            .iter()
                            .any(|argument| matches!(argument, DirectArgument::MethodReceiver))
                } else {
                    direct
                        .arguments
                        .iter()
                        .any(|argument| matches!(argument, DirectArgument::MethodReceiver))
                }
            {
                return Err(unsupported(
                    direct.pc,
                    Some(Op::Call),
                    "direct-method profile does not connect ATTR to CALL",
                ));
            }
            for between in &code.instructions[attr_pc + 1..direct.pc] {
                let op = Op::try_from(between.opcode).map_err(|error| {
                    Error::Backend(format!("verified opcode could not be decoded: {error}"))
                })?;
                if !matches!(op, Op::Const | Op::Move)
                    || between.a == attr.a
                    || between.a == attr.b
                    || (op == Op::Move && between.b == attr.a)
                {
                    return Err(unsupported(
                        direct.pc,
                        Some(Op::Call),
                        "method argument setup cannot be replayed atomically",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// A deliberately small, side-effect-free subset can be re-executed from the
/// caller's CALL PC if a guard fails, which makes deoptimization atomic.
pub fn is_direct_call_inlineable(code: &CodeObject) -> bool {
    if validate_structural_safety(code).is_err() {
        return false;
    }
    let mut returned = false;
    for instruction in &code.instructions {
        let Ok(op) = Op::try_from(instruction.opcode) else {
            return false;
        };
        match op {
            Op::Return => {
                returned = true;
                break;
            }
            Op::Const => {
                if encode_constant(&code.constants[instruction.b as usize]).is_none() {
                    return false;
                }
            }
            Op::Move | Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul => {}
            _ => return false,
        }
    }
    returned
}

/// Validate the straight-line subset whose parameter-derived values can stay
/// unboxed as F64 for the complete direct leaf. This dataflow check prevents a
/// generic or uninitialized value from reaching native float arithmetic.
pub fn is_direct_float_leaf_inlineable(code: &CodeObject) -> bool {
    if validate_structural_safety(code).is_err() {
        return false;
    }
    let mut floats = vec![false; code.registers as usize];
    for slot in floats.iter_mut().take(code.params as usize) {
        *slot = true;
    }
    for instruction in &code.instructions {
        let Ok(op) = Op::try_from(instruction.opcode) else {
            return false;
        };
        match op {
            Op::Move if floats[instruction.b as usize] => {
                floats[instruction.a as usize] = true;
            }
            Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul
                if floats[instruction.b as usize] && floats[instruction.c as usize] =>
            {
                floats[instruction.a as usize] = true;
            }
            Op::Return => return floats[instruction.a as usize],
            _ => return false,
        }
    }
    false
}

/// Returns true when omitting empty `*args`/`**kwargs` materialization cannot be
/// observed by the deliberately straight-line direct-leaf subset.
pub fn unobserved_variadic_parameters(code: &CodeObject) -> bool {
    let variadic = [code.signature.vararg, code.signature.kwarg];
    if variadic.iter().all(Option::is_none) {
        return true;
    }
    for instruction in &code.instructions {
        let Ok(op) = Op::try_from(instruction.opcode) else {
            return false;
        };
        let reads = match op {
            Op::Const => [None, None],
            Op::Move => [Some(instruction.b), None],
            Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul => {
                [Some(instruction.b), Some(instruction.c)]
            }
            Op::Return => [Some(instruction.a), None],
            _ => return false,
        };
        if reads
            .into_iter()
            .flatten()
            .any(|register| variadic.contains(&Some(register)))
        {
            return false;
        }
        if op == Op::Return {
            break;
        }
    }
    true
}

fn analyze_float_execution(
    code: &CodeObject,
    exact_float_parameters: &[u16],
) -> Result<Option<FloatAnalysis>, Error> {
    if exact_float_parameters.is_empty() {
        return Ok(None);
    }
    if exact_float_parameters
        .iter()
        .enumerate()
        .any(|(index, register)| {
            *register >= code.params || exact_float_parameters[..index].contains(register)
        })
    {
        return Err(Error::Backend(
            "exact-float parameter profile is duplicated or out of bounds".into(),
        ));
    }
    let mut before = vec![None; code.instructions.len()];
    let mut initial = vec![false; code.registers as usize];
    for register in exact_float_parameters {
        initial[*register as usize] = true;
    }
    if before.is_empty() {
        return Ok(None);
    }
    before[0] = Some(initial);
    let mut queue = VecDeque::from([0usize]);
    let mut float_ops = 0usize;
    let mut float_return = false;
    while let Some(pc) = queue.pop_front() {
        let mut after = before[pc].clone().expect("queued state exists");
        let instruction = code.instructions[pc];
        let op = Op::try_from(instruction.opcode).map_err(|error| {
            Error::Backend(format!("verified opcode could not be decoded: {error}"))
        })?;
        let is_float = |register: u16| after[register as usize];
        match op {
            Op::Const => {
                after[instruction.a as usize] =
                    matches!(code.constants[instruction.b as usize], Constant::Float(_));
            }
            Op::Move => after[instruction.a as usize] = is_float(instruction.b),
            Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul => {
                let left = is_float(instruction.b);
                let right = is_float(instruction.c);
                if left != right {
                    return Ok(None);
                }
                after[instruction.a as usize] = left;
                if left {
                    float_ops += 1;
                }
            }
            Op::FloorDiv | Op::Mod | Op::Div => {
                if is_float(instruction.b) || is_float(instruction.c) {
                    return Ok(None);
                }
                after[instruction.a as usize] = false;
            }
            Op::Neg | Op::Pos | Op::Not => {
                if is_float(instruction.b) {
                    return Ok(None);
                }
                after[instruction.a as usize] = false;
            }
            Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                if is_float(instruction.b) || is_float(instruction.c) {
                    return Ok(None);
                }
                after[instruction.a as usize] = false;
            }
            Op::JumpFalse | Op::JumpTrue => {
                if is_float(instruction.a) {
                    return Ok(None);
                }
            }
            Op::LoadGlobal => after[instruction.a as usize] = false,
            Op::Return => {
                float_return |= is_float(instruction.a);
            }
            Op::Jump => {}
            Op::Call
            | Op::Attr
            | Op::BeginArgs
            | Op::ArgPos
            | Op::ArgStar
            | Op::ArgNamed
            | Op::ArgMapping
            | Op::CallExpanded => return Ok(None),
            _ => return Ok(None),
        }
        let mut successors = [None, None];
        match op {
            Op::Return => {}
            Op::Jump => successors[0] = Some(instruction.a as usize),
            Op::JumpFalse | Op::JumpTrue => {
                successors[0] = Some(instruction.b as usize);
                successors[1] = (pc + 1 < code.instructions.len()).then_some(pc + 1);
            }
            _ => successors[0] = (pc + 1 < code.instructions.len()).then_some(pc + 1),
        }
        for successor in successors.into_iter().flatten() {
            let changed = match &mut before[successor] {
                None => {
                    before[successor] = Some(after.clone());
                    true
                }
                Some(existing) => {
                    let mut changed = false;
                    for (known, incoming) in existing.iter_mut().zip(&after) {
                        let merged = *known && *incoming;
                        changed |= merged != *known;
                        *known = merged;
                    }
                    changed
                }
            };
            if changed {
                queue.push_back(successor);
            }
        }
    }
    if float_ops == 0 || !float_return {
        return Ok(None);
    }
    let mut slots = vec![false; code.registers as usize];
    let deopt_maps = before
        .iter()
        .enumerate()
        .filter_map(|(pc, state)| {
            state.as_ref().map(|state| {
                let unboxed_float_registers = state
                    .iter()
                    .enumerate()
                    .filter_map(|(register, float)| {
                        if *float {
                            slots[register] = true;
                            Some(register as u16)
                        } else {
                            None
                        }
                    })
                    .collect();
                DeoptMap {
                    pc,
                    register_count: code.registers as usize,
                    unboxed_float_registers,
                }
            })
        })
        .collect();
    Ok(Some(FloatAnalysis {
        before,
        slots,
        deopt_maps,
    }))
}

fn validate_materialized_constants(
    code: &CodeObject,
    constants: &[MaterializedConstant],
) -> Result<(), Error> {
    for (index, constant) in constants.iter().enumerate() {
        if constants[..index]
            .iter()
            .any(|previous| previous.pc == constant.pc)
        {
            return Err(Error::Backend(format!(
                "duplicate materialized constant at bytecode PC {}",
                constant.pc
            )));
        }
        let Some(instruction) = code.instructions.get(constant.pc) else {
            return Err(Error::Backend(format!(
                "materialized constant PC {} is outside the code object",
                constant.pc
            )));
        };
        if Op::try_from(instruction.opcode) != Ok(Op::Const) || constant.value == VALUE_UNBOUND {
            return Err(Error::Backend(format!(
                "materialized constant PC {} is not a valid bound CONST",
                constant.pc
            )));
        }
    }
    Ok(())
}

fn validate_supported(
    code: &CodeObject,
    direct_calls: &[DirectCall<'_>],
    materialized_constants: &[MaterializedConstant],
) -> Result<(), Error> {
    if code.class_body || !code.cell_locals.is_empty() || !code.free_vars.is_empty() {
        return Err(unsupported(
            0,
            None,
            "class bodies and closures require runtime state",
        ));
    }
    for (pc, instruction) in code.instructions.iter().enumerate() {
        let op = Op::try_from(instruction.opcode).map_err(|error| {
            Error::Backend(format!("verified opcode could not be decoded: {error}"))
        })?;
        if !matches!(
            op,
            Op::Const
                | Op::Move
                | Op::LoadGlobal
                | Op::Add
                | Op::InplaceAdd
                | Op::Sub
                | Op::Mul
                | Op::Div
                | Op::FloorDiv
                | Op::Mod
                | Op::Neg
                | Op::Pos
                | Op::Not
                | Op::Eq
                | Op::Ne
                | Op::Lt
                | Op::Le
                | Op::Gt
                | Op::Ge
                | Op::Jump
                | Op::JumpFalse
                | Op::JumpTrue
                | Op::Call
                | Op::Attr
                | Op::BeginArgs
                | Op::ArgPos
                | Op::ArgStar
                | Op::ArgNamed
                | Op::ArgMapping
                | Op::CallExpanded
                | Op::Return
        ) {
            return Err(unsupported(
                pc,
                Some(op),
                "opcode needs the generic runtime",
            ));
        }
        if op == Op::Const
            && encode_constant(&code.constants[instruction.b as usize]).is_none()
            && !materialized_constants
                .iter()
                .any(|constant| constant.pc == pc)
        {
            return Err(unsupported(
                pc,
                Some(op),
                "constant is not an immediate int/bool/None",
            ));
        }
        if op == Op::Attr
            && !direct_calls.iter().any(|call| {
                call.method_attr_pc == Some(pc)
                    || (call.method_attr_pc.is_none()
                        && call.pc > pc
                        && code.instructions[call.pc].b == instruction.a
                        && code.instructions[pc + 1..call.pc].iter().all(|between| {
                            matches!(Op::try_from(between.opcode), Ok(Op::Const | Op::Move))
                                && between.a != instruction.a
                        }))
            })
        {
            return Err(unsupported(
                pc,
                Some(op),
                "generic attribute load has no guarded direct-call consumer",
            ));
        }
        if matches!(op, Op::JumpFalse | Op::JumpTrue) && pc + 1 >= code.instructions.len() {
            return Err(unsupported(
                pc,
                Some(op),
                "conditional jump has no fallthrough",
            ));
        }
        if !matches!(op, Op::Jump | Op::Return) && pc + 1 >= code.instructions.len() {
            return Err(unsupported(pc, Some(op), "instruction has no fallthrough"));
        }
    }
    Ok(())
}

fn unsupported(pc: usize, opcode: Option<Op>, reason: &str) -> Error {
    Error::Unsupported(Unsupported {
        pc,
        opcode,
        reason: reason.into(),
    })
}

fn encode_constant(constant: &Constant) -> Option<u64> {
    match constant {
        Constant::None => Some(VALUE_NONE),
        Constant::Bool(value) => Some(if *value {
            VALUE_TRUE as u64
        } else {
            VALUE_FALSE as u64
        }),
        Constant::Int(value) => value
            .parse::<i64>()
            .ok()
            .filter(|value| (MIN_INT..=MAX_INT).contains(value))
            .map(encode_i64),
        Constant::Float(_) | Constant::Str(_) => None,
    }
}

pub fn encode_i64(value: i64) -> u64 {
    debug_assert!((MIN_INT..=MAX_INT).contains(&value));
    ((value as u64) << 3) | INT_TAG as u64
}
pub fn decode_i64(value: u64) -> Option<i64> {
    (value & TAG_MASK as u64 == INT_TAG as u64).then_some((value as i64) >> 3)
}

fn offset(register: u16) -> i32 {
    i32::from(register) * mem::size_of::<u64>() as i32
}
fn word_offset(word: usize) -> i32 {
    // Root-buffer size is checked before code generation.
    (word * mem::size_of::<u64>()) as i32
}
fn load(builder: &mut FunctionBuilder<'_>, registers: Value, register: u16) -> Value {
    builder
        .ins()
        .load(types::I64, MemFlags::trusted(), registers, offset(register))
}
fn load_word(builder: &mut FunctionBuilder<'_>, registers: Value, word: usize) -> Value {
    builder.ins().load(
        types::I64,
        MemFlags::trusted(),
        registers,
        word_offset(word),
    )
}
fn store(builder: &mut FunctionBuilder<'_>, registers: Value, register: u16, value: Value) {
    builder
        .ins()
        .store(MemFlags::trusted(), value, registers, offset(register));
}
fn store_word(builder: &mut FunctionBuilder<'_>, registers: Value, word: usize, value: Value) {
    builder
        .ins()
        .store(MemFlags::trusted(), value, registers, word_offset(word));
}
fn fallthrough(
    builder: &mut FunctionBuilder<'_>,
    blocks: &[cranelift_codegen::ir::Block],
    pc: usize,
    registers: Value,
) {
    builder.ins().jump(blocks[pc + 1], &[registers]);
}
fn exact_int(builder: &mut FunctionBuilder<'_>, raw: Value) -> Value {
    let tag = builder.ins().band_imm(raw, TAG_MASK);
    builder.ins().icmp_imm(IntCC::Equal, tag, INT_TAG)
}
fn both_exact_int(builder: &mut FunctionBuilder<'_>, left: Value, right: Value) -> Value {
    let left = exact_int(builder, left);
    let right = exact_int(builder, right);
    builder.ins().band(left, right)
}
fn decode_int(builder: &mut FunctionBuilder<'_>, raw: Value) -> Value {
    builder.ins().sshr_imm(raw, 3)
}
fn encode_int(builder: &mut FunctionBuilder<'_>, integer: Value) -> Value {
    let shifted = builder.ins().ishl_imm(integer, 3);
    builder.ins().bor_imm(shifted, INT_TAG)
}
fn immediate_range(builder: &mut FunctionBuilder<'_>, integer: Value) -> Value {
    let lower = builder
        .ins()
        .icmp_imm(IntCC::SignedGreaterThanOrEqual, integer, MIN_INT);
    let upper = builder
        .ins()
        .icmp_imm(IntCC::SignedLessThanOrEqual, integer, MAX_INT);
    builder.ins().band(lower, upper)
}
fn comparison(op: Op) -> IntCC {
    match op {
        Op::Eq => IntCC::Equal,
        Op::Ne => IntCC::NotEqual,
        Op::Lt => IntCC::SignedLessThan,
        Op::Le => IntCC::SignedLessThanOrEqual,
        Op::Gt => IntCC::SignedGreaterThan,
        Op::Ge => IntCC::SignedGreaterThanOrEqual,
        _ => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_generic_binary(
    builder: &mut FunctionBuilder<'_>,
    registers: Value,
    instruction: &tonic_core::bytecode::Instr,
    op: Op,
    blocks: &[cranelift_codegen::ir::Block],
    pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
    runtime_signature: cranelift_codegen::ir::SigRef,
    runtime_helper: Value,
    runtime_context: Value,
    root_count: usize,
) {
    let left = load(builder, registers, instruction.b);
    let right = load(builder, registers, instruction.c);
    let root_count = builder.ins().iconst(pointer_type, root_count as i64);
    let operation = builder
        .ins()
        .iconst(types::I32, i64::from(runtime_binary_op(op) as u32));
    let output = builder
        .ins()
        .iadd_imm(registers, i64::from(offset(instruction.a)));
    let call = builder.ins().call_indirect(
        runtime_signature,
        runtime_helper,
        &[
            runtime_context,
            registers,
            root_count,
            operation,
            left,
            right,
            output,
        ],
    );
    let status = builder.inst_results(call)[0];
    let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
    let registers = runtime_guard(builder, success, registers, pc, pointer_type);
    fallthrough(builder, blocks, pc, registers);
}

fn runtime_binary_op(op: Op) -> RuntimeOp {
    match op {
        Op::Add => RuntimeOp::Add,
        Op::InplaceAdd => RuntimeOp::InplaceAdd,
        Op::Sub => RuntimeOp::Sub,
        Op::Mul => RuntimeOp::Mul,
        Op::FloorDiv => RuntimeOp::FloorDiv,
        Op::Mod => RuntimeOp::Mod,
        Op::Eq => RuntimeOp::Eq,
        Op::Ne => RuntimeOp::Ne,
        Op::Lt => RuntimeOp::Lt,
        Op::Le => RuntimeOp::Le,
        Op::Gt => RuntimeOp::Gt,
        Op::Ge => RuntimeOp::Ge,
        _ => unreachable!("non-binary opcode has no runtime binary operation"),
    }
}
fn immediate_truth(builder: &mut FunctionBuilder<'_>, raw: Value) -> (Value, Value) {
    let integer = exact_int(builder, raw);
    let decoded = decode_int(builder, raw);
    let integer_truth = builder.ins().icmp_imm(IntCC::NotEqual, decoded, 0);
    let integer_truth = builder.ins().band(integer, integer_truth);
    let true_bool = builder.ins().icmp_imm(IntCC::Equal, raw, VALUE_TRUE);
    let false_bool = builder.ins().icmp_imm(IntCC::Equal, raw, VALUE_FALSE);
    let none = builder.ins().icmp_imm(IntCC::Equal, raw, VALUE_NONE as i64);
    let truth = builder.ins().bor(integer_truth, true_bool);
    let booleans = builder.ins().bor(true_bool, false_bool);
    let valid = builder.ins().bor(integer, booleans);
    let valid = builder.ins().bor(valid, none);
    (valid, truth)
}
fn guard(
    builder: &mut FunctionBuilder<'_>,
    condition: Value,
    registers: Value,
    pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
) -> Value {
    let pass = builder.create_block();
    builder.append_block_param(pass, pointer_type);
    let fail = builder.create_block();
    builder.ins().brif(condition, pass, &[registers], fail, &[]);
    builder.switch_to_block(fail);
    let status = builder.ins().iconst(types::I64, pc as i64);
    builder.ins().return_(&[status]);
    builder.switch_to_block(pass);
    builder.block_params(pass)[0]
}

fn runtime_guard(
    builder: &mut FunctionBuilder<'_>,
    condition: Value,
    registers: Value,
    pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
) -> Value {
    let pass = builder.create_block();
    builder.append_block_param(pass, pointer_type);
    let fail = builder.create_block();
    builder.ins().brif(condition, pass, &[registers], fail, &[]);
    builder.switch_to_block(fail);
    let status = builder
        .ins()
        .iconst(types::I64, (ERROR_FLAG | pc as u64) as i64);
    builder.ins().return_(&[status]);
    builder.switch_to_block(pass);
    builder.block_params(pass)[0]
}

#[allow(clippy::too_many_arguments)]
fn emit_float_entry_initialization(
    builder: &mut FunctionBuilder<'_>,
    mut registers: Value,
    start_pc: Value,
    analysis: &FloatAnalysis,
    slots: &[Option<StackSlot>],
    scratch: StackSlot,
    runtime_signature: cranelift_codegen::ir::SigRef,
    runtime_helper: Value,
    runtime_context: Value,
    root_count: usize,
    pointer_type: cranelift_codegen::ir::Type,
) -> Value {
    let root_count = builder.ins().iconst(pointer_type, root_count as i64);
    let operation = builder
        .ins()
        .iconst(types::I32, i64::from(RuntimeOp::UnboxFloat as u32));
    let zero = builder.ins().iconst(types::I64, 0);
    for (register, slot) in slots.iter().enumerate() {
        let Some(slot) = slot else { continue };
        let mut needed = None;
        for (pc, state) in analysis.before.iter().enumerate() {
            if state.as_ref().is_some_and(|state| state[register]) {
                let at_pc = builder.ins().icmp_imm(IntCC::Equal, start_pc, pc as i64);
                needed = Some(match needed {
                    Some(previous) => builder.ins().bor(previous, at_pc),
                    None => at_pc,
                });
            }
        }
        let needed = needed.expect("float slot has at least one live PC");
        let initialize = builder.create_block();
        builder.append_block_param(initialize, pointer_type);
        let skip = builder.create_block();
        builder.append_block_param(skip, pointer_type);
        let next = builder.create_block();
        builder.append_block_param(next, pointer_type);
        builder
            .ins()
            .brif(needed, initialize, &[registers], skip, &[registers]);

        builder.switch_to_block(initialize);
        let initialize_registers = builder.block_params(initialize)[0];
        let boxed = load(builder, initialize_registers, register as u16);
        let output = builder.ins().stack_addr(pointer_type, scratch, 0);
        let call = builder.ins().call_indirect(
            runtime_signature,
            runtime_helper,
            &[
                runtime_context,
                initialize_registers,
                root_count,
                operation,
                boxed,
                zero,
                output,
            ],
        );
        let status = builder.inst_results(call)[0];
        let helper_ok = builder.ins().icmp_imm(IntCC::Equal, status, 0);
        let inspect = builder.create_block();
        builder.append_block_param(inspect, pointer_type);
        let helper_failed = builder.create_block();
        builder.ins().brif(
            helper_ok,
            inspect,
            &[initialize_registers],
            helper_failed,
            &[],
        );
        builder.switch_to_block(helper_failed);
        let error_flag = builder.ins().iconst(types::I64, ERROR_FLAG as i64);
        let error = builder.ins().bor(start_pc, error_flag);
        builder.ins().return_(&[error]);

        builder.switch_to_block(inspect);
        let inspect_registers = builder.block_params(inspect)[0];
        let present = builder.ins().stack_load(types::I64, scratch, 8);
        let present = builder.ins().icmp_imm(IntCC::NotEqual, present, 0);
        let store_float = builder.create_block();
        builder.append_block_param(store_float, pointer_type);
        let guard_failed = builder.create_block();
        builder.ins().brif(
            present,
            store_float,
            &[inspect_registers],
            guard_failed,
            &[],
        );
        builder.switch_to_block(guard_failed);
        builder.ins().return_(&[start_pc]);

        builder.switch_to_block(store_float);
        let store_registers = builder.block_params(store_float)[0];
        let bits = builder.ins().stack_load(types::I64, scratch, 0);
        let float = builder.ins().bitcast(types::F64, MemFlags::new(), bits);
        builder.ins().stack_store(float, *slot, 0);
        builder.ins().jump(next, &[store_registers]);

        builder.switch_to_block(skip);
        let skip_registers = builder.block_params(skip)[0];
        builder.ins().jump(next, &[skip_registers]);
        builder.switch_to_block(next);
        registers = builder.block_params(next)[0];
    }
    registers
}

#[allow(clippy::too_many_arguments)]
fn emit_backedge_poll(
    builder: &mut FunctionBuilder<'_>,
    registers: Value,
    target: cranelift_codegen::ir::Block,
    runtime_signature: cranelift_codegen::ir::SigRef,
    runtime_helper: Value,
    runtime_context: Value,
    root_count: usize,
    poll_slot: StackSlot,
    pc: usize,
    target_pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
    float_state: Option<&[bool]>,
    float_slots: &[Option<StackSlot>],
) {
    let remaining = builder.ins().stack_load(types::I64, poll_slot, 0);
    let remaining = builder.ins().iadd_imm(remaining, -1);
    builder.ins().stack_store(remaining, poll_slot, 0);
    let due = builder.ins().icmp_imm(IntCC::Equal, remaining, 0);
    let poll = builder.create_block();
    builder.append_block_param(poll, pointer_type);
    let continue_loop = builder.create_block();
    builder.append_block_param(continue_loop, pointer_type);
    builder
        .ins()
        .brif(due, poll, &[registers], continue_loop, &[registers]);

    builder.switch_to_block(poll);
    let registers = builder.block_params(poll)[0];
    let count = builder.ins().iconst(pointer_type, root_count as i64);
    let operation = builder
        .ins()
        .iconst(types::I32, i64::from(RuntimeOp::Poll as u32));
    let unused = builder.ins().iconst(types::I64, 0);
    let output = builder.ins().stack_addr(pointer_type, poll_slot, 0);
    let call = builder.ins().call_indirect(
        runtime_signature,
        runtime_helper,
        &[
            runtime_context,
            registers,
            count,
            operation,
            unused,
            unused,
            output,
        ],
    );
    let status = builder.inst_results(call)[0];
    let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
    let registers = runtime_guard(builder, success, registers, pc, pointer_type);
    let deopt_requested = builder.ins().stack_load(types::I64, poll_slot, 0);
    let interval = builder.ins().iconst(types::I64, BACKEDGE_POLL_INTERVAL);
    builder.ins().stack_store(interval, poll_slot, 0);
    if float_state.is_some_and(|state| state.iter().any(|value| *value)) {
        let requested = builder.ins().icmp_imm(IntCC::NotEqual, deopt_requested, 0);
        let deopt = builder.create_block();
        builder.append_block_param(deopt, pointer_type);
        let resume = builder.create_block();
        builder.append_block_param(resume, pointer_type);
        builder
            .ins()
            .brif(requested, deopt, &[registers], resume, &[registers]);
        builder.switch_to_block(deopt);
        let deopt_registers = builder.block_params(deopt)[0];
        emit_float_deopt(
            builder,
            deopt_registers,
            target_pc,
            float_state.expect("checked float state"),
            float_slots,
            runtime_signature,
            runtime_helper,
            runtime_context,
            root_count,
            pointer_type,
        );
        builder.switch_to_block(resume);
        let registers = builder.block_params(resume)[0];
        builder.ins().jump(target, &[registers]);
    } else {
        builder.ins().jump(target, &[registers]);
    }

    builder.switch_to_block(continue_loop);
    let registers = builder.block_params(continue_loop)[0];
    builder.ins().jump(target, &[registers]);
}

#[allow(clippy::too_many_arguments)]
fn emit_float_deopt(
    builder: &mut FunctionBuilder<'_>,
    mut registers: Value,
    pc: usize,
    float_state: &[bool],
    float_slots: &[Option<StackSlot>],
    runtime_signature: cranelift_codegen::ir::SigRef,
    runtime_helper: Value,
    runtime_context: Value,
    root_count: usize,
    pointer_type: cranelift_codegen::ir::Type,
) {
    let root_count = builder.ins().iconst(pointer_type, root_count as i64);
    let operation = builder
        .ins()
        .iconst(types::I32, i64::from(RuntimeOp::BoxFloat as u32));
    let zero = builder.ins().iconst(types::I64, 0);
    for (register, float) in float_state.iter().copied().enumerate() {
        if !float {
            continue;
        }
        let value = builder.ins().stack_load(
            types::F64,
            float_slots[register].expect("deopt map float stack slot"),
            0,
        );
        let bits = builder.ins().bitcast(types::I64, MemFlags::new(), value);
        let output = builder
            .ins()
            .iadd_imm(registers, i64::from(offset(register as u16)));
        let call = builder.ins().call_indirect(
            runtime_signature,
            runtime_helper,
            &[
                runtime_context,
                registers,
                root_count,
                operation,
                bits,
                zero,
                output,
            ],
        );
        let status = builder.inst_results(call)[0];
        let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
        registers = runtime_guard(builder, success, registers, pc, pointer_type);
    }
    let status = builder.ins().iconst(types::I64, pc as i64);
    builder.ins().return_(&[status]);
}

#[allow(clippy::too_many_arguments)]
fn emit_method_load(
    builder: &mut FunctionBuilder<'_>,
    instruction: &tonic_core::bytecode::Instr,
    direct: &DirectCall<'_>,
    registers: Value,
    runtime_signature: cranelift_codegen::ir::SigRef,
    runtime_helper: Value,
    runtime_context: Value,
    blocks: &[cranelift_codegen::ir::Block],
    pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
    root_count: usize,
    method_cache: usize,
) {
    let owner = load(builder, registers, instruction.b);
    let initialized = load_word(builder, registers, method_cache + 3);
    let initialized = builder
        .ins()
        .icmp_imm(IntCC::Equal, initialized, VALUE_TRUE);
    let cached = builder.create_block();
    builder.append_block_param(cached, pointer_type);
    builder.append_block_param(cached, types::I64);
    let initialize = builder.create_block();
    builder.append_block_param(initialize, pointer_type);
    builder.append_block_param(initialize, types::I64);
    let ready = builder.create_block();
    builder.append_block_param(ready, pointer_type);
    builder.ins().brif(
        initialized,
        cached,
        &[registers, owner],
        initialize,
        &[registers, owner],
    );

    builder.switch_to_block(cached);
    let registers = builder.block_params(cached)[0];
    let owner = builder.block_params(cached)[1];
    let cached_owner = load_word(builder, registers, method_cache + 2);
    let same_owner = builder.ins().icmp(IntCC::Equal, owner, cached_owner);
    let registers = guard(builder, same_owner, registers, pc, pointer_type);
    builder.ins().jump(ready, &[registers]);

    builder.switch_to_block(initialize);
    let registers = builder.block_params(initialize)[0];
    let owner = builder.block_params(initialize)[1];
    let register_count = builder.ins().iconst(pointer_type, root_count as i64);
    let operation = builder
        .ins()
        .iconst(types::I32, i64::from(RuntimeOp::LoadMethod as u32));
    let binding = direct.method_binding.expect("validated method binding") as u64;
    let selector = u64::from(instruction.c) | (binding << 32);
    let symbol = builder.ins().iconst(types::I64, selector as i64);
    let output = builder
        .ins()
        .iadd_imm(registers, i64::from(word_offset(method_cache)));
    let call = builder.ins().call_indirect(
        runtime_signature,
        runtime_helper,
        &[
            runtime_context,
            registers,
            register_count,
            operation,
            owner,
            symbol,
            output,
        ],
    );
    let status = builder.inst_results(call)[0];
    let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
    let registers = runtime_guard(builder, success, registers, pc, pointer_type);
    store_word(builder, registers, method_cache + 2, owner);
    let initialized = builder.ins().iconst(types::I64, VALUE_TRUE);
    store_word(builder, registers, method_cache + 3, initialized);
    builder.ins().jump(ready, &[registers]);

    builder.switch_to_block(ready);
    let registers = builder.block_params(ready)[0];
    let function = load_word(builder, registers, method_cache);
    store(builder, registers, instruction.a, function);
    let expected = builder.ins().iconst(types::I64, direct.callee as i64);
    let exact = builder.ins().icmp(IntCC::Equal, function, expected);
    let registers = guard(builder, exact, registers, pc, pointer_type);
    fallthrough(builder, blocks, pc, registers);
}

#[allow(clippy::too_many_arguments)]
fn emit_direct_call(
    builder: &mut FunctionBuilder<'_>,
    caller: &CodeObject,
    instruction: &tonic_core::bytecode::Instr,
    direct: &DirectCall<'_>,
    mut registers: Value,
    direct_call_counter: Value,
    blocks: &[cranelift_codegen::ir::Block],
    pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
    runtime_signature: cranelift_codegen::ir::SigRef,
    runtime_helper: Value,
    runtime_context: Value,
    root_count: usize,
    method_cache: Option<usize>,
    argument_root: Option<usize>,
) {
    let deopt_pc = direct
        .method_attr_pc
        .or(direct.expanded_begin_pc)
        .unwrap_or(pc);
    let callee = load(builder, registers, instruction.b);
    let expected = builder.ins().iconst(types::I64, direct.callee as i64);
    let exact = builder.ins().icmp(IntCC::Equal, callee, expected);
    registers = guard(builder, exact, registers, deopt_pc, pointer_type);

    let site = (Op::try_from(instruction.opcode) == Ok(Op::Call))
        .then(|| &caller.calls[instruction.c as usize]);
    if direct.float {
        emit_direct_float_call(
            builder,
            instruction,
            direct,
            registers,
            direct_call_counter,
            blocks,
            pc,
            pointer_type,
            runtime_signature,
            runtime_helper,
            runtime_context,
            root_count,
            site.expect("validated float ordinary call site"),
            argument_root.expect("validated float result root"),
        );
        return;
    }
    let unbound = builder.ins().iconst(types::I64, VALUE_UNBOUND as i64);
    let mut inline_registers = vec![unbound; direct.target.registers as usize];
    let mut arguments_bound = None;
    let mut next_argument_root = argument_root;
    for (slot, source) in direct.arguments.iter().cloned().enumerate() {
        let argument = match source {
            DirectArgument::Caller(offset) => {
                let site = site.expect("validated ordinary call site");
                let argument = load(builder, registers, site.first + offset);
                let bound = builder
                    .ins()
                    .icmp_imm(IntCC::NotEqual, argument, VALUE_UNBOUND as i64);
                arguments_bound = Some(match arguments_bound {
                    Some(previous) => builder.ins().band(previous, bound),
                    None => bound,
                });
                argument
            }
            DirectArgument::Register(register) => load(builder, registers, register),
            DirectArgument::SequenceItem {
                register,
                index,
                length,
            } => {
                let output_root = next_argument_root.expect("validated sequence argument root");
                next_argument_root = Some(output_root + 1);
                let owner = load(builder, registers, register);
                let count = builder.ins().iconst(pointer_type, root_count as i64);
                let operation = builder
                    .ins()
                    .iconst(types::I32, i64::from(RuntimeOp::LoadSequenceItem as u32));
                let selector = u64::from(index) | (u64::from(length) << 32);
                let selector = builder.ins().iconst(types::I64, selector as i64);
                let output = builder
                    .ins()
                    .iadd_imm(registers, i64::from(word_offset(output_root)));
                let call = builder.ins().call_indirect(
                    runtime_signature,
                    runtime_helper,
                    &[
                        runtime_context,
                        registers,
                        count,
                        operation,
                        owner,
                        selector,
                        output,
                    ],
                );
                let status = builder.inst_results(call)[0];
                let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                registers = runtime_guard(builder, success, registers, deopt_pc, pointer_type);
                let argument = load_word(builder, registers, output_root);
                let present =
                    builder
                        .ins()
                        .icmp_imm(IntCC::NotEqual, argument, VALUE_UNBOUND as i64);
                registers = guard(builder, present, registers, deopt_pc, pointer_type);
                argument
            }
            DirectArgument::MappingItem {
                register,
                symbol,
                key_count,
            } => {
                let output_root = next_argument_root.expect("validated mapping argument root");
                next_argument_root = Some(output_root + 1);
                let owner = load(builder, registers, register);
                let count = builder.ins().iconst(pointer_type, root_count as i64);
                let operation = builder
                    .ins()
                    .iconst(types::I32, i64::from(RuntimeOp::LoadMappingItem as u32));
                let selector = u64::from(symbol) | (u64::from(key_count) << 32);
                let selector = builder.ins().iconst(types::I64, selector as i64);
                let output = builder
                    .ins()
                    .iadd_imm(registers, i64::from(word_offset(output_root)));
                let call = builder.ins().call_indirect(
                    runtime_signature,
                    runtime_helper,
                    &[
                        runtime_context,
                        registers,
                        count,
                        operation,
                        owner,
                        selector,
                        output,
                    ],
                );
                let status = builder.inst_results(call)[0];
                let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                registers = runtime_guard(builder, success, registers, deopt_pc, pointer_type);
                let argument = load_word(builder, registers, output_root);
                let present =
                    builder
                        .ins()
                        .icmp_imm(IntCC::NotEqual, argument, VALUE_UNBOUND as i64);
                registers = guard(builder, present, registers, deopt_pc, pointer_type);
                argument
            }
            DirectArgument::VariadicTuple { first, count } => {
                let output_root = next_argument_root.expect("validated variadic tuple root");
                next_argument_root = Some(output_root + 1);
                let root_count_value = builder.ins().iconst(pointer_type, root_count as i64);
                let operation = builder
                    .ins()
                    .iconst(types::I32, i64::from(RuntimeOp::BuildTuple as u32));
                let first = builder.ins().iconst(types::I64, i64::from(first));
                let count = builder.ins().iconst(types::I64, i64::from(count));
                let output = builder
                    .ins()
                    .iadd_imm(registers, i64::from(word_offset(output_root)));
                let call = builder.ins().call_indirect(
                    runtime_signature,
                    runtime_helper,
                    &[
                        runtime_context,
                        registers,
                        root_count_value,
                        operation,
                        first,
                        count,
                        output,
                    ],
                );
                let status = builder.inst_results(call)[0];
                let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                registers = runtime_guard(builder, success, registers, deopt_pc, pointer_type);
                load_word(builder, registers, output_root)
            }
            DirectArgument::VariadicDict { items } => {
                let output_root = next_argument_root.expect("validated variadic dict root");
                next_argument_root = Some(output_root + 1);
                let root_count_value = builder.ins().iconst(pointer_type, root_count as i64);
                let build = builder
                    .ins()
                    .iconst(types::I32, i64::from(RuntimeOp::BuildDict as u32));
                let zero = builder.ins().iconst(types::I64, 0);
                let output = builder
                    .ins()
                    .iadd_imm(registers, i64::from(word_offset(output_root)));
                let call = builder.ins().call_indirect(
                    runtime_signature,
                    runtime_helper,
                    &[
                        runtime_context,
                        registers,
                        root_count_value,
                        build,
                        zero,
                        zero,
                        output,
                    ],
                );
                let status = builder.inst_results(call)[0];
                let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                registers = runtime_guard(builder, success, registers, deopt_pc, pointer_type);
                for (symbol, value_register) in items {
                    let owner = load_word(builder, registers, output_root);
                    let operation = builder
                        .ins()
                        .iconst(types::I32, i64::from(RuntimeOp::DictSetSymbol as u32));
                    let selector = u64::from(symbol) | (u64::from(value_register) << 32);
                    let selector = builder.ins().iconst(types::I64, selector as i64);
                    let call = builder.ins().call_indirect(
                        runtime_signature,
                        runtime_helper,
                        &[
                            runtime_context,
                            registers,
                            root_count_value,
                            operation,
                            owner,
                            selector,
                            output,
                        ],
                    );
                    let status = builder.inst_results(call)[0];
                    let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                    registers = runtime_guard(builder, success, registers, deopt_pc, pointer_type);
                }
                load_word(builder, registers, output_root)
            }
            DirectArgument::MethodReceiver => load_word(
                builder,
                registers,
                method_cache.expect("validated method receiver cache") + 1,
            ),
            DirectArgument::Default(value) => builder.ins().iconst(types::I64, value as i64),
        };
        inline_registers[slot] = argument;
    }
    if let Some(arguments_bound) = arguments_bound {
        registers = guard(builder, arguments_bound, registers, deopt_pc, pointer_type);
    }

    for target_instruction in &direct.target.instructions {
        let op = Op::try_from(target_instruction.opcode)
            .expect("validated direct-call target opcode could not be decoded");
        match op {
            Op::Const => {
                inline_registers[target_instruction.a as usize] = builder.ins().iconst(
                    types::I64,
                    encode_constant(&direct.target.constants[target_instruction.b as usize])
                        .expect("validated direct-call constant") as i64,
                );
            }
            Op::Move => {
                inline_registers[target_instruction.a as usize] =
                    inline_registers[target_instruction.b as usize];
            }
            Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul => {
                let left = inline_registers[target_instruction.b as usize];
                let right = inline_registers[target_instruction.c as usize];
                let exact = both_exact_int(builder, left, right);
                registers = guard(builder, exact, registers, deopt_pc, pointer_type);
                let left = decode_int(builder, left);
                let right = decode_int(builder, right);
                let (result, valid) = match op {
                    Op::Add | Op::InplaceAdd => (builder.ins().iadd(left, right), None),
                    Op::Sub => (builder.ins().isub(left, right), None),
                    Op::Mul => {
                        let (result, overflow) = builder.ins().smul_overflow(left, right);
                        let valid = builder.ins().icmp_imm(IntCC::Equal, overflow, 0);
                        (result, Some(valid))
                    }
                    _ => unreachable!(),
                };
                let in_range = immediate_range(builder, result);
                let valid = valid
                    .map(|valid| builder.ins().band(valid, in_range))
                    .unwrap_or(in_range);
                let (guarded_registers, result) =
                    guard_value(builder, valid, registers, result, deopt_pc, pointer_type);
                registers = guarded_registers;
                inline_registers[target_instruction.a as usize] = encode_int(builder, result);
            }
            Op::Return => {
                let result = inline_registers[target_instruction.a as usize];
                store(builder, registers, instruction.a, result);
                let count =
                    builder
                        .ins()
                        .load(types::I64, MemFlags::trusted(), direct_call_counter, 0);
                let count = builder.ins().iadd_imm(count, 1);
                builder
                    .ins()
                    .store(MemFlags::trusted(), count, direct_call_counter, 0);
                fallthrough(builder, blocks, pc, registers);
                return;
            }
            _ => unreachable!("validated non-inlineable direct-call opcode"),
        }
    }
    unreachable!("validated direct-call target has no return")
}

#[allow(clippy::too_many_arguments)]
fn emit_direct_float_call(
    builder: &mut FunctionBuilder<'_>,
    instruction: &tonic_core::bytecode::Instr,
    direct: &DirectCall<'_>,
    mut registers: Value,
    direct_call_counter: Value,
    blocks: &[cranelift_codegen::ir::Block],
    pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
    runtime_signature: cranelift_codegen::ir::SigRef,
    runtime_helper: Value,
    runtime_context: Value,
    root_count: usize,
    site: &tonic_core::bytecode::CallSite,
    result_root: usize,
) {
    let scratch =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3));
    let root_count_value = builder.ins().iconst(pointer_type, root_count as i64);
    let operation = builder
        .ins()
        .iconst(types::I32, i64::from(RuntimeOp::UnboxFloat as u32));
    let zero = builder.ins().iconst(types::I64, 0);
    let mut inline_registers = vec![None; direct.target.registers as usize];
    for (slot, argument) in direct.arguments.iter().enumerate() {
        let DirectArgument::Caller(offset) = argument else {
            unreachable!("validated float direct argument")
        };
        let boxed = load(builder, registers, site.first + *offset);
        let output = builder.ins().stack_addr(pointer_type, scratch, 0);
        let call = builder.ins().call_indirect(
            runtime_signature,
            runtime_helper,
            &[
                runtime_context,
                registers,
                root_count_value,
                operation,
                boxed,
                zero,
                output,
            ],
        );
        let status = builder.inst_results(call)[0];
        let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
        registers = runtime_guard(builder, success, registers, pc, pointer_type);
        let present = builder.ins().stack_load(types::I64, scratch, 8);
        let present = builder.ins().icmp_imm(IntCC::NotEqual, present, 0);
        registers = guard(builder, present, registers, pc, pointer_type);
        let bits = builder.ins().stack_load(types::I64, scratch, 0);
        inline_registers[slot] = Some(builder.ins().bitcast(types::F64, MemFlags::new(), bits));
    }

    for target_instruction in &direct.target.instructions {
        let op = Op::try_from(target_instruction.opcode)
            .expect("validated float direct-call opcode could not be decoded");
        match op {
            Op::Move => {
                inline_registers[target_instruction.a as usize] =
                    inline_registers[target_instruction.b as usize];
            }
            Op::Add | Op::InplaceAdd | Op::Sub | Op::Mul => {
                let left = inline_registers[target_instruction.b as usize]
                    .expect("validated initialized float operand");
                let right = inline_registers[target_instruction.c as usize]
                    .expect("validated initialized float operand");
                inline_registers[target_instruction.a as usize] = Some(match op {
                    Op::Add | Op::InplaceAdd => builder.ins().fadd(left, right),
                    Op::Sub => builder.ins().fsub(left, right),
                    Op::Mul => builder.ins().fmul(left, right),
                    _ => unreachable!(),
                });
            }
            Op::Return => {
                let result = inline_registers[target_instruction.a as usize]
                    .expect("validated initialized float result");
                let bits = builder.ins().bitcast(types::I64, MemFlags::new(), result);
                let operation = builder
                    .ins()
                    .iconst(types::I32, i64::from(RuntimeOp::BoxFloat as u32));
                let output = builder
                    .ins()
                    .iadd_imm(registers, i64::from(word_offset(result_root)));
                let call = builder.ins().call_indirect(
                    runtime_signature,
                    runtime_helper,
                    &[
                        runtime_context,
                        registers,
                        root_count_value,
                        operation,
                        bits,
                        zero,
                        output,
                    ],
                );
                let status = builder.inst_results(call)[0];
                let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                registers = runtime_guard(builder, success, registers, pc, pointer_type);
                let boxed = load_word(builder, registers, result_root);
                store(builder, registers, instruction.a, boxed);
                let count =
                    builder
                        .ins()
                        .load(types::I64, MemFlags::trusted(), direct_call_counter, 0);
                let count = builder.ins().iadd_imm(count, 1);
                builder
                    .ins()
                    .store(MemFlags::trusted(), count, direct_call_counter, 0);
                fallthrough(builder, blocks, pc, registers);
                return;
            }
            _ => unreachable!("validated non-inlineable float direct-call opcode"),
        }
    }
    unreachable!("validated float direct-call target has no return")
}

fn side_exit(builder: &mut FunctionBuilder<'_>, _registers: Value, pc: usize) {
    let status = builder
        .ins()
        .iconst(types::I64, (SIDE_EXIT_FLAG | pc as u64) as i64);
    builder.ins().return_(&[status]);
}

fn backedge_count(code: &CodeObject) -> usize {
    code.instructions
        .iter()
        .enumerate()
        .filter(|(pc, instruction)| match Op::try_from(instruction.opcode) {
            Ok(Op::Jump) => usize::from(instruction.a) <= *pc,
            Ok(Op::JumpFalse | Op::JumpTrue) => usize::from(instruction.b) <= *pc,
            _ => false,
        })
        .count()
}
fn guard_value(
    builder: &mut FunctionBuilder<'_>,
    condition: Value,
    registers: Value,
    value: Value,
    pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
) -> (Value, Value) {
    let value_type = builder.func.dfg.value_type(value);
    let pass = builder.create_block();
    builder.append_block_param(pass, pointer_type);
    builder.append_block_param(pass, value_type);
    let fail = builder.create_block();
    builder
        .ins()
        .brif(condition, pass, &[registers, value], fail, &[]);
    builder.switch_to_block(fail);
    let status = builder.ins().iconst(types::I64, pc as i64);
    builder.ins().return_(&[status]);
    builder.switch_to_block(pass);
    (builder.block_params(pass)[0], builder.block_params(pass)[1])
}

#[allow(clippy::too_many_arguments)]
fn guard_values(
    builder: &mut FunctionBuilder<'_>,
    condition: Value,
    registers: Value,
    first: Value,
    second: Value,
    pc: usize,
    pointer_type: cranelift_codegen::ir::Type,
) -> (Value, Value, Value) {
    let first_type = builder.func.dfg.value_type(first);
    let second_type = builder.func.dfg.value_type(second);
    let pass = builder.create_block();
    builder.append_block_param(pass, pointer_type);
    builder.append_block_param(pass, first_type);
    builder.append_block_param(pass, second_type);
    let fail = builder.create_block();
    builder
        .ins()
        .brif(condition, pass, &[registers, first, second], fail, &[]);
    builder.switch_to_block(fail);
    let status = builder.ins().iconst(types::I64, pc as i64);
    builder.ins().return_(&[status]);
    builder.switch_to_block(pass);
    (
        builder.block_params(pass)[0],
        builder.block_params(pass)[1],
        builder.block_params(pass)[2],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(source: &str) -> tonic_core::bytecode::VerifiedProgram {
        tonic_compiler::compile(source, "jit-test").unwrap()
    }

    #[test]
    fn malformed_public_code_objects_fail_before_codegen() {
        let program = function("def value():\n    return 1");
        let original = &program.program().code[1];

        let mut bad_constant = original.clone();
        bad_constant.instructions[0].b = u16::MAX;
        assert!(matches!(
            compile(&bad_constant),
            Err(Error::InvalidBytecode {
                pc: Some(0),
                ref message
            }) if message == "constant out of bounds"
        ));
        assert!(!is_direct_call_inlineable(&bad_constant));
        assert!(!is_direct_float_leaf_inlineable(&bad_constant));

        let mut bad_register = original.clone();
        bad_register.instructions[1].a = bad_register.registers;
        assert!(matches!(
            compile(&bad_register),
            Err(Error::InvalidBytecode {
                pc: Some(1),
                ref message
            }) if message == "register out of bounds"
        ));

        let program = function("def choose(x):\n    if x:\n        return 1\n    return 2");
        let mut bad_jump = program.program().code[1].clone();
        let (pc, instruction) = bad_jump
            .instructions
            .iter_mut()
            .enumerate()
            .find(|(_, instruction)| {
                matches!(
                    Op::try_from(instruction.opcode),
                    Ok(Op::Jump | Op::JumpFalse | Op::JumpTrue)
                )
            })
            .expect("conditional has a jump");
        if Op::try_from(instruction.opcode) == Ok(Op::Jump) {
            instruction.a = u16::MAX;
        } else {
            instruction.b = u16::MAX;
        }
        assert!(matches!(
            compile(&bad_jump),
            Err(Error::InvalidBytecode {
                pc: Some(error_pc),
                ref message
            }) if error_pc == pc && message == "jump out of bounds"
        ));

        assert!(matches!(
            compile_with_execution_profile(original, &[], &[], &[original.registers]),
            Err(Error::InvalidBytecode {
                pc: None,
                ref message
            }) if message == "invalid exact-float parameter profile"
        ));
    }

    #[test]
    fn compiles_numeric_loop_and_returns_materialized_value() {
        let program = function(
            "def sum_to(n):\n    total=0\n    i=0\n    while i<n:\n        total+=i\n        i+=1\n    return total",
        );
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = encode_i64(100);
        let outcome = compiled.run(&mut registers).unwrap();
        assert!(compiled.metadata().code_bytes > 0);
        assert_eq!(CRANELIFT_VERSION, "0.119.0");
        let Outcome::Returned { value, pc } = outcome else {
            panic!("numeric loop unexpectedly deoptimized");
        };
        assert_eq!(value, encode_i64(4950));
        assert_eq!(
            Op::try_from(code.instructions[pc].opcode).unwrap(),
            Op::Return
        );
    }

    #[test]
    fn unboxed_float_loop_has_complete_deopt_maps_and_resumable_state() {
        struct FloatRuntime {
            boxed: Vec<f64>,
            request_deopt: bool,
        }
        impl Runtime for FloatRuntime {
            fn load_global(
                &mut self,
                _symbol: u32,
                _registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                unreachable!("test loop has no globals")
            }

            fn binary(
                &mut self,
                op: RuntimeOp,
                left: u64,
                right: u64,
                _registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                let left = decode_i64(left).unwrap();
                let right = decode_i64(right).unwrap();
                Ok(match op {
                    RuntimeOp::InplaceAdd | RuntimeOp::Add => encode_i64(left + right),
                    RuntimeOp::Lt => {
                        if left < right {
                            VALUE_TRUE as u64
                        } else {
                            VALUE_FALSE as u64
                        }
                    }
                    _ => unreachable!("test loop uses increment and less-than"),
                })
            }

            fn unbox_float(
                &mut self,
                value: u64,
                _registers: &[u64],
            ) -> Result<Option<u64>, RuntimeFailure> {
                Ok(match value {
                    0x100 => Some(0.0_f64.to_bits()),
                    0x108 => Some(0.5_f64.to_bits()),
                    0x110 => Some(2.5_f64.to_bits()),
                    _ => None,
                })
            }

            fn box_float(&mut self, bits: u64, _registers: &[u64]) -> Result<u64, RuntimeFailure> {
                let handle = 0x200 + self.boxed.len() as u64 * 8;
                self.boxed.push(f64::from_bits(bits));
                Ok(handle)
            }

            fn poll(&mut self, _registers: &[u64]) -> Result<u64, RuntimeFailure> {
                Ok(u64::from(self.request_deopt))
            }
        }

        let program = function(
            "def accumulate(value,step,n):\n    i=0\n    while i<n:\n        value+=step\n        i+=1\n    return value",
        );
        let code = &program.program().code[1];
        let compiled = compile_with_execution_profile(code, &[], &[], &[0, 1]).unwrap();
        assert!(compiled.deopt_maps().iter().all(|map| {
            map.register_count == code.registers as usize
                && map
                    .unboxed_float_registers
                    .iter()
                    .all(|register| *register < code.registers)
        }));
        let arithmetic_map = compiled
            .deopt_maps()
            .iter()
            .find(|map| map.pc == 8)
            .unwrap();
        assert_eq!(arithmetic_map.unboxed_float_registers, [0, 1, 7, 8]);

        let mut runtime = FloatRuntime {
            boxed: Vec::new(),
            request_deopt: false,
        };
        let mut registers = vec![VALUE_UNBOUND; compiled.metadata().root_count];
        registers[0] = 0x100;
        registers[1] = 0x108;
        registers[2] = encode_i64(10);
        let Outcome::Returned { value, .. } = compiled
            .run_with_runtime(&mut registers, &mut runtime)
            .unwrap()
        else {
            panic!("unboxed loop unexpectedly deoptimized")
        };
        assert_eq!(value, 0x200);
        assert_eq!(runtime.boxed, [5.0]);

        registers.fill(VALUE_UNBOUND);
        registers[0] = 0x110;
        registers[1] = 0x108;
        registers[2] = encode_i64(10);
        registers[3] = encode_i64(5);
        runtime.boxed.clear();
        let Outcome::Returned { value, .. } =
            compiled.run_from(&mut registers, 2, &mut runtime).unwrap()
        else {
            panic!("resumed unboxed loop unexpectedly deoptimized")
        };
        assert_eq!(value, 0x200);
        assert_eq!(runtime.boxed, [5.0]);

        registers.fill(VALUE_UNBOUND);
        registers[0] = 0x110;
        registers[1] = encode_i64(1);
        registers[2] = encode_i64(10);
        registers[3] = encode_i64(5);
        let original = registers.clone();
        assert_eq!(
            compiled.run_from(&mut registers, 2, &mut runtime).unwrap(),
            Outcome::Deopt { pc: 2 }
        );
        assert_eq!(registers, original);

        registers.fill(VALUE_UNBOUND);
        registers[0] = 0x100;
        registers[1] = 0x108;
        registers[2] = encode_i64(2_000);
        runtime.boxed.clear();
        runtime.request_deopt = true;
        assert_eq!(
            compiled
                .run_with_runtime(&mut registers, &mut runtime)
                .unwrap(),
            Outcome::Deopt { pc: 2 }
        );
        assert_eq!(runtime.boxed.len(), 2);
        assert_eq!(runtime.boxed[1], 0.5);
        assert_eq!(registers[0], 0x200);
        assert_eq!(registers[1], 0x208);
    }

    #[test]
    fn exact_callee_integer_leaf_is_inlined_with_atomic_guard_deopt() {
        let program = function(
            "def add(a,b):\n    return a+b\ndef caller(f,n):\n    i=0\n    total=0\n    while i<n:\n        total=f(total,1)\n        i+=1\n    return total",
        );
        let program = program.program();
        let target = &program.code[1];
        let caller = &program.code[2];
        let pc = caller
            .instructions
            .iter()
            .position(|instruction| Op::try_from(instruction.opcode) == Ok(Op::Call))
            .unwrap();
        let callee = 0x1234_5678_u64;
        let compiled = compile_with_direct_calls(
            caller,
            &[DirectCall {
                pc,
                callee,
                target,
                arguments: vec![DirectArgument::Caller(0), DirectArgument::Caller(1)],
                method_attr_pc: None,
                method_binding: None,
                expanded_begin_pc: None,
                float: false,
            }],
        )
        .unwrap();
        let mut registers = vec![VALUE_UNBOUND; caller.registers as usize];
        registers[0] = callee;
        registers[1] = encode_i64(100);
        let Outcome::Returned { value, .. } = compiled.run(&mut registers).unwrap() else {
            panic!("direct integer leaf unexpectedly deoptimized");
        };
        assert_eq!(decode_i64(value), Some(100));
        assert_eq!(compiled.metadata().direct_call_sites, 1);

        registers.fill(VALUE_UNBOUND);
        registers[0] = callee + 1;
        registers[1] = encode_i64(1);
        assert_eq!(compiled.run(&mut registers).unwrap(), Outcome::Deopt { pc });
    }

    #[test]
    fn direct_leaf_uses_prebound_keyword_and_default_arguments() {
        let program = function(
            "def add(a,/,b=1,*,bias=2):\n    return a+b+bias\ndef caller(f,n):\n    i=0\n    total=0\n    while i<n:\n        total=f(total,bias=2)\n        i+=1\n    return total",
        );
        let program = program.program();
        let target = &program.code[1];
        let caller = &program.code[2];
        let pc = caller
            .instructions
            .iter()
            .position(|instruction| Op::try_from(instruction.opcode) == Ok(Op::Call))
            .unwrap();
        let callee = 0x1234_5678_u64;
        let compiled = compile_with_direct_calls(
            caller,
            &[DirectCall {
                pc,
                callee,
                target,
                arguments: vec![
                    DirectArgument::Caller(0),
                    DirectArgument::Default(encode_i64(1)),
                    DirectArgument::Caller(1),
                ],
                method_attr_pc: None,
                method_binding: None,
                expanded_begin_pc: None,
                float: false,
            }],
        )
        .unwrap();
        let mut registers = vec![VALUE_UNBOUND; caller.registers as usize];
        registers[0] = callee;
        registers[1] = encode_i64(100);
        let Outcome::Returned { value, .. } = compiled.run(&mut registers).unwrap() else {
            panic!("bound direct leaf unexpectedly deoptimized");
        };
        assert_eq!(decode_i64(value), Some(300));
    }

    #[test]
    fn direct_leaf_omits_only_unobserved_empty_variadics() {
        let accepted = function("def add(a,b,*rest,**kw):\n    return a+b");
        let accepted = &accepted.program().code[1];
        assert!(is_direct_call_inlineable(accepted));
        assert!(unobserved_variadic_parameters(accepted));

        let observed = function("def reveal(a,*rest,**kw):\n    return rest");
        let observed = &observed.program().code[1];
        assert!(is_direct_call_inlineable(observed));
        assert!(!unobserved_variadic_parameters(observed));
    }

    #[test]
    fn direct_method_load_binds_receiver_and_misses_at_attr_pc() {
        struct MethodRuntime {
            function: u64,
            owner: u64,
            calls: usize,
            saw_cached_roots: bool,
        }
        impl Runtime for MethodRuntime {
            fn binary(
                &mut self,
                _op: RuntimeOp,
                _left: u64,
                _right: u64,
                _registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                Err(RuntimeFailure::new("JitError", "unexpected binary helper"))
            }
            fn load_global(
                &mut self,
                _symbol: u32,
                _registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                Err(RuntimeFailure::new("JitError", "unexpected global helper"))
            }
            fn load_method(
                &mut self,
                owner: u64,
                _selector: u64,
                registers: &[u64],
            ) -> Result<MethodLookup, RuntimeFailure> {
                self.calls += 1;
                assert_eq!(
                    &registers[registers.len() - 4..],
                    &[
                        VALUE_UNBOUND,
                        VALUE_UNBOUND,
                        VALUE_UNBOUND,
                        VALUE_FALSE as u64,
                    ]
                );
                Ok(MethodLookup {
                    function: self.function,
                    receiver: owner,
                })
            }
            fn poll(&mut self, registers: &[u64]) -> Result<u64, RuntimeFailure> {
                self.saw_cached_roots |=
                    registers.contains(&self.function) && registers.contains(&self.owner);
                Ok(0)
            }
        }

        let program = function(
            "class Counter:\n    def add(self,a,/,b=1):\n        return a+b\ndef loop(counter,n):\n    i=0\n    total=0\n    while i<n:\n        total=counter.add(total,b=1)\n        i+=1\n    return total",
        );
        let program = program.program();
        let target = &program.code[2];
        let caller = &program.code[3];
        let attr_pc = caller
            .instructions
            .iter()
            .position(|instruction| Op::try_from(instruction.opcode) == Ok(Op::Attr))
            .unwrap();
        let call_pc = caller
            .instructions
            .iter()
            .position(|instruction| Op::try_from(instruction.opcode) == Ok(Op::Call))
            .unwrap();
        let callee = 0x1234_5678_u64;
        let compiled = compile_with_direct_calls(
            caller,
            &[DirectCall {
                pc: call_pc,
                callee,
                target,
                arguments: vec![
                    DirectArgument::MethodReceiver,
                    DirectArgument::Caller(0),
                    DirectArgument::Caller(1),
                ],
                method_attr_pc: Some(attr_pc),
                method_binding: Some(MethodBinding::Instance),
                expanded_begin_pc: None,
                float: false,
            }],
        )
        .unwrap();
        let owner = 0x2222_2220;
        let mut registers = vec![VALUE_UNBOUND; compiled.metadata().root_count];
        registers[0] = owner;
        registers[1] = encode_i64(2500);
        let mut runtime = MethodRuntime {
            function: callee,
            owner,
            calls: 0,
            saw_cached_roots: false,
        };
        let outcome = compiled
            .run_with_runtime(&mut registers, &mut runtime)
            .unwrap();
        let Outcome::Returned { value, .. } = outcome else {
            panic!("direct method unexpectedly deoptimized: {outcome:?}");
        };
        assert_eq!(decode_i64(value), Some(2500));
        assert_eq!(compiled.metadata().direct_method_sites, 1);
        assert_eq!(
            compiled.metadata().root_count,
            caller.registers as usize + 4
        );
        assert_eq!(runtime.calls, 1);
        assert!(runtime.saw_cached_roots);

        registers.fill(VALUE_UNBOUND);
        registers[0] = owner;
        registers[1] = encode_i64(1);
        runtime.function = callee + 8;
        assert_eq!(
            compiled
                .run_with_runtime(&mut registers, &mut runtime)
                .unwrap(),
            Outcome::Deopt { pc: attr_pc }
        );
        assert_eq!(runtime.calls, 2);
    }

    #[test]
    fn generic_attribute_load_side_exits_at_exact_pc() {
        let program = function(
            "def add(a,b):\n    return a+b\ndef read(value):\n    return value.answer(1,2)",
        );
        let program = program.program();
        let target = &program.code[1];
        let caller = &program.code[2];
        let attr_pc = caller
            .instructions
            .iter()
            .position(|instruction| Op::try_from(instruction.opcode) == Ok(Op::Attr))
            .unwrap();
        let call_pc = caller
            .instructions
            .iter()
            .position(|instruction| Op::try_from(instruction.opcode) == Ok(Op::Call))
            .unwrap();
        let compiled = compile_with_direct_calls(
            caller,
            &[DirectCall {
                pc: call_pc,
                callee: 0x1234_5678,
                target,
                arguments: vec![DirectArgument::Caller(0), DirectArgument::Caller(1)],
                method_attr_pc: None,
                method_binding: None,
                expanded_begin_pc: None,
                float: false,
            }],
        )
        .unwrap();
        let mut registers = vec![VALUE_UNBOUND; caller.registers as usize];
        registers[0] = 0x2222_2220;
        assert_eq!(
            compiled.run(&mut registers).unwrap(),
            Outcome::SideExit { pc: attr_pc }
        );
        assert!(compiled.metadata().resumable);
    }

    #[test]
    fn expanded_argument_segment_side_exits_at_begin() {
        let program = function("def caller(function,values):\n    return function(1,*values)");
        let code = &program.program().code[1];
        let begin_pc = code
            .instructions
            .iter()
            .position(|instruction| Op::try_from(instruction.opcode) == Ok(Op::BeginArgs))
            .unwrap();
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = 0x2222_2220;
        assert_eq!(
            compiled.run(&mut registers).unwrap(),
            Outcome::SideExit { pc: begin_pc }
        );
        assert!(compiled.metadata().resumable);
    }

    #[test]
    fn guard_failure_returns_exact_resume_pc() {
        let program = function("def add(a,b):\n    return a+b");
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = VALUE_TRUE as u64;
        registers[1] = encode_i64(2);
        let Outcome::Deopt { pc } = compiled.run(&mut registers).unwrap() else {
            panic!("expected deoptimization");
        };
        assert_eq!(Op::try_from(code.instructions[pc].opcode).unwrap(), Op::Add);
    }

    #[test]
    fn bound_global_loads_read_the_current_materialized_value_directly() {
        let program = function("def read():\n    return target");
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let symbol = code
            .instructions
            .iter()
            .find(|instruction| Op::try_from(instruction.opcode) == Ok(Op::LoadGlobal))
            .expect("global load")
            .b as usize;
        let mut globals = vec![VALUE_UNBOUND; symbol + 1];
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        let mut runtime = UnavailableRuntime;

        globals[symbol] = encode_i64(41);
        let Outcome::Returned { value, .. } = compiled
            .run_from_with_globals(&mut registers, &globals, 0, &mut runtime)
            .unwrap()
        else {
            panic!("bound global unexpectedly deoptimized");
        };
        assert_eq!(value, encode_i64(41));

        globals[symbol] = encode_i64(42);
        let Outcome::Returned { value, .. } = compiled
            .run_from_with_globals(&mut registers, &globals, 0, &mut runtime)
            .unwrap()
        else {
            panic!("rebound global unexpectedly deoptimized");
        };
        assert_eq!(value, encode_i64(42));
    }

    #[test]
    fn multiplication_and_python_floor_operations_are_native() {
        let program = function("def arithmetic(a,b):\n    return a*b + a//b + a%b");
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = encode_i64(-7);
        registers[1] = encode_i64(3);
        let Outcome::Returned { value, .. } = compiled.run(&mut registers).unwrap() else {
            panic!("integer arithmetic unexpectedly deoptimized");
        };
        assert_eq!(decode_i64(value), Some(-22));
    }

    #[test]
    fn integer_overflow_and_zero_division_deoptimize() {
        let program = function("def multiply(a,b):\n    return a*b");
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = encode_i64(MAX_INT);
        registers[1] = encode_i64(MAX_INT);
        assert!(matches!(
            compiled.run(&mut registers).unwrap(),
            Outcome::Deopt { .. }
        ));

        let program = function("def divide(a,b):\n    return a//b");
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = encode_i64(1);
        registers[1] = encode_i64(0);
        let Outcome::Deopt { pc } = compiled.run(&mut registers).unwrap() else {
            panic!("zero division must deoptimize");
        };
        assert_eq!(
            Op::try_from(code.instructions[pc].opcode).unwrap(),
            Op::FloorDiv
        );
    }

    #[test]
    fn backedge_poll_exposes_materialized_roots_periodically() {
        #[derive(Default)]
        struct PollRuntime {
            polls: usize,
            saw_root: bool,
            fail: bool,
        }
        impl Runtime for PollRuntime {
            fn binary(
                &mut self,
                _op: RuntimeOp,
                _left: u64,
                _right: u64,
                _registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                unreachable!("poll test has no division")
            }

            fn load_global(
                &mut self,
                _symbol: u32,
                _registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                unreachable!("poll test has no globals")
            }

            fn poll(&mut self, registers: &[u64]) -> Result<u64, RuntimeFailure> {
                self.polls += 1;
                self.saw_root |= registers.contains(&0x1_0000_0000);
                if self.fail {
                    Err(RuntimeFailure::new("KeyboardInterrupt", "interrupted"))
                } else {
                    Ok(0)
                }
            }
        }

        let program =
            function("def keep(value,n):\n    i=0\n    while i<n:\n        i+=1\n    return value");
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = 0x1_0000_0000;
        registers[1] = encode_i64(2500);
        let mut runtime = PollRuntime::default();
        let Outcome::Returned { value, .. } = compiled
            .run_with_runtime(&mut registers, &mut runtime)
            .unwrap()
        else {
            panic!("polling loop unexpectedly deoptimized");
        };
        assert_eq!(value, 0x1_0000_0000);
        assert_eq!(runtime.polls, 2);
        assert!(runtime.saw_root);

        registers.fill(VALUE_UNBOUND);
        registers[0] = 0x1_0000_0000;
        registers[1] = encode_i64(2500);
        runtime.fail = true;
        let Error::Runtime { pc, failure } = compiled
            .run_with_runtime(&mut registers, &mut runtime)
            .unwrap_err()
        else {
            panic!("poll error was not propagated");
        };
        assert!(matches!(
            Op::try_from(code.instructions[pc].opcode).unwrap(),
            Op::Jump | Op::JumpFalse | Op::JumpTrue
        ));
        assert_eq!(failure.kind, "KeyboardInterrupt");
    }

    #[test]
    fn loop_binary_type_miss_uses_runtime_without_deoptimizing() {
        #[derive(Default)]
        struct BinaryRuntime {
            calls: usize,
            saw_operands_rooted: bool,
        }
        impl Runtime for BinaryRuntime {
            fn binary(
                &mut self,
                op: RuntimeOp,
                left: u64,
                right: u64,
                registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                assert_eq!(op, RuntimeOp::InplaceAdd);
                self.calls += 1;
                self.saw_operands_rooted |= registers.contains(&left) && registers.contains(&right);
                Ok(left)
            }

            fn load_global(
                &mut self,
                _symbol: u32,
                _registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                unreachable!("binary loop test has no globals")
            }

            fn poll(&mut self, _registers: &[u64]) -> Result<u64, RuntimeFailure> {
                Ok(0)
            }
        }

        let program = function(
            "def accumulate(value,step,n):\n    i=0\n    while i<n:\n        value+=step\n        i+=1\n    return value",
        );
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = 0x1_0000_0000;
        registers[1] = 0x2_0000_0000;
        registers[2] = encode_i64(10);
        let mut runtime = BinaryRuntime::default();
        let Outcome::Returned { value, .. } = compiled
            .run_with_runtime(&mut registers, &mut runtime)
            .unwrap()
        else {
            panic!("generic binary loop unexpectedly deoptimized");
        };
        assert_eq!(value, 0x1_0000_0000);
        assert_eq!(runtime.calls, 10);
        assert!(runtime.saw_operands_rooted);
        assert!(compiled.metadata().safepoints > 0);
    }

    #[test]
    fn runtime_helper_returns_value_and_exact_error_pc() {
        #[derive(Default)]
        struct TestRuntime {
            calls: usize,
            roots: usize,
            fail: bool,
        }
        impl Runtime for TestRuntime {
            fn binary(
                &mut self,
                op: RuntimeOp,
                _left: u64,
                _right: u64,
                registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                assert_eq!(op, RuntimeOp::Div);
                self.calls += 1;
                self.roots = registers.len();
                if self.fail {
                    Err(RuntimeFailure::new("ZeroDivisionError", "division by zero"))
                } else {
                    Ok(VALUE_TRUE as u64)
                }
            }

            fn load_global(
                &mut self,
                _symbol: u32,
                _registers: &[u64],
            ) -> Result<u64, RuntimeFailure> {
                unreachable!("division test does not load globals")
            }

            fn poll(&mut self, _registers: &[u64]) -> Result<u64, RuntimeFailure> {
                unreachable!("division test has no backedge")
            }
        }

        let program = function("def divide(a,b):\n    return a/b");
        let code = &program.program().code[1];
        let compiled = compile(code).unwrap();
        let mut registers = vec![VALUE_UNBOUND; code.registers as usize];
        registers[0] = encode_i64(6);
        registers[1] = encode_i64(2);
        let mut runtime = TestRuntime::default();
        let Outcome::Returned { value, .. } = compiled
            .run_with_runtime(&mut registers, &mut runtime)
            .unwrap()
        else {
            panic!("runtime helper unexpectedly deoptimized");
        };
        assert_eq!(value, VALUE_TRUE as u64);
        assert_eq!(runtime.calls, 1);
        assert_eq!(runtime.roots, code.registers as usize);

        runtime.fail = true;
        let Error::Runtime { pc, failure } = compiled
            .run_with_runtime(&mut registers, &mut runtime)
            .unwrap_err()
        else {
            panic!("runtime error was not propagated");
        };
        assert_eq!(Op::try_from(code.instructions[pc].opcode).unwrap(), Op::Div);
        assert_eq!(failure.kind, "ZeroDivisionError");
    }
}
