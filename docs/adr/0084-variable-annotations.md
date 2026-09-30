# ADR 0084 — Variable annotations and scope-owned dictionaries

## Status

Accepted.

## Context

Tonic already retained function parameter and return annotations, but rejected
annotated assignments. Supporting the syntax requires three different runtime
behaviors: module and class names contribute to an annotation dictionary,
function-local names participate in lexical binding without evaluating their
annotation, and attribute/subscript targets preserve ordinary target side
effects without recording metadata.

## Decision

The Tonic-owned AST represents an annotated assignment with its assignment
target, annotation expression, optional value and Python parser `simple` bit.
HIR treats every name target as a binding. A simple annotation in a function
therefore makes the name local even when no value is assigned, so a later read
raises `UnboundLocalError`. Function-local annotation expressions are not
evaluated. Annotating a global or nonlocal name from a function or class is a
compile-time `SyntaxError`.

A module or class code object containing any reachable syntactic simple
annotation initializes one managed `__annotations__` dictionary in its
prologue. The scan descends through control-flow suites but stops at nested
function and class scopes. Executing a simple annotation evaluates and stores
an optional right-hand side first, then eagerly evaluates the annotation in the
defining scope and writes it under the source name. This preserves Tonic's
existing eager annotation policy from ADR 0079. It is an intentional initial
semantic choice; Python 3.14's deferred annotation machinery is not copied into
the runtime fast path.

Parenthesized names and attribute/subscript annotations are non-simple. They do
not write annotation metadata or evaluate the annotation expression. Their
target expressions are still evaluated once when no value is present; when a
value is present, ordinary assignment ordering evaluates the right-hand side
before the target owner and key.

The dictionary uses the existing managed dict implementation and `SET_ITEM`
path. Class dictionaries remain normal class attributes and can be reached as
`Class.__annotations__`; module dictionaries are normal versioned globals. No
new bytecode opcode, object layout or native pointer is introduced.

## JIT and GC consequences

Annotation setup and mutation use already verified generic bytecode operations,
so unsupported native lowering falls through the existing exact-PC interpreter
continuation. Managed dictionaries, keys and values are traced through module
or class ownership, and every mutation uses the dict write barrier. Annotation
storage does not alter call ABI or JIT guards.

## Validation

Compiler tests cover the owned AST, module/class dictionary lowering,
function-local suppression, and global/nonlocal rejection. Runtime tests cover
execution order, annotation-only bindings, dead control-flow declarations,
complex target evaluation, class-held annotation values, interpreter/JIT
callers and collection at every allocation. Differential cases compare the
shared class, local-binding and complex-target behavior with Python 3.14.

Type parameters, type aliases and deferred annotation thunks remain separate
roadmap work.
