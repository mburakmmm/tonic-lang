# ADR 0117: Optional None identity branch refinement

## Status

Accepted.

## Context

ADR 0116 can guard an `int | None` or `bool | None` entry, but deliberately does
not treat the combined fact as numeric. Code that first tests `value is None`
therefore reached the safe generic arithmetic path even though the remaining
branch proves an exact scalar. The typed overlay needs edge-specific facts to
remove that cost and prove optional returns.

Bytecode lowering may copy a parameter through temporary registers before the
identity comparison. Refining every earlier source of a move is unsafe if that
source was assigned again after the copy: the compared temporary and current
source no longer denote the same value.

## Decision

The typed analysis records basic-block starts and recognizes an `Is` or `IsNot`
definition consumed by a conditional jump in the same block. When exactly one
operand is `None` and the other is `IntOrNone` or `BoolOrNone`, it creates
separate true and false successor states. `is` assigns `None` to the true edge
and the exact scalar to the false edge; `is not` swaps them. `JumpTrue` and
`JumpFalse` map those states to their target and fallthrough edges explicitly.

Refinement walks backward only through pure `Move` definitions in the same
basic block. It propagates to a source register only when no instruction between
the copy and comparison writes that source. The current fact must also still be
the same optional fact. This conservative rule can miss an optimization but
cannot reinterpret a stale alias as the compared value.

## Consequences

The non-None branch of common optional code can use exact integer or boolean
lowering, guard elimination and typed return proof. Reversed operands and both
identity operators have the same semantics. Annotation mismatches and stale
aliases remain on the generic path with the original Python exception behavior.

The analysis does not yet model arbitrary SSA value identity, equality-based
narrowing, `isinstance`, pattern matching, general union subtraction, or
user-class shape facts. Those require broader provenance and dominance data.
