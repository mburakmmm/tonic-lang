# ADR 0157 — Function-local expression-store annotations

## Status

Accepted and implemented as the sixth local typed-data-flow boundary.

## Context

The earlier advisory-result slices recognize initializers whose final value is
produced directly by a call, attribute/item/global/cell load or expanded call.
A normal annotation can also cover a conditional or multi-instruction
expression. Its final pre-store opcode may not be a safe place to attach a type
fact, and the annotation must remain non-enforcing.

## Decision

Bytecode version 39 permits `AdvisoryResult` on `MOVE`. When a simple
function-local scalar annotation has a value but no recognized direct producer,
the compiler attaches the hint to the local store move emitted after the RHS.

Generated code exits at that exact `MOVE` PC. The VM performs the ordinary local
copy after the RHS has completed with all Python evaluation order and side
effects intact. Native re-entry at the successor PC applies the exact scalar or
union guard to the destination register before typed data flow consumes it.

If the value does not match, the guard deoptimizes at the successor PC and the
same materialized local continues through generic bytecode without a new type
error.

## Consequences

Conditional and multi-step annotated initializers can seed typed local regions
without requiring a special producer opcode or trusting source spelling. This
first implementation pays a side exit for the guarded store. General
definite-assignment, loop-phi and graph-wide SSA inference remain future work.
