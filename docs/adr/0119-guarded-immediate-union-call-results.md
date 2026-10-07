# ADR 0119: Guarded immediate-union call results

## Status

Accepted.

## Context

Annotation-guided callers could import a proven exact-int or exact-float result,
but the immediate union facts introduced by ADR 0116 and ADR 0118 stopped at a
function boundary. A caller of an identity-style `int | None` leaf therefore
lost the fact before its own None check, and an `int | bool` result could not
feed native integer arithmetic.

Trusting the return annotation alone remains unsound. Python annotations are
advisory, and mutation of a callee annotation or replacement of the callee must
not leave a stale caller assumption. Inlining must also remain atomically
replayable when a guard fails.

## Decision

The existing exact-global annotated direct-call planner accepts proven `bool`,
`None`, `int | None`, `bool | None`, and `int | bool` results when the target is
inside the side-effect-free direct-inline subset. Every non-receiver parameter
must have a supported immediate scalar plan; an implicit method receiver may
remain the existing guarded dynamic prefix. The public JIT `DirectCall.result`
stores the corresponding scalar fact, so typed analysis propagates it from the
CALL destination into later caller instructions.

The callee body must pass the public all-reachable-return proof before a summary
is created. Caller guards retain callee function/code/execution identity,
annotation object, content version, canonical plan hash and class dependencies.
Changing the annotation invalidates the caller entry. Exact callee identity is
also checked at the call PC, where failure deoptimizes atomically.

## Consequences

An optional result can be narrowed by a caller's `is None` branch and an
int-bool result can use guarded native numeric lowering without an intervening
generic call. Exact bool and None identity/constant leaves also retain their
facts across the boundary.

Branches, effects, recursion/SCC summaries and general union members remain
outside this direct-inline slice. A target that cannot prove its annotated
return or cannot be replayed atomically produces no summary and uses ordinary
call semantics.
