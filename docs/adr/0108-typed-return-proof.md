# ADR 0108: Typed return proof

## Status

Accepted.

## Context

Annotation-guided entries already guard arguments and validate returned values in
the VM. The typed bytecode overlay can know the exact scalar fact at each
reachable `RETURN`, so repeating the same result check after native execution is
unnecessary when every control-flow path proves the declared scalar result.

Removing the host check based only on the annotation would be unsound: Python
annotations are advisory, a function can return a different type, and unknown
dynamic operations must remain valid.

## Decision

The scalar analysis compares the value fact before every reachable `RETURN` with
the typed signature result. It publishes `typed_return_proven` only when at least
one return is reachable and all reachable returns agree exactly.

The VM skips its post-native annotation result guard only when this proof bit is
set on an annotation-compiled entry. Otherwise it retains the existing result
check and deoptimizes at the exact `RETURN` PC on a mismatch. The VM also counts
elided return guards for diagnostics.

The proof relies on verified bytecode and guarded scalar inputs. Integer overflow
or another operation that leaves the proven small-scalar path deoptimizes before
the return. The interpreter remains responsible for the dynamic continuation.

## Consequences

Straight-line and control-flow numeric functions avoid a duplicate tag or heap
type check at the native boundary. A false annotation does not become a runtime
type requirement and does not receive a proof.

This proof covers the current function's returns. It does not yet summarize an
annotated callee or propagate a direct call result into its caller.
