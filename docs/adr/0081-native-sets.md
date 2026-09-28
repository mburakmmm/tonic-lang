# ADR 0081 — Native set storage and comprehension mutation

## Status

Accepted.

## Context

Set literals and set comprehensions require uniqueness under Python-style hash
and equality semantics. Implementing them as lists would make membership linear,
accept unhashable elements, and diverge for user-defined `__hash__`/`__eq__`.
Maintaining a second hash-table engine would duplicate collision, mutation,
suspension, and GC logic already exercised by dictionaries.

## Decision

`Object::Set` owns the same compact insertion-ordered hash table used by native
dictionaries. Set entries retain the managed element as the key and use an
internal `None` value slot that is never exposed. Low-level candidate, version,
entry, and hashed-insert operations accept either container, while public dict
indexing and mutation APIs continue to reject sets.

Bytecode v23 adds `SET(destination)` and `SET_ADD(owner, value)`. Set literals
evaluate and add elements from left to right. Sync and async set comprehensions
reuse hidden comprehension scopes and emit `SET_ADD` at the terminal clause.
The opcode enters the existing suspending hash/equality state machine, so
custom methods, collisions, duplicate elimination, and error ordering match
dictionary-key behavior.

Membership performs a hashed candidate search instead of a linear iterator
scan. Equality first guards size, then searches every left element in the right
set through the same hash/equality path. Iteration retains insertion order as an
implementation detail and checks the storage version for structural mutation;
language semantics do not guarantee that order.

The canonical runtime `set` type identifies literal and comprehension results.
The general `set()` constructor and mutating method surface remain standard
library/builtin follow-up work; empty native sets are already expressible with
an empty set comprehension.

## JIT and GC consequences

Set elements are precise managed edges. Every insertion crosses the shared
write barrier, and pending hash/equality continuations trace the owner and
candidate value across moving collection. `SET` and `SET_ADD` are verified but
remain explicit generic-runtime boundaries for Cranelift.

## Validation

Compiler tests cover owned set AST, hidden sync/async set-comprehension code,
and `SET_ADD` emission. Verifier tests reject malformed constructor/mutation
operands. Runtime tests cover duplicate and collision behavior, custom hash and
equality calls, membership, equality, iteration, empty representation, async
comprehensions, JIT callers, and collection after every allocation. Python
differential cases cover the stable observable surface and unhashable elements.
