# ADR 0078: User finalizers outside collector bookkeeping

## Context

Tonic uses a moving generational collector. Running guest `__del__` while the
collector is marking or compacting would allow arbitrary allocation, mutation,
resurrection and exceptions to invalidate the collector's invariants. A user
finalizer also must not be called again merely because resurrection kept the
object alive.

## Decision

The collector recognizes unreachable `Instance` and `Exception` objects whose
class MRO exposes `__del__`. It places each logical handle in a bounded
finalizer queue and marks the object graph as live for that collection. The VM
drains the queue at safepoints, with the object and the currently pending guest
exception held in `finalizer_roots` while the callback executes.

Each logical handle is recorded as finalized before its callback starts. This
gives resurrection exactly-once behavior. The callback executes through the
normal Tonic call binder and frame machinery, so it can allocate, mutate the
heap and resume through the same interpreter/JIT caller boundary. Exceptions
are counted as unraisable finalizer errors and do not replace the active guest
exception or abort the surrounding program. Physical reclamation remains a
later collection after the finalizer roots are released.

Shutdown queues all remaining user finalizers before runtime roots are cleared;
the existing foreign and suspended-generator finalization paths remain
separate.

## Consequences

The common allocation and collection paths do not execute guest code. A
bounded queue prevents a large unreachable graph from creating an unbounded
safepoint pause. Full unraisable-hook dispatch and a cross-type finalization
ordering guarantee remain future work; until then errors are exposed through
runtime statistics and are deliberately fail-closed.

