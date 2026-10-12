# ADR 0141: Add bounded module import JIT warmup

## Status

Accepted.

## Context

Annotation-guided first-call compilation and `@tonic.compile` avoid profile
latency, but deployment code may need predictable compilation during module
initialization. Import-time compilation must not invent runtime argument values,
change annotation semantics, compile imported functions accidentally, exhaust
the process code cache, or make unsupported JIT bytecode fail an otherwise valid
import.

## Decision

The native `tonic` module exports the variadic `warmup` function. It is valid
only at source-module scope. With no arguments it selects exact Tonic functions
defined in the calling module and visible at that point, in deterministic module
symbol order. Explicit arguments narrow the selection and must be functions from
that same module. The runtime validates the complete explicit list before making
changes, removes duplicate identities, and never traverses arbitrary Python
objects.

Every selected function receives the same identity-preserving explicit marker
used by `@tonic.compile`. A function with a complete supported annotation guard
is compiled immediately without fabricated argument objects or profile facts.
Unannotated or unsupported annotation plans are deferred to one explicit
first-call attempt, where real values can provide safe profile information.
Generators and coroutines remain outside the JIT subset.

One warmup request may accept at most 16 MiB of native code by default and also
shares the execution-wide 64 MiB ceiling. `Vm::jit_warmup_max_code_bytes` and
`Vm::jit_max_code_bytes` let embedders lower or raise those limits. Functions
are considered in deterministic order and charged by emitted machine-code size.
Budget, unsupported-bytecode, and backend failures preserve the interpreter
entry and record a stable rejection. Unsupported warmup compilation remains
eligible for the single first-call retry because actual arguments may unlock a
guarded profile path; budget and backend failures do not retry.

Warmup requests, candidates, successful compilations, deferred functions, and
rejections have separate VM counters. Existing annotation, total compile,
code-size, and rejection statistics remain authoritative.

The module linker remaps function-annotation and code-level type-parameter
symbols into the combined symbol table. This is required so imported modules
build the same canonical annotation plan as the entry module.

## Consequences

Library authors can place:

```python
import tonic

def transform(value: int) -> int:
    return value + 1

tonic.warmup()
```

at the end of a module. In JIT mode `transform` is ready before import returns.
In interpreter mode the source remains valid and the function keeps normal
Python-style advisory annotation behavior. Calling `warmup` from a function or
passing a foreign/non-function object is a normal `TypeError`.
