# ADR 0089 — Streaming `any` and `all` builtins

## Status

Accepted.

## Context

Tonic already had short-circuit truth evaluation and generic iteration, but no
global `any` or `all`. Implementing either builtin by first collecting an
iterable would retain all yielded objects, run past a decisive item, and change
the ordering of generator and truth-protocol side effects. Native-container-only
loops would preserve speed but reject the general iterator protocol required by
Python source compatibility.

## Decision

`any(iterable, /)` and `all(iterable, /)` share one streaming boolean-reduction
continuation. The continuation stores the logical iterator and the target truth
value. `any` returns at the first true item and returns false on exhaustion;
`all` returns at the first false item and returns true on exhaustion. No item
collection, positional tuple, keyword dictionary, or wrapper iterator is
created by the builtins.

Iterator acquisition and `__next__` may suspend in guest frames. Item truth
evaluation uses the existing `__bool__`, `__len__`, and `__index__` continuation
chain and therefore preserves return-type and negative-length validation.
Exact native values bypass special-method lookup and use the compact value or
native object representation directly. Native subclasses and user objects stay
on the protocol path.

Only `StopIteration` escaping an iterator's `__next__` boundary means normal
exhaustion. A `StopIteration` raised by `__bool__`, `__len__`, or `__index__`
propagates like any other truth-evaluation exception. Short-circuiting stops
before later `__next__` calls and later truth hooks.

## GC and JIT consequences

Suspended state contains one movable-GC-safe iterator `Value` and one boolean.
Both iterator and truth continuations trace that iterator; no native object
address is cached. A JIT caller uses the existing generic builtin boundary and
resumes at the exact caller PC after a suspended iterator or truth hook.

## Validation and measurement

Runtime tests cover empty/native iterables, generators, custom iterators,
short-circuit side-effect order, suspending `__bool__`/`__len__`/`__index__`,
large scans, exception boundaries, argument binding, JIT callers, and collection
after every allocation. The differential corpus compares the same observable
behavior and ten failure classes with Python 3.14.6.

The permanent benchmark scans two 1,000-element boolean lists 100 times. The
native builtin path is compared with equivalent guest loops and with CPython;
the raw samples and environment limits are recorded under
`docs/benchmarks/any-all-stage-0089`.
