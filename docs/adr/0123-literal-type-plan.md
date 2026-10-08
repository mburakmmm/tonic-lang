# ADR 0123: Literal type plans and guarded scalar routing

## Status

Accepted.

## Context

Annotation-driven compilation needs more information than a broad exact type in
some APIs. `typing.Literal` carries a finite set of values, but Python annotations
remain advisory and Python equality alone is insufficient for canonicalization:
`1 == True`, while `Literal[1]` and `Literal[True]` are distinct.

Treating Literal as an ordinary generic class would also give it tuple equality
and hashing, incorrectly merging boolean and integer members. Enforcing the
listed values at calls would change Python language semantics.

## Decision

The runtime exposes `typing.Literal` as a dedicated traced special-form object.
Subscription creates an ordinary opaque GenericAlias representation, while the
Literal path flattens nested aliases and removes duplicates using both value and
literal type. Alias equality and structural hashing use the same canonical,
type-sensitive material, so member order does not affect equality or dictionary
lookup.

TypePlan schema v4 adds canonical Literal members for `None`, booleans, arbitrary
integers, strings and `Ellipsis`. Unsupported runtime members remain representable
in the annotation object but produce the deterministic `unsupported-value`
planning result.

A Literal plan containing only `None`, bool and int members may lower to the
corresponding existing scalar guard category. The guard deliberately checks the
safe scalar category rather than enforcing membership in the finite set. This
lets annotations route suitable code to the typed JIT on its first call while
preserving Python's advisory annotation behavior and generic fallback.

## Consequences

Numeric and optional Literal annotations can use the existing unboxed JIT paths
without making annotations a runtime type system. String and Ellipsis Literal
plans are retained for future narrowing but currently stay outside typed scalar
entry selection. Callable, bytes, enum values, buffers and general union
refinement remain separate roadmap work.
