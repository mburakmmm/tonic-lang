## Problem

<!-- What concrete problem does this change solve, and why does it matter? -->

## Changes

<!-- Describe the resulting behavior and relevant implementation boundaries. -->

## Validation

<!-- List focused tests, differential checks, and benchmarks that reviewers need. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] Relevant differential/stress-GC tests
- [ ] Benchmark or ADR when the runtime contract or hot path changes

## Limitations and risks

<!-- State remaining limitations, compatibility differences, or rollback concerns. -->
