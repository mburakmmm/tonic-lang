# ADR 0089 `any`/`all` intermediate benchmark

This directory records the `any_all` row extracted from the 16-case comparison
run on 1 October 2026. The workload fully scans one 1,000-element false list and
one 1,000-element true list 100 times. It is an intermediate acceptance
measurement, not the final roadmap benchmark.

Commands:

```sh
cargo bench -p tonic-runtime --bench interpreter --locked --offline
cargo build --release -p tonic-cli --locked --offline
cargo bench -p tonic-runtime --bench python_compare --locked --offline --no-run
python3 benches/compare_python.py \
  --tonic target/release/tonic \
  --warm-binary target/release/deps/python_compare-204da8f4e4390c4a \
  --output /tmp/tonic-any-all-stage-0089
```

`comparison.csv` contains medians and p95 values. `comparison-samples.csv`
contains all 150 retained compile, warm-run, and cold-CLI samples for `any_all`.
Process and source provenance is in `process.json`. Desktop load and CPU
frequency were not controlled; affinity and hardware counters were unavailable.
