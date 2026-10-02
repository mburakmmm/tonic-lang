# ADR 0091 — Callable/sentinel iterators

## Status

Accepted.

## Context

Python's two-argument `iter(callable, sentinel)` is a stateful iterator protocol,
not an eager collection helper. Each `next` calls the zero-argument callable,
compares the sentinel with the returned value through rich equality and truth
conversion, yields unequal values, and becomes permanently exhausted after an
equal value or a `StopIteration` raised by the callable.

The callable and equality hooks may execute arbitrary Tonic code. A synchronous
native-only shortcut would therefore reject valid functions and protocol hooks;
placing the iterator in the ordinary native `Heap::next` path would also make a
suspending guest call impossible.

## Decision

Tonic represents this iterator as a managed `CallIterator` containing its hidden
`callable_iterator` class, callable, sentinel, and exhaustion bit. The hidden
class exposes normal bound `__iter__` and `__next__` builtin descriptors, so all
existing consumers use the same iterator protocol boundary. `__iter__` returns
the object itself.

`__next__` uses the VM's bounded native-callback reentry path to finish one
zero-argument callable invocation and one sentinel comparison before returning.
The reentry depth is discarded before the next item is requested, so a long scan
does not grow the Rust stack. Equality uses the same suspending direct/reflected
binary protocol and truth pipeline as `==`. CPython's observable operand order is
preserved: the sentinel is the left operand and the returned item is the right
operand.

An equal result marks the iterator exhausted and raises a fresh empty
`StopIteration`. A `StopIteration` raised by the callable does the same and is
normalized to an empty exhaustion exception. Other callable errors propagate.
Errors from equality or truth propagate without exhausting the iterator. Every
consumer clears a caught pending `StopIteration` at its iterator boundary.

## GC and JIT consequences

The iterator traces its class, callable, and sentinel. The iterator is an
explicit VM root during reentry; the returned item is rooted before equality can
reach a safepoint. Persistent caches contain only logical `Value` handles.

JIT callers keep the existing generic builtin/iterator fallback. Guest callable,
equality, and truth functions may themselves tier into Cranelift during bounded
reentry, while the caller resumes at its exact bytecode location afterward.

## Validation and measurement

Interpreter and JIT tests under collection after every allocation cover manual
`next`, `for`, list/tuple collection, `sum`, `any`/`all`, `min`/`max`, custom
equality/truth hooks, permanent exhaustion, callable-raised `StopIteration`,
error propagation, and a 5,000-item scan. The Python 3.14.6 differential corpus
checks the same observable behavior. The permanent comparison workload performs
100,000 callable items; raw results are stored under
`docs/benchmarks/callable-iterator-stage-0091`.
