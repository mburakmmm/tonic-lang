# ADR 0126: Buffer annotation entry guard

## Status

Accepted.

## Context

ADR 0125 gives the native `f64` buffer a canonical TypePlan, but metadata alone
does not route annotated functions into the JIT. Treating a buffer as an
unboxed scalar would lose its managed owner and violate moving-GC root rules.
Trusting the annotation without inspecting the argument would also turn an
advisory Python hint into an unsound runtime type assertion.

The current JIT can side-exit for a generic call and resume at its successor PC.
The existing `fastmath.sum` native implementation already traverses a buffer's
`f64` backing allocation without boxing elements or copying the exported data.

## Decision

The annotation entry model accepts `TypePlan::Buffer` as a managed parameter.
Before entering compiled code, the VM guards the exact native buffer object and
implemented `f64` storage. It also checks rank and mutability whenever those
constraints are present in the plan. The value itself remains a tagged entry in
the precise JIT root buffer and is represented as dynamic in the scalar dataflow
lattice.

An annotated function can therefore compile on its first matching call. A
buffer call into the current native sum path side-exits through the ordinary VM
call boundary, performs the zero-copy native loop, and resumes compiled code at
the successor PC. A non-buffer argument misses the annotation entry guard and
executes generic bytecode; no annotation-induced `TypeError` is added.

## Consequences

Buffer annotations now affect execution routing with a concrete, GC-safe guard
rather than acting only as cache metadata. The native loop is still reached
through one generic call boundary. Direct indexed-loop lowering, shape/stride
guards inside machine code, bounds elimination, writable stores, alias analysis
and escape materialization remain later work.
