# ADR 0109: Guarded annotated call results

## Status

Accepted.

## Context

Function-local scalar propagation stopped at every call. An exact global Tonic
function can already be inlined by the Cranelift subset, but the profile-backed
path requires warmup and does not establish that an annotation describes the
callee's actual returns.

Trusting `-> int` alone is unsound because annotations are advisory. A function
may declare `int` and return `True` or another dynamic value. Propagating that
claim into the caller could remove a tag guard and misdecode the value.

## Decision

For an ordinary call whose callee register comes from an unchanged `LOAD_GLOBAL`,
the runtime may build a first-call direct edge when all of these conditions hold:

- the value is an exact Tonic function in the current execution;
- ordinary signature binding succeeds without closures or unsupported state;
- scalar parameter and result TypePlans resolve to an inlineable all-int or
  all-float leaf;
- the public JIT return-proof API proves every reachable callee return matches
  the result TypePlan.

The direct edge retains the exact function handle and generated code guards that
identity at the call PC. The caller annotation guard additionally records the
callee code, execution, annotation object, mutation version, canonical plan hash,
and class-plan dependencies. A callee annotation mutation invalidates the caller
entry before execution. A global rebind misses the generated exact-callee guard
and deoptimizes at the call.

Only after these guards and the body proof succeed does the typed overlay assign
the summarized scalar fact to the `CALL` destination. That fact can feed later
arithmetic and the caller's own return proof.

## Consequences

Small annotated numeric call chains can compile and inline on the caller's first
valid invocation without waiting for adaptive call profiles. No temporary guest
argument tuple or keyword dictionary is introduced.

Lying or dynamic callee annotations remain advisory and do not produce a summary.
The first implementation covers exact global integer and float leaves. Recursive
graphs, methods, class/shape dependencies, and graph-wide compilation budgets
remain future work.
