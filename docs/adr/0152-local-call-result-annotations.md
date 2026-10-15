# ADR 0152 — Function-local call-result annotations

## Status

Accepted and implemented as the first local typed-data-flow boundary.

## Context

Python function-local variable annotations create lexical bindings but their
annotation expressions are not evaluated or stored in a function
`__annotations__` dictionary. Tonic previously preserved that semantic behavior
but discarded the annotation before bytecode/JIT analysis. Consequently a value
returned by an unannotated or non-inlineable call always became `Unknown`, even
when the programmer wrote a supported scalar annotation at the assignment.

Treating the annotation as a runtime assertion would be incompatible with
Python. Trusting it without checking would also be unsafe: builtin names may be
shadowed and annotations may be wrong.

## Decision

Bytecode version 34 adds Tonic-owned advisory call-result metadata. For a simple
function-local assignment of the form `name: scalar = ordinary_call()`, the
compiler recognizes exact syntactic `int`, `bool`, `float`, `None`/`NoneType`
and their PEP 604 unions. It records the ordinary `CALL` PC and a canonical
scalar category. The annotation expression remains unevaluated.

The call always crosses the ordinary VM side-exit boundary. Before compiling the
typed caller continuation, the runtime imports the local hint as a guarded call
result and disables a competing direct-call lowering at that PC. On re-entry at
the successor PC, generated code checks the materialized value. Exact float
results are unboxed into the existing F64 state only after that check. A match
feeds the verified typed overlay and the following local `Move`; a mismatch
deoptimizes at the successor PC and executes normal bytecode semantics.

The verifier rejects duplicate hints, out-of-range PCs and hints attached to
anything other than an ordinary `CALL`. Runtime statistics report accepted local
boundaries separately from all typed call-result sites.

## Consequences

Supported local annotations can now restart native typed execution after a
dynamic call without making annotations mandatory. A false hint does not add a
`TypeError`, and shadowing a spelling such as `int` cannot corrupt execution
because the real value is guarded.

This slice intentionally excludes expanded calls, non-call producers,
attribute/subscript targets, cells/closures, general definite-assignment
analysis and continuous function-wide unboxed SSA regions. Those remain under
the local/closure completion gate in `docs/ROADMAP.md`.
