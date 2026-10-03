# ADR 0094 — Eager annotation semantics for Tonic 0.x

## Status

Accepted.

## Context

Python 3.14 implements PEP 649 and PEP 749. Function, class and module
annotations are represented by deferred `__annotate__` functions; the
`annotationlib` API can request value, forward-reference, string and
fake-globals formats. PEP 749 extends the same deferred machinery to PEP 695
bounds and PEP 696 type-parameter defaults.

Tonic already has one coherent eager model. Function annotations are evaluated
when the function object is created, module and class annotations are evaluated
when their statement runs, and type-parameter bounds/defaults are evaluated in
declaration order. Values are stored in managed dictionaries or metadata and
traced precisely. This behavior is observable when an annotation has side
effects or contains a forward reference.

Adopting the CPython mechanism now would require hidden annotation code objects,
lazy owner descriptors, multiple result formats, ForwardRef objects,
fake-global namespaces, partial-module caching rules, metaclass integration and
new suspended roots. Adding only a `__annotate__` name without those semantics
would create a misleading compatibility claim.

## Decision

Tonic 0.x retains eager annotation, bound and type-parameter-default evaluation
as a documented language-level semantic choice. It does not expose
`__annotate__`, `annotationlib` formats or fake-globals execution. The parser
continues to accept mainstream annotation syntax, and `__annotations__`,
`__type_params__`, `__bound__` and `__default__` expose the eager managed
values.

This is a compatibility difference from Python 3.14, not an accidental missing
fast path. A future compatibility layer may provide deferred thunks behind an
explicit mode, but such a mode must implement the complete owner, cache,
format, ForwardRef, metaclass and GC contract before it is advertised.

## Runtime and JIT consequences

No hidden frame or code object is retained solely for annotation evaluation.
Annotation values use existing object, dictionary and write-barrier paths.
Function-call ABI, exact-callee guards, class versions and deoptimization maps
remain independent of annotation evaluation. Errors occur at the definition or
executed annotation statement, where Tonic already preserves source spans.

## Validation

Existing compiler and runtime suites verify evaluation order for defaults,
function annotations, variable annotations, class metadata, type-parameter
bounds and defaults. The tests run through interpreter and JIT callers with
collection after every allocation. Differential cases are limited to the
side-effect-free observable subset shared with Python 3.14; documentation does
not claim `__annotate__` or `annotationlib` compatibility.
