# ADR 0143: Lower the three-way immediate union in the typed JIT

## Status

Accepted.

## Context

Tonic already canonicalizes PEP 604 unions and lowers `int | None`,
`bool | None`, and `int | bool` into guarded typed-JIT signatures. A common
Python annotation combines all three members. Leaving `int | bool | None` in the
generic tier loses useful information after a `None` identity check even though
every member has a compact immediate representation.

Annotations remain advisory. The optimization therefore needs a complete entry
guard, exact arbitrary-PC reconstruction behavior, and safe fallback for values
outside the annotation.

## Decision

Add `IntOrBoolOrNone` to the runtime annotation scalar and Cranelift scalar-fact
lattices. The entry guard accepts only an immediate integer, either boolean
singleton, or `None`. Control-flow merges preserve the smallest representable
fact and widen mixed integer, boolean, and None paths to the three-way fact.

An identity comparison with `None`, including reversed operands and `is not`,
refines the matching edge to `None` and the other edge to `IntOrBool`. Existing
integer lowering then preserves Python's boolean-as-integer arithmetic without a
redundant tag guard. The same fact can be used for proven returns and guarded
direct-call result propagation.

## Consequences

Annotated functions using the three-way union can enter the typed JIT on their
first call and keep non-None arithmetic native. A value outside the annotation
fails the entry or resume guard and executes the ordinary generic semantics; no
runtime type error is introduced by the annotation.

This decision does not claim general union narrowing. ADR 0144 adds
builtin-rebinding-safe `isinstance` narrowing for immediate unions; heap-backed
unions such as `int | float` and user-class shape refinement remain separate
roadmap work.
