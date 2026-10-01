# ADR 0088 `sum` intermediate benchmark

This directory records the two `sum` rows extracted from the 15-case comparison
run on 1 October 2026. It is an intermediate acceptance measurement, not the
final roadmap benchmark.

Commands:

```sh
cargo bench -p tonic-runtime --bench interpreter --locked --offline
cargo build --release -p tonic-cli --locked --offline
cargo bench -p tonic-runtime --bench python_compare --locked --offline --no-run
python3 benches/compare_python.py \
  --tonic target/release/tonic \
  --warm-binary target/release/deps/python_compare-204da8f4e4390c4a \
  --output /tmp/tonic-sum-python-stage
```

`comparison.csv` contains medians and p95 values. `comparison-samples.csv`
contains all retained samples for `sum_integer` and `sum_float`. Process and
source provenance is in `process.json`. The desktop load and CPU frequency were
not controlled; no affinity or hardware counters were available.
