# ADR 0159 — Captured local annotation propagation

## Status

Accepted and implemented as the eighth local typed-data-flow boundary.

## Context

Bytecode v40 applies a scalar annotation to ordinary register-local stores in
one function. A local captured by a nested function is represented by a precise
managed cell instead. The child previously had no annotation plan for the free
variable, so every `LOAD_CELL` became an unknown fact even when the owner had a
unique supported annotation.

Direct native access to cell storage would couple generated code to heap layout
and would risk bypassing stale-handle checks, unbound errors, tracing roots, and
the generational write barrier. A nonlocal writer may also put a value in the
cell that disagrees with the advisory annotation.

## Decision

Bytecode version 41 propagates a unique supported local scalar plan through the
compiler's lexical capture graph. Only symbols present in the child scope's
`free` set are inherited. Nested function, lambda, and forwarding closure
chains receive the plan; class bodies remain a separate unsupported boundary.
Conflicting or unsupported annotations in the owning function propagate no
plan.

Every inherited or owner-local cell read attaches `AdvisoryResult` to its
`LOAD_CELL`. Native code exits at that exact PC. The VM performs the ordinary
cell load, including unbound validation and precise rooting, and the successor
guard converts the materialized register into a typed fact. Cell stores remain
on the VM path and continue to use the heap mutation API and write barrier.

If two annotation sources try to describe the same producer with different
scalar plans, compiler metadata for that producer is removed rather than
choosing one. Equal plans are deduplicated.

## Consequences

An annotation on an outer local can now drive typed arithmetic and return proof
inside an annotated nested function without repeating the local annotation in
the child. A later `nonlocal` mismatch deoptimizes after the actual cell load and
preserves normal Python behavior. Cell values are still boxed managed `Value`s;
native cell storage, unboxed closure ABI, and escape analysis remain future
work.
