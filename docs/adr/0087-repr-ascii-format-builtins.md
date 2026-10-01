# ADR 0087 — `repr`, `ascii`, and `format` builtins

## Status

Accepted.

## Context

Tonic's f-string bytecode already supported suspending `__str__`, `__repr__`,
and `__format__` calls, return-type validation, Unicode ASCII escaping, and the
native format mini-language. The equivalent global builtins were missing. A
separate builtin implementation would duplicate protocol ordering and risk
observable differences between `repr(value)`, `ascii(value)`, `format(value,
spec)`, and their f-string forms.

Class objects also require special methods from their metaclass. The original
f-string path used ordinary instance lookup and therefore skipped metaclass
`__repr__` and `__format__` overrides.

## Decision

`repr` and `ascii` accept exactly one positional-only argument. Both invoke
`__repr__`; `ascii` then escapes every non-ASCII code point in the returned
string. `format` accepts one or two positional-only arguments, supplies an
empty managed string when the spec is omitted, requires string-compatible spec
storage, and invokes `__format__(spec)`. Native values without a user hook use
the existing representation and format-spec engines.

The builtin and f-string opcode paths share two VM helpers. Their special-method
resolver dispatches class values through the metaclass and all other managed
values through their runtime class. Guest hooks run in ordinary resumable
frames and their results pass through one string-result validator.

## GC and JIT consequences

The value and format spec stay in caller registers until invocation and in
callee registers while a guest hook is suspended. Return actions contain no
raw pointer or additional managed edge. No bytecode or runtime-helper ABI was
added; a JIT caller exits at the existing generic call boundary and resumes at
the exact native caller PC.

## Validation

Runtime tests cover user and metaclass hooks, Unicode and nested-container ASCII
escaping, native integer/float/string specs, f-string parity, bad hook returns,
invalid specs, arity, and positional-only keyword rejection under interpreter
and JIT execution with collection at every allocation. The differential corpus
compares the same results and exception classes with Python 3.14.
