# ADR 0090 `min`/`max` intermediate benchmark

The `min_max` workload from the 17-case comparison run on 1 October 2026 scans
one 1,000-element integer list with both builtins 100 times. This is an
intermediate acceptance measurement, not the final roadmap benchmark.

The standard interpreter and Python comparison commands in `docs/BENCHMARKS.md`
were used. `comparison.csv` stores medians and p95 values;
`comparison-samples.csv` stores 150 retained compile, warm-run, and cold-CLI
samples. `process.json` records provenance and limitations.
