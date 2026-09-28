# ADR 0082 — Structural pattern matching

## Status

Accepted.

## Context

Python structural matching combines control flow with equality, identity,
sequence extraction, hashed mapping lookup, class hierarchy checks, descriptors
and conditional name binding. Lowering it through ordinary indexing alone would
turn normal non-matches into exceptions and would duplicate runtime protocol
logic. Committing capture names while a nested pattern is still failing would
also expose partially matched state to later alternatives.

## Decision

Tonic owns `MatchCase`, `Pattern` and every Python pattern form in its AST. The
compiler evaluates the subject once and lowers each case to guarded register
control flow. Captures remain staged in temporary registers until their whole
pattern succeeds. OR alternatives must produce the same names and merge them
into common registers. A false guard advances to the next case after preserving
the completed captures, matching Python's observable behavior.

Bytecode v29 adds explicit verified generic-runtime boundaries for sequence and
mapping guards, optional hashed key reads, class checks, optional dynamic
attributes, `__match_args__`, positional class extraction and collision-aware
duplicate detection. Fixed and starred sequence patterns materialize native
list/tuple/range storage, including native subclasses, and exclude strings.
Mapping lookup shares dict's
suspending `__hash__`/`__eq__` machinery; `**rest` reuses `DICT_MERGE` and
`DEL_ITEM`. Dynamic duplicate mapping keys raise `ValueError` only after the
mapping-size guard.

Class patterns use the runtime's C3 instance check. Keyword and positional
attributes share the existing descriptor-aware attribute continuation. A fresh
managed sentinel converts only a final missing-attribute result into pattern
failure; descriptor and user-code exceptions still propagate. Positional forms
validate `__match_args__`, reject excess or non-string descriptors, detect
duplicate positional/keyword attributes in evaluation order, and support the
single self-pattern of Tonic's corresponding builtin types.

## JIT and GC consequences

Pattern temporaries, materialized sequence views, rest mappings, duplicate sets,
sentinels and suspended protocol states are ordinary precise managed roots. The
new opcodes are accepted by both bytecode verifiers and remain explicit
unsupported boundaries for Cranelift, so a caller may be JIT-compiled while the
match-containing function runs in the adaptive interpreter without semantic
drift.

## Validation

Compiler tests inspect the owned pattern tree and emitted opcode family. Core
and public JIT tests validate operands and generic fallback. Runtime tests run
value/singleton, capture, OR, guard, nested/starred sequence, mapping/rest,
custom hash/equality, class inheritance, descriptors, `__match_args__`, builtin
self-pattern and validation failures under interpreter/JIT caller modes with a
collection after every allocation. Python differential cases compare the same
observable output and exception kinds against Python 3.14.6.
