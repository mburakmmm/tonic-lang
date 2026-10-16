# ADR 0155 — Function-local expanded-call result annotations

## Status

Accepted and implemented as the fourth local typed-data-flow boundary.

## Context

An annotated initializer may call through Python's expanded argument syntax:
`local: int = callee(*items, **mapping)`. Tonic lowers this form to a balanced
`BEGIN_ARGS`/argument-operations/`CALL_EXPANDED` segment. Treating only the final
call as an ordinary side exit would lose the VM argument stack state, while
trusting the source annotation would make a non-enforcing Python hint affect
runtime behavior.

## Decision

Bytecode version 37 permits `AdvisoryResult` on `CALL_EXPANDED`. The compiler
emits the metadata when a simple function-local scalar annotation is initialized
directly by an expanded call.

The advisory site removes any competing direct expanded-call lowering. Native
execution exits at the segment's `BEGIN_ARGS` instruction. The VM records the
current expanded-argument stack depth, evaluates every positional, starred,
named and mapping argument with the existing Python-compatible semantics, binds
and invokes the callee, then marks the caller for native resumption. Re-entry at
`CALL_EXPANDED + 1` applies the existing exact scalar or union result guard before
typed data flow consumes the destination register.

If expansion, binding or the call raises, the normal VM error path remains in
control. If the returned value disagrees with the annotation, the successor
guard deoptimizes and ordinary bytecode semantics continue with that same value.

## Consequences

Function-local annotations can now restart typed native execution after both
ordinary and expanded calls without duplicating Python's argument machinery in
the JIT. This first implementation pays a generic boundary for each guarded
expanded call. Closure/cell propagation and general function-wide
definite-assignment/SSA inference remain outside this decision.
