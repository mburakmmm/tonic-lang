# ADR 0130: Generalize constant buffer induction ranges

## Status

Accepted.

## Context

ADR 0129 proves the exact `index = 0`, `index += 1`, `index < bound` loop. The
same entry guard can safely cover a broader class of constant induction ranges
without adding arithmetic to the generated guard.

## Decision

The buffer bounds analysis accepts any compile-time integer start greater than
or equal to zero and any compile-time integer step greater than zero. The loop
must retain the other ADR 0129 restrictions: direct `buffer[index]` access, an
exact annotated integer bound parameter, a strict less-than condition, one
induction update after the load and straight-line control flow.

The native entry still guards `bound <= buffer_length`. If the loop executes,
the induction value starts nonnegative, remains increasing and is strictly less
than `bound`, so every direct index is within the buffer. A typed integer
overflow at the update guard deoptimizes at that bytecode PC. Arbitrary-PC
resume uses the checked indexed-load path.

Negative starts are rejected because Python negative indexing has different
normalization semantics. Zero, negative and dynamic steps are rejected because
they do not establish monotonic progress. Offset expressions such as
`buffer[index + 1]` remain outside the proof.

## Consequences

Loops such as `index = 1; while index < count: ...; index += 2` now perform one
entry length guard and no per-item bounds check. The analysis remains
conservative for affine offsets, dynamic ranges, descending loops and
multidimensional shape/stride calculations.
