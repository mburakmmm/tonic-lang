# ADR 0124: Callable signature type plans

## Status

Accepted.

## Context

Partial static compilation needs callable signature information to reason about
indirect calls and higher-order functions. Treating `typing.Callable` as an
opaque class loses parameter and return information. Treating its parameter list
as an ordinary runtime list also makes an otherwise immutable annotation
unhashable and unsuitable for canonical cache keys.

Annotations remain advisory. A Callable annotation must not make an arbitrary
object call valid, reject a value earlier than Python would, or imply that the
callable object itself has an unboxed machine representation.

## Decision

The runtime exposes `typing.Callable` as a traced special form. Subscription
normalizes the short `Callable[T, R]` spelling to one positional parameter and
preserves the standard `Callable[[A, B], R]`, `Callable[[], R]` and
`Callable[..., R]` shapes. Reflection returns flattened `__args__`, while repr,
structural equality, hashing and dictionary keys retain the callable signature.

TypePlan schema v5 adds `Callable { parameters, result }`. Parameters are either
`Any` for the ellipsis form or an ordered positional plan list. The result is a
nested TypePlan. Unsupported parameter/result values and malformed arity produce
the existing deterministic rejection codes.

Callable plans are metadata for future guarded call-target analysis. They do not
currently select a scalar JIT ABI or enforce argument types. Native lowering may
use the plan only after proving an exact callable target or installing the
necessary identity, signature and result guards.

## Consequences

Function signatures can now participate in canonical annotation cache keys and
future direct-call graph propagation. ParamSpec, Concatenate, keyword shape,
overload sets and arbitrary protocol-callable objects remain later extensions.
