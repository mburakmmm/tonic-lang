# ADR 0134: Hoist canonical rank-2 column bounds

## Status

Accepted.

## Context

Exact rank-2 read-only f64 buffers can be indexed directly in native code, but
each matrix access still checks both axes. Row-oriented numeric loops commonly
hold one row and scan columns with a canonical integer induction variable. The
rank-1 range proof already establishes the safety conditions needed to move that
repeated column check to native entry.

## Decision

The verified buffer range analysis now considers each component of an
immediately consumed rank-2 tuple independently. For the current slice, it
accepts the same straight-line canonical loop as rank one: a nonnegative
constant induction start, a positive constant step, an exact-int bound
parameter, a `<` loop condition, and no mutation of the bound or buffer owner.

When the proven induction expression supplies the column component, native
entry guards the dynamic bound against the descriptor's column count rather than
the flattened buffer length. On a normal entry the generated column address path
omits its per-item bounds check. The row component remains checked unless it has
an independent proof. Any arbitrary-PC resume re-enables both per-axis checks.

If the entry guard fails, execution deoptimizes at PC zero. The generic loop then
produces the established result or `IndexError`; the annotation remains advisory.
Functions with exception regions, suspension, noncanonical control flow,
negative starts, nonpositive steps, or mutable owner/bound registers do not
receive the proof.

## Consequences

Canonical row scans pay one dynamic column-shape guard per call instead of one
column bounds guard per element. The existing row-major native load, precise
owner root and tuple-PC deoptimization rules are unchanged.

Nested-loop multi-axis proofs, arbitrary strides, rank-N lowering and writable
native stores remain future work.
