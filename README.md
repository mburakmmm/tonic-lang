# Tonic

[![CI](https://github.com/mburakmmm/tonic-lang/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/mburakmmm/tonic-lang/actions/workflows/ci.yml)
[![Rust 1.86+](https://img.shields.io/badge/Rust-1.86%2B-000000?logo=rust)](rust-toolchain.toml)
[![Status: Alpha](https://img.shields.io/badge/status-alpha-orange)](docs/ROADMAP.md)

**English** · [Türkçe](README.tr.md)

Tonic is an independent programming language and runtime written in Rust. It
uses familiar Python syntax while owning its compiler pipeline, register
bytecode, adaptive virtual machine, object model, precise generational garbage
collector, native extension ABI, and tiered Cranelift JIT.

> Tonic is an alpha-stage research and engineering project. It is not yet a
> complete Python implementation or a production-ready runtime.

Tonic does not embed CPython, run source through a Python subprocess, or use
CPython's object layout in its core runtime. CPython interoperability lives in a
separate adapter crate and pays compatibility costs only at that boundary.

## Why Tonic?

Tonic explores how Python-style dynamic code can be executed with compact values,
explicit registers, adaptive specialization, shape-based objects, moving GC, and
guarded native code without inheriting `PyObject*` or reference counting as the
runtime foundation.

The current implementation includes:

- Python-compatible parsing through a replaceable parser adapter and a Tonic-owned AST/HIR;
- versioned 8-byte register bytecode with verification before execution;
- immediate integers, booleans and `None`, plus arbitrary-precision integers;
- functions, closures, defaults, positional-only/keyword-only and variadic calls;
- classes, C3 multiple inheritance, shapes, descriptors, properties and `super`;
- adaptive integer quickening and bounded mono/polymorphic inline caches;
- precise generational tracing GC, compaction, write barriers and remembered sets;
- a tiered Cranelift JIT with loop OSR, safepoints, guards and PC-indexed deoptimization maps;
- a versioned opaque C ABI, persistent handles, callbacks, typed zero-copy buffers and foreign-object tracing;
- an isolated CPython bridge with primitive/container conversion, `PyTonicProxy` protocol forwarding, and bounded cross-collector cycle tracing;
- differential tests against Python and repeatable interpreter/JIT/interop benchmarks.

The accepted native ecosystem roadmap adds an isolated HPy Universal host and
an aHPy compatibility lane. The target is to load `.hpy0` extensions without
libpython while preserving Tonic's object layout and moving GC. This is planned
work, not a current compatibility claim; see the
[HPy/aHPy strategy](docs/HPY_AHPY_STRATEGY.md).

The unsupported surface is reported explicitly. Comprehensions, generators,
async execution, structural matching, general filesystem
imports, a full standard library, and several remaining protocols are still
tracked in the [roadmap](docs/ROADMAP.md).

## Architecture

```text
Python-compatible source
        |
        v
parser adapter -> Tonic AST -> scope analysis / HIR
        |
        v
verified register bytecode
        |
        +-------------------+
        |                   |
        v                   v
adaptive interpreter   profiling / quickening
        |                   |
        +---------+---------+
                  v
             Cranelift JIT
                  |
                  v
       guarded native execution
```

The workspace keeps the major boundaries explicit:

| Crate | Responsibility |
| --- | --- |
| `tonic-core` | Tonic AST, bytecode, verifier, diagnostics and source spans |
| `tonic-compiler` | Parser adapter, scope analysis, HIR and bytecode lowering |
| `tonic-runtime` | Value representation, heap, GC, VM, native ABI and builtins |
| `tonic-jit` | Cranelift lowering, guards, OSR, safepoints and deoptimization metadata |
| `tonic-cpython` | Isolated libpython adapter and `PyTonicProxy` bridge |
| `tonic-cli` | File, stdin and command-line execution |

Architectural constraints are recorded in [AGENTS.md](AGENTS.md), the interop
design in [TONIC_INTEROP_RUNTIME.md](TONIC_INTEROP_RUNTIME.md), and implemented
decisions in [the ADR directory](docs/adr).

## Requirements

- Rust stable 1.86 or newer;
- Cargo;
- a C11 compiler for the public header smoke test;
- CPython 3.12+ development/runtime files only when building the full workspace
  or the optional `tonic-cpython` crate.

The core CLI does not require CPython. The bridge build uses
`python3-config --ldflags --embed`; set `PYTHON_CONFIG` to select another
interpreter installation.

## Build and run

```sh
cargo build --workspace --locked
cargo run -p tonic-cli -- examples/fib.tonic
cargo run -p tonic-cli -- -c 'print(20 + 22)'
cargo run -p tonic-cli -- --check examples/classes.tonic
cargo run -p tonic-cli -- --dump-bytecode examples/fib.tonic
cargo run -p tonic-cli -- --jit --stats examples/fib.tonic
cargo run -p tonic-cli -- --gc-every 1 examples/closures.tonic
```

`tonic -` reads source from standard input. `.py` and `.tonic` files use the
same compiler and runtime pipeline. `--check` parses, compiles, and verifies
bytecode. Diagnostics include filename, line, column, and function context.

Exit codes are `0` for success, `1` for a guest language/runtime error, and `2`
for CLI or file errors.

## Example

```python
def sum_to(n):
    total = 0
    while n:
        n -= 1
        total += n
    return total

print(sum_to(100))
```

```sh
cargo run -p tonic-cli -- --jit -c $'def sum_to(n):\n total=0\n while n:\n  n-=1\n  total+=n\n return total\nprint(sum_to(100))'
```

## Validation

The repository currently contains 266 Rust tests and a differential corpus of
284 output cases plus 109 exception cases. The documented local matrix covers
debug/release, interpreter/JIT, and normal/allocation-stress GC execution. CI
also gates the JIT on Linux x86-64 and macOS AArch64 and runs both fuzz targets
under AddressSanitizer on each architecture.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --release --workspace --locked
python3 tests/differential/run.py
TONIC_GC_EVERY=1 python3 tests/differential/run.py
TONIC_JIT=1 python3 tests/differential/run.py
TONIC_JIT=1 TONIC_GC_EVERY=1 python3 tests/differential/run.py
cc -std=c11 -Wall -Wextra -Werror -Iinclude -fsyntax-only tests/c_header_smoke.c
```

See [VALIDATION.md](docs/VALIDATION.md) for the exact matrix, covered invariants,
and the limits of these claims.

## Benchmarks

Benchmark harnesses cover interpreter dispatch, adaptive caches, JIT throughput
and compile cost, generational GC, native ABI calls, buffers, callbacks, foreign
object lifecycle, CPython bridge crossings, and shared workloads against Python.

```sh
cargo bench -p tonic-runtime --bench interpreter --locked
cargo bench -p tonic-runtime --bench jit --locked
cargo bench -p tonic-runtime --bench generational_gc --locked
cargo bench -p tonic-cpython --bench bridge --locked
cargo bench -p tonic-cpython --bench cross_runtime_gc --locked
```

Recorded results and methodology are indexed in
[BENCHMARKS.md](docs/BENCHMARKS.md). The current Python comparison is documented
in [PYTHON_COMPARISON_STAGE8.md](docs/PYTHON_COMPARISON_STAGE8.md). These are
development baselines; the final reproducible benchmark matrix will be captured
after the remaining roadmap is complete.

## Project status

The runtime already executes a meaningful Python-style subset, but compatibility
and production hardening are incomplete. In particular:

- syntax compatibility is broader than executable semantics;
- CPython ABI compatibility is intentionally outside the core runtime;
- HPy Universal loading and aHPy-generated extension execution are planned and
  not implemented yet;
- GC pauses are not yet bounded and user-language finalizer semantics are open;
- JIT coverage is focused on profiled numeric loops and guarded call paths;
- the native C ABI remains versioned but pre-stable;
- this release does not provide a resource-isolation sandbox.

Use [ROADMAP.md](docs/ROADMAP.md) as the source of truth for completed and open
work. Performance claims should be read together with the raw samples and host
metadata committed under `docs/benchmarks/`.

## Contributing

Contributions are welcome while the project is in active development. Read
[CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Architectural
changes should include a benchmark or an ADR when they affect a hot path, object
representation, GC invariant, bytecode contract, or JIT assumption.

Security reports should follow [SECURITY.md](SECURITY.md).

## License

No open-source license has been selected yet. Copyright remains with the project
owner. A license will be added before the first public release; until then, the
repository is available for review and contribution under the terms explicitly
accepted by the owner.
