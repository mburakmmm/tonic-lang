# ADR 0146: Refine `int | float` into native numeric branches

## Status

Accepted.

## Context

`int | float` spans two runtime representations: small integers are immediate
tagged words, while floats are movable heap objects whose payload can live in a
JIT F64 stack slot. Treating the union itself as one unboxed machine type would
either lose Python value identity or require an implicit coercion that changes
semantics.

## Decision

Add `IntOrFloat` to the canonical annotation scalar, public typed signature,
and verified scalar lattice. Entry and arbitrary-PC guards accept an exact
small-int tag or a value confirmed by the runtime's exact-float unbox helper.
The logical value stays rooted while it is still a union.

An identity-guarded exact builtin `isinstance(value, float)` call refines the
true edge to `Float` and the false edge to `Int`. The corresponding exact
`isinstance(value, int)` test refines to `Int` and `Float`. The float edge uses
the existing guarded branch-edge F64 initialization; the integer edge decodes
the immediate value. Native arithmetic and return lowering then operate on the
representation proven for that control-flow path.

Guarded direct-call summaries may carry `IntOrFloat` across a generic CALL
boundary. Successor-PC re-entry validates the materialized result before its
fact is used. A result outside the annotation deoptimizes without imposing a
runtime type check.

Control-flow that merges an unboxed float back into an unresolved mixed union
continues to reject typed-float lowering until explicit edge materialization is
available.

## Consequences

Python-compatible `int | float` annotations can select native i64 and F64 paths
inside one function without changing advisory semantics. Builtin/type rebinding
invalidates the refinement through existing logical-handle resume guards.
Booleans, big integers outside the immediate range, foreign objects, and other
nonmatching inputs execute through the generic runtime.

Broader numeric unions, implicit mixed arithmetic without a narrowing branch,
and materialized union phis remain future work.
