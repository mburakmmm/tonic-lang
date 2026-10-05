# ADR 0112: Opaque receiver call summaries

## Status

Accepted.

## Context

The first annotated class edge covered static binding only. Ordinary instance
methods and `classmethod` prepend a receiver slot, and Python convention does
not require `self` or `cls` to be annotated. Requiring a scalar annotation for
that slot would reject common code; pretending that the receiver is an integer
or float would be unsound.

The receiver still needs to remain a precise managed root and preserve ordinary
descriptor behavior. Instance attributes may shadow methods, class bindings may
change, and a classmethod receiver must be the dynamically resolved class.

## Decision

`ScalarType` includes `Dynamic` for parameters that stay as materialized guest
values and contribute no scalar fact. The typed overlay maps such a parameter to
`Unknown`, emits no scalar tag guard or guard-elision claim for it, and never
accepts `Dynamic` as a typed result. The public return-proof API returns false
for a dynamic result; the public compile API rejects it deterministically.

For an exact global instance method or exact global class `classmethod`, the
runtime may reserve the leading receiver parameter as `Dynamic`. Every remaining
parameter and the result must still resolve to the supported exact scalar plan,
and every reachable return must pass bytecode proof. The direct argument plan
uses `MethodReceiver`; generated code invokes the existing method-load helper to
guard instance/class binding behavior and exact function identity at the
attribute PC.

The caller retains the callee annotation dependency. Annotation mutation
invalidates the caller entry. Instance shadowing, owner replacement, or class
rebinding fails the generated method guard and resumes generic execution at the
exact attribute PC.

## Consequences

Common annotated numeric instance methods and classmethods can feed first-call
caller dataflow without adding artificial `self`/`cls` annotations or allocating
bound-method objects. The receiver remains opaque to scalar lowering and precise
to the GC.

This decision does not yet infer an instance from a caller's class annotation.
That requires a class/shape parameter guard and remains part of the typed class
roadmap. Recursive/SCC summaries and graph-wide code budgets also remain open.
