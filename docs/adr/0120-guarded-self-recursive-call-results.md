# ADR 0120: Guarded self-recursive call results

## Status

Accepted.

## Context

Annotation-guided direct leaves can carry a proven result into their caller, but
a recursive target cannot use the deliberately straight-line inliner. Treating
the return annotation as an unchecked recursive fact would be unsound because
Python annotations are advisory and the recursive global may be rebound.

The VM already leaves native code at an ordinary `CALL`, executes the full
Python call path, and resumes the suspended caller at the successor bytecode PC.
That boundary provides a precise place to validate a speculative result without
requiring a native recursive call ABI.

## Decision

The annotation planner recognizes exact-global self-recursive ordinary calls
whose parameters and result use the supported immediate scalar plans. It emits
a `GuardedCallResult` rather than a direct-inline plan. Cranelift side-exits at
the call as before. When the child finishes, arbitrary-PC native re-entry guards
the call destination against the summarized result before any typed arithmetic
or return consumes it.

The typed overlay may use the guarded result in branch merges and return proof.
All non-recursive reachable returns must still establish the annotated result.
If a recursive child returns a mismatching value, the successor-PC guard fails
and execution continues through generic bytecode. Annotation mutation invalidates
the compiled entry through the existing function-plan guard.

## Consequences

Self-recursive integer, boolean, `None`, optional-immediate and int-bool functions
can retain typed local and return flow across recursive calls. The call itself
still pays the generic VM/frame boundary, so this is a correctness-preserving
analysis and continuation step rather than the final native recursion ABI.

Native recursive entry calls, SCC-wide code generation and graph-wide code
budgets remain future work. Guarded mutual-recursive and non-inline method result
edges were added later in ADR 0121.
