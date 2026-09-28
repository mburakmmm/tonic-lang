# ADR 0080 — Async comprehension execution

## Status

Accepted.

## Context

Tonic already had coroutine, async-iteration, async-generator, and hidden
comprehension-scope machinery, but the parser rejected every asynchronous
comprehension. Treating an eager async comprehension as an ordinary function
would expose its intermediate awaitables, while delaying the outermost
`iter()`/`aiter()` call would differ from Python's observable construction
order.

## Decision

Each Tonic-owned comprehension clause records whether it uses `for` or `async
for`. The comprehension also records whether its hidden code suspends because
of an async clause or an `await` in its element, value, filter, or later
iterable. Scope resolution marks this hidden code as a coroutine without
leaking comprehension targets into the enclosing scope.

The outermost iterable is evaluated in the enclosing scope and immediately
converted with `ITER` or `GET_AITER`. The hidden function receives that iterator
as its first positional argument. Later async clauses use `GET_AITER`; every
async clause advances through `GET_ANEXT`, the existing await state machine,
and an exception region ending in `END_ASYNC_FOR`.

An eager list or dict comprehension calls its hidden coroutine and implicitly
awaits the result. An async generator expression calls hidden code marked as
both coroutine and generator, emits `ASYNC_YIELD`, and returns the lazy async
generator object without awaiting it. This also permits module-level async
generator expressions. `GET_AITER` is therefore verifier-safe in non-coroutine
code, while `GET_ANEXT`, awaiting, and async-loop exhaustion remain restricted
to coroutine code.

Set comprehensions remain deferred until Tonic has a native set representation
and its hashing, equality, iteration, and mutation protocols.

## JIT and GC consequences

Suspending comprehension code uses interpreter frames and the existing precise
coroutine/async-generator roots. JIT callers side-exit through the normal call
and await boundaries; no new native ABI or raw heap address is introduced. The
outer iterator argument and accumulator stay in verified VM registers across
suspension and collection.

## Validation

Compiler tests cover async clauses, await-only comprehensions, hidden code
metadata, `GET_ANEXT`, and `ASYNC_YIELD`. Verifier tests allow eager
`GET_AITER` outside coroutine code but continue to reject `GET_ANEXT` there.
Runtime and Python differential tests cover list/dict results, mixed clauses,
outer `__aiter__` timing, module-level async generator expressions, JIT callers,
and collection after every allocation.
