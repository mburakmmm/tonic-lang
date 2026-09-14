"""Aggregate independent compare_python.py runs into one reproducible dataset."""
import csv
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs/benchmarks/python-comparison-stage8"
RUNS = [ROOT / f"docs/benchmarks/python-comparison-stage8-run{i}" for i in range(1, 6)]


def percentile(values, fraction):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * fraction) - 1)]


rows = []
metadata = []
for run, prefix in enumerate(RUNS, 1):
    metadata.append(json.loads(Path(str(prefix) + "-process.json").read_text()))
    with Path(str(prefix) + "-samples.csv").open() as handle:
        for row in csv.DictReader(handle):
            rows.append({"run": run, **row})

with Path(str(OUT) + "-samples.csv").open("w", newline="") as handle:
    fields = ["run", "engine", "case", "phase", "sample", "seconds"]
    writer = csv.DictWriter(handle, fieldnames=fields)
    writer.writeheader()
    writer.writerows(rows)

grouped = {}
case_order = []
for row in rows:
    if row["case"] not in case_order:
        case_order.append(row["case"])
    grouped.setdefault((row["engine"], row["case"], row["phase"]), []).append(
        float(row["seconds"])
    )

fields = [
    "case", "phase", "samples_per_engine", "tonic_median_us", "cpython_median_us",
    "tonic_p95_us", "cpython_p95_us", "tonic_min_us", "cpython_min_us",
    "tonic_max_us", "cpython_max_us", "tonic_over_cpython",
]
with Path(str(OUT) + ".csv").open("w", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=fields)
    writer.writeheader()
    for case in case_order:
        for phase in ("compile", "warm_run", "cold_cli"):
            tonic = grouped[("tonic", case, phase)]
            python = grouped[("cpython", case, phase)]
            tm = statistics.median(tonic)
            pm = statistics.median(python)
            writer.writerow({
                "case": case,
                "phase": phase,
                "samples_per_engine": len(tonic),
                "tonic_median_us": f"{tm * 1e6:.3f}",
                "cpython_median_us": f"{pm * 1e6:.3f}",
                "tonic_p95_us": f"{percentile(tonic, .95) * 1e6:.3f}",
                "cpython_p95_us": f"{percentile(python, .95) * 1e6:.3f}",
                "tonic_min_us": f"{min(tonic) * 1e6:.3f}",
                "cpython_min_us": f"{min(python) * 1e6:.3f}",
                "tonic_max_us": f"{max(tonic) * 1e6:.3f}",
                "cpython_max_us": f"{max(python) * 1e6:.3f}",
                "tonic_over_cpython": f"{tm / pm:.4f}",
            })

base = metadata[0]
for item in metadata[1:]:
    for key in ("tonic_sha256", "warm_binary_sha256", "cargo_lock_sha256", "case_source_sha256"):
        if item[key] != base[key]:
            raise RuntimeError(f"metadata changed across runs: {key}")
aggregate = {
    **base,
    "scope": "five-process intermediate Tonic-vs-CPython comparison; not final language benchmark",
    "independent_runs": len(RUNS),
    "warm_samples_per_engine_case": len(RUNS) * base["samples_per_warm_case"],
    "cold_samples_per_engine_case": len(RUNS) * base["samples_per_cold_case"],
    "cpython_jit_available": bool(
        hasattr(sys, "_jit") and sys._jit.is_available()
    ),
    "cpython_jit_enabled": bool(hasattr(sys, "_jit") and sys._jit.is_enabled()),
    "raw_samples_sha256": hashlib.sha256(
        Path(str(OUT) + "-samples.csv").read_bytes()
    ).hexdigest(),
}
Path(str(OUT) + "-process.json").write_text(json.dumps(aggregate, indent=2) + "\n")
