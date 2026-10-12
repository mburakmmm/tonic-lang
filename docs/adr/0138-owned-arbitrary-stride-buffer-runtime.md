# ADR 0138: Own and validate arbitrary-stride f64 buffer views

## Status

Accepted.

## Context

Tonic's buffer descriptor already carried shape and byte strides, but the owned
runtime buffer could only construct dense C-contiguous storage. Generic item
access flattened indices as if every buffer were dense, `len` and `sum` observed
the complete backing allocation, and the C boundary always advertised
C-contiguity. Adding stride loads directly to generated code on top of those
semantics would make interpreter fallback and native execution disagree.

An arbitrary-stride view can start inside its backing allocation, move backward,
skip padding, or repeat one cell with a zero stride. The runtime must prove every
reachable address before exposing such a view. Guest and extension APIs must see
logical elements rather than incidental backing cells.

## Decision

An owned `Buffer` stores a boxed f64 backing allocation, a logical element
origin, checked logical element count, shape, and signed byte strides. The public
Rust constructor copies the backing and treats `origin` as an element index.
Construction rejects rank/stride mismatch, non-f64-aligned strides, arithmetic
overflow, origins outside the allocation, and any minimum or maximum reachable
offset outside the backing. Negative and zero strides are valid when this proof
succeeds. Empty views may use an in-allocation or one-past origin and never
dereference it.

Tuple indexing normalizes each Python index per axis and applies the stored byte
stride. Scalar indexing remains a logical C-order flat index: it is decomposed
into coordinates and then mapped through the same strides. `len` returns the
logical element count. Assignment mutates the mapped backing cell and retains
the existing epoch contract. Native `fastmath.sum` retains a direct slice loop
for contiguous storage and walks logical flat indices for other layouts, so
padding and reverse/broadcast layouts have the same observable semantics as
generic item access.

The C descriptor's data pointer starts at the logical origin, and shape/strides
describe all further addresses. Its byte length is the logical element count
times item size. Actual layout determines `BUFFER_C_CONTIGUOUS`; requesting that
capability for a non-contiguous view fails before an owner handle is created.
A request without it receives the exact signed strides. Writable capability and
owner lifetime rules remain unchanged.

The current Cranelift descriptor accepts only C-contiguous views. A strided
annotated argument therefore deoptimizes at entry to the now-correct generic
path until stride-pointer lowering is added separately.

## Consequences

Runtime, native extension, and future JIT tiers now share one validated layout
model. Padded, transposed, reversed, and broadcast views no longer need copied
materialization for correctness. Backing storage remains owned and non-moving,
so existing precise root and pointer-lifetime guarantees continue to hold.

Non-contiguous logical iteration is currently an index fold and favors
correctness over a specialized stride iterator. The next JIT stage can add the
stride pointer to the call-lifetime descriptor and lower `sum(index * stride)`
without changing guest semantics or the extension ABI.
