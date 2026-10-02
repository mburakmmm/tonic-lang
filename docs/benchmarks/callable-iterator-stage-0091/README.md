# ADR 0091 callable/sentinel iterator intermediate benchmark

The `callable_iterator` workload from the 18-case comparison run on 2 October
2026 creates 100 managed callable iterators and streams 1,000 integer items from
each into `sum`. It therefore measures 100,000 guest callable invocations,
sentinel comparisons, and iterator steps. This is an intermediate acceptance
measurement, not the final roadmap benchmark.

The standard interpreter and Python comparison commands in `docs/BENCHMARKS.md`
were used. `comparison.csv` stores medians and p95 values;
`comparison-samples.csv` stores 150 retained compile, warm-run, and cold-CLI
samples. `process.json` records provenance and limitations.
