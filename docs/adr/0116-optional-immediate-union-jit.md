# ADR 0116: Optional immediate union JIT signatures

## Status

Accepted.

## Context

ADR 0115 introduced canonical PEP 604 union objects and TypePlan v2, but the
typed JIT still rejected every union signature. The common `T | None` form
should select first-call native execution when both members already have compact
immediate representations. Python annotations remain advisory, so a non-member
argument must keep ordinary dynamic behavior rather than raise a new type error.

A union fact also appears at control-flow joins such as a function returning an
integer on one path and `None` on another. Collapsing that join to unknown loses
a valid return proof. Treating it as an exact integer would instead allow unsafe
unboxing on the `None` path.

## Decision

The public typed signature and internal scalar lattice add `IntOrNone` and
`BoolOrNone`. Canonical TypePlan unions with exactly those members map to the new
facts. Native entry and arbitrary-PC resume guards accept either the exact scalar
tag or `VALUE_NONE`; all other values deopt at the current entry PC.

The data-flow merge of `Int`, `None`, and `IntOrNone` produces `IntOrNone`; the
equivalent rule applies to booleans. Return proof accepts either member or the
merged optional fact. Optional facts are deliberately not integer-like: numeric
lowering still requires an exact scalar fact. Host argument and result guards use
the same member checks, and a failed annotation assumption executes the generic
interpreter without changing Python-visible semantics.

## Consequences

Functions using `int | None` and `bool | None` parameters or returns can compile
on the first matching call. Multi-path optional returns can eliminate the
duplicate host return guard while preserving exact-PC deoptimization.

This decision does not yet refine the original operand after `is None` or
`is not None`. General unions, float/object optionals, recursive union call
summaries, and branch-local narrowing remain roadmap work.
