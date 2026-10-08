# ADR 0127: Parameterized f64 buffer annotations

## Status

Accepted.

## Context

The initial `fastmath.Buffer` plan knows that the only implemented backing
storage is `f64`, but it cannot express rank or mutability assumptions. The
future strict fixed-width value system belongs under `tonic.types`; introducing
placeholder dtype classes there before their storage and ABI semantics exist
would make the public contract misleading.

Python class subscription already provides a familiar annotation spelling and
produces managed GenericAlias objects without changing the grammar.

## Decision

The advisory native buffer supports these annotation forms:

- `fastmath.Buffer[float]` selects the implemented `f64` storage;
- `fastmath.Buffer[float, rank]` adds an exact nonnegative `u32` rank;
- `fastmath.Buffer[float, rank, writable]` adds exact mutability, where `True`
  means writable and `False` means read-only;
- `...` in the rank or mutability position leaves that constraint unrestricted.

Plain `Buffer`, `Buffer[float]`, and `Buffer[float, ..., ...]` resolve to the
same canonical plan and hash. Only `float` is accepted because no other backing
storage is implemented. Unsupported dtype, invalid rank, invalid mutability and
invalid arity have distinct deterministic TypePlan rejection codes. TypePlan
schema v7 includes those rejection categories.

The existing annotation-JIT entry guard consumes the rank and mutability fields.
A mismatch skips compiled entry and executes generic bytecode without adding a
runtime type error.

## Consequences

User code can state the assumptions needed by later buffer loop lowering while
the current entry guard already enforces them safely. The syntax remains an
advisory Tonic extension using ordinary Python grammar. Additional storage
dtypes, strict fixed-width types, dimension extents, strides, direct indexed
machine loops and writable stores remain separate roadmap work.
