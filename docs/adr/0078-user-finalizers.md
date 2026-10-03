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
exception or abort the surrounding program. The VM creates a managed
`sys.UnraisableHookArgs` instance with `exc_type`, `exc_value`, `exc_traceback`,
`err_msg` and `object`, roots it precisely, and calls the current
`sys.unraisablehook`. `sys.__unraisablehook__` retains the silent default hook.
A missing, non-callable or raising user hook is contained and recorded by
`unraisable_hook_errors`. Suspended-generator close failures use the same hook.
Physical reclamation remains a later collection after the finalizer roots are
released.

Every VM boundary drains logical categories in this order: suspended generators,
user `__del__` callbacks, then foreign payload destructors. Explicit collection
and shutdown empty each category; normal safepoints take at most eight generator
and eight object callbacks. JIT safepoints only discover and root pending work,
then return to this VM ordering boundary. No language-level order is promised
between objects in the same category.

Shutdown queues all remaining generator and user finalizers before runtime roots
are cleared, then runs foreign destructors while their traced handle edges are
still valid.

## Consequences

The collector itself never executes guest code. Bounded VM draining prevents a
large unreachable graph from adding an unbounded callback batch to one normal
safepoint. Full-heap tracing and compaction still have no bounded-pause
guarantee. Hook arguments and retained exceptions are ordinary managed graphs,
so storing them from a hook preserves them through the normal write barrier and
moving collector.
