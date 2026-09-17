# Tonic fuzz targets

These targets are intentionally outside the normal Cargo workspace. They use
libFuzzer through `cargo-fuzz` and keep generated corpora and crash artifacts
local.

```text
cargo +nightly fuzz run parser -- -max_total_time=300
cargo +nightly fuzz run jit_code_object -- -max_total_time=300
```

If the host AddressSanitizer runtime cannot initialize, coverage-only diagnosis
can use `--sanitizer none`; that run must not be reported as an ASan pass.

`parser` feeds valid UTF-8 source to the complete compiler/verifier pipeline.
`jit_code_object` mutates metadata and every instruction word in a small code
object, then exercises both public inlineability probes and native compilation.
Malformed input must return a normal error or `false`; a Rust panic, sanitizer
finding, or process crash is a bug.
