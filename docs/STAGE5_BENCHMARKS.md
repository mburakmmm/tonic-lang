# Property ara ölçümü

1 Eylül 2026. **Nihai dil benchmarkı değildir.** macOS 26.6 ARM64, Rust stable
1.86.0, release thin LTO / tek codegen unit; JIT ve CPython hız karşılaştırması yoktur.

26 program × iki GC modu, kombinasyon başına 2 warmup + 15 örnek; compile ayrıca
ölçülür ve her run fresh VM'dir.

| İş yükü | GC açık median µs | GC kapalı median µs | Dispatch/s | Guest allocation/run | Peak heap byte |
|---|---:|---:|---:|---:|---:|
| property_load_10000 | 1078.333 | 1056.791 | 157.679 milyon | 9 | 3004 |
| property_store_10000 | 1201.291 | 1213.084 | 141.543 milyon | 12 | 3244 |

Class/instance kuruluşu ile 10.000 getter veya setter çağrısı ölçülür. Allocation
sayıları sabittir; döngü başına BoundMethod ya da arg tuple/dict oluşmaz. Bu generic
frame çağrısıdır, inline cache/JIT değildir. Process peak RSS 14.008.320 byte;
wall 1.740 s. Native scope 100.000 işlem median 2.535 ms, active handle=0.

- [52 satır özet](benchmarks/stage5.csv)
- [1185 ham örnek](benchmarks/stage5-samples.csv): 780 run, 390 compile, 15 native scope
- [Manifest ve SHA-256](benchmarks/stage5-process.json)
- [Native scope kaydı](benchmarks/stage5.stderr.txt)

Masaüstü yükü/frekans/affinity kontrolsüzdür. Nihai kapsam
[benchmark planında](FINAL_BENCHMARK_PLAN.md) tanımlıdır.
