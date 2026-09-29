# ADR 0083 — F-strings and formatting

## Status

Accepted.

## Context

Python f-strings combine expression evaluation, optional string/repr/ASCII
conversion, recursively evaluated format specifications and the `__format__`
protocol. Lowering them to global `str`, `repr` or `format` calls would make
their behavior depend on rebinding those builtin names. Formatting directly in
the compiler would skip user protocols and could reorder observable effects.

## Decision

Tonic owns `JoinedString`, `FormattedValue` and `FormatConversion` in its AST.
The compiler evaluates every joined part from left to right. A formatted part
evaluates its value once, applies an optional `CONVERT`, evaluates its nested
format specification, then executes `FORMAT_VALUE`. Joined parts use ordinary
string addition, so no temporary guest tuple or keyword dictionary is created.

Bytecode v30 assigns explicit wire opcodes to `CONVERT` and `FORMAT_VALUE`.
Both verifiers validate register operands and the closed conversion-mode range.
The opcodes remain generic-runtime boundaries for Cranelift; a JIT caller can
enter a format-containing interpreted frame without changing behavior.

`!s`, `!r` and `!a` use special-method lookup for user `__str__` and `__repr__`.
`!a` escapes non-ASCII code points after repr conversion. `FORMAT_VALUE` invokes
user `__format__` with the fully evaluated string spec. These calls use normal
Tonic frames and return actions, so they may suspend through nested guest calls.
The continuation requires a string result and retains all values as precise GC
roots. Missing user methods use the native value formatter.

The native formatter parses fill/alignment, sign, negative-zero coercion,
alternate form, zero padding, width, grouping, precision and presentation type.
It supports Unicode string width/precision, integer `b/o/d/x/X/c`, and numeric
`e/E/f/F/g/G/%/n` presentation. General formatting uses significant-digit
rounding and normalized signed exponents. Tonic intentionally keeps locale
selection outside this runtime layer; `n` is deterministic until locale support
is introduced as part of the standard library.

## Consequences

Formatting no longer depends on mutable global builtin names. User protocol
errors and non-string returns propagate at the original f-string span. Native
formatting allocates only the resulting string and intermediate joined results;
it does not allocate argument containers. A future string-builder
superinstruction may remove repeated concatenation if benchmarks show a useful
gain, without changing the AST or protocol boundary.

## Validation

Compiler tests inspect the owned tree and emitted opcode family. Core and public
JIT tests cover operand validation and generic fallback. Runtime tests cover
evaluation order, nested specs, conversions, Unicode formatting, user methods,
return validation and collection after every allocation in interpreter and JIT
caller modes. The CPython 3.14.6 differential suite covers output and exception
kinds. Additional deterministic grids compare 216 mixed string/int/float cases
and 2,684 seeded numeric value/spec combinations with no observable mismatch.
