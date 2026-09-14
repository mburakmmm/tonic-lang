"""Compare the current Tonic interpreter slice with the running CPython."""
import argparse
import contextlib
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
WARMUP = 5
SAMPLES = 30
COLD_WARMUP = 3
COLD_SAMPLES = 15


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def timed_python(cases, raw):
    for name, path, source, expected in cases:
        for sample in range(WARMUP + SAMPLES):
            start = time.perf_counter()
            code = compile(source, str(path), "exec")
            seconds = time.perf_counter() - start
            if sample >= WARMUP:
                raw.append(("cpython", name, "compile", sample - WARMUP, seconds))
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            exec(code, {})
        if output.getvalue().encode() != expected:
            raise RuntimeError(f"CPython checksum mismatch: {name}")
        for sample in range(WARMUP + SAMPLES):
            namespace = {}
            sink = io.StringIO()
            start = time.perf_counter()
            with contextlib.redirect_stdout(sink):
                exec(code, namespace)
            seconds = time.perf_counter() - start
            if sink.getvalue().encode() != expected:
                raise RuntimeError(f"CPython timed checksum mismatch: {name}")
            if sample >= WARMUP:
                raw.append(("cpython", name, "warm_run", sample - WARMUP, seconds))


def timed_cold(cases, tonic, raw):
    engines = [("tonic", [str(tonic)]), ("cpython", [sys.executable])]
    env = dict(os.environ, PYTHONHASHSEED="0")
    for name, path, _, expected in cases:
        for sample in range(COLD_WARMUP + COLD_SAMPLES):
            ordered = engines if sample % 2 == 0 else list(reversed(engines))
            for engine, command in ordered:
                start = time.perf_counter()
                result = subprocess.run(
                    command + [str(path)],
                    cwd=ROOT,
                    env=env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=30,
                )
                seconds = time.perf_counter() - start
                if result.returncode or result.stdout != expected:
                    raise RuntimeError(
                        f"{engine} cold checksum failed for {name}: "
                        f"code={result.returncode} stdout={result.stdout!r} stderr={result.stderr!r}"
                    )
                if sample >= COLD_WARMUP:
                    raw.append((engine, name, "cold_cli", sample - COLD_WARMUP, seconds))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tonic", type=Path, required=True)
    parser.add_argument("--warm-binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    tonic = args.tonic.resolve()
    warm_binary = args.warm_binary.resolve()
    prefix = args.output.resolve()
    prefix.parent.mkdir(parents=True, exist_ok=True)

    expected = {
        "integer_loop": b"4999950000\n",
        "fib_calls": b"102334155000\n",
        "known_calls": b"50015000\n",
        "float_loop": b"5000.0\n",
        "list_iteration": b"20000\n",
        "closure_calls": b"10000\n",
        "keyword_calls": b"50045000\n",
        "dict_lookup": b"20000\n",
        "dict_insert": b"10000\n",
        "attribute_load": b"10000\n",
        "bound_method_calls": b"50005000\n",
        "descriptor_load": b"10000\n",
        "super_calls": b"50015000\n",
    }
    cases = []
    for path in sorted((ROOT / "benches/comparison").glob("*.py")):
        cases.append((path.stem, path, path.read_text(), expected[path.stem]))
    if set(expected) != {case[0] for case in cases}:
        raise RuntimeError("comparison case manifest mismatch")

    result = subprocess.run(
        [str(warm_binary)], cwd=ROOT, text=True, stdout=subprocess.PIPE, check=True
    )
    rows = list(csv.DictReader(io.StringIO(result.stdout)))
    raw = [
        (row["engine"], row["case"], row["phase"], int(row["sample"]), float(row["seconds"]))
        for row in rows
    ]
    timed_python(cases, raw)
    timed_cold(cases, tonic, raw)

    raw_path = Path(str(prefix) + "-samples.csv")
    with raw_path.open("w", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["engine", "case", "phase", "sample", "seconds"])
        writer.writerows(raw)

    grouped = {}
    for engine, case, phase, _, seconds in raw:
        grouped.setdefault((engine, case, phase), []).append(seconds)
    summary_path = Path(str(prefix) + ".csv")
    with summary_path.open("w", newline="") as handle:
        fields = [
            "case", "phase", "tonic_median_us", "cpython_median_us",
            "tonic_p95_us", "cpython_p95_us", "tonic_over_cpython",
        ]
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for case in expected:
            for phase in ("compile", "warm_run", "cold_cli"):
                tonic_values = grouped[("tonic", case, phase)]
                python_values = grouped[("cpython", case, phase)]
                tonic_median = statistics.median(tonic_values)
                python_median = statistics.median(python_values)
                writer.writerow({
                    "case": case,
                    "phase": phase,
                    "tonic_median_us": f"{tonic_median * 1e6:.3f}",
                    "cpython_median_us": f"{python_median * 1e6:.3f}",
                    "tonic_p95_us": f"{percentile(tonic_values, .95) * 1e6:.3f}",
                    "cpython_p95_us": f"{percentile(python_values, .95) * 1e6:.3f}",
                    "tonic_over_cpython": f"{tonic_median / python_median:.4f}",
                })

    source_hash = hashlib.sha256()
    for _, path, source, _ in cases:
        source_hash.update(path.name.encode())
        source_hash.update(source.encode())
    metadata = {
        "scope": "intermediate Tonic-vs-CPython comparison; not final language benchmark",
        "platform": platform.platform(),
        "machine": platform.machine(),
        "python": sys.version.replace("\n", " "),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "tonic_binary": str(tonic.relative_to(ROOT)),
        "tonic_sha256": hashlib.sha256(tonic.read_bytes()).hexdigest(),
        "warm_binary": str(warm_binary.relative_to(ROOT)),
        "warm_binary_sha256": hashlib.sha256(warm_binary.read_bytes()).hexdigest(),
        "cargo_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
        "case_source_sha256": source_hash.hexdigest(),
        "case_count": len(cases),
        "warmup_per_warm_case": WARMUP,
        "samples_per_warm_case": SAMPLES,
        "warmup_per_cold_case": COLD_WARMUP,
        "samples_per_cold_case": COLD_SAMPLES,
        "limitations": [
            "uncontrolled desktop load and CPU frequency",
            "no CPU affinity or hardware performance counters",
            "no per-workload RSS or host allocator counters",
            "Tonic has no adaptive tier or JIT yet",
            "warm Tonic uses a fresh VM outside the timer; both engines execute into a fresh module namespace",
            "results apply only to the listed shared syntax subset",
        ],
    }
    Path(str(prefix) + "-process.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    main()
