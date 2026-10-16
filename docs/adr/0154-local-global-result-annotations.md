# ADR 0154 — Function-local global-result annotations

## Status

Accepted and implemented as the third local typed-data-flow boundary.

## Context

`LOAD_GLOBAL` is a value producer, but unlike the generic call, attribute and
item paths covered by ADR 0152 and ADR 0153, its ordinary JIT lowering reads a
bound global directly or invokes a runtime helper and then continues in native
code. Importing a local annotation into typed analysis without another guard
would therefore trust source spelling and could retain a stale fact after the
global is rebound.

## Decision

Bytecode version 36 permits `AdvisoryResult` on `LOAD_GLOBAL`. The compiler emits
the metadata when a simple function-local scalar annotation is initialized by a
global name load.

At a guarded global-result site, generated code bypasses the native global fast
path and exits at the exact `LOAD_GLOBAL` PC. The VM executes normal name lookup
and materializes the current binding. The code object is marked resumable, and
native re-entry at `pc + 1` applies the existing exact scalar or union guard
before typed data flow consumes the value.

Global version invalidation is not required for correctness on this path because
every execution reloads through the VM. A rebound value that no longer matches
the annotation fails the successor guard and continues under ordinary bytecode
semantics without an annotation-induced exception.

## Consequences

Function-local annotations can restart typed native execution after dynamic
global lookup while observing every rebinding. This first implementation pays a
side-exit per guarded global load. A later inline guarded global-slot path may
remove that boundary when module-version dependencies and deoptimization state
are represented directly.

Expanded calls, closure/cell values and general function-wide definite-assignment
analysis remain outside this decision.
