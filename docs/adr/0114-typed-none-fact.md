# ADR 0114: Typed `None` fact

## Status

Accepted.

## Context

TypePlan already distinguishes the source annotation value `None` and the exact
`NoneType` class, but annotation-guided JIT entry accepted only integer, float
and boolean scalar signatures. Functions such as `def close() -> None` therefore
remained generic even though the value has a unique immediate representation and
requires no allocation, unboxing or heap-layout assumption.

Python annotations remain advisory. A parameter annotated with `None` must not
raise a new type error when another value arrives.

## Decision

The public JIT signature and internal scalar lattice include a `None` fact.
Runtime TypePlan mapping treats both the annotation value `None` and exact
`NoneType` as that fact. Host entry selection compares parameters with the
singleton value and falls back to generic execution on mismatch.

The Cranelift overlay propagates `None` through parameters, `Const None` and
`Move`. Normal and arbitrary-PC entry guards compare the materialized value word
with `VALUE_NONE`. The value remains in the precise root buffer, although it is
an immediate and does not create a managed heap edge. A return is proven only
when every reachable `RETURN` carries the `None` fact; otherwise the existing
host return guard and exact-PC deoptimization remain active.

## Consequences

Common side-effecting annotated functions with `-> None` can enter typed baseline
JIT on their first call without adding allocation or a runtime type-enforcement
rule. The public proof API can also validate `None` summaries for later call-graph
work.

This change does not implement `Optional[T]` or PEP 604 unions. Those require a
canonical union runtime object, narrowing rules and complete observable Python
semantics before they can become TypePlan inputs.
