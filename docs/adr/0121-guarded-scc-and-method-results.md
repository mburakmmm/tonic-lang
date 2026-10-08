# ADR 0121: Guarded SCC and non-inline method results

## Status

Accepted.

## Context

ADR 0120 carries an annotated self-recursive result across a generic VM call by
guarding the value when native execution resumes. Mutual recursion has the same
boundary, but each function depends on another annotation plan. Methods with
branches or other non-leaf control flow also cannot use the atomic direct inliner
even when their result annotation would help the caller.

Requiring a whole-SCC native compilation unit before propagating any result
would leave useful typed caller continuations unavailable. Trusting the
annotation without checking the actual result would violate advisory Python
semantics.

## Decision

An exact annotated callee with supported immediate scalar parameters and result
may produce a `GuardedCallResult` when its body cannot use the direct leaf path.
The ordinary VM executes the call. Cranelift validates the materialized result
at the successor PC before typed arithmetic, branch or return logic consumes it.

For mutual-recursive edges, each caller records the other function's annotation
plan as a dependency. Mutation invalidates dependent compiled entries. Global or
method rebinding still executes through generic lookup/call semantics; a result
that does not match the summarized immediate type fails the resume guard and
continues in generic bytecode.

Generic method attribute lookup is accepted only when its destination feeds the
guarded ordinary call through replay-safe constant/move setup. `ATTR` and `CALL`
remain separate exact-PC side exits.

## Consequences

Mutual recursion and non-inline annotated methods retain typed result flow in
their callers without changing Python call, descriptor or rebinding behavior.
Incorrect annotations remain performance misses rather than runtime contracts.

The call boundary still allocates/uses ordinary VM frames. Native recursive
entry calls, SCC-wide code generation, graph-wide inline budgets and annotated
field/shape propagation remain separate roadmap work.
