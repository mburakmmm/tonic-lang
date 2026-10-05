# ADR 0107: Typed scalar bool lattice

## Status

Accepted.

## Context

The first annotation-guided data-flow slice tracked exact small integers with a
boolean bit per virtual register. That representation could not distinguish an
unknown value from exact `bool` or `float`, and accepting `bool` annotations
without a typed model would either lose optimization or incorrectly treat the
distinct immediate boolean encoding as an integer tag.

Python also defines `bool` as an `int` subtype for numeric operations while
preserving exact boolean identity and representation. Tonic must retain that
observable behavior without weakening annotation guards or exposing its value
layout.

## Decision

The verified-bytecode overlay uses a four-state scalar fact:

- `Unknown`
- exact small `Int`
- exact heap `Float`
- exact immediate `Bool`

Facts propagate through parameters, constants, moves, numeric unary and binary
operations, `not`, comparisons, identity comparisons, branches, and control-flow
joins. A join retains a fact only when every incoming edge agrees.

Typed entry validates every integer or boolean register required by the selected
bytecode PC. This applies to normal entry and arbitrary-PC resume. Float values
continue to use the existing unbox helper, F64 stack slots, deoptimization maps,
and materialization path.

When the overlay proves an operand is boolean, generated integer arithmetic
maps `False` to zero and `True` to one before the operation. The result is an
exact tagged integer, matching Python. Integer/boolean comparisons use the same
numeric mapping. Proven integer or boolean truth tests do not repeat generic
immediate-value guards. Overflow, division by zero, a malformed resume state, or
an annotation mismatch still deoptimizes at the exact bytecode PC.

Exact `bool` annotations are advisory. A non-boolean argument skips the typed
entry and executes ordinary dynamic semantics; annotations do not introduce a
new runtime `TypeError`.

## Consequences

Boolean-heavy annotated control flow can compile on its first valid call and
avoids repeated tag checks. The public JIT metadata and VM statistics report
boolean guard eliminations separately from integer eliminations.

Values are still tagged in the precise root buffer. This decision does not add
an unboxed boolean call ABI, annotated call-result propagation, or a direct
callee summary; those remain separate roadmap items.
