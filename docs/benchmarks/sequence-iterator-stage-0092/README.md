# ADR 0092 sequence-iterator intermediate benchmark

The `sequence_iterator` workload from the 19-case comparison run on 2 October
2026 creates 100 managed fallback iterators and streams 1,000 integer items from
each into `sum`. It therefore measures 100,000 guest `__getitem__` invocations
plus exhaustion checks. This is an intermediate acceptance measurement, not the
final roadmap benchmark.

The standard interpreter and Python comparison commands in `docs/BENCHMARKS.md`
were used. `comparison.csv` stores medians and p95 values;
`comparison-samples.csv` stores 150 retained compile, warm-run, and cold-CLI
samples. `process.json` records provenance and limitations.
