# ADR 0085 — Type parameters, type aliases and generic aliases

## Status

Accepted.

## Context

The parser recognized Python 3.12 type-parameter and `type` alias syntax, but
the adapter rejected it. Merely accepting the grammar would leave annotations,
function and class bodies, nested closures, and runtime introspection with
incompatible name resolution. Copying CPython's typing objects or annotation
scope frames would also couple the Tonic object model to CPython internals.

## Decision

The Tonic-owned AST represents `TypeVar`, `TypeVarTuple` and `ParamSpec`
parameters, optional TypeVar bounds, and `type` alias statements. Duplicate
type parameter names are rejected before lowering. HIR records every declared
parameter and identifies the parameters that are not shadowed by an ordinary
value parameter, assignment, global or nonlocal declaration. Active parameters
receive initialized hidden local slots; they become ordinary precise cells when
a nested function or method captures them. Normal Python whole-scope shadowing
therefore still produces `UnboundLocalError` in functions and `NameError` in
class bodies.

Definition lowering evaluates decorators and ordinary defaults outside the
type-parameter environment. It then creates managed type-parameter values,
temporarily resolves bounds, bases and annotations through those values, and
stores the tuple on generic functions and classes as `__type_params__`.
Function-site metadata carries only verified source registers. Code metadata
maps declared names to optional initialized local slots, so a parameter with the
same name can shadow the body binding while the declared type parameter remains
introspectable.

Bytecode v31 adds `TYPE_PARAM` and `TYPE_ALIAS`. `TYPE_PARAM` constructs one of
the three managed parameter kinds; `TYPE_ALIAS` combines a source name, a
managed parameter tuple and the evaluated alias value. Alias objects expose
`__name__`, `__type_params__` and `__value__`; parameters expose `__name__`,
`__bound__` and an empty `__constraints__` tuple. Tonic currently applies its
documented eager annotation policy to bounds and alias values. Deferred thunks
can be introduced later without changing the public `Value` ABI.

Subscription of builtin container classes, PEP 695 generic classes and generic
type aliases creates a managed generic-alias object with `__origin__` and
`__args__`. A class generic alias delegates construction to its origin. Generic
aliases in a base list resolve to their class origin for MRO construction. This
keeps type metadata out of instance layout and the ordinary class fast path.

## JIT and GC consequences

Functions carrying type-parameter metadata are excluded from direct-call
inlining until native entry setup can materialize their hidden slots. Ordinary
calls initialize the slots before cell creation, so interpreted, resumable and
baseline-JIT execution see identical values. `TYPE_PARAM` and `TYPE_ALIAS`
remain explicit generic-runtime fallback operations in Cranelift.

Function/class tuples, parameter bounds, alias values, generic origins and
generic arguments are all precise managed edges. Construction and class
attribute publication use existing safepoints and write barriers; no native
address is embedded in bytecode or metadata.

## Validation

Compiler tests cover all three parameter kinds, bounds, generic functions,
classes and aliases. Verifier tests reject bad kinds, symbols, registers, slots
and function-site counts. Runtime tests exercise annotations, body visibility,
nested cells, shadowing, generic bases, builtin/class/type-alias subscription,
introspection and construction under interpreter/JIT callers with collection at
every allocation. Differential cases compare the shared Python 3.14 behavior.

Python 3.13 type-parameter defaults and CPython 3.14 deferred annotation thunk
internals are not part of this initial runtime contract.
