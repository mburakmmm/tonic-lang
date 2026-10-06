# ADR 0113: Annotated class parameter method edge

## Status

Accepted.

## Context

An annotated caller such as `def call(owner: Counter, value: int) -> int` has
enough information to attempt `owner.add(value)` on its first invocation. The
owner is not a scalar and cannot be represented as an `i64` or `f64` fact. Its
runtime class, class version, descriptor binding and possible instance shadowing
can also change observable Python behavior.

Treating the annotation as a mandatory runtime type check would be incompatible
with normal Python annotation semantics. Trusting it without guards would allow
wrong-code execution. Resolving arbitrary register provenance while building the
edge would also amount to an incomplete second data-flow analysis.

## Decision

A supported user-class parameter plan records its logical class handle, compact
type id and class version. The native-entry host guard requires the argument's
exact runtime class and the current class metadata to match that plan. The public
JIT signature receives `ScalarType::Dynamic`, so the argument remains a
materialized, precisely rooted guest `Value` and contributes no scalar fact.

An `ATTR` call edge may use this owner only when backward provenance consists of
zero or more `Move` instructions ending at the parameter register. The existing
generated method-load helper then guards descriptor binding kind and exact
callee function at the attribute bytecode PC. The callee's annotation plan and
return proof remain dependencies of the caller.

If the first-call argument has another class, annotation compilation is skipped
and generic Python semantics run without adding `TypeError`. Instance shadowing
fails the generated method guard and resumes at the exact `ATTR` PC. Class
mutation changes the class dependency version and invalidates the caller entry.

## Consequences

Common typed service/object parameters can feed a first-call method result into
the scalar overlay without a profile warmup or bound-method allocation. Moving
GC safety and Python descriptor behavior remain unchanged.

This is not general field or shape inference. Attribute values, constructor
definite assignment, arbitrary register provenance, recursive/SCC summaries and
graph-wide code budgets remain separate roadmap work.
