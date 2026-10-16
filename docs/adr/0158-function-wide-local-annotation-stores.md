# ADR 0158 — Function-wide local annotation store guards

## Status

Accepted and implemented as the seventh local typed-data-flow boundary.

## Context

Bytecode v39 could seed a typed local fact at an annotated initializer, but a
later ordinary assignment to the same local replaced that fact with `Unknown`.
Bare annotations such as `value: int` did not seed any store at all. Branch and
loop joins therefore lost an otherwise useful scalar plan even though the
source had declared one for the function-local name.

Python local annotations remain advisory in Tonic. A mismatching assignment
must keep the assigned value and continue with normal Python behavior rather
than raise a new annotation error.

## Decision

Bytecode version 40 performs a function-scope prepass for simple local scalar
annotations. A unique, supported annotation applies to ordinary stores of that
local throughout the function, including stores textually before a bare
annotation and stores in branches and loops. Each such store carries an
`AdvisoryResult` on its final local `MOVE`.

The JIT exits at the exact store PC, lets the VM perform the ordinary copy, and
guards the destination at the successor PC. A match restores the scalar fact
for the existing CFG fixed-point analysis; branch joins and loop backedges can
therefore prove later arithmetic and returns. A mismatch deoptimizes with the
materialized value intact and resumes generic bytecode.

Multiple identical annotations are accepted. Conflicting or unsupported
annotations for the same local disable the function-wide plan for that name.
Nested function and class bodies are separate scopes and are not scanned into
the owner.

The annotation statement's own initializer keeps the more precise direct
producer boundary introduced by earlier revisions. Function-wide propagation
uses `MOVE` only for other stores, avoiding duplicate metadata at one store.

## Consequences

Bare and initialized scalar annotations now sustain typed local regions across
ordinary reassignment, branch merges, and loop phi-like joins without changing
language semantics. The first implementation pays a generic side exit at every
guarded store. Native cell-store propagation, alias/escape analysis, and a raw
unboxed local/call ABI remain separate work.
