# ADR 0142: Add immutable bytes runtime values and TypePlan v8

## Status

Accepted.

## Context

The annotation roadmap listed `bytes` as an open exact builtin, but a canonical
metadata name without a real guest value would create a false optimization
contract. Byte literals must retain arbitrary octets instead of passing through
UTF-8 text, and the runtime needs coherent identity, iteration, sequence, hash,
dictionary-key, GC, and annotation behavior before `bytes` can enter TypePlan.

## Decision

Tonic owns byte literals as `Constant::Bytes(Vec<u8>)` from AST through verified
bytecode. Materialization creates an immutable `Object::Bytes(Vec<u8>)` managed
by the normal handle heap. The object has a distinct builtin `bytes` class and
constructor. Construction supports an empty value, a non-negative integer
count, another bytes value, and arbitrary iterables of integers in `0..=255`;
the iterable path uses the existing resumable collection machinery so guest
iterators and generators retain their observable behavior.

Bytes expose Python-style truth, repr, length, integer index and iteration,
slice, concatenation, repetition, lexicographic comparison, integer or bytes
subsequence membership, and content hashing. Dictionary keys own a byte copy,
so moving collection and handle reuse cannot affect lookup identity. Persistent
caches never retain a raw `Vec` address.

TypePlan schema v8 adds `ExactTypePlan::Bytes` and
`LiteralTypePlan::Bytes(Vec<u8>)`. Canonical encoding includes the byte length
and exact octets and remains type-sensitive, so a byte literal cannot collide
with a text literal. Duplicate byte literals in `typing.Literal` collapse by
value like other same-type literals.

## Consequences

Annotations using exact `bytes` or bytes Literals now produce stable plans and
deterministic hashes. Bytes remain heap-backed managed values; schema v8 does
not classify them as an immediate scalar or promise unboxed Cranelift lowering.
Annotation behavior is advisory, and unsupported native optimization continues
through the generic interpreter/JIT boundary without changing program results.

The initial constructor deliberately leaves the two-argument text-plus-encoding
form and mutable `bytearray` for later standard-library/runtime coverage. Those
gaps do not weaken the exact representation or TypePlan contract introduced
here.
