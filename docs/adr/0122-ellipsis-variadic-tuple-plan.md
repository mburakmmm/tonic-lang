# ADR 0122: Ellipsis and variadic tuple plans

## Status

Accepted.

## Context

Python expresses a homogeneous variadic tuple annotation as `tuple[T, ...]`.
Tonic previously accepted fixed tuple aliases but did not own the `Ellipsis`
literal or singleton, so it could neither preserve the syntax nor distinguish a
variadic tuple from a fixed tuple in its canonical type plan.

Treating the second alias argument as punctuation only would make parser and
runtime behavior diverge from Python and would leave no ordinary value for
`Ellipsis`, `type(...)`, hashing or reflection.

## Decision

`Ellipsis` is a dedicated immediate `Value`, alongside `None`, booleans and
`NotImplemented`. The parser and bytecode constant pool preserve `...` as a
Tonic-owned constant. The runtime exposes the `Ellipsis` builtin and
`types.EllipsisType`, with singleton identity, representation, truth and hash
behavior available through the normal object protocols.

TypePlan schema v3 maps exactly two tuple alias arguments whose second member is
`Ellipsis` to `VariadicTuple(item)`. Other tuple aliases containing Ellipsis are
rejected with `invalid-generic-arity`. The canonical encoding gives variadic
tuples a distinct tag, so they cannot collide with fixed tuple plans.

Cranelift may materialize the immediate constant directly. It does not infer a
numeric scalar fact from Ellipsis, so unsupported typed operations retain their
ordinary generic fallback.

## Consequences

Tonic now preserves Python's `...` runtime behavior and can carry
`tuple[T, ...]` through annotation planning without enforcing annotations at
runtime. Typed tuple storage, length guards, element unboxing and container
loop lowering remain later optimization work.
