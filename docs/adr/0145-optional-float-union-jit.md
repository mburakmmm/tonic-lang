# ADR 0145: Unbox `float | None` after control-flow refinement

## Status

Accepted.

## Context

Immediate optional unions can be guarded with value tags, while Tonic floats
are heap objects whose payload is unboxed into F64 stack slots by the JIT.
Supporting `float | None` therefore requires more than adding another lattice
label: the non-None control-flow edge must initialize native float state, and
arbitrary-PC entry must reconstruct the same state without exposing heap
addresses.

## Decision

Add `FloatOrNone` to the annotation scalar, public typed signature, and verified
scalar-fact lattice. Entry guards accept only `None` or a value confirmed by the
runtime's exact-float unbox operation. An identity comparison with `None`
refines the other edge to `Float`.

When a conditional edge changes a register from boxed `FloatOrNone` to exact
`Float`, generated code unboxes the still-rooted logical value into its F64
stack slot before entering the target block. Direct arbitrary-PC entry uses the
same guarded reconstruction. Float constants and arithmetic remain in F64 form;
guest-visible returns are boxed at the existing allocation safepoint.

The typed-float analysis accepts only operations whose materialization state is
explicit. A control-flow edge that would lose exact F64 state by merging back
into a boxed union is rejected until edge materialization exists. Generic
semantics remain authoritative in that case.

## Consequences

Canonical optional-float functions can compile on their first valid call and
run refined arithmetic without per-operation boxing. Values outside the
annotation take the ordinary advisory fallback. Deopt maps identify every live
unboxed register, and persistent metadata contains logical values only.

This is the first heap-backed union lowering. Mixed `int | float`, nested
container unions, and user-class shape unions remain roadmap work, but they can
reuse the branch-edge initialization and materialization rules introduced here.
