# ADR 0133: Lower exact rank-2 f64 buffer reads in native code

## Status

Accepted.

## Context

The generic buffer path now defines rank-N tuple indexing, negative-index
normalization and bounds failures. The first native buffer tier only lowered a
scalar index into an exact rank-1 read-only f64 buffer. A rank-2 source
expression such as `values[row, column]` therefore allocated a guest tuple and
left native code even when annotations proved the owner, dtype, rank and
mutability.

The optimized path must keep the buffer owner visible to the moving collector,
must not retain raw heap addresses between calls, and must be able to reconstruct
valid interpreter state when an axis guard fails.

## Decision

An exact `Buffer[float, 2, False]` parameter is eligible for native tuple-index
lowering when the compiler finds a two-element `TUPLE` immediately followed by
the consuming `ITEM`. Both tuple components must have integer-like typed facts.

At native entry the runtime validates an exact read-only f64 buffer with rank two
and C-contiguous strides. It writes the call-lifetime data pointer, flattened
length, row count and column count into a non-root native stack slot. The tagged
buffer owner remains in the precise managed-root array.

The successful path does not materialize the guest tuple. It normalizes each
negative index against its own dimension, checks both axes, computes
`row * columns + column`, and loads the f64 value directly. The buffer allocation
invariant already proves that the shape product equals the backing length, so
the guarded row-major offset is within the allocation.

If either axis guard fails, native code deoptimizes to the skipped `TUPLE`
instruction rather than `ITEM`. The interpreter first materializes the tuple and
then executes the generic item operation, preserving a complete register state
and the established `IndexError` behavior. A rank or layout mismatch deoptimizes
at function entry.

## Consequences

Common annotated matrix reads avoid tuple allocation, per-item runtime helpers
and f64 boxing inside native numeric code. Negative indexing and bounds errors
retain Python behavior, and moving GC remains safe because native metadata never
replaces the managed owner root.

The optimization is deliberately limited to rank two, C-contiguous read-only
storage and an immediately consumed tuple. Rank-N and arbitrary-stride lowering,
writable native stores, mutation/version guards, and alias or escape
materialization remain separate roadmap work.
