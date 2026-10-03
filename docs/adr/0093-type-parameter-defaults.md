# ADR 0093 — Python 3.13 type-parameter defaults

## Status

Accepted.

## Context

PEP 696 adds defaults to TypeVar, TypeVarTuple and ParamSpec declarations on
functions, classes and type aliases. Tonic's pinned RustPython 0.4 bootstrap
parser owns Python 3.12 type-parameter syntax but its AST has no
`default_value` field. Replacing Tonic-owned AST/HIR/runtime metadata with
parser-specific types would violate the replaceable parser boundary.

Python evaluates type-parameter bounds and defaults lazily. Tonic already has a
documented eager annotation and bound policy. The roadmap explicitly keeps that
policy until the combined PEP 649/749 deferred-annotation design is selected.

## Decision

The parser adapter recognizes only declaration type-parameter lists, extracts
top-level `=` expressions, and replaces those source bytes with spaces before
calling the pinned parser. Newlines and every byte offset remain unchanged.
Each extracted expression is parsed through the same expression adapter and is
stored on the Tonic-owned `TypeParam` node. A separate bit preserves
`*Ts = *tuple[...]` without introducing a general starred-expression AST node.
The adapter rejects a non-default parameter after a default and rejects a
defaulted TypeVar immediately after TypeVarTuple, following PEP 696 ordering.

HIR resolves bounds and defaults inside the type-parameter lexical environment.
Lowering creates parameters in declaration order, so a default can refer to an
earlier parameter. Bytecode v33 adds `TYPE_PARAM_DEFAULT(parameter, value,
unpacked)` after `TYPE_PARAM`; the verifier checks both registers and the
boolean unpack mode.

Managed TypeVar, TypeVarTuple and ParamSpec objects expose `__default__`.
Missing defaults share one rooted `typing.NoDefault` object. Starred defaults
use a small traced wrapper that preserves Python-compatible formatting plus
`__origin__`, `__args__` and `__unpacked__`. Generic class subscription fills
missing trailing scalar defaults and expands a trailing starred TypeVarTuple
default. Type-alias aliases retain Python's observable rule that `__args__`
contains the explicitly supplied arguments.

## JIT and GC consequences

Type-parameter construction remains outside native code generation and causes
the whole function to use the generic interpreter tier. JIT callers may enter
such code through the existing resumable side exit. Defaults, unpack wrappers,
generic-alias origins/arguments and the `typing.NoDefault` singleton are precise
managed edges and roots. No native pointer or parser-owned node enters bytecode.

## Validation

Compiler tests cover all three parameter kinds, defaults referring to earlier
parameters, starred defaults, ordering failures and malformed expressions.
Verifier and JIT tests cover bytecode v33 operands and explicit generic-tier
fallback. Runtime tests execute metadata access and class default filling under
interpreter and JIT callers with collection after every allocation. The Python
3.14 differential corpus compares `__default__`, `typing.NoDefault` identity,
starred metadata, class aliases and type-alias arguments.

Deferred evaluation remains part of the open PEP 649/749 roadmap item.
