# AGENTS.md — Tonic Language Runtime

## 0. Mission

You are working on **Tonic**, a new programming language and runtime written in Rust.

Tonic has:

- **Python-compatible syntax** as a hard product requirement.
- Its **own runtime, object model, bytecode, VM, memory manager, and JIT**.
- A high-performance interpreter for cold/warm code.
- A tiered **Cranelift JIT** for hot code.
- A compact handle/value ABI inspired by the architectural goals of HPy, but Tonic is not an HPy wrapper and must not inherit CPython's object layout.
- A design goal of making dynamic Python-style code substantially cheaper to execute than a naïve `PyObject*`/reference-counted implementation.
- Rust as the implementation language for the compiler, VM, runtime, GC, JIT integration, tooling, and standard library substrate.

The primary goal is:

> Preserve Python syntax and familiar dynamic-language behavior while designing every internal representation around execution speed, memory locality, specialization, and JIT-friendliness.

Do not accidentally turn Tonic into a CPython reimplementation.

---

# 1. Product definition

## 1.1 Syntax

Tonic source code should parse like Python.

The long-term goal is to accept all mainstream Python syntax, including:

- indentation-sensitive blocks
- functions
- async functions
- classes
- decorators
- comprehensions
- generators
- `yield` / `yield from`
- `async` / `await`
- structural pattern matching
- annotations
- lambdas
- exception handling
- context managers
- positional-only arguments
- keyword-only arguments
- unpacking
- f-strings
- walrus operator
- slices
- imports
- assignment expressions
- chained comparisons
- descriptors and class syntax
- metaclass syntax
- `match` / `case`

Syntax compatibility and runtime compatibility are separate concerns.

Do not couple parsing decisions to CPython object internals.

## 1.2 Semantics

Tonic should feel unsurprising to a Python programmer, but exact CPython implementation behavior is not automatically a requirement.

Unless a feature is explicitly marked CPython-compatible:

- match Python's language-level observable semantics when practical;
- do not preserve CPython implementation accidents;
- do not expose object addresses as stable identities;
- do not make CPython reference counting the foundation of the runtime;
- do not guarantee immediate physical reclamation when a reference disappears;
- do not assume CPython C-extension ABI compatibility.

Where compatibility conflicts with the performance architecture, isolate compatibility behind a bridge rather than contaminating the fast path.

## 1.3 Non-goals for the core runtime

The core runtime must not depend on:

- `PyObject`
- `PyTypeObject`
- `Py_INCREF`
- `Py_DECREF`
- CPython's allocator
- CPython's garbage collector
- the CPython C ABI
- the CPython stable ABI

A future compatibility layer may interact with CPython, but it must live outside the fast native object model.

---

# 2. Core architectural principles

Follow these principles unless a benchmark proves a different design is better.

## 2.1 Optimize the common path, not the universal path

Every dynamic operation should have:

1. a very cheap specialized fast path;
2. a correct generic fallback;
3. profiling information that can promote repeated behavior into a specialization.

Example:

```text
ADD_GENERIC
    |
    +-- int + int observed repeatedly
    |
    v
ADD_I64
```

Avoid designs where every operation permanently pays for the most dynamic possible case.

## 2.2 Representation independence

Guest-language values must not expose native heap addresses.

Use opaque values and handles so that:

- the GC may move objects;
- storage strategies may change;
- objects may be compacted;
- JIT code does not depend on Rust object addresses;
- runtime internals can evolve without breaking the language ABI.

## 2.3 No allocation unless semantics require it

Do not heap-allocate:

- small integers when they fit an immediate representation;
- booleans;
- `None`;
- common singleton values;
- temporary argument tuples solely for function calls;
- temporary keyword dictionaries solely for calls;
- iterator wrappers when a specialized loop can avoid them;
- boxed numeric temporaries inside hot specialized code.

## 2.4 JIT-friendly first

Every interpreter data structure must be designed with later machine-code lowering in mind.

If an interpreter opcode cannot be translated cleanly into:

- guards,
- typed operations,
- branches,
- loads/stores,
- calls into runtime helpers,

reconsider the opcode design.

## 2.5 Measure before adding complexity

For every major optimization:

- add a benchmark first;
- record baseline throughput;
- implement the optimization;
- verify correctness;
- record the new result;
- keep the optimization only if it has a meaningful benefit or enables an important future tier.

Do not preserve speculative complexity without evidence.

---

# 3. Recommended execution architecture

Tonic should use a **tiered execution engine**.

```text
Source
  |
  v
Parser
  |
  v
AST
  |
  v
HIR
  |
  v
Lowering / Scope Resolution
  |
  v
Bytecode / VM IR
  |
  +------------------------+
  |                        |
  v                        v
Adaptive Interpreter     Profiling
  |                        |
  +-----------+------------+
              |
              v
        Cranelift JIT
              |
              v
       Specialized Native Code
```

The initial execution tier is the interpreter.

Hot functions/loops are promoted to Cranelift-generated native code.

---

# 4. VM design

## 4.1 Default VM choice

Prefer a **register-based adaptive bytecode VM** over a traditional stack VM.

Rationale:

- fewer dispatches for expression-heavy dynamic code;
- easier mapping to SSA;
- easier Cranelift lowering;
- explicit operand locations;
- fewer push/pop operations;
- easier liveness analysis;
- better support for specialization and superinstructions.

Conceptual bytecode:

```text
LOAD_CONST   r0, const[0]
LOAD_CONST   r1, const[1]
ADD          r2, r0, r1
RETURN       r2
```

rather than:

```text
LOAD_CONST
LOAD_CONST
ADD
RETURN
```

Do not treat this as dogma. Benchmark a stack VM if a concrete design appears competitive, but keep the register VM as the architectural default.

## 4.2 Bytecode properties

Bytecode should be:

- compact;
- cache-friendly;
- fixed-width where that improves decoding;
- quick to decode;
- easy to patch in place for specialization;
- independent from AST node layout;
- independent from Rust enum layout;
- versioned.

Candidate instruction representation:

```rust
#[repr(C)]
struct Instr {
    opcode: u16,
    a: u16,
    b: u16,
    c: u16,
}
```

or another packed format supported by benchmarks.

Do not serialize Rust enum discriminants directly as the bytecode format.

## 4.3 Adaptive specialization / quickening

Generic instructions should be replaceable with specialized variants.

Examples:

```text
LOAD_ATTR
    -> LOAD_ATTR_SHAPE_SLOT
    -> LOAD_ATTR_CLASS_SLOT
    -> LOAD_ATTR_MODULE_SLOT

ADD
    -> ADD_I64
    -> ADD_F64
    -> ADD_STR

CALL
    -> CALL_TONIC_FUNCTION
    -> CALL_BUILTIN_FAST
    -> CALL_BOUND_METHOD_FAST
```

Specialized instructions must include guards or cache metadata.

On guard failure:

1. fall back safely;
2. update profiling;
3. de-specialize if the site becomes unstable.

Never let specialization change language semantics.

## 4.4 Inline caches

Use monomorphic caches first.

Promote to small polymorphic inline caches where measurements justify it.

Avoid unbounded polymorphic cache structures at each instruction.

A cache entry may contain:

```rust
struct AttrCache {
    shape_id: ShapeId,
    slot: u32,
    version: u32,
}
```

Do not store raw object addresses in persistent caches.

## 4.5 Dispatch

Keep the interpreter dispatch loop minimal.

Benchmark at least:

- Rust `match` dispatch;
- compact decoded-op dispatch;
- unsafe threaded/function-pointer variants if they can be implemented soundly enough;
- superinstructions.

Do not introduce unsafe dispatch tricks merely because they are theoretically faster. Keep them only if Tonic benchmarks prove a material gain.

The VM hot loop should avoid:

- heap allocation;
- locking;
- hash-map lookups;
- string comparisons;
- unnecessary bounds checks;
- repeated type decoding.

## 4.6 Superinstructions

Allow common opcode sequences to fuse, for example:

```text
LOAD_LOCAL + LOAD_LOCAL + ADD_I64
```

into a specialized superinstruction if benchmarks show dispatch reduction matters.

Do not explode the opcode set uncontrollably.

---

# 5. Tonic value representation

## 5.1 `Value`

The central guest-language value should be a compact machine-word representation.

Preferred target on 64-bit platforms:

```rust
#[repr(transparent)]
pub struct Value(u64);
```

Investigate and benchmark:

- NaN-boxing;
- low-bit pointer/handle tagging;
- tagged 64-bit handles;
- immediate small integers;
- immediate booleans;
- immediate `None`.

The representation must not assume that arbitrary native pointers remain permanently stable.

## 5.2 Immediate values

Strong candidates for immediate representation:

- `None`
- `True`
- `False`
- small signed integers
- possibly selected sentinels
- possibly tiny interned symbols

Do not prematurely encode short strings inline unless benchmarks justify the added complexity.

## 5.3 Heap handles

Heap values should normally refer to objects through stable logical handles.

Concept:

```rust
#[derive(Clone, Copy)]
pub struct Handle {
    index: u32,
    generation: u32,
}
```

A handle table can map a logical handle to a movable heap location.

Generation/version bits should detect stale handles in debug builds and, where cheap enough, release builds.

Do not expose handle-table implementation details in public APIs.

---

# 6. Object model

## 6.1 Object header

Keep object headers small.

Possible logical data:

```rust
struct ObjectHeader {
    type_id: TypeId,
    gc_bits: GcBits,
    flags: ObjectFlags,
}
```

Only add fields proven necessary.

Avoid a per-object native mutex.

Avoid a per-object hash table for attributes.

Avoid storing a full type pointer if a compact `TypeId` is sufficient.

## 6.2 Shapes / hidden classes

Instances should use a shape-based property layout.

Example:

```text
Shape 42
x -> slot 0
y -> slot 1
name -> slot 2
```

Instance:

```text
header
shape_id = 42
slots = [value_x, value_y, value_name]
```

Attribute lookup fast path:

```text
guard instance.shape_id == cached_shape
load instance.slots[cached_slot]
```

Support transitions:

```text
Shape 42 + "z" -> Shape 57
```

Intern shape transitions where useful.

## 6.3 Dictionary fallback

Objects that become highly dynamic may transition to dictionary mode.

Do not force every instance to pay for dictionary storage.

## 6.4 Type versioning

Types/classes need version counters or equivalent invalidation tokens.

Changing:

- class attributes;
- descriptors;
- method definitions;
- MRO-relevant state;

must invalidate dependent inline caches and JIT assumptions.

Prefer cheap coarse invalidation before designing highly granular dependency graphs.

---

# 7. Core containers

## 7.1 List

Initial general representation:

```text
List<Value>
```

Potential specialized storage:

- `List<i64>`
- `List<f64>`
- byte-oriented storage
- homogeneous object-handle storage

Only specialize storage if transitions and deoptimization remain correct.

A specialized list receiving an incompatible value may:

1. widen to a more general representation;
2. deopt to `List<Value>`.

Never silently coerce values to preserve specialization.

## 7.2 Tuple

Tuples should be compact and immutable.

Small tuples may have specialized allocation paths.

The JIT should be able to scalar-replace short-lived tuples where escape analysis proves it safe.

## 7.3 Dict

Dictionary performance is critical.

Design for:

- string/symbol-key fast paths;
- compact tables;
- split/shared-key layouts for instances where appropriate;
- version tags;
- predictable probing;
- low allocation count.

Benchmark candidate hashing algorithms.

Do not expose a hash algorithm as a language guarantee.

## 7.4 Strings

Strings should be immutable.

Consider:

- UTF-8 internal storage;
- cached hash;
- interned identifiers/symbols;
- separate byte-string type;
- slicing strategy carefully.

Do not make substring views keep giant backing strings alive without evidence that this tradeoff is beneficial.

---

# 8. Memory management

## 8.1 Preferred direction

Tonic should not use CPython-style reference counting as the fundamental memory manager.

Preferred architecture:

- nursery / young generation;
- generational tracing GC;
- compact/moving collection where the handle abstraction permits it;
- separate large-object allocation path;
- arenas/slabs for small fixed-size metadata where useful.

## 8.2 GC design goals

Optimize for:

- cheap allocation;
- good locality;
- low young-generation collection cost;
- bounded pause behavior where practical;
- JIT cooperation;
- precise root tracking;
- object movement without guest-visible pointer invalidation.

## 8.3 Roots

Root sources include:

- VM register frames;
- globals/modules;
- handle scopes;
- native runtime temporary handles;
- JIT stack maps;
- suspended generators/coroutines;
- exception state;
- finalization queues.

Roots must be explicit.

Avoid conservative scanning unless used only as an early bootstrap implementation.

## 8.4 Write barriers

If a generational GC is used, old-to-young references need a write barrier.

Design the object mutation API so barriers cannot easily be forgotten.

Prefer:

```rust
heap.store_field(owner, slot, value)
```

over arbitrary field mutation that bypasses GC metadata.

## 8.5 Finalization

Separate:

- logical finalization;
- physical memory reclamation.

Objects with user-visible finalizers require carefully specified behavior.

Do not depend on deterministic immediate physical destruction.

Document any difference from CPython when it becomes user-observable.

---

# 9. Function calls

Function calls are a first-order performance target.

## 9.1 Calling convention

Do not create argument tuples or keyword dictionaries unless the callee actually needs materialized objects.

Preferred internal shape:

```rust
fn call(
    vm: &mut Vm,
    callee: Value,
    positional: &[Value],
    keywords: &[KeywordArg],
) -> Result<Value, Exception>
```

At lower levels, use raw pointer/count or equivalent compact ABI if profiling proves slice construction significant.

## 9.2 Fast Tonic-to-Tonic calls

For known Tonic functions, specialized call sites should directly:

- validate expected argument layout;
- allocate/reuse a frame;
- populate registers;
- jump into interpreter or JIT entry.

## 9.3 Frames

Avoid heap allocating a frame for every call if possible.

Investigate:

- VM frame stack;
- segmented stack;
- reusable frame arenas;
- JIT native stack frames with GC stack maps.

Frames must support introspection only to the extent required by Tonic semantics.

Do not make all frames maximally introspectable if doing so prevents optimization.

---

# 10. Compiler pipeline

Use explicit stages.

Recommended structure:

```text
lexer/tokenizer
    |
parser
    |
AST
    |
name/scope analysis
    |
HIR
    |
bytecode IR
    |
bytecode
```

The JIT consumes bytecode plus runtime profiling or a JIT-oriented IR derived from it.

## 10.1 Parser

Python syntax compatibility is a hard requirement.

Do not hand-wave grammar edge cases.

Maintain parser conformance tests for:

- valid Python syntax samples;
- invalid syntax samples;
- indentation;
- f-strings;
- pattern matching;
- function parameter grammar;
- comprehensions;
- precedence;
- chained comparisons.

The parser architecture may use PEG or another suitable strategy.

If an external Rust parser crate is used during bootstrap, hide it behind Tonic-owned AST/lowering interfaces so it can be replaced later.

## 10.2 AST

Do not let downstream runtime code depend directly on parser crate AST types.

Convert to Tonic-owned AST nodes.

## 10.3 HIR

HIR should resolve:

- local variables;
- cells/free variables;
- globals;
- nonlocals;
- closures;
- control-flow structure;
- syntactic sugar;
- comprehension scopes.

## 10.4 Bytecode lowering

Bytecode registers are virtual VM registers, not physical CPU registers.

Perform enough liveness/register reuse to keep frame size reasonable.

---

# 11. Cranelift JIT

Cranelift is the intended JIT backend.

## 11.1 Tiering

Do not JIT everything immediately.

Suggested tiers:

```text
Tier 0: adaptive register interpreter
Tier 1: quickened/specialized interpreter
Tier 2: baseline Cranelift JIT
Tier 3: more aggressive specialized JIT, if later justified
```

Initially, Tier 1 and Tier 2 are sufficient.

## 11.2 Hotness

Track hotness per:

- function;
- loop/backedge;
- possibly call site.

Use simple counters first.

Do not build a complex profiler before benchmarks justify it.

## 11.3 JIT assumptions

JIT code may specialize on facts such as:

- argument value tags;
- exact type IDs;
- shapes;
- class version;
- global/module version;
- container storage kind.

Every assumption must have:

- a guard;
- an invalidation/deoptimization strategy;
- or a guaranteed immutable invariant.

## 11.4 Deoptimization

Design deoptimization from the beginning.

A guard failure must be able to reconstruct interpreter-visible state.

JIT metadata should map machine state back to:

- bytecode instruction index;
- VM registers;
- materialized object state if scalar replacement is later added.

Do not add an optimization that cannot safely return to generic execution.

## 11.5 Runtime helpers

Keep runtime helper ABI small and explicit.

Examples:

```text
tonic_add_generic
tonic_load_attr_generic
tonic_call_generic
tonic_raise
tonic_alloc
tonic_gc_safepoint
```

JIT fast paths should inline cheap checks and call helpers for complex cases.

## 11.6 Safepoints

JIT code must expose GC safepoints.

Safepoints commonly occur at:

- allocation;
- calls;
- loop backedges;
- explicit polls if necessary.

Provide precise stack maps for live managed references.

---

# 12. Exceptions

Do not model normal success paths around expensive Rust error allocation.

Tonic exceptions need:

- exception type;
- value/message;
- traceback information;
- propagation state.

Investigate a low-cost internal propagation strategy compatible with both interpreter and JIT.

Do not use Rust panics for ordinary guest-language exceptions.

Rust panic means an internal bug or unrecoverable host-side condition, not `raise ValueError`.

---

# 13. Iteration and loops

Generic iteration semantics must exist, but hot built-in loops should specialize.

Example:

```python
for x in some_list:
    ...
```

may become a fast indexed loop when:

- object type is exact Tonic list;
- layout/version assumptions hold.

Avoid creating an iterator object in optimized paths where observable semantics do not require it.

Keep a generic iterator fallback.

---

# 14. Builtins

Core builtins should be runtime intrinsics where performance matters.

Candidates:

- `len`
- `range`
- `isinstance`
- `type`
- numeric constructors
- selected container operations
- iteration primitives

Do not special-case arbitrary user functions by name.

Intrinsics are tied to known builtin identities/versioned bindings.

If a builtin is rebound, generic semantics must still work.

---

# 15. Modules and globals

Module/global lookup should use versioned storage.

Possible model:

```text
Module {
    slots / dict
    version
}
```

JIT may cache a global if guarded by module/version state.

Rebinding a global must invalidate or fail the guard.

---

# 16. Symbols and identifiers

Intern source identifiers into a symbol table.

Prefer compact `SymbolId` values over repeated string hashing for:

- local names;
- attribute names;
- type names;
- opcode metadata;
- compiler symbol resolution.

The symbol table must not become an uncontrollably growing leak in long-lived processes.

---

# 17. Async and generators

Generators/coroutines should compile to resumable state machines.

Suspended state must include:

- resume instruction;
- live VM registers;
- exception state where needed;
- closure/cell state.

Do not emulate suspension using native Rust stacks.

JIT support may initially fall back to interpreted resumable frames.

Correctness first; specialize later.

---

# 18. FFI and native extensions

Tonic should eventually provide a stable native extension interface, but not expose internal object layouts.

Design toward an opaque handle API.

Concept:

```rust
pub struct TonicContext { /* opaque */ }
pub struct TonicHandle(u64);
```

Native extensions should request operations from the runtime rather than dereference object internals.

Desired properties:

- implementation-independent;
- movable-GC-compatible;
- debug handle validation;
- explicit lifetime scopes;
- zero-copy buffer access where safe;
- stable ABI potential.

A future CPython compatibility layer should be a separate adapter.

---

# 19. Buffer protocol

Numeric/data workloads require a first-class buffer abstraction.

Represent:

- data pointer or external memory handle;
- byte length;
- element type;
- shape;
- strides;
- mutability;
- owner/lifetime token.

The design should support zero-copy interoperability with native Rust code and future Arrow/NumPy-like ecosystems.

Do not copy buffers merely to cross an internal API boundary.

---

# 20. Concurrency

Do not introduce a global interpreter lock as an unquestioned default.

However, do not compromise memory safety to advertise parallelism early.

Recommended progression:

1. single-threaded VM correctness;
2. thread-safe runtime metadata boundaries;
3. isolated runtimes/interpreters;
4. shared immutable data where useful;
5. parallel execution design backed by benchmarks and a clear GC model.

If a GIL-like lock is temporarily used during bootstrap, keep it behind an abstraction and document it as temporary.

---

# 21. Unsafe Rust policy

Performance-sensitive runtime code may require `unsafe`.

Rules:

- keep unsafe blocks small;
- state the safety invariant directly above the unsafe block;
- prefer safe APIs at subsystem boundaries;
- fuzz unsafe parsers/decoders;
- use Miri/sanitizers where applicable;
- never use unsafe merely to silence borrow-checker design problems.

Example:

```rust
// SAFETY:
// - `index < frame.register_count` is established by verified bytecode.
// - the register array is initialized for this instruction's live operands.
unsafe {
    frame.get_unchecked(index)
}
```

Bytecode must be verified before execution if the VM relies on unchecked accesses.

---

# 22. Bytecode verification

Before executing a code object, verify:

- opcode validity;
- register indices;
- constant indices;
- jump destinations;
- exception table ranges;
- operand shape;
- stack/register metadata;
- cache metadata bounds.

After verification, the interpreter may use unchecked accesses in carefully audited hot paths.

Malformed bytecode must never create memory unsafety.

---

# 23. Repository architecture

Prefer clear subsystem boundaries.

Suggested workspace:

```text
tonic/
├── Cargo.toml
├── AGENTS.md
├── crates/
│   ├── tonic-cli/
│   ├── tonic-parser/
│   ├── tonic-ast/
│   ├── tonic-hir/
│   ├── tonic-bytecode/
│   ├── tonic-compiler/
│   ├── tonic-vm/
│   ├── tonic-runtime/
│   ├── tonic-gc/
│   ├── tonic-jit/
│   ├── tonic-stdlib/
│   └── tonic-bench/
├── tests/
│   ├── syntax/
│   ├── semantics/
│   ├── jit/
│   └── differential/
└── benches/
```

Do not create this many crates prematurely if it slows iteration.

Early implementation may merge related crates, but preserve conceptual module boundaries.

---

# 24. Rust style

Target stable Rust unless a specific nightly feature has a benchmark-backed justification.

Use:

- `rustfmt`;
- `clippy`;
- explicit small types for IDs;
- newtypes instead of raw `u32` when IDs from different namespaces could be mixed;
- `Result` for recoverable host errors;
- guest exception types for Tonic exceptions.

Avoid:

- giant modules;
- pervasive `Arc<Mutex<_>>`;
- allocating boxed trait objects in hot loops;
- dynamic dispatch where a compact enum or function pointer is measurably cheaper;
- cloning strings to satisfy ownership issues;
- hash maps in the VM inner loop unless unavoidable.

---

# 25. Dependency policy

Dependencies are allowed when they accelerate development without owning Tonic's architecture.

For every substantial dependency ask:

1. Is it on a hot path?
2. Does it constrain object representation?
3. Does it constrain bytecode format?
4. Can it be replaced later?
5. Does it allocate unexpectedly?
6. Does it use global state?
7. Is its license suitable?
8. Is the version pinned appropriately?

Cranelift APIs evolve. Keep Cranelift integration isolated inside `tonic-jit` and avoid leaking Cranelift types throughout the runtime.

---

# 26. Correctness testing

Every feature needs tests at the lowest relevant layer.

## Parser tests

Test source -> AST behavior.

## Compiler tests

Test AST/HIR -> bytecode.

Provide bytecode snapshots where useful.

## VM tests

Construct bytecode directly for execution edge cases.

## Language tests

Execute `.tonic` / `.py`-syntax files through the public CLI/runtime.

## JIT differential tests

For the same program/input:

```text
interpreter result == JIT result
interpreter exception == JIT exception
observable side effects match
```

## Differential Python tests

Where Tonic intentionally follows Python semantics, compare selected programs against a reference Python implementation.

Do not blindly assert identical representation strings, traceback formatting, GC timing, or implementation-specific behavior unless compatibility explicitly requires it.

---

# 27. Fuzzing

High-value fuzz targets:

- tokenizer/parser;
- indentation;
- f-strings;
- bytecode verifier;
- bytecode decoder;
- serializer/deserializer;
- dict operations;
- shape transitions;
- GC mutation sequences;
- interpreter-vs-JIT equivalence;
- exception/control-flow combinations.

A crash in guest input must not become host memory corruption.

---

# 28. Benchmarking

Performance work is not complete without benchmarks.

Maintain microbenchmarks for:

- integer arithmetic;
- float arithmetic;
- local variable access;
- function calls;
- method calls;
- attribute loads/stores;
- loops;
- list iteration;
- dict lookup;
- class instance creation;
- closures;
- exceptions;
- string operations;
- allocations;
- GC;
- JIT compilation overhead;
- warm JIT throughput.

Maintain macrobenchmarks for representative programs.

Measure at least:

- operations/sec;
- wall time;
- allocations;
- bytes allocated;
- peak memory;
- interpreter dispatch count where useful;
- JIT compile time;
- code size.

Never optimize based only on intuition.

---

# 29. Profiling

Use real profiling tools before rewriting hot code.

When investigating performance:

1. reproduce with a stable benchmark;
2. profile;
3. identify the dominant cost;
4. change one major variable;
5. benchmark again.

Common expected hotspots:

- dispatch;
- generic attribute lookup;
- calls;
- allocation;
- hash lookup;
- GC barriers;
- boxing/unboxing;
- JIT/runtime boundary calls.

---

# 30. Performance budgets

Treat these as architectural goals, not guaranteed initial numbers.

The interpreter should aim to make:

- local register access nearly trivial;
- specialized integer arithmetic allocation-free;
- specialized attribute access a shape guard plus slot load;
- known Tonic function calls avoid tuple/dict construction;
- list iteration avoid generic iterator overhead when specialized;
- JIT numeric loops operate predominantly on unboxed machine values.

A regression that adds allocation to one of these paths needs explicit justification.

---

# 31. Debug vs release instrumentation

Debug builds should strongly validate invariants.

Useful debug checks:

- stale handle detection;
- shape consistency;
- GC ownership;
- root registration;
- bytecode validity;
- JIT deopt metadata;
- type/version assumptions.

Release builds may remove expensive checks, but never checks required for memory safety unless verification proves the invariant.

---

# 32. Diagnostics

Compiler/runtime diagnostics should be excellent.

Preserve source spans through:

```text
tokens -> AST -> HIR -> bytecode
```

Runtime errors should be able to map instruction locations back to source.

Do not force JIT code to lose source traceback information.

---

# 33. REPL

The REPL should use the same compiler/runtime pipeline as files.

Avoid a separate interpreter implementation for the REPL.

Incremental module state may be specialized later.

---

# 34. Serialization

If bytecode caching is added:

- use an explicit format version;
- include architecture/runtime compatibility metadata;
- validate before loading;
- never deserialize raw Rust structs by layout;
- never trust cached bytecode.

JIT machine code caching is a separate future feature.

---

# 35. Standard library strategy

Do not attempt to implement the full Python standard library before the runtime is stable.

Early standard library priorities:

- primitives;
- collections;
- math;
- iteration;
- basic I/O;
- strings;
- filesystem basics;
- time;
- modules/imports.

Performance-sensitive operations should be native Rust implementations exposed through Tonic's opaque runtime API.

---

# 36. Import system

Keep import semantics separate from filesystem mechanics.

Conceptual layers:

```text
import statement
    |
module resolver
    |
loader
    |
compiler/cache
    |
module object
```

Version module globals for JIT guards.

Circular imports must fail or behave according to documented Tonic semantics, not accidentally because of implementation order.

---

# 37. Compatibility modes

If CPython interoperability is implemented later, preserve a strict boundary:

```text
Tonic fast world
    |
bridge
    |
CPython compatibility world
```

Crossing the bridge may require boxing/materialization.

Do not convert the entire runtime to CPython-compatible object layouts.

Compatibility costs should be paid only at compatibility boundaries.

---

# 38. Optimization order

Prefer optimizations in roughly this order:

1. correct register VM;
2. compact `Value`;
3. allocation-free immediates;
4. efficient frames/calls;
5. shapes + attribute inline caches;
6. adaptive opcode specialization;
7. efficient dict/list;
8. generational allocator/GC improvements;
9. Cranelift baseline JIT;
10. guarded unboxed JIT paths;
11. deoptimization;
12. advanced escape/scalar replacement;
13. deeper container specialization.

Do not begin with exotic JIT optimizations before interpreter semantics and object invariants are stable.

---

# 39. Initial implementation milestones

## Milestone 0 — Skeleton

Deliver:

- Cargo workspace;
- CLI;
- source file loading;
- basic error infrastructure;
- benchmark harness.

## Milestone 1 — Minimal parser/compiler/VM

Support:

```python
x = 1
y = 2
print(x + y)
```

Implement:

- constants;
- names;
- integer arithmetic;
- assignment;
- expression statements;
- function calls to minimal builtins.

## Milestone 2 — Functions and control flow

Support:

- `def`;
- local variables;
- arguments;
- return;
- `if`;
- `while`;
- basic `for`;
- comparisons.

## Milestone 3 — Real object model

Implement:

- `Value`;
- heap;
- strings;
- lists;
- dicts;
- functions;
- classes;
- instances;
- shapes.

## Milestone 4 — Adaptive interpreter

Add:

- counters;
- quickening;
- arithmetic specialization;
- attribute inline caches;
- call specialization.

## Milestone 5 — GC

Move from bootstrap allocation strategy to a proper precise generational design.

## Milestone 6 — Cranelift

JIT simple functions:

- local numeric arithmetic;
- comparisons;
- branches;
- loops;
- calls via runtime helpers.

## Milestone 7 — Deoptimization

Allow guarded exact-int/float/shape specialization and safe fallback.

## Milestone 8 — Broader Python syntax

Increase parser/compiler conformance systematically.

---

# 40. Bootstrap rules

It is acceptable for the first working version to use temporary simpler systems, for example:

- non-moving GC before moving GC;
- generic `Value` containers before specialized containers;
- `match` dispatch before advanced dispatch;
- simple interpreter frames before frame reuse;
- all generic attribute operations before inline caches.

However, bootstrap code must be marked clearly when it violates the intended final architecture.

Use comments such as:

```rust
// BOOTSTRAP:
// This uses a non-moving allocation path.
// Replace after handle-root infrastructure is complete.
```

Do not let temporary choices silently become permanent architecture.

---

# 41. Decision records

For major irreversible decisions, add an ADR or design note.

Examples:

- `Value` encoding;
- GC strategy;
- bytecode instruction width;
- frame layout;
- shape representation;
- JIT deopt format;
- exception propagation ABI.

Each decision note should include:

- problem;
- alternatives;
- chosen design;
- benchmark data if relevant;
- consequences;
- migration risks.

---

# 42. Agent behavior when modifying Tonic

When asked to implement a feature:

1. inspect the relevant modules before editing;
2. identify the execution layer involved;
3. preserve subsystem boundaries;
4. implement the smallest correct vertical slice;
5. add tests;
6. run relevant tests;
7. run formatting/lints where practical;
8. benchmark if the change touches a hot path;
9. explain any new invariant in code comments/docs.

Do not perform large unrelated refactors during a feature task.

## 42.1 Before introducing a new abstraction

Ask internally:

- Is this on the hot path?
- Can this be a compact enum/newtype instead of a trait object?
- Will JIT code need to understand it?
- Does GC need to trace it?
- Does it contain managed references?
- Can it move?
- Can a cache retain it?
- What invalidates it?

## 42.2 Before storing a pointer

Ask:

- Is the pointee GC-managed?
- Can it move?
- What owns it?
- Can collection occur while this pointer is live?
- Should this be a handle or ID instead?

Raw pointers to managed objects require a documented invariant.

## 42.3 Before allocating

Ask:

- Can this be immediate?
- Can this live in the VM frame?
- Can storage be reused?
- Can JIT scalar replacement remove it?
- Is allocation observable?

---

# 43. Agent behavior for performance claims

Never write:

> "This is faster"

without either:

- a benchmark;
- a very local mechanically obvious reduction such as removing an allocation;
- or clearly labeling it as an expectation that still requires measurement.

Prefer:

> "This removes one temporary tuple allocation from the known-call fast path. Benchmark `call_known_3_args` before and after."

---

# 44. Agent behavior for Cranelift work

When changing Cranelift integration:

- check the exact Cranelift version in `Cargo.lock` / workspace dependencies;
- use APIs matching that pinned version;
- keep backend-specific code in the JIT crate/module;
- do not invent APIs from memory when the local crate source/docs are available;
- add interpreter-vs-JIT correctness tests;
- test guard failure paths;
- test deoptimization or fallback paths;
- verify GC roots at JIT safepoints.

Do not let generated code keep untracked managed references across possible GC points.

---

# 45. Agent behavior for parser work

For any grammar change:

- add at least one valid example;
- add at least one invalid/edge example;
- test precedence where relevant;
- test source spans;
- preserve Tonic-owned AST independence.

Python syntax compatibility is a product requirement, so "close enough" grammar is not sufficient.

---

# 46. Agent behavior for GC work

GC bugs are memory-safety bugs.

When touching GC:

- enumerate roots;
- enumerate write barriers;
- test cycles;
- test old-to-young references;
- test finalizers;
- test handles surviving movement;
- test allocation during collection if allowed;
- test JIT/interpreter roots;
- run stress collection with very small nursery sizes.

Prefer a correct slower collector over an unsound faster collector.

---

# 47. Agent behavior for object semantics

For any new object type define:

- type identity;
- equality;
- hashing;
- truthiness;
- repr/str;
- attribute behavior;
- GC tracing;
- mutability;
- iteration;
- callability if relevant;
- JIT specialization opportunities.

Do not add a managed object type without implementing its trace behavior.

---

# 48. Naming conventions

Use explicit runtime terminology.

Preferred:

- `Value`
- `Handle`
- `TypeId`
- `ShapeId`
- `SymbolId`
- `CodeId`
- `ModuleId`
- `GcRef` only if its movement/lifetime semantics are extremely clear
- `Frame`
- `CodeObject`
- `InlineCache`
- `JitCode`

Avoid borrowing CPython names such as `PyObject` unless code is specifically inside a CPython compatibility module.

---

# 49. Invariants to protect

These invariants are fundamental.

1. Guest code cannot forge arbitrary host pointers.
2. Malformed bytecode cannot create Rust memory unsafety.
3. GC movement cannot invalidate guest-visible values.
4. Caches cannot retain invalid raw movable-object pointers.
5. JIT code cannot hide managed roots from the GC.
6. Specialization cannot change semantics.
7. Guard failure always has a correct generic path.
8. CPython compatibility code cannot leak its layout assumptions into Tonic native objects.
9. Parser implementation details cannot leak into runtime representation.
10. Rust panic is not guest exception control flow.

---

# 50. Preferred mental model

Think of Tonic as:

> a Python-syntax dynamic language with a VM architecture closer to a modern JavaScript/Lua-family optimizing runtime than to CPython's traditional object-execution model.

The desired fast path is:

```text
Python-looking source
        |
        v
register bytecode
        |
        v
adaptive specialization
        |
        v
shape/type guards
        |
        v
unboxed operations
        |
        v
Cranelift machine code
```

not:

```text
source
  |
  v
heap-box every value
  |
PyObject-like pointer
  |
refcount every movement
  |
generic lookup for every operation
```

---

# 51. Concrete north-star examples

## Integer loop

Source:

```python
def sum_to(n):
    total = 0
    i = 0
    while i < n:
        total += i
        i += 1
    return total
```

Desired hot JIT behavior:

```text
guard n is small/exact integer
unbox n
total = i64 0
i = i64 0

loop:
    compare i, n
    add total, i
    add i, 1
    branch

box result only when required by caller boundary
```

No per-iteration allocation.

No per-iteration dynamic operator lookup.

## Attribute load

Source:

```python
return point.x
```

Desired specialized behavior:

```text
guard exact/compatible instance
guard shape == expected shape
load slot N
return
```

Generic descriptor/MRO behavior remains the fallback.

## Known function call

Source:

```python
z = add(x, y)
```

Desired specialized behavior:

```text
guard callee identity/version
populate callee registers directly
call JIT/interpreter entry
```

No argument tuple.

No keyword dict if not needed.

---

# 52. What not to optimize away

Tonic is still a dynamic language.

Do not make optimizations that incorrectly assume:

- classes never mutate;
- globals never change;
- builtins cannot be rebound;
- descriptors do not exist;
- `__getattribute__` cannot be customized;
- container element types never change;
- monkey patching never happens;
- functions cannot escape;
- closures are immutable;
- exceptions are rare enough to ignore correctness.

Use guards and invalidation.

---

# 53. Documentation expectations

When implementing a subsystem, document:

- ownership;
- object movement;
- invalidation;
- hot-path behavior;
- fallback path;
- GC interaction;
- JIT interaction;
- thread-safety assumptions.

Architecture documentation should explain why a representation exists, not merely restate its fields.

---

# 54. Definition of done

A runtime feature is complete only when:

- syntax/compiler path exists;
- interpreter behavior is correct;
- GC tracing is correct;
- error behavior is defined;
- tests exist;
- JIT either supports it or explicitly falls back;
- specialization has a generic fallback;
- benchmarks exist if it is performance-sensitive;
- docs/comments state important invariants.

---

# 55. Default priority order for coding agents

When requirements conflict, use this order:

1. memory safety;
2. language correctness;
3. architectural invariants;
4. debuggability;
5. performance;
6. implementation brevity.

Performance is central to Tonic, but incorrect or unsound shortcuts are unacceptable.

Once correctness is established, aggressively profile and optimize hot paths.

---

# 56. First recommended vertical slice

If the repository is empty, build this first:

```python
def fib(n):
    a = 0
    b = 1
    while n > 0:
        a, b = b, a + b
        n -= 1
    return a

print(fib(40))
```

The first implementation may not yet support full tuple semantics internally; the compiler may lower parallel assignment directly when semantics permit.

The slice should exercise:

- parser;
- function definition;
- locals;
- constants;
- arithmetic;
- compare;
- branches;
- loop;
- call;
- return;
- builtin print;
- VM frames.

Then add an interpreter benchmark.

After that, make this same function the first Cranelift JIT target.

---

# 57. Long-term target

The intended end state is:

- Python-compatible parser;
- compact register bytecode;
- adaptive quickening;
- tagged values;
- shape-based objects;
- efficient dynamic calls;
- generational/moving GC behind stable handles;
- precise GC roots;
- Cranelift tiered JIT;
- guarded unboxed machine operations;
- deoptimization;
- versioned globals/classes;
- opaque native extension ABI;
- zero-copy buffers;
- optional CPython interoperability at an explicit boundary.

All architectural work should move Tonic toward this model.
