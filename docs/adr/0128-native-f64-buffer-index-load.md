# ADR 0128: Native rank-1 f64 buffer index loads

## Status

Accepted.

## Context

The native buffer object already has stable logical identity, immutable boxed
f64 storage, shape/stride metadata and advisory TypePlan constraints. Its first
JIT integration guarded entry but executed useful work only after a generic
side exit to `fastmath.sum`. A partial-static annotation needs to affect code
inside the function without exposing movable object addresses or treating the
annotation as an enforced runtime type declaration.

## Decision

An exact read-only `fastmath.Buffer[float, 1, False]` parameter may feed direct
`ITEM` lowering. The guest value remains tagged in the precise root array. At
native entry, a narrow runtime helper validates an exact native buffer with rank
one and an eight-byte C-contiguous stride, then returns an ephemeral data pointer
and element count in non-root stack storage.

Writable or mutability-unconstrained plans remain generic so external writable
buffer exports cannot create a native data race. The helper boundary has an
explicit unsafe contract: the runtime must keep the initialized f64 allocation
valid while the owner remains rooted and the current compiled invocation
executes. Tonic satisfies it because moving GC relocates the object containing
the Box handle, not the immutable boxed allocation.

Generated code accepts exact small integers and Python bool indices, normalizes
negative indices, guards both bounds and loads one f64 directly. A failed type,
layout or bounds guard returns to the exact `ITEM` bytecode PC. The generic VM
then preserves advisory annotation behavior and produces the ordinary result or
`IndexError`. Loaded values participate in the existing unboxed float dataflow;
only an observable return or deoptimization materializes a guest float.

## Consequences

Annotated rank-1 numeric loops can perform allocation-free native element loads
and float arithmetic. No heap address enters persistent bytecode, inline-cache
or JIT metadata, and no NumPy object layout is assumed. The backend reports
native buffer parameter and item-site counts for deterministic validation.

Bounds checks remain at arbitrary indexed loads. ADR 0129 removes them for one
verified canonical loop form. General loop-range proof, multidimensional stride
lowering, writable stores, mutation versioning and alias/escape materialization
remain separate roadmap work.
