# ADR 0153 — Function-local generic producer annotations

## Status

Accepted and implemented as the second local typed-data-flow boundary.

## Context

ADR 0152 carries a scalar local annotation across an ordinary call. Attribute
and item loads have the same useful execution property in the current JIT: when
they cannot use a proven native path, generated code exits to the VM and resumes
at the following bytecode PC after the result has been materialized. Keeping the
metadata and JIT API call-specific would duplicate this mechanism and obscure
the shared safety rule.

Attribute sites may also participate in direct method lowering, and f64 buffer
item sites may use a native indexed load. An advisory local annotation must not
race either optimization, because its source spelling is not a runtime type
assertion.

## Decision

Bytecode version 35 renames the metadata to `AdvisoryResult` and permits it only
on ordinary `CALL`, `ATTR`, and `ITEM` instructions. The JIT-facing summary is
renamed to `GuardedResult`. The compiler emits a summary only when one of those
instructions is the final producer for a simple function-local annotated name.

The runtime removes a direct call or method-attribute lowering that overlaps an
advisory result. The JIT forces guarded attribute and item producers through the
generic side-exit. On re-entry at `pc + 1`, the existing typed-entry machinery
checks the actual `int`, `bool`, `float`, `None`, or supported PEP 604 union
before importing the fact. Failure returns to ordinary bytecode semantics.

For a guarded buffer item, native buffer-load and bounds-elision metadata is
removed for that site. The owner and index still follow normal buffer semantics,
but the item is materialized by the VM before a float fact can enter native F64
data flow.

## Consequences

Annotated attribute and item results can now restart typed native execution
without enforcing the annotation. Direct method and native buffer optimizations
cannot bypass the materialized-value guard, and metrics describe the path that
actually executed.

Global loads are excluded because their current JIT lowering continues natively
without a side-exit. Expanded calls, general expression producers, closures and
cells require distinct reconstruction or inline-guard work and remain open in
the roadmap.
