# Tonic Interoperability Runtime Architecture

## 0. Purpose

This document defines the interoperability runtime architecture for **Tonic**.

Tonic is a Python-syntax language implemented in Rust with:

- a custom object model;
- a custom register-based VM;
- adaptive specialization;
- a generational/moving GC;
- a Cranelift JIT;
- a compact `Value` representation;
- an opaque native interoperability ABI.

The goal of the interoperability layer is:

> Allow Tonic values, native Rust code, native extensions, foreign runtimes, CPython-compatible components, and zero-copy data systems to interact without forcing Tonic's core runtime to adopt CPython's object layout or memory-management model.

The interoperability architecture must preserve Tonic's native fast path.

---

# 1. Fundamental rule

Tonic must never make this its universal object model:

```text
PyObject*
Py_INCREF
Py_DECREF
PyTypeObject
CPython allocator
```

Instead, the system is split into two conceptual worlds:

```text
┌───────────────────────────────────────┐
│          Tonic Native World           │
│                                       │
│ Value                                 │
│ Handle                                │
│ Shape-based objects                   │
│ Tagged immediates                     │
│ Moving / generational GC              │
│ Cranelift JIT                         │
│ Native containers                     │
│ Zero-copy buffers                     │
└──────────────────┬────────────────────┘
                   │
                   │ Interop Runtime
                   │
┌──────────────────▼────────────────────┐
│         Foreign Runtime World         │
│                                       │
│ Rust native code                      │
│ C ABI extensions                      │
│ CPython                               │
│ NumPy-like runtimes                   │
│ Arrow                                 │
│ external memory                       │
│ system libraries                      │
└───────────────────────────────────────┘
```

Interop costs must be paid at the boundary.

They must not contaminate ordinary Tonic execution.

---

# 2. Design goals

The interop runtime should provide:

- stable opaque handles;
- GC-safe object access;
- implementation-independent ABI;
- explicit ownership;
- explicit lifetime scopes;
- native function calls;
- zero-copy buffers;
- foreign-object wrapping;
- callback support;
- exception translation;
- thread/runtime state handling;
- CPython compatibility through a separate bridge;
- future Stable ABI potential.

The design should permit the Tonic runtime internals to change without breaking extension binaries.

---

# 3. Tonic `Value`

The internal VM/JIT representation should remain distinct from the public extension ABI.

Internal representation may be:

```rust
#[repr(transparent)]
pub struct Value(u64);
```

Possible encodings:

```text
xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx000
    heap handle

xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx001
    small integer

xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx010
    false

xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx011
    true

xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx100
    none
```

Exact tag bits are an implementation detail.

Native extensions must not depend on them.

This is critical.

The public ABI should expose an opaque handle rather than raw `Value`.

---

# 4. Public opaque handle

The native interoperability interface should expose something conceptually like:

```rust
#[repr(transparent)]
pub struct TonicHandle(u64);
```

C-compatible form:

```c
typedef uint64_t TonicHandle;
```

A `TonicHandle` is not:

- a native pointer;
- an object address;
- a Rust reference;
- a stable heap location.

It is a logical reference understood by the runtime.

Conceptually:

```text
TonicHandle
    │
    ▼
Handle Table
    │
    ▼
Managed Object
```

Example:

```text
Handle 0x0000012A
       │
       ▼
┌──────────────────────┐
│ Handle table         │
├──────────────────────┤
│ 0x0128 -> Object A   │
│ 0x0129 -> Object B   │
│ 0x012A -> Object C   │
│ 0x012B -> Object D   │
└──────────────────────┘
```

If the GC moves Object C:

```text
before:
0x012A -> 0x10002000

after:
0x012A -> 0x700A4000
```

the extension-visible handle remains unchanged.

---

# 5. Handle representation

A possible logical representation:

```rust
#[repr(C)]
pub struct HandleBits {
    index: u32,
    generation: u32,
}
```

or packed into `u64`.

The generation field helps detect stale handles.

Example:

```text
bits 0..31   = handle table index
bits 32..63  = generation
```

When an entry is reused:

```text
index = 42
generation = 8
```

becomes:

```text
index = 42
generation = 9
```

Old handles can then be rejected.

Exact bit allocation remains private runtime implementation.

---

# 6. Handle table

The handle table acts as an indirection layer.

Conceptually:

```rust
struct HandleEntry {
    location: HeapLocation,
    generation: u32,
    flags: HandleFlags,
}
```

Possible flags:

- strong;
- weak;
- pinned;
- foreign;
- finalizable;
- borrowed;
- persistent.

The handle table itself should be optimized heavily.

Hot access should approximately become:

```text
handle
  ↓
index extraction
  ↓
entry load
  ↓
object address
```

Avoid:

- hash map lookup;
- mutex acquisition;
- string lookup;
- dynamic allocation;

for ordinary handle resolution.

---

# 7. Handle scopes

Extensions should not manually perform reference counting for every temporary.

Instead, use handle scopes.

Concept:

```rust
let mut scope = ctx.scope();

let a = scope.from_i64(10)?;
let b = scope.from_i64(20)?;

let result = ctx.add(a, b)?;

return scope.escape(result);
```

At scope exit:

```text
all local handles
    ↓
released in bulk
```

C-style ABI:

```c
TonicScope scope;
tonic_scope_enter(ctx, &scope);

TonicHandle x = tonic_int_from_i64(ctx, 10);
TonicHandle y = tonic_int_from_i64(ctx, 20);

TonicHandle z = tonic_add(ctx, x, y);

tonic_scope_escape(ctx, &scope, z);
tonic_scope_leave(ctx, &scope);
```

This is conceptually similar to scoped roots.

It should be cheap enough to use per native call.

---

# 8. Local and persistent handles

Two important handle classes should exist.

## 8.1 Local handles

Lifetime:

```text
native call / handle scope
```

Cheap.

Automatically released.

Use for temporary values.

## 8.2 Persistent handles

Lifetime:

```text
explicitly managed by extension
```

Used when native code stores a Tonic object beyond the current call.

API concept:

```rust
let persistent = ctx.persist(local)?;
...
ctx.release_persistent(persistent);
```

Persistent handles are GC roots.

Do not make all handles persistent.

That would retain too much memory and hurt GC.

---

# 9. Borrowed native views

For performance, the runtime may expose temporary native views.

Example:

```rust
pub struct TonicStrView<'scope> {
    ptr: *const u8,
    len: usize,
    _scope: PhantomData<&'scope ()>,
}
```

Rules:

- valid only within a documented scope;
- invalid after GC movement unless object is pinned;
- must not be stored persistently;
- must not escape into another thread;
- runtime calls that may trigger GC may invalidate them.

This should be enforced through safe Rust APIs when possible.

C APIs need explicit documentation.

---

# 10. Pinning

Some foreign APIs require a stable address.

Tonic should support explicit pinning as an expensive operation.

Concept:

```rust
let pinned = ctx.pin(handle)?;
let ptr = pinned.data_ptr();
```

Pinning should:

- prevent object movement;
- increase GC complexity;
- be avoided on normal code paths;
- have clear lifetime;
- be explicitly released.

Possible implementation:

```text
movable object
    │
pin()
    ▼
pinned region / non-moving generation
```

or:

```text
object stays in place while pin count > 0
```

Do not make every object permanently pinned for FFI convenience.

---

# 11. Interop context

Every extension function should receive a runtime context.

Conceptual Rust API:

```rust
pub struct TonicContext {
    runtime: *mut Runtime,
    thread_state: *mut ThreadState,
    api: *const TonicApi,
}
```

Public ABI should keep this opaque.

C:

```c
typedef struct TonicContext TonicContext;
```

The context provides:

- handle operations;
- object creation;
- attribute operations;
- calls;
- conversions;
- exception state;
- buffer access;
- GC-safe temporary rooting;
- type inspection.

Extensions must not reach into VM internals directly.

---

# 12. Function-table ABI

For long-term binary compatibility, the public C ABI should be function-table based.

Concept:

```c
typedef struct TonicApi {
    uint32_t abi_version;

    TonicHandle (*int_from_i64)(
        TonicContext*,
        int64_t
    );

    int (*int_as_i64)(
        TonicContext*,
        TonicHandle,
        int64_t*
    );

    TonicHandle (*add)(
        TonicContext*,
        TonicHandle,
        TonicHandle
    );

    TonicHandle (*get_attr)(
        TonicContext*,
        TonicHandle,
        TonicHandle
    );

    TonicHandle (*call)(
        TonicContext*,
        TonicHandle,
        const TonicHandle*,
        size_t
    );

} TonicApi;
```

The extension receives:

```text
TonicContext*
      │
      ▼
TonicApi*
```

This permits runtime implementations to change behind the ABI.

Extensions must not link directly against internal Rust symbols.

---

# 13. Stable ABI strategy

A future Tonic Stable ABI should expose only:

- fixed-width integers;
- opaque pointers;
- opaque handles;
- explicit structs with ABI versioning;
- function pointers;
- size/version fields.

Avoid exposing:

- Rust enums;
- Rust `Vec`;
- Rust `String`;
- Rust trait objects;
- Rust references;
- compiler-specific layout;
- internal object headers.

Use:

```c
struct {
    uint32_t struct_size;
    uint32_t abi_version;
    ...
}
```

where future extension is expected.

---

# 14. Native function registration

A native extension should register callable functions.

Conceptual ABI:

```c
typedef TonicHandle (*TonicNativeFn)(
    TonicContext* ctx,
    const TonicHandle* args,
    size_t nargs,
    const TonicKwArg* kwargs,
    size_t nkw
);
```

No temporary argument tuple is required.

The Tonic VM may call this directly from a specialized `CALL_NATIVE` path.

---

# 15. Native calls from the VM

Fast path:

```text
CALL_NATIVE
    │
    ├─ guard callable type
    ├─ obtain function pointer
    ├─ establish handle scope
    ├─ root arguments
    ├─ invoke native function
    └─ process result/exception
```

Do not convert all arguments to boxed foreign objects unless requested.

Native Tonic extensions should operate on Tonic handles directly.

---

# 16. Rust-native API

Rust extensions can provide a more ergonomic API over the same ABI concepts.

Example:

```rust
fn native_add(
    ctx: &mut Context,
    args: &[Handle],
) -> TonicResult<Handle> {
    let a = ctx.to_i64(args[0])?;
    let b = ctx.to_i64(args[1])?;

    Ok(ctx.from_i64(a + b))
}
```

The safe wrapper should:

- validate handles;
- manage scopes;
- translate exceptions;
- guard against stale handles;
- hide raw API tables.

The lower-level C ABI remains the compatibility layer.

---

# 17. Foreign object wrapper

Sometimes a foreign runtime owns the object.

Tonic should represent this using a foreign object wrapper.

Concept:

```rust
struct ForeignObject {
    type_id: ForeignTypeId,
    payload: *mut c_void,
    vtable: *const ForeignVTable,
}
```

The wrapper is managed by the Tonic GC.

The foreign payload may have its own lifetime rules.

Foreign vtable might define:

```rust
struct ForeignVTable {
    drop_payload: unsafe extern "C" fn(*mut c_void),
    get_attr: ...,
    set_attr: ...,
    call: ...,
    repr: ...,
    trace_foreign_refs: ...,
}
```

Foreign objects should not automatically receive full Tonic optimization assumptions.

They use generic fallback behavior unless specialized adapters exist.

---

# 18. Ownership of foreign memory

Foreign buffers or resources need explicit ownership modes.

Useful categories:

```text
Borrowed
OwnedByTonic
OwnedByForeignRuntime
Shared
Pinned
```

For example:

```rust
enum BufferOwner {
    Tonic(Handle),
    Foreign(ForeignOwnerToken),
    Static,
}
```

Lifetime must be tied to an owner token.

Never store a raw external pointer without a lifetime/ownership strategy.

---

# 19. Zero-copy buffer interoperability

Tonic should have a first-class buffer protocol.

Conceptual descriptor:

```rust
#[repr(C)]
pub struct TonicBuffer {
    pub data: *mut u8,
    pub byte_len: usize,

    pub ndim: u32,
    pub shape: *const usize,
    pub strides: *const isize,

    pub item_size: usize,
    pub dtype: TonicDType,

    pub flags: u64,
    pub owner: TonicHandle,
}
```

Important fields:

- data location;
- total bytes;
- element type;
- shape;
- strides;
- mutability;
- contiguity;
- ownership token.

---

# 20. Buffer lifetime

The buffer owner's handle keeps backing memory alive.

Conceptually:

```text
Tonic array
    │
    ├── BufferView
    │      │
    │      └── owner handle
    │
    └── underlying allocation
```

The consumer must release the view.

If the allocation is movable, exporting a pointer must either:

- pin it;
- expose an external non-moving allocation;
- or use a buffer allocation that the GC never relocates.

Large numeric buffers should generally live outside the moving object heap.

---

# 21. Large data architecture

Prefer:

```text
Tonic Array Object
        │
        ├── metadata in GC heap
        │
        └── data buffer in non-moving buffer heap
```

This avoids pinning large data blocks.

The small managed object can move.

The data allocation remains stable.

Example:

```text
GC object:
{
    dtype
    shape
    strides
    buffer_handle
}

Buffer heap:
[ 1.0 ][ 2.0 ][ 3.0 ][ 4.0 ]
```

This is a strong foundation for Arrow/NumPy-like interoperability.

---

# 22. DType system

The core buffer ABI should support primitive data types.

Examples:

```text
i8
u8
i16
u16
i32
u32
i64
u64
f16
f32
f64
bool
complex64
complex128
```

Potential future:

```text
datetime
decimal
struct
categorical
```

Do not encode high-level language type IDs directly as buffer dtype IDs.

Buffer dtype is a data-layout concept.

---

# 23. Calls into foreign C libraries

Tonic's general FFI layer may eventually support signatures such as:

```python
lib.sqrt: (float) -> float
```

The compiler/runtime may lower primitive-only calls directly.

Example:

```text
Tonic float Value
     │
     ▼
unbox f64
     │
     ▼
native C ABI call
     │
     ▼
box/tag result
```

No object handle is needed for pure primitive calls.

This is distinct from the object-extension ABI.

---

# 24. Primitive FFI path

A high-performance primitive ABI should support:

- signed/unsigned integers;
- floating-point values;
- pointers;
- byte slices;
- typed buffers;
- structs with explicit layout where safe.

JIT code may emit direct foreign calls when:

- signature is known;
- calling convention is known;
- GC rules are respected;
- no managed pointer is hidden from the runtime.

---

# 25. GC safety across foreign calls

Before calling foreign code, Tonic must know which managed values remain live.

Interpreter:

```text
VM registers
    │
    └── already roots
```

JIT:

```text
machine registers / stack
    │
    ▼
stack map / safepoint metadata
```

If foreign code can call back into Tonic or allocate, the runtime must assume a GC may occur.

Therefore:

- raw movable-object addresses cannot remain untracked;
- live values need roots;
- exported raw views may require pinning;
- handle-based references remain valid.

---

# 26. No-GC sections

For certain highly optimized operations, Tonic may expose a limited no-GC scope.

Concept:

```rust
let nogc = ctx.enter_no_gc();

let view = nogc.borrow_str(handle)?;
native_process(view);

drop(nogc);
```

Constraints:

- no allocation;
- no callback into Tonic;
- no operation that can trigger GC;
- short duration only.

This allows temporary raw access without pinning.

No-GC scopes must never be silently entered by generic extension code.

---

# 27. Exception model

Interop functions need explicit failure semantics.

A simple ABI approach:

```text
return invalid/null handle
+
exception stored in context
```

Example:

```c
TonicHandle value = api->get_attr(ctx, obj, name);

if (tonic_handle_is_error(value)) {
    return value;
}
```

Alternatively:

```c
int tonic_get_attr(
    TonicContext*,
    TonicHandle object,
    TonicHandle name,
    TonicHandle* out
);
```

where status is separate.

Choose one model consistently.

---

# 28. Rust exception wrapper

Safe Rust-facing API:

```rust
pub type TonicResult<T> = Result<T, TonicException>;

pub fn get_attr(
    &mut self,
    object: Handle,
    name: Handle,
) -> TonicResult<Handle>;
```

Never use Rust panic for guest exceptions.

Panics crossing a C ABI boundary are forbidden.

Use `catch_unwind` only as a defensive boundary if appropriate, not as ordinary control flow.

---

# 29. Callback into Tonic

Foreign code may hold a persistent Tonic callable.

Example:

```text
Tonic function
    │
persistent handle
    │
foreign library
    │
later callback
    ▼
Tonic runtime
```

A callback adapter must:

1. attach to a valid runtime/thread state;
2. establish a handle scope;
3. convert arguments;
4. invoke the callable;
5. translate result/error;
6. release temporary roots.

Callback lifetimes must be explicit.

---

# 30. Thread state

Every Tonic call from foreign code needs valid runtime/thread state.

Concept:

```rust
struct ThreadState {
    runtime_id: RuntimeId,
    roots: RootStack,
    exception: ExceptionState,
    gc_state: ThreadGcState,
}
```

A thread not currently attached to the runtime may need:

```text
tonic_thread_attach(runtime)
tonic_thread_detach(runtime)
```

Do not assume foreign callbacks arrive on a Tonic-created thread.

---

# 31. Multiple runtimes

Tonic handles should belong to a specific runtime.

Never allow:

```text
handle from runtime A
        ↓
used directly in runtime B
```

Possible debug representation:

```text
runtime_id
generation
index
```

or validate through context ownership.

Cross-runtime transfer requires:

- serialization;
- immutable shared buffer;
- explicit transfer API;
- or foreign shared owner.

---

# 32. CPython interoperability

CPython support must be a distinct compatibility subsystem.

Architecture:

```text
Tonic Native World
        │
        ▼
Tonic CPython Bridge
        │
        ▼
CPython World
```

The bridge must prevent CPython's object layout from becoming Tonic's native layout.

---

# 33. Tonic object entering CPython

Suppose Tonic has:

```python
x = SomeTonicObject()
```

and a CPython extension needs it.

Do not reinterpret the Tonic object as `PyObject*`.

Instead create a proxy:

```text
Tonic Handle
      │
      ▼
┌─────────────────────────┐
│ TonicProxy PyObject     │
│                         │
│ tonic_runtime*          │
│ persistent_handle       │
└─────────────────────────┘
```

CPython sees an actual `PyObject`.

The proxy holds a persistent Tonic handle.

Operations forward back into Tonic.

---

# 34. Proxy example

Conceptual CPython-side layout:

```c
typedef struct {
    PyObject_HEAD

    TonicRuntime* runtime;
    TonicPersistentHandle handle;

} PyTonicProxy;
```

Possible operations:

```text
tp_getattro
    -> tonic_get_attr

tp_setattro
    -> tonic_set_attr

tp_call
    -> tonic_call

tp_repr
    -> tonic_repr
```

This bridge is inherently slower than native Tonic execution.

That is acceptable because it is a compatibility boundary.

---

# 35. CPython object entering Tonic

A CPython object used in Tonic becomes a foreign wrapper:

```text
PyObject*
   │
   ▼
ForeignPyObject
```

Concept:

```rust
struct ForeignPyObject {
    py_obj: *mut PyObject,
    owning_interpreter: PyInterpreterId,
}
```

The wrapper must maintain CPython ownership correctly.

This compatibility module may use:

```text
Py_INCREF
Py_DECREF
```

internally.

The rest of Tonic must not.

---

# 36. CPython GIL concerns

If embedding CPython, calls into CPython must obey the active CPython threading requirements.

Bridge operations may need:

```text
acquire CPython execution state
perform operation
release state
```

This cost must remain localized to CPython interaction.

Tonic itself should not inherit a global lock merely because the compatibility bridge requires one.

---

# 37. CPython extension compatibility levels

Different compatibility levels should be distinguished.

## Level 1 — Tonic-native extensions

Best performance.

Use Tonic ABI directly.

```text
TonicHandle
TonicContext
TonicBuffer
```

## Level 2 — CPython package through adapter

Package code operates on CPython objects.

Tonic creates proxies/wrappers.

Moderate/high boundary cost.

## Level 3 — full legacy C-extension compatibility

Potentially expensive.

May require embedded CPython.

Should be considered compatibility mode rather than normal execution.

---

# 38. Conversion strategy

When moving values between Tonic and CPython, use a hierarchy.

## Immediate conversion

For cheap immutable primitives:

```text
Tonic int
    ↕
PyLong

Tonic float
    ↕
PyFloat

Tonic str
    ↕
PyUnicode
```

May involve copying/boxing.

## Proxy conversion

For dynamic user objects:

```text
Tonic object
    ↕
proxy object
```

Avoid deep conversion.

## Buffer sharing

For arrays:

```text
Tonic buffer
     ↕
buffer protocol / zero-copy
```

Prefer shared memory.

---

# 39. Identity semantics across the bridge

Do not promise:

```text
id(tonic_object)
==
address(proxy)
```

Tonic identity is logical.

A bridge may cache proxy objects so repeated export of the same Tonic object produces the same live proxy where useful.

Concept:

```text
Tonic handle
    │
    └── weak proxy cache
```

This is a bridge-level identity optimization.

It is not the core object model.

---

# 40. Cycles across runtimes

Cross-runtime cycles are difficult.

Example:

```text
Tonic Object A
    │
    ▼
CPython Object B
    │
    ▼
Tonic Object A
```

Neither independent collector may see the whole cycle.

Possible strategies:

- weak bridge references;
- explicit proxy cycle protocol;
- periodic bridge cycle detection;
- ownership restrictions;
- requiring one side to be weak for specific wrappers.

This must be designed explicitly before claiming transparent cyclic interoperability.

Do not assume two independent GCs automatically collect cross-runtime cycles.

---

# 41. Finalization across runtimes

Finalizers must not rely on arbitrary runtime destruction order.

A foreign wrapper should:

1. mark itself logically dead;
2. release foreign resource at an allowed runtime point;
3. avoid calling into a shutting-down runtime;
4. tolerate interpreter teardown.

Bridge teardown requires a defined lifecycle.

---

# 42. Module interoperability

Possible future imports:

```python
import tonic_native_module
import cpython_package
```

The importer should know module kind:

```text
Tonic bytecode module
Tonic native module
CPython compatibility module
foreign shared library module
```

Do not make import syntax reveal the runtime implementation unless necessary.

The loader can choose a backend based on metadata.

---

# 43. Native extension manifest

A Tonic native extension should ship metadata such as:

```toml
[tonic-extension]
name = "fastmath"
abi = 1
architecture = "x86_64"
runtime = "tonic"
```

Potential features:

```toml
features = [
    "buffer-v1",
    "native-call-v1",
]
```

The exact packaging format can evolve.

ABI compatibility must be checked before loading machine code.

---

# 44. Extension entry point

A shared library may export a single stable symbol:

```c
const TonicExtension*
tonic_extension_init(
    const TonicApi* api,
    uint32_t abi_version
);
```

The extension returns descriptors:

```c
typedef struct {
    const char* module_name;

    const TonicFunctionDef* functions;
    size_t function_count;

    const TonicTypeDef* types;
    size_t type_count;

} TonicExtension;
```

This avoids symbol-per-function ABI coupling.

---

# 45. Native type registration

Extensions should be able to define native-backed types.

Concept:

```rust
struct NativeTypeDef {
    name: &'static str,
    size: usize,
    align: usize,

    trace: Option<TraceFn>,
    drop: Option<DropFn>,
    call: Option<CallFn>,
    get_attr: Option<GetAttrFn>,
}
```

But do not expose Rust layout directly in the stable C ABI.

The runtime should own the outer managed object.

Native payload may live:

- inline if immovable and layout-safe;
- in external allocation;
- behind a stable payload pointer.

---

# 46. GC tracing of extension types

A native extension type that stores Tonic handles must report them to the GC.

Preferred approach:

```text
managed references represented as handles
```

Persistent handles already behave as roots.

For inline managed references, provide a trace callback.

Example:

```c
void my_type_trace(
    TonicContext* ctx,
    void* payload,
    TonicTraceVisitor* visitor
);
```

Never let opaque native memory silently contain movable raw Tonic addresses.

---

# 47. Native payload destruction

Native resource destruction must use explicit finalization callbacks.

Example:

```rust
unsafe extern "C" fn destroy(payload: *mut c_void)
```

The runtime determines when the callback may safely execute.

Do not run arbitrary foreign destructors in the middle of GC bookkeeping unless the collector design explicitly supports it.

A finalization queue is safer.

---

# 48. File descriptors and OS handles

Foreign resources like:

- file descriptors;
- sockets;
- GPU handles;
- database handles;
- operating-system objects;

should be stored as native payloads with explicit cleanup.

They are not managed pointers.

Do not confuse resource lifetime with GC object movement.

---

# 49. JIT and interoperability

Cranelift-generated code should distinguish several call categories.

```text
CALL_TONIC_JIT
CALL_TONIC_INTERPRETED
CALL_TONIC_NATIVE
CALL_FOREIGN_PRIMITIVE
CALL_GENERIC
```

Each has different cost and safety rules.

---

# 50. Direct JIT-to-native calls

If a Tonic-native function has a stable internal ABI, JIT code may call it directly.

For example:

```text
known builtin `len(list)`
```

may compile to:

```text
guard exact list type
load length field
```

without entering the general interop API.

This is an internal optimization.

External ABI stability must not constrain internal JIT helper signatures.

---

# 51. JIT call into extension

A native extension call can use a compact ABI such as:

```rust
extern "C" fn(
    ctx: *mut TonicContext,
    args: *const TonicHandle,
    nargs: usize,
) -> TonicHandle
```

If VM registers hold internal `Value`s, the JIT may need to create temporary rooted handles.

Optimization opportunity:

```text
internal Value
    │
temporary local handle
    │
native extension
```

For immediate values, handle creation should be extremely cheap.

---

# 52. Internal vs external ABI

Maintain two separate contracts.

## Internal runtime ABI

May change between Tonic builds.

Optimized.

Used by:

- JIT;
- VM;
- builtins;
- runtime helpers.

## Stable extension ABI

Versioned.

Opaque.

Used by separately compiled extensions.

Do not compromise internal performance to preserve stable ABI.

Bridge through thin adapters.

---

# 53. Interop fast path

Ideal Tonic-native interop:

```text
VM/JIT
  │
  ▼
known native extension
  │
  ├─ local handle scope
  ├─ zero-copy argument access
  └─ direct native call
```

No CPython object creation.

No reference-count traffic for every temporary.

No dictionary materialization for argument passing.

---

# 54. Compatibility slow path

Legacy CPython interaction:

```text
Tonic Value
   │
   ▼
proxy/materialize
   │
   ▼
PyObject*
   │
   ▼
CPython extension
   │
   ▼
PyObject*
   │
   ▼
convert/wrap
   │
   ▼
Tonic Value
```

This can be relatively expensive.

The architectural requirement is not to make it free.

The requirement is to isolate it.

---

# 55. Interop caching

Conversion caches may improve repeated crossings.

Examples:

```text
Tonic object -> CPython proxy weak cache
CPython object -> Tonic wrapper weak cache
interned string conversion cache
type adapter cache
```

Caches must be:

- weak where appropriate;
- version-aware;
- runtime-specific;
- bounded or collectable.

Never build a permanent global cache of every bridged object.

---

# 56. Type adapter system

Interop may use adapter descriptors.

Concept:

```rust
struct TypeAdapter {
    tonic_type: TypeId,
    foreign_type: ForeignTypeId,

    to_foreign: ConvertFn,
    from_foreign: ConvertFn,

    buffer_export: Option<BufferExportFn>,
}
```

Examples:

```text
Tonic Int <-> CPython PyLong
Tonic Str <-> CPython PyUnicode
Tonic Array <-> NumPy ndarray-like object
```

Adapters should be cached by type IDs.

---

# 57. Zero-copy array bridge

A desirable path:

```text
Tonic Array
    │
    ├── metadata
    └── shared non-moving buffer
              │
              ▼
         foreign array
```

Foreign side should hold an owner token that keeps the Tonic buffer alive.

No element-by-element conversion.

---

# 58. Strings and zero-copy

String zero-copy is harder.

If Tonic uses UTF-8 and the foreign runtime needs another encoding/layout, conversion may be unavoidable.

Do not contort Tonic's internal string representation solely for CPython compatibility.

Cache conversions only when profiling proves a benefit.

---

# 59. Interop for dictionaries

Generic dict crossing often requires semantic adaptation.

Options:

- materialize foreign dict;
- expose mapping proxy;
- use specialized adapter for string-key dictionaries.

Do not assume layouts are compatible.

For high-frequency crossings, a mapping proxy may be preferable.

---

# 60. Interop for iterators

Foreign iteration should avoid unnecessary list materialization.

Expose iterator adapters:

```text
Tonic iterator
     │
     ▼
foreign iterator wrapper
```

and vice versa.

Iteration boundary cost is acceptable but should not allocate the whole sequence eagerly.

---

# 61. Descriptor and attribute semantics

Foreign objects may implement attribute behavior differently.

Tonic foreign wrappers should delegate generic operations through the adapter/vtable.

Do not apply shape-slot optimizations to arbitrary foreign objects.

Specialization may occur only if an adapter explicitly exposes a stable property contract.

---

# 62. FFI security model

The native extension ABI is trusted native code.

It can crash or corrupt the process if malicious.

Tonic should clearly distinguish:

```text
safe guest code
trusted native extension
sandboxed external process
```

Do not imply that opaque handles make arbitrary native extensions memory-safe.

They protect runtime architecture and reduce accidental misuse.

---

# 63. Out-of-process interoperability

For untrusted or unstable native components, support a future process boundary.

Concept:

```text
Tonic
  │
IPC / shared memory
  │
foreign process
```

Shared buffer descriptors may permit efficient data transfer.

This is separate from the in-process extension ABI.

---

# 64. ABI versioning

Every stable extension ABI needs a version.

Example:

```c
#define TONIC_ABI_VERSION 1
```

Runtime startup:

```text
extension ABI 1
runtime supports ABI 1
        ↓
load
```

If incompatible:

```text
reject extension with precise error
```

Do not attempt undefined best-effort loading.

---

# 65. Capability negotiation

Instead of changing the base ABI constantly, extensions may query capabilities.

Example:

```text
buffer-v1
weak-handle-v1
async-callback-v1
foreign-type-v2
```

Concept:

```c
void* tonic_query_api(
    TonicContext*,
    const char* capability_name,
    uint32_t version
);
```

This allows optional subsystems to evolve independently.

---

# 66. Weak handles

The runtime should eventually support weak handles.

Concept:

```text
weak handle
    │
    ├─ object alive -> upgrade succeeds
    └─ object dead  -> None/error
```

Useful for:

- proxy caches;
- adapter caches;
- foreign object identity caches.

Weak handles do not keep objects alive.

---

# 67. Handle invalidation

Persistent handles must have explicit release.

After release:

```text
generation increments
```

A stale handle should fail predictably.

Debug mode should produce diagnostics such as:

```text
invalid TonicHandle:
index=142
expected generation=8
actual generation=9
```

Never allow stale handles to become arbitrary object access.

---

# 68. Runtime shutdown

Interop complicates shutdown.

The runtime should have phases:

```text
Running
   ↓
ShuttingDown
   ↓
Finalizing
   ↓
Dead
```

Foreign callbacks should be rejected once the runtime can no longer safely execute guest code.

Persistent handles should become invalid after final shutdown.

Native destructors should receive documented guarantees.

---

# 69. Extension unload

Dynamic unloading is dangerous.

An extension cannot be unloaded while:

- registered functions point into it;
- native objects use its vtables;
- callbacks reference its code;
- finalizers are pending.

Initial Tonic versions should prefer:

```text
load once, keep loaded until runtime/process shutdown
```

unless a safe unload protocol is designed.

---

# 70. Serialization is not interop

Do not confuse:

```text
object interoperability
```

with:

```text
serialization
```

A Tonic object crossing into a remote process may require serialization.

A Tonic object crossing into native Rust in the same runtime should normally use handles/views.

---

# 71. Suggested Rust module layout

Possible runtime structure:

```text
tonic-runtime/
├── value.rs
├── handle.rs
├── handle_table.rs
├── scope.rs
├── context.rs
├── native_api.rs
├── buffer.rs
├── foreign.rs
├── callback.rs
├── thread_state.rs
├── exception.rs
├── pin.rs
└── interop/
    ├── mod.rs
    ├── c_api.rs
    ├── rust_api.rs
    ├── adapters.rs
    └── cpython/
        ├── mod.rs
        ├── proxy.rs
        ├── foreign_pyobject.rs
        ├── conversion.rs
        └── gil.rs
```

The exact crate boundaries can evolve.

Conceptual separation is mandatory.

---

# 72. Suggested public Rust API

Conceptual example:

```rust
pub trait TonicApi {
    fn none(&self) -> Handle;

    fn from_i64(&mut self, value: i64) -> TonicResult<Handle>;
    fn to_i64(&mut self, value: Handle) -> TonicResult<i64>;

    fn from_f64(&mut self, value: f64) -> TonicResult<Handle>;
    fn to_f64(&mut self, value: Handle) -> TonicResult<f64>;

    fn from_str(&mut self, value: &str) -> TonicResult<Handle>;

    fn get_attr(
        &mut self,
        object: Handle,
        name: Handle,
    ) -> TonicResult<Handle>;

    fn set_attr(
        &mut self,
        object: Handle,
        name: Handle,
        value: Handle,
    ) -> TonicResult<()>;

    fn call(
        &mut self,
        callable: Handle,
        args: &[Handle],
        kwargs: &[KeywordArg],
    ) -> TonicResult<Handle>;

    fn export_buffer(
        &mut self,
        object: Handle,
        flags: BufferFlags,
    ) -> TonicResult<BufferView<'_>>;
}
```

This is an ergonomic Rust facade.

It is not necessarily the exact stable C ABI.

---

# 73. Safe local handle example

```rust
fn dot_product(
    ctx: &mut Context,
    a: Handle,
    b: Handle,
) -> TonicResult<Handle> {
    let a = ctx.buffer::<f64>(a)?;
    let b = ctx.buffer::<f64>(b)?;

    if a.len() != b.len() {
        return Err(ctx.value_error("length mismatch"));
    }

    let mut total = 0.0;

    for (x, y) in a.iter().zip(b.iter()) {
        total += x * y;
    }

    ctx.from_f64(total)
}
```

Desired behavior:

- no element boxing;
- no list conversion;
- no CPython bridge;
- zero-copy numeric access.

---

# 74. Native extension fast numerical path

Ideal flow:

```text
Tonic array
     │
     ▼
buffer export
     │
     ▼
&[f64]
     │
     ▼
Rust native loop / SIMD
     │
     ▼
Tonic float
```

This is one of the strongest reasons to make the buffer API first-class.

---

# 75. SIMD and interop

Native extensions may exploit:

- Rust auto-vectorization;
- explicit SIMD;
- BLAS;
- GPU libraries;
- platform intrinsics.

The Tonic ABI should not force values through object-by-object APIs when contiguous typed buffers are available.

---

# 76. Tonic-native class extension

An extension may register a native-backed class:

```python
class Matrix:
    ...
```

Internally:

```text
Tonic managed object
    │
    ├── Tonic type/shape metadata
    │
    └── native payload handle
```

Methods are Tonic-callable native functions.

The object remains part of Tonic's type system.

Its native payload is an implementation detail.

---

# 77. Foreign type specialization

Tonic's JIT should initially treat foreign objects conservatively.

Possible later optimization:

```text
guard foreign adapter id
guard adapter version
direct native slot call
```

Only use such optimization where invalidation can be defined clearly.

---

# 78. Bridge cost visibility

For performance tooling, Tonic should be able to report interop crossings.

Useful counters:

```text
native extension calls
CPython bridge calls
proxy creations
foreign wrapper creations
buffer exports
buffer copies
pin operations
persistent handle count
```

This helps users diagnose accidental slow compatibility paths.

---

# 79. Debug interop mode

A debug mode should aggressively verify:

- handle validity;
- runtime ownership;
- scope lifetime;
- stale handles;
- double persistent release;
- buffer lifetime;
- illegal GC during no-GC scope;
- pinned object accounting;
- callbacks after shutdown.

Interop bugs are often subtle.

Diagnostics should favor catching them early.

---

# 80. Interop benchmark suite

Maintain benchmarks for:

- create/release local handle;
- resolve handle;
- persistent handle creation;
- native no-arg call;
- native 2-arg call;
- int conversion;
- string conversion;
- attribute operation through foreign wrapper;
- zero-copy buffer export;
- pinned buffer export;
- callback into Tonic;
- CPython proxy call;
- Tonic <-> CPython primitive conversion.

Measure boundary overhead separately from guest execution.

---

# 81. Interop performance targets

These are architectural targets, not promises.

## Tonic-native handle path

Should be cheap enough to use per native call.

## Handle lookup

Should be close to indexed table access.

## Primitive conversion

Should avoid heap allocation where possible.

## Buffer export

Should usually be metadata-only for compatible native buffers.

## CPython bridge

May be significantly slower.

Its overhead is acceptable because it is a compatibility path.

---

# 82. Avoiding accidental boxing

An extension function should be able to request typed primitives.

Bad path:

```text
small Tonic int
   ↓
heap object
   ↓
handle
   ↓
foreign conversion
   ↓
C int
```

Preferred path where possible:

```text
small Tonic int
   ↓
decode immediate
   ↓
C int
```

The extension API still receives a handle/value abstraction, but conversion helpers can detect immediates directly.

---

# 83. Interop and shape objects

Tonic-native extensions that query ordinary attributes should use normal runtime APIs.

They must not inspect shape slots directly through the stable ABI.

Otherwise:

```text
shape layout
```

would become ABI.

An optional unstable internal extension API may expose deeper structures for builtins compiled together with the runtime.

Keep it separate from stable third-party ABI.

---

# 84. Builtin modules vs third-party extensions

Tonic's built-in native modules may use an unstable privileged internal API.

Third-party extensions use the stable opaque ABI.

Architecture:

```text
runtime-internal builtins
    │
    └── Internal ABI

third-party native modules
    │
    └── Stable Tonic ABI
```

Do not force builtins through unnecessary stable-ABI overhead when they ship with the runtime.

---

# 85. Memory allocation API

Extensions sometimes need memory.

Provide explicit allocator helpers only where useful.

For plain native payload memory:

```text
system allocator / Rust allocator
```

may be used if ownership stays entirely native.

For managed objects:

```text
must go through Tonic runtime
```

Extensions must not allocate fake managed objects themselves.

---

# 86. Crossing GC boundaries

Any native API operation should be categorized.

## GC-safe

May allocate and trigger collection.

Raw borrowed object pointers cannot remain live across it.

## No-GC

Guaranteed not to collect.

Can be used with temporary borrowed views.

Document the category of each low-level API.

---

# 87. Example extension execution

Tonic source:

```python
import fastmath

result = fastmath.sum(values)
```

Possible flow:

```text
CALL_NATIVE
    │
    ▼
create local handle scope
    │
    ▼
pass `values` handle
    │
    ▼
native `sum`
    │
    ├── request f64 buffer
    ├── get zero-copy view
    ├── compute
    └── return immediate/boxed float
    │
    ▼
scope cleanup
    │
    ▼
resume VM/JIT
```

No Python tuple.

No Python dict.

No CPython.

No element boxing.

---

# 88. Example CPython compatibility execution

Tonic source:

```python
import legacy_pkg

result = legacy_pkg.process(obj)
```

Possible flow:

```text
Tonic obj
   │
   ▼
lookup bridge adapter
   │
   ▼
create/reuse PyTonicProxy
   │
   ▼
construct CPython call arguments
   │
   ▼
invoke legacy package
   │
   ▼
receive PyObject*
   │
   ▼
primitive convert or ForeignPyObject wrap
   │
   ▼
Tonic Value
```

This path is intentionally isolated.

---

# 89. Future HPy interoperability

Tonic may eventually support an HPy-compatible adapter.

Architectural similarity:

```text
HPy handle model
        ↕
Tonic opaque handle model
```

However, do not assume the semantics or binary representation are identical.

An adapter could translate:

```text
TonicHandle
    ↕
HPy
```

inside a compatibility subsystem.

Tonic's core ABI remains independent.

---

# 90. Foreign runtime adapters

The same architecture can support other runtimes.

Example:

```text
Tonic
  │
  ├── CPython adapter
  ├── HPy adapter
  ├── Arrow adapter
  ├── JVM process adapter
  └── JavaScript/WASM adapter
```

The common abstraction is:

```text
opaque ownership
+
typed conversion
+
buffer exchange
+
callback
+
exception translation
```

Do not hard-code CPython assumptions into the generic foreign-object subsystem.

---

# 91. WASM considerations

A future WASM-compatible interop API should avoid raw native pointer assumptions.

Opaque handle IDs already help.

Potential WASM architecture:

```text
WASM module
    │
integer handle IDs
    │
Tonic host functions
```

Buffers can be shared through explicit linear-memory copying/mapping depending on runtime capabilities.

---

# 92. ABI safety rules

Stable native ABI rules:

1. Never expose movable object addresses as stable identity.
2. Never expose Rust object layout.
3. Never let foreign code construct arbitrary handles.
4. Validate runtime ownership.
5. Validate handle generation.
6. Require explicit persistent ownership.
7. Root managed references across possible GC.
8. Pin or externalize memory before exposing stable pointers.
9. Never unwind Rust panics through C.
10. Never execute callbacks after runtime death.

---

# 93. Performance rules

1. Tonic-native interop should not allocate argument tuples.
2. Typed buffer access should avoid element boxing.
3. Handle resolution should use indexed structures.
4. Local handle cleanup should be bulk/scoped.
5. CPython conversion should remain outside native hot paths.
6. Native builtins may use a faster internal ABI.
7. JIT may bypass generic APIs for proven builtins.
8. External stable ABI must stay opaque.
9. Pinning must be explicit and rare.
10. Copies must be observable in profiling counters.

---

# 94. Correctness rules

1. Handle lifetime errors must not become arbitrary memory access.
2. Foreign object destruction must happen exactly once.
3. Runtime shutdown must invalidate future callbacks.
4. Cross-runtime handles must be rejected.
5. Buffer owner lifetime must dominate buffer view lifetime.
6. CPython objects must obey CPython ownership inside the bridge.
7. Tonic objects must obey Tonic GC rules everywhere else.
8. Bridge cycles need explicit policy.
9. Exception state must not leak between calls.
10. Generic fallback must exist when adapters cannot specialize.

---

# 95. Suggested implementation stages

## Stage 1 — Native Rust API

Implement:

- `Handle`;
- handle table;
- local scope;
- persistent handle;
- `Context`;
- primitive conversions;
- native function registration.

Do not start with CPython.

## Stage 2 — Native extension ABI

Implement:

- C-compatible function table;
- ABI version;
- extension init;
- argument calls;
- exceptions;
- stable opaque types.

## Stage 3 — Buffers

Implement:

- buffer descriptor;
- non-moving buffer allocation;
- owner token;
- zero-copy Rust views;
- tests.

## Stage 4 — Callbacks

Implement:

- persistent callable;
- thread attach;
- callback trampoline;
- exception translation.

## Stage 5 — Foreign wrappers

Implement:

- generic foreign payload;
- vtable;
- lifecycle;
- adapter registry.

## Stage 6 — CPython bridge

Implement:

- embedded/interfaced CPython boundary;
- `PyTonicProxy`;
- `ForeignPyObject`;
- primitive conversion;
- call conversion.

## Stage 7 — Advanced bridge optimization

Add:

- weak proxy caches;
- buffer sharing;
- type adapters;
- cross-runtime diagnostics.

---

# 96. First concrete interop milestone

A good first milestone is a Rust-native `fastmath` module.

Tonic:

```python
import fastmath

print(fastmath.add(20, 22))
```

Rust implementation:

```rust
fn add(
    ctx: &mut Context,
    args: &[Handle],
) -> TonicResult<Handle> {
    let a = ctx.to_i64(args[0])?;
    let b = ctx.to_i64(args[1])?;

    ctx.from_i64(a + b)
}
```

This milestone should prove:

- module registration;
- native call dispatch;
- handle scope;
- primitive conversion;
- result return;
- exception path.

---

# 97. Second interop milestone

Implement:

```python
import fastmath

values = array([1.0, 2.0, 3.0, 4.0])

print(fastmath.sum(values))
```

Prove:

```text
Tonic array
    ↓
zero-copy f64 slice
    ↓
native Rust computation
    ↓
Tonic result
```

No per-element boxing.

No buffer copy.

---

# 98. Third interop milestone

Implement a minimal CPython bridge demo.

Example goal:

```python
import cpython_compat

obj = tonic_object()

result = cpython_compat.call_legacy(obj)
```

Prove:

- Tonic object -> CPython proxy;
- CPython operation -> proxy forwards into Tonic;
- returned PyObject -> Tonic conversion/wrapper;
- clean lifetime management.

Do not optimize this before correctness is complete.

---

# 99. North-star architecture

The intended final architecture is:

```text
                         Tonic Source
                              │
                              ▼
                       VM / Cranelift
                              │
                         internal Value
                              │
              ┌───────────────┼────────────────┐
              │               │                │
              ▼               ▼                ▼
       tagged immediate   heap handle     native buffer
                              │                │
                              ▼                ▼
                        handle table      buffer heap
                              │
                              ▼
                         moving heap

                              │
                              ▼
                    Interoperability Runtime
                              │
         ┌────────────────────┼────────────────────┐
         │                    │                    │
         ▼                    ▼                    ▼
  Tonic Native ABI      CPython Bridge       Data/Buffer ABI
         │                    │                    │
         ▼                    ▼                    ▼
      Rust/C             PyObject world       Arrow/NumPy/etc
```

---

# 100. Final principle

Tonic's interoperability layer should behave like a **semantic bridge**, not a shared object layout.

The system should preserve this invariant:

> Tonic objects belong to Tonic. Foreign runtimes interact through opaque handles, adapters, proxies, or buffers. Compatibility never defines the native representation.

That architectural boundary is what allows Tonic to combine:

- Python-like language behavior;
- a modern moving GC;
- tagged values;
- shape-based objects;
- specialization;
- Cranelift JIT;
- zero-copy native data access;
- optional CPython compatibility;

without inheriting the performance constraints of CPython's internal object model.
