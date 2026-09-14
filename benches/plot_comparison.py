"""Render the aggregate stage-8 Tonic/CPython timing ratios."""
import csv
from pathlib import Path

import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parents[1]
summary = ROOT / "docs/benchmarks/python-comparison-stage8.csv"
rows = list(csv.DictReader(summary.open()))
names = [row["case"] for row in rows if row["phase"] == "warm_run"]
warm = {
    row["case"]: float(row["tonic_over_cpython"])
    for row in rows
    if row["phase"] == "warm_run"
}
cold = {
    row["case"]: 1 / float(row["tonic_over_cpython"])
    for row in rows
    if row["phase"] == "cold_cli"
}

fig, axes = plt.subplots(1, 2, figsize=(14, 8))
y = list(range(len(names)))
left = axes[0].barh(
    y,
    [warm[name] for name in names],
    color=["#2a9d8f" if warm[name] <= 1 else "#e76f51" for name in names],
)
axes[0].axvline(1, color="#333333", linewidth=1)
axes[0].set_yticks(y, names)
axes[0].invert_yaxis()
axes[0].set_xlabel("Tonic / CPython median (lower is better)")
axes[0].set_title("Warm execution")
axes[0].bar_label(left, fmt="%.2fx", padding=3, fontsize=8)
axes[0].set_xlim(0, max(warm.values()) * 1.18)

right = axes[1].barh(y, [cold[name] for name in names], color="#457b9d")
axes[1].axvline(1, color="#333333", linewidth=1)
axes[1].set_yticks(y, names)
axes[1].invert_yaxis()
axes[1].set_xlabel("CPython / Tonic median (higher means faster Tonic startup)")
axes[1].set_title("Cold CLI")
axes[1].bar_label(right, fmt="%.2fx", padding=3, fontsize=8)
axes[1].set_xlim(0, max(cold.values()) * 1.18)

fig.suptitle("Tonic vs CPython 3.14.6 — stage 8 intermediate benchmark", fontsize=14)
fig.subplots_adjust(left=0.17, right=0.98, bottom=0.13, top=0.88, wspace=0.36)
fig.text(
    0.5,
    0.025,
    "5 independent runs; 150 warm and 75 cold samples per engine/workload; no Tonic or CPython JIT",
    ha="center",
    fontsize=9,
)
fig.savefig(ROOT / "docs/benchmarks/python-comparison-stage8.png", dpi=180)
