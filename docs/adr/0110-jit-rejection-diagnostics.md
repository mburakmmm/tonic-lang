# ADR 0110: JIT rejection diagnostics

## Status

Accepted.

## Context

Falling back to the interpreter is correct, but a bare `Unsupported` cache entry
does not tell users or embedders why a function failed to compile. Annotation-
guided first-call compilation especially needs deterministic explanations so a
developer can distinguish an unsupported opcode from profitability, code-budget,
or unstable-guard decisions.

## Decision

The runtime records at most one current persistent JIT rejection per code object.
A public `JitRejection` contains:

- code id and function name;
- a stable category code;
- optional bytecode PC and opcode;
- a deterministic human-readable reason.

The initial categories are `unprofitable`, `code-budget`,
`unsupported-bytecode`, and `unstable-guards`. Successful compilation or cache
invalidation clears a stale record. The public `Vm::jit_rejections()` iterator
exposes current records without leaking JIT or heap addresses.

CLI `--stats` prints each record as one `jit-rejection` line after the aggregate
counters. Quoted fields use Rust debug escaping so whitespace and source names do
not make the line ambiguous.

## Consequences

Fallback remains non-fatal and preserves dynamic language behavior. CI and
future tooling can assert exact rejection categories and locations, while human
users receive a concrete explanation.

This is runtime decision metadata, not a promise that every transient guard miss
will be retained. Per-site event streams and source-span rendering may be added
later if measurements justify their cost.
