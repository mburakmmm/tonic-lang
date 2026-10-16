# ADR 0156 — Function-local closure-cell result annotations

## Status

Accepted and implemented as the fifth local typed-data-flow boundary.

## Context

An annotated initializer inside a closure may read an outer binding:
`local: int = captured`. The compiler represents this as `LOAD_CELL` followed by
the local store. Cell contents can change through another escaping closure and
must remain precise moving-GC roots. Trusting the annotation or embedding heap
cell layout in native code would make nonlocal mutation unsafe.

## Decision

Bytecode version 38 permits `AdvisoryResult` on `LOAD_CELL`. Functions with cell
locals or free variables are no longer rejected solely for carrying closure
metadata. `LOAD_CELL` and `STORE_CELL` themselves always side exit at their exact
bytecode PC in this first implementation.

The VM performs the ordinary cell read or write, including unbound-variable
diagnostics and the GC write barrier. Cell owners remain in the VM's precise
root set while native code runs. Re-entry after an annotated load applies the
existing exact scalar or union guard before typed data flow consumes the loaded
register. A mismatching value deoptimizes at the successor PC and continues with
the same materialized value.

Direct-call inlining of closure targets remains disabled, so captured state is
never copied into an unguarded native call plan.

## Consequences

Annotated functions that read captured scalar values can use the typed JIT even
when another closure mutates the cell. Each cell access still crosses the generic
boundary. General cell SSA, definite-assignment proofs, versioned native cell
loads and stores, and direct closure-call specialization remain future work.
