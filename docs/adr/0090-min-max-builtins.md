# ADR 0090 — Streaming `min` and `max` builtins

## Status

Accepted.

## Context

`min` and `max` combine generic iteration, arbitrary callables, rich comparison,
and truth conversion. Materializing an iterable or eagerly computing all keys
would retain values, reorder side effects, and violate Tonic's streaming design.

## Decision

Tonic supports both the single-iterable and two-or-more positional forms.
`key` and `default` are keyword-only; `default` is valid only for the iterable
form and is returned without calling `key` when the iterable is empty. `key=None`
uses each item directly. Strict `<` or `>` preserves the first item on equal keys.

One continuation owns the iterator or compact host positional slice, optional
key callable, default, current item, and current best item/key. `__iter__`,
`__next__`, the key callable, rich comparison, and comparison-result truth
conversion may all suspend. Only `StopIteration` escaping `__next__` ends the
scan; the same exception from key/comparison/truth propagates. No guest list,
tuple, keyword dict, or per-item managed object is created by the builtin.

## GC and JIT consequences

Every managed continuation value is traced through frame actions, binary
completion, and truth completion. State contains logical `Value` handles rather
than heap addresses. JIT callers use the generic builtin side exit and resume at
the exact caller PC after any suspended protocol.

## Validation and measurement

Tests cover iterable/variadic calls, empty/default behavior, key order, stable
ties, generators, custom iterators, suspending comparisons and truth, exception
boundaries, JIT callers, and collection after every allocation. Python 3.14.6
differential tests cover the same outputs and ten new error cases. The permanent
benchmark scans 200,000 integer elements; raw data is under
`docs/benchmarks/min-max-stage-0090`.
