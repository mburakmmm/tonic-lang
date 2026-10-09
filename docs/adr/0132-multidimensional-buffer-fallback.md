# ADR 0132: Define multidimensional buffer fallback semantics

## Status

Accepted.

## Context

Native buffers already preserve shape and byte strides, but guest item access
treated every buffer as one flattened sequence and item assignment supported
only lists and dictionaries. Native multidimensional JIT loads and writable
stores need a correct generic path for deoptimization.

## Decision

A tuple key on a buffer supplies exactly one integer index per dimension. Each
index follows Python negative-index normalization against its own dimension.
Rank mismatch and an index outside its dimension raise `IndexError`. Because
the current owned Buffer representation is C-contiguous, the generic path
computes the flat element offset in row-major order with checked arithmetic.

A scalar integer key retains the existing flattened element access behavior.
This remains useful for existing rank-1 code and does not silently reinterpret
a tuple rank mismatch.

Item assignment is permitted only when the buffer's writable flag is set. The
assigned value must convert to f64 using the runtime's ordinary numeric
conversion. Read-only buffers raise `TypeError`. The storage contains no guest
handles, so an f64 write requires no managed-heap write barrier.

## Consequences

Interpreter execution and deoptimized code now have complete rank-N read and
write semantics for the owned contiguous f64 buffer. Native JIT writes remain
disabled until mutation versioning, external writable-export alias rules and
thread ownership are explicit. Multidimensional JIT reads can now fall back at
an exact bytecode PC without losing tuple-index semantics.
