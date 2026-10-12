# ADR 0140: Add an explicit first-call JIT decorator

## Status

Accepted.

## Context

Supported annotations already route functions to guarded typed JIT compilation
on their first valid call. Unannotated functions wait for profile hotness and
small straight-line functions may remain in the adaptive tier under the
profitability threshold. Deployment and library authors also need an explicit
way to request compilation without changing Python call semantics or defining a
second function kind.

An opt-in must not promise that arbitrary bytecode is compilable, bypass the
process native-code budget, turn annotations into runtime type checks, or wrap a
function in an identity-changing proxy. Unsupported code must keep working in
the interpreter and expose a deterministic rejection reason.

## Decision

The built-in `tonic` module exports `compile` as a normal one-argument
decorator. It accepts an exact Tonic function, or a `staticmethod`/
`classmethod` wrapper containing one, sets a bit on the underlying GC-managed
function, and returns the original object. Both decorator orders therefore
preserve descriptor behavior and object identity. Other values raise
`TypeError` during ordinary decorator evaluation.

At function entry, the VM reads the marker only from a function belonging to
the current execution and matching code object. In JIT execution mode an
untried marked function becomes an explicit candidate on its first call. It
bypasses hotness deferral and the small-function profitability rejection. It
does not bypass structural verification, supported-opcode checks, guards,
deoptimization, annotation dependency validation, or the global native-code
budget.

When a supported annotation plan exists, the existing advisory typed signature
and argument/return guards are used. A mismatching argument runs the generic
path and may allow a later matching call to compile. Without a supported plan,
the normal safe JIT subset and available runtime profile facts are used.
Unsupported bytecode records the existing per-code rejection and continues in
the interpreter.

VM statistics separately count explicit candidates, successful explicit
compilations, and explicit rejections. Existing compile-attempt, code-size,
fallback, annotation, and rejection diagnostics remain authoritative.

## Consequences

Users can write:

```python
import tonic

@tonic.compile
def transform(value):
    return value + 1
```

and request a first-call native attempt without waiting for a sampling threshold.
The same source remains valid in interpreter mode and unsupported bodies retain
their normal result and exceptions. Module-wide policy and import-time warmup
remain separate work because they require scheduling and budget allocation
across several code objects.
