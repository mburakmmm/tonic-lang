# ADR 0118: Int-bool immediate union JIT

## Status

Accepted.

## Context

The first union JIT slice covered optionals whose members were a scalar and
`None`. A useful next case is `int | bool`: both values are immediate in Tonic,
and Python defines booleans as integer-compatible values for arithmetic. A
single tag assumption would reject one valid member, while treating the union as
unknown would retain redundant runtime checks at every operation.

## Decision

The public typed signature, runtime annotation summary and scalar lattice add an
`IntOrBool` fact. Canonical TypePlan members `bool` and `int` map to that fact.
Entry and arbitrary-PC guards accept either an exact immediate integer or exact
boolean. Host guards use the same rule and preserve advisory fallback for every
other value.

`IntOrBool` is integer-like. Native decode selects `False=0` and `True=1` for
boolean tags and otherwise decodes the immediate integer payload. Truth testing
uses the same guarded distinction. Arithmetic and comparisons may therefore use
the existing integer lowering without allocation. Merging `Int` and `Bool`
facts produces `IntOrBool`, and return proof accepts either member or the merged
fact.

## Consequences

Annotated `int | bool` numeric code can compile on its first matching call and
elide repeated integer-tag checks while retaining Python numeric behavior. A
float or other non-member executes the generic path without a new type error.

This decision does not imply that arbitrary unions are numeric. Mixed float,
heap object, user-class and container unions require representation-specific
guards, deoptimization metadata and, where applicable, branch refinement.
