# ADR 0125: Concrete f64 buffer type plan

## Status

Accepted.

## Context

Annotation-guided partial static compilation needs a stable buffer identity and
canonical dtype metadata before it can install shape, bounds, mutability and
alias guards. Tonic already has a movable-GC-safe native buffer object and a
zero-copy export descriptor, but the current backing storage is specifically
`f64`. The public ABI reserves identifiers for more dtypes without implementing
their storage yet.

Treating every reserved dtype as available would make the optimizer and public
type contract unsound. Treating the buffer as an ordinary user class would lose
the known storage fact and make later native loop lowering depend on class
layout accidents.

## Decision

The runtime exposes the concrete native object through `fastmath.Buffer`.
`type(fastmath.array(...))` returns that class and `isinstance` recognizes it;
the class is retained as an explicit runtime root and remains valid across a
moving collection.

TypePlan schema v6 adds a dedicated buffer plan containing dtype, optional rank
and mutability. The current `fastmath.Buffer` annotation resolves to `f64`, no
rank constraint and `Any` mutability. This describes the implemented runtime
object exactly. Rank and mutability are not inferred from a function's call
history or from one particular buffer value during annotation planning.

The plan is advisory metadata. It does not itself enforce an argument type or
select a native buffer ABI. Later lowering must guard the actual object kind,
dtype, rank, dimensions, strides and mutability that its generated operations
depend on, then deopt to generic bytecode when any guard fails.

## Consequences

Buffer annotations now have a stable canonical cache representation and can be
distinguished from user classes before typed loop work begins. The runtime does
not claim support for the other reserved ABI dtype identifiers. Parameterized
dtype/rank/mutability syntax, indexed native loops, bounds elimination,
write-barrier behavior and alias/escape materialization remain explicit roadmap
items.
