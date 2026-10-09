# ADR 0129: Hoist canonical buffer loop bounds

## Status

Accepted.

## Context

ADR 0128 lowers rank-1 read-only f64 buffer reads directly, but checks each
index because an arbitrary Python integer may be negative or outside the
buffer. Partial-static execution should remove that repeated work only when the
bytecode itself proves the complete index range.

## Decision

The JIT recognizes a deliberately narrow verified loop:

- the induction local is initialized to integer zero before the loop;
- the condition is `induction < bound`, where `bound` is an exact annotated int
  parameter that is not written in the loop;
- the indexed expression uses the induction local directly;
- the loop has no internal control-flow edge;
- the only induction write is `induction += 1` after the indexed load;
- the buffer parameter is not rebound in the loop.

Generators, coroutines and code objects with exception regions are rejected by
this first analysis. Return, raise and suspension terminators also prevent a
straight-line proof.

For a proven loop, native entry guards `bound <= buffer_length`. Negative bounds
are accepted because the loop executes zero iterations. Normal execution from
bytecode PC zero bypasses the per-item bounds normalization and comparison.
Arbitrary-PC entry after a side exit retains the ordinary per-item check because
the reconstructed induction state did not necessarily originate from the
proven loop preheader.

Typed integer comparison and increment lower directly even while the unboxed
float overlay is active. The memory-safety proof therefore does not depend on a
safe runtime helper returning semantically correct comparison results.

## Consequences

The common annotated numeric loop performs one length guard per invocation and
no bounds check per element. A too-large bound deoptimizes at entry and generic
execution preserves the eventual `IndexError`. Offset indices, different starts
or steps, nested control flow and loop-local buffer/bound writes remain checked.
Exception regions and suspended functions remain checked as well.

General range analysis, shape extents, multidimensional stride proofs and loop
versioning remain future work.
