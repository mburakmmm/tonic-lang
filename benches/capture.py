"""Capture a prebuilt Rust benchmark in a separate process (macOS/Linux).

Build first: cargo bench -p tonic-runtime --bench interpreter --locked --no-run
Then: python3 benches/capture.py --output docs/benchmarks/stage2
No cargo/rustc process contributes to the measured child's peak RSS.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import resource
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path)
parser.add_argument('--output', type=Path, required=True, help='output file prefix')
args = parser.parse_args()
if args.binary is None:
    candidates = [p for p in (ROOT / 'target/release/deps').glob('interpreter-*')
                  if p.is_file() and not p.suffix and os.access(p, os.X_OK)]
    if not candidates:
        parser.error('build the benchmark first with cargo bench --no-run')
    args.binary = max(candidates, key=lambda p: p.stat().st_mtime)
binary = args.binary.resolve()
prefix = args.output.resolve()
prefix.parent.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, TONIC_BENCH_RAW=str(prefix) + '-samples.csv')
start = time.perf_counter()
with Path(str(prefix) + '.csv').open('w') as stdout, Path(str(prefix) + '.stderr.txt').open('w') as stderr:
    result = subprocess.run([str(binary)], stdout=stdout, stderr=stderr, env=env, cwd=ROOT, timeout=300)
wall = time.perf_counter() - start
usage = resource.getrusage(resource.RUSAGE_CHILDREN)
# Read this snapshot before running metadata commands, which are also children.
peak_rss = usage.ru_maxrss * (1 if platform.system() == 'Darwin' else 1024)
metadata = {
    'scope': 'intermediate regression measurement; not final language benchmark',
    'platform': platform.platform(), 'machine': platform.machine(),
    'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
    'binary': str(binary.relative_to(ROOT)) if binary.is_relative_to(ROOT) else str(binary),
    'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
    'cargo_lock_sha256': hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest(),
    'exit_code': result.returncode,
    'process_wall_s': wall, 'process_user_s': usage.ru_utime, 'process_system_s': usage.ru_stime,
    'process_peak_rss_bytes': peak_rss,
    'rss_scope': 'one benchmark process, all workloads including parser, VM setup and teardown; not per-workload RSS',
    'samples_per_case': 15, 'warmup_per_case': 2,
    'gc_modes': ['default (1024 allocation interval)', 'disabled'],
    'limitations': ['uncontrolled desktop load', 'fixed workload order', 'no host allocator counters',
                    'no CPU affinity/frequency control', 'no JIT or CPython speed comparison'],
}
Path(str(prefix) + '-process.json').write_text(json.dumps(metadata, indent=2) + '\n')
print(json.dumps(metadata, indent=2))
raise SystemExit(result.returncode)
