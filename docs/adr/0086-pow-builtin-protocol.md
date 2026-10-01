# ADR 0086 — Two and three argument `pow`

## Status

Accepted.

## Context

Tonic already lowered `**` and `**=` through its adaptive binary protocol, but
the global `pow` builtin was absent. Implementing only `pow(base, exp)` as an
alias would omit Python's keyword signature, ternary special-method dispatch,
modular inverse behavior, and bounded modular arithmetic. Materializing
`base ** exp` before applying a modulus would also violate Tonic's allocation
and resource goals for large exponents.

## Decision

`pow` binds `base`, `exp`, and optional `mod` from positional and keyword
arguments. An omitted modulus and an explicit `None` share the existing `**`
protocol. A non-`None` modulus uses the same binary continuation with a closed
`PowMod` completion kind. Candidates receive two arguments, enabling
`__pow__(exp, mod)` and Python 3.14's ternary `__rpow__(base, mod)` behavior.
The established strict-subclass priority, same-class suppression,
`NotImplemented` fallback, descriptor binding, and metaclass dispatch remain
shared with other binary operators.

The native fallback accepts exact integer-compatible storage, including bools
and native int subclasses. It rejects zero modulus and non-integer operands.
Nonnegative exponents use `BigInt::modpow`, which keeps intermediates bounded by
the modulus. Negative exponents first use `BigInt::modinv`; failure to find an
inverse raises `ValueError`. `num-bigint`'s floor-mod convention gives Python's
result interval for both positive and negative modulus. Modulus `1` and `-1`
return zero directly.

## GC and JIT consequences

Each protocol candidate stores at most two logical `Value` arguments. The
continuation traces base, exponent, modulus, pending callable/receiver pairs,
both pending arguments, and its outer completion. No heap address enters a
cache or frame. The builtin introduces no bytecode instruction or native ABI;
Cranelift callers use the existing exact-PC generic call fallback and resume in
their native frame.

## Validation

Runtime tests execute native, user, subclass-reflected, metaclass, keyword,
negative-exponent, negative-modulus, and huge-exponent cases in interpreter and
JIT modes with collection at every allocation. The Python 3.14 differential
corpus checks the same observable results and error classes, including invalid
arity, duplicate or unknown keywords, zero modulus, non-invertible bases, and
non-integer ternary operands. A seeded 1,500-case BigInt oracle is also run in
debug/release interpreter, JIT, and allocation-stress JIT configurations
against Python 3.14.
