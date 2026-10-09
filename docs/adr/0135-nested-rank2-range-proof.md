# ADR 0135: Prove both axes of canonical nested rank-2 loops

## Status

Accepted.

## Context

The first rank-2 range proof recognized one straight-line loop and hoisted one
axis bound. A canonical matrix traversal has an outer row loop and an inner
column loop. Selecting only the first backedge after `ITEM` cannot associate the
row induction with its outer loop, while rejecting every earlier branch also
prevents the inner loop from being recognized inside the outer body.

Simply relaxing those checks would be unsafe: an initialization may be skipped,
an update may belong to another backedge, or control flow may reach a loop header
without establishing the assumed nonnegative induction value.

## Decision

Range analysis examines every backward jump following the item and pairs it with
the conditional branch whose exit target is immediately after that backedge. It
then applies the existing induction, bound, start, step, owner-stability and
offset checks independently to each tuple axis.

Two control-flow facts are now required. Every writer in the constant/move chain
that initializes the induction must dominate the selected loop header, and the
accepted arithmetic/update chain must dominate the selected backedge. Index and
offset move chains must likewise dominate the item. Dominance is proven over the
verified bytecode CFG while exception and suspension code remains excluded. The
loop may contain nested control flow, but only one write to its induction
register is accepted; the buffer owner and dynamic bound must remain unchanged.

For a canonical nested rank-2 traversal this yields two entry guards:
`rows <= shape0` and `columns <= shape1`. On normal entry both per-item axis
checks are omitted. Arbitrary-PC resume still executes both checks, and any entry
guard miss deoptimizes at PC zero to preserve generic Python behavior.

## Consequences

An annotated rectangular matrix scan can execute its row-major native load loop
without repeated axis bounds checks. Conditional initialization that does not
dominate the header is rejected, and the independent safe axis may still retain
its proof.

The analysis remains deliberately canonical. Rank-N descriptors, arbitrary
strides, writable native stores, mutation/version guards and alias escape rules
remain future work.
