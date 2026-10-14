# ADR 0144: Guard builtin `isinstance` before narrowing immediate unions

## Status

Accepted.

## Context

The typed overlay can represent `int`, `bool`, `None`, and their immediate
unions, but a Python program commonly narrows those values with `isinstance`.
Treating a source name as intrinsically special would be incorrect because both
`isinstance` and the type argument can be rebound. The generic call may also
have already produced an observable result before native execution resumes.

## Decision

The runtime emits a guarded type-test summary only when the materialized callee
is the exact Tonic `isinstance` builtin, the call has exactly two positional
arguments, and the materialized type object is the exact builtin `bool` or
`int`. The call remains a generic side exit. At its exact successor PC, native
code compares the callee and type-object logical handle words with the summary
before importing the boolean result or refining either branch.

For a `bool` test, the true edge is `Bool` and the false edge excludes `Bool`.
For an `int` test, the true edge includes `Bool` to preserve Python's
`bool <: int` rule. The analysis only follows the latest writer of the branch
condition and safe `Move` aliases of the tested value. A stale or overwritten
condition cannot activate the refinement.

If either identity guard fails, native code returns the exact successor PC. The
generic call result already present in the register file remains authoritative,
so the interpreter executes the branch with current Python semantics. Persistent
caches contain logical values rather than heap addresses.

## Consequences

Annotated `int | bool | None` and related immediate unions can keep arithmetic
native after ordinary Python `isinstance` checks. Rebinding the builtin or the
type object changes behavior correctly and produces a measured deoptimization
instead of stale specialization.

This slice does not narrow heap-backed unions, arbitrary type tuples, protocols,
or user classes. Those require type/class version and shape dependencies in the
broader partial-static roadmap.
