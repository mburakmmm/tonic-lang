# ADR 0079 — Function annotation metadata and managed dictionaries

## Status

Accepted.

## Context

Tonic previously rejected every parameter and return annotation even though the
replaceable Python parser already recognized the syntax. Passing parser-owned
nodes through the runtime would violate the compiler boundary, while attaching
native pointers or unverified register indices to function objects would break
the moving-GC and bytecode-safety invariants.

## Decision

The Tonic AST stores optional annotations on ordinary, positional-only,
keyword-only, `*args`, and `**kwargs` parameters plus the function return node.
HIR visits these expressions in the defining lexical scope. Lowering evaluates
defaults first and then annotations in source parameter order, producing
`FunctionSite.annotations: Vec<(SymbolId, register)>`; the reserved `return`
symbol is the observable return key.

The verifier rejects annotation symbols or registers outside their owning
program/code object and rejects duplicate keys. `FUNCTION` materializes an
insertion-ordered managed dict only when annotation metadata is present. An
unannotated function allocates its empty dict lazily on first
`__annotations__` access, preserving the one-object ordinary-function
allocation path. Bound methods forward this attribute to their underlying
function. The function object traces the optional dictionary, and dict mutation
continues through the existing generational write barrier.

Annotation expressions currently use eager definition-time evaluation. This is
an explicit Tonic semantic choice for the initial surface; deferred annotation
thunks can be added later without exposing parser nodes or changing the public
value ABI.

## JIT and GC consequences

Annotations are construction metadata, not part of a function's call ABI.
Existing exact-callee guards and direct-call plans therefore remain valid.
Compiled callers see the same function identity, while the managed annotation
dict stays reachable through normal object tracing. No heap address is embedded
in bytecode or native code.

## Validation

Compiler tests cover every parameter category and return metadata. Verifier
tests reject out-of-range symbols/registers and duplicate keys. Runtime tests
read the ordered dictionary, call the annotated function through interpreter
and JIT modes, and retain a class whose only remaining edge is an annotation
value under collection at every allocation. A CPython differential case covers
the shared observable, side-effect-free annotation surface.

Variable annotations and class/module annotation dictionaries are specified by
ADR 0084. Type parameters and type aliases remain separate roadmap work.
