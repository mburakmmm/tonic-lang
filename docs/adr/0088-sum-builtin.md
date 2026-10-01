# ADR 0088 — Streaming `sum` builtin

## Status

Accepted.

## Context

Tonic had generic iteration and complete binary `+` protocol dispatch, but no
global `sum`. Collecting an iterable into a temporary list before adding would
change the ordering of generator effects and addition hooks, retain every item,
and violate the runtime's no-allocation common-path goal. A plain left-to-right
float loop would also differ observably from Python 3.12 and later, whose exact
float path uses compensated summation.

## Decision

`sum(iterable, /, start=0)` binds one positional-only iterable and an optional
positional or keyword start value. Iterator acquisition occurs before the
string-start rejection, matching Python's observable ordering. Each item is
consumed and added immediately; no argument tuple, keyword dictionary, iterator
wrapper, or item collection is materialized by the builtin.

The generic path reuses Tonic's binary Add protocol, including strict-subclass
reflected priority, `NotImplemented`, metaclass lookup, and ordinary resumable
guest frames. Only `StopIteration` escaping the iterator's `__next__` boundary
ends the reduction. An exception from `__add__` or `__radd__`, including
`StopIteration`, propagates normally.

Exact integers use an unboxed checked `i64` accumulator while possible. On
overflow Tonic materializes a managed arbitrary-precision integer and stays on
the generic path. Exact-float accumulation implements Python 3.14's improved
Kahan-Babuška/Neumaier pair of high and correction terms. Exact integers and
bools may feed that path; an unsupported value materializes the compensated
result and permanently returns to generic addition. Native subclasses retain
their protocol behavior at the same boundaries as Python.

## GC and JIT consequences

The continuation stores the logical iterator plus either an unboxed integer,
two unboxed floats, or one managed generic total. Tracing visits only managed
variants, so moving collection can run while `__iter__`, `__next__`, or an
addition hook is suspended. Persistent state contains no heap address. A JIT
caller exits through the existing generic call boundary and resumes at its
exact caller PC; no bytecode or runtime-helper ABI change is required.

## Validation and measurement

Runtime tests execute native containers, generators, custom iterators,
suspending direct/reflected addition, strict-subclass dispatch, BigInt overflow,
list accumulation, exception boundaries, keyword binding, and evaluation order
under interpreter and JIT callers with collection after every allocation. The
differential corpus adds 160 seeded high-dynamic-range numeric sequences and
compares results and error classes with Python 3.14.6. Interpreter and
Tonic-versus-Python benchmark manifests include 1,000-element integer and float
reductions repeated 100 times.
