# ADR 0136: Generalize native f64 buffer reads to rank N

## Status

Accepted.

## Context

The first multidimensional native tier stored two shape dimensions in a fixed
four-word descriptor and recognized only two-element index tuples. This proved
the owner-root, deoptimization and per-axis range model, but extending the same
layout with a new scalar type and code path for every rank would duplicate the
JIT and stop at an arbitrary dimensionality.

The generic interpreter already defines exact-rank tuple indexing, per-axis
negative-index normalization and row-major contiguous storage semantics. The
native path needs to preserve those observable rules while keeping movable heap
objects out of generated addresses.

## Decision

An exact read-only C-contiguous f64 buffer entry descriptor contains the data
pointer, flat element length, shape pointer and rank. The managed buffer owner
remains in the precise root array. Both pointers refer to boxed non-moving
storage owned by that rooted object and are valid only for the synchronous
compiled invocation. The runtime validates exact buffer identity, read-only
mutability, expected rank and C-contiguous strides before exposing the view.

The typed signature represents this contract as `F64Buffer { rank }`. For rank
one, scalar indexing retains the flat-length path. For rank two and above, the
JIT accepts only an immediately consumed tuple whose arity equals the annotated
rank. It loads each `shape[axis]`, applies Python negative-index normalization
and a bounds guard, then folds indices in row-major order:

```text
flat = index[0]
flat = flat * shape[1] + index[1]
...
flat = flat * shape[N-1] + index[N-1]
```

Any failed axis guard deoptimizes at the tuple-construction PC so generic
execution can materialize the key and raise the normal `IndexError`. A rank or
layout mismatch deoptimizes at entry.

The existing dominance-based induction proof applies independently to every
tuple component. A canonical N-level nested traversal therefore creates one
`bound <= shape[axis]` entry guard per proven axis and omits those axis checks on
normal entry. Arbitrary-PC resume remains checked. Metadata records all rank-N
tuple item sites while retaining the rank-two counter for compatibility and
focused regression checks.

## Consequences

Contiguous tensor-style reads no longer require rank-specific native code.
Rank-three direct and VM tests cover row-major addressing, negative indices,
axis/rank failures and three nested range proofs. Shape metadata does not become
a guest-visible pointer or a persistent cache entry.

Arbitrary strides, writable native stores, mutation/version guards and
alias/escape materialization remain separate work. A future strided descriptor
can extend the call-lifetime metadata without changing guest object identity.
