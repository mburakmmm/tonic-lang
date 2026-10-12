# ADR 0139: Lower arbitrary-stride buffer addresses in Cranelift

## Status

Accepted.

## Context

ADR 0138 made signed byte strides, logical origins, and non-contiguous fallback
semantics authoritative in the runtime. The typed JIT still requested only
C-contiguous buffers and flattened tuple indices with row-major multiplication.
Annotated padded, reversed, or broadcast buffers therefore deoptimized at entry
even though their complete reachable address range had already been validated.

The JIT must preserve Python negative-index and bounds behavior, exact-PC deopt,
loop range proofs, writable mutation epochs, and foreign alias rejection. Stride
support cannot weaken any of those guards or derive addresses from guest-owned
unvalidated metadata.

## Decision

`F64BufferView` now supplies a call-lifetime pointer to `rank` signed byte
strides. The runtime helper writes data, logical length, shape, rank, mutation
epoch, and strides into a six-word JIT-private stack descriptor. Data already
points at the validated logical origin. Shape and stride allocations remain
stable while the managed owner stays in the precise root array.

For rank-N tuple access, generated code retains the existing per-axis integer,
negative-index, bounds, and range-proof logic. After normalization it computes
the byte displacement as the sum of each `index * stride[axis]`. For scalar flat
access, bounds are checked against logical length; the index is decomposed from
the last axis with unsigned remainder/division and each coordinate contributes
the same signed stride product. Rank zero maps its sole logical scalar directly
to the origin.

The final signed displacement is added to the logical-origin pointer. Negative
strides use ordinary two's-complement pointer-width arithmetic. The runtime
constructor has already proven that every in-bounds coordinate maps inside the
initialized owned backing allocation, so generated code needs no new address
guard or runtime helper. Zero stride naturally maps several logical coordinates
to one cell.

Loads and stores share this address builder. Store guards and float deopt state
materialization still finish before the side effect; mutation epoch advancement
and active writable-export rejection are unchanged. Shape-based loop bounds
proofs remain valid because storage layout does not change logical dimensions.

## Consequences

Exact annotated f64 buffers with padded, transposed, reversed, or broadcast
layouts can execute rank-N loads, logical flat loads, loops, and writable stores
in native code without tuple allocation, element boxing, or per-item helpers.
Contiguous buffers follow the same address lowering and keep their existing
entry, root, bounds, and deoptimization contracts.

Direct JIT tests cover negative rank-one loads, padded rank-two tuple loads, and
padded writable stores. Runtime integration covers padded rank-two tuple and
flat loads, native mutation, nested range-proof loops, moving stress GC, and
zero deoptimizations on valid inputs. General container alias/escape
materialization remains separate work.
