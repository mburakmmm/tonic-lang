# ADR 0115: PEP 604 union runtime and canonical TypePlan

## Status

Accepted.

## Context

Partial-static annotation analysis needs union and optional plans, but a plan
variant alone cannot represent Python source such as `int | str` or
`list[int] | None`. The annotation expression is evaluated at definition time,
so the runtime object, operator dispatch and introspection are observable before
the JIT reads it. User metaclasses may also override `__or__` or `__ror__`.

Union member order does not affect equality or hashing. Equivalent spelling must
also produce the same persistent TypePlan hash; otherwise cache keys and
invalidation behavior would depend on source ordering.

## Decision

Tonic has a managed `UnionType` object containing a logical class handle and a
flat member vector. `type`, `types.GenericAlias` and `types.UnionType` expose
builtin `__or__`/`__ror__` descriptors. Normal metaclass lookup runs first, so a
user override keeps Python precedence. The builtin constructor accepts classes,
generic aliases, type aliases, type parameters and `None`; nested unions are
flattened and structurally duplicate members are removed.

Union and generic-alias objects are precisely traced. They expose their runtime
classes; unions expose `__args__` and `__origin__`. Union equality and hashing are
structural and independent of member order. `isinstance` and `issubclass` test
members in order, preserving the normal invalid-member error boundary. The
`types` module publishes `UnionType` and `GenericAlias`.

TypePlan schema version 2 adds `Union(Vec<TypePlan>)`. Resolution recursively
converts members, flattens nested plans, normalizes exact `NoneType` to the `None`
member inside a union, sorts and deduplicates plans, and retains every user-class
version dependency. The explicit encoding receives a new stable union tag and
never serializes Rust enum layout or native addresses.

## Consequences

`A | B` and `T | None` are now real runtime annotations and deterministic inputs
to annotation-JIT planning. Equivalent member order produces one canonical plan
hash, enabling later specialization and cache reuse.

This decision does not yet narrow unions across branches or lower a union
signature to native code. Entry tag-set guards, `is None` refinement, phi joins,
direct-call union summaries and deoptimization metadata remain typed-overlay
roadmap work.
