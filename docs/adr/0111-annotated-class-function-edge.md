# ADR 0111: Guarded annotated class-function edge

## Status

Accepted.

## Context

Annotation-guided call-result propagation initially recognized only an ordinary
exact global function. Python code commonly groups numeric helpers on a class
and calls them as `Math.leaf(value)`. Tonic already had profile-backed
allocation-free method fusion, but waiting for a method profile prevented this
shape from participating in first-call typed compilation.

Resolving an attribute once and trusting it permanently would be unsound. Class
attributes, descriptor wrappers, and the global owner binding can all change.
Trusting the return annotation alone would also allow a lying annotation to
remove required tag guards in the caller.

## Decision

During annotation-guided caller compilation, an `ATTR` consumed by a replayable
ordinary `CALL` may create a direct edge when:

- the attribute owner register is produced by an ordinary defined global;
- the current owner resolves through `Heap::direct_method` to a plain
  class-level function or `staticmethod` with static binding behavior;
- ordinary signature binding produces an allocation-free direct argument plan;
- the callee has an exact scalar annotation plan supported by the typed tier;
- every reachable callee return is proven to match that result plan; and
- the target is an inlineable integer leaf.

The generated method-load helper reads the current owner, validates static
binding behavior, and guards exact callee identity at the attribute PC. The
caller's annotation guard also retains the callee function/code/execution and
annotation identity, mutation version, canonical hash, and class dependencies.
Callee annotation mutation therefore invalidates the entry before execution;
owner or class-attribute rebinding takes the exact-PC deoptimization path and
replays generic descriptor semantics.

Float class leaves remain excluded in this slice because the existing unboxed
float direct-call ABI does not support fused method caches. Instance methods and
`classmethod` require a dynamic receiver parameter in the typed summary and are
also deferred.

## Consequences

Annotated integer helpers grouped on a class can feed caller dataflow and return
proof on the first call without a warmup profile or bound-method allocation.
Changing annotations or class bindings preserves Python behavior through
invalidation or deoptimization. A false return annotation does not create a
typed result fact.

The remaining graph work includes instance/shape guards, class receivers,
recursion/SCC analysis, and graph-wide inline/code-size budgets.
