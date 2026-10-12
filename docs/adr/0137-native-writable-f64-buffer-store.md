# ADR 0137: Lower exact writable f64 buffer stores

## Status

Accepted.

## Context

The rank-N native buffer path could read exact read-only C-contiguous storage,
while every assignment still deoptimized to the interpreter. Writing through a
raw descriptor is not safe merely because an annotation says `True`: generated
code must preserve Python index failures, moving-GC roots, unboxed deopt state,
mutation invalidation and foreign buffer-export aliases.

An assignment also has a stronger atomicity requirement than a load. A guard
failure must occur before storage changes, otherwise generic replay would apply
the assignment twice or observe a partially committed operation.

## Decision

`F64Buffer { rank, writable }` carries exact mutability into the typed JIT
signature. `SET_ITEM` is accepted only for `writable: true`, a proven f64 value
and an exact buffer source. Rank-one scalar keys and rank-N immediate tuple keys
reuse the native negative-index, per-axis bounds and canonical range proofs.

Each Buffer owns a boxed, non-moving mutation state. Interpreter assignment and
native assignment both increment its `u64` epoch. The JIT descriptor includes a
pointer to that epoch. Generated code validates owner identity, rank,
mutability, layout, indices and epoch headroom before writing data; it then
stores the f64 and advances the epoch. Epoch exhaustion deoptimizes before the
write, where the generic path reports the defined runtime error.

Writable C exports are explicit capabilities attached to their BufferOwner
handles. Creating one increments the active-export count; explicit release or
scope cleanup decrements it and conservatively advances the mutation epoch
because external code may have written through the pointer. A read-only request
does not advertise writable capability even when the backing buffer is mutable.
Compiled buffer access fails closed at entry while any writable export is
active. The runtime remains single-thread-owned; extensions must not race raw
buffer access from another thread.

The writable data address is derived from `Box<[f64]>::as_mut_ptr()` through a
unique heap borrow, rather than casting a pointer obtained from an immutable
slice. The buffer owner stays in the precise root array for the entire compiled
invocation and its boxed data, shape and mutation state do not move.

All store guards precede the side effect. Buffer index guards now materialize
every live unboxed f64 register before returning a deoptimization PC. Generic
execution can therefore replay the exact `SET_ITEM` or tuple-construction PC
with complete interpreter-visible state. The f64 payload contains no managed
handles, so the store needs no GC write barrier.

## Consequences

Exact writable numeric loops can update rank-N contiguous storage without tuple
allocation, element boxing or a per-store runtime helper. Reads from the same
writable buffer use the same guarded descriptor and observe the current data.
Metadata and VM statistics expose native store sites separately.

Arbitrary byte strides and general container alias/escape materialization remain
future work. The call-lifetime descriptor can add a stride pointer without
changing the mutation or owner-root contract established here.
