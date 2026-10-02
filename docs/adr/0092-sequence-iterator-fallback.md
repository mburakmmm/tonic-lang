# ADR 0092 — Sequence iteration through `__getitem__`

## Status

Accepted.

## Context

Python keeps a legacy sequence protocol alongside `__iter__`: when a type has no
`__iter__` method but does have `__getitem__`, iteration requests consecutive
integer indices starting at zero. `IndexError` or `StopIteration` ends the
sequence. An explicit `__iter__`, including `__iter__ = None`, suppresses this
fallback.

The `__getitem__` method may be a normal, static, class, or metaclass method and
may execute arbitrary Tonic code. Capturing a raw function or object address in
the iterator would violate class-rebinding semantics and the movable-heap ABI.

## Decision

Tonic represents fallback iteration with a managed `SequenceIterator` containing
its hidden `iterator` class, source value, next signed index, and exhaustion bit.
Every `__next__` dynamically resolves `__getitem__` through the ordinary
instance/metaclass operator path and invokes it through bounded VM reentry.
Successful lookup advances the index exactly once. `IndexError` and
`StopIteration` permanently exhaust the iterator and become a fresh empty
`StopIteration`; other errors propagate without advancing or exhausting it.

The common iterator-construction helper first uses native iteration, then checks
for an explicit `__iter__`, and only then creates a sequence iterator when
`__getitem__` exists. All iterable consumers use this helper. Iterators returned
by an explicit `__iter__` still require `__next__`; they do not receive a second
sequence fallback.

## GC and JIT consequences

The iterator traces its class and source. It is an explicit root while guest
`__getitem__` code runs. A shared `invoke_iterator_next_with_action` boundary
also traces the consumer's pending `ReturnAction` before invoking `__next__`.
This keeps partially built dicts, collected values, generic numeric totals,
extremum state, membership operands, and expanded arguments alive even when a
managed builtin iterator performs nested guest reentry synchronously.

JIT callers use the existing generic iteration side exit and resume at the exact
bytecode location. The `__getitem__` body may independently tier into Cranelift;
no native code embeds heap addresses or sequence storage.

## Validation and measurement

Interpreter and JIT tests with collection after every allocation cover manual
`next`, `for`, list/tuple/dict collection, unpacking, `*args`, `sum`, `any`/`all`,
`min`/`max`, membership, static/class/metaclass binding, class rebinding,
error retry, permanent exhaustion, and a 5,000-item scan. The Python 3.14.6
differential corpus checks the same observable boundaries.

The 100,000-item interpreter workload completes in 45.273 ms versus 81.849 ms
for an explicit guest `__iter__`/`__next__` control on the measured host. Raw
Tonic and CPython results are stored under
`docs/benchmarks/sequence-iterator-stage-0092`.
