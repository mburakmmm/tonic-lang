# ADR 0131: Prove constant affine buffer indices

## Status

Accepted.

## Context

ADR 0129 and ADR 0130 eliminate repeated bounds checks only when an indexed
load uses the induction variable directly. Numeric kernels commonly access a
fixed neighbor such as `buffer[index + 1]` or `buffer[index - 1]`.

## Decision

The range analysis recognizes a direct induction variable or one `Add`,
`InplaceAdd`, `Sub` or `InplaceSub` with a compile-time integer constant. The
result is represented as `induction + offset`. `constant + induction` is valid;
`constant - induction`, multiple arithmetic operations and dynamic offsets are
rejected.

The compile-time lower-bound proof requires `start + offset >= 0` without i64
overflow. The entry upper-bound proof is equivalent to
`bound + offset <= buffer_length`:

- for a positive offset, the guard first requires `buffer_length >= offset`
  and then checks `bound <= buffer_length - offset`;
- for a negative offset, it checks `bound <= buffer_length - offset`, saturating
  the expanded length at i64 maximum if the host addition would overflow;
- for zero, it retains `bound <= buffer_length`.

The generated index arithmetic keeps its typed overflow guard. Overflow
deoptimizes at that arithmetic instruction, and arbitrary-PC resume retains the
ordinary checked indexed-load path. Consequently unchecked memory access never
depends on wrapping arithmetic.

## Consequences

Proven constant-neighbor loops avoid per-element normalization and bounds
checks. A loop whose first effective index would be negative remains checked,
preserving Python negative-index semantics. Dynamic offsets, nested affine
expressions, descending induction and multidimensional shape calculations
remain future work.
