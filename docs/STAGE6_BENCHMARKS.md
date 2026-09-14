# Slice ara baseline'ı

1 Eylül 2026. **Nihai dil benchmarkı değildir.** macOS 26.6 ARM64, Rust stable
1.86.0, release thin LTO / tek codegen unit; JIT ve CPython hız karşılaştırması yoktur.

27 program × iki GC modu, kombinasyon başına 2 warmup + 15 örnek; compile ayrıca
ölçülür ve her run fresh VM'dir. Yeni iş yükü on elemanlı aynı listeden 1.000 kez
`xs[1:9:2]` üretir.

| İş yükü | GC açık median µs | GC kapalı median µs | Dispatch/s | Guest allocation/run | Peak heap byte |
|---|---:|---:|---:|---:|---:|
| slice_copy_1000 | 201.625 | 194.959 | 94.383 milyon | 2.001 | 98.460 |

Her iterasyon bir yönetilen Slice ve yeni sonuç List nesnesi ayırır; başlangıç
listesiyle toplam 2.001 guest allocation beklenen generic baseline'dır. GC açık
koşuda bir collection 1.004 nesne geri aldı. İki median arasındaki yaklaşık %3,4
fark kontrolsüz masaüstü koşusunda tek başına anlamlı bir regresyon veya kazanç
kanıtı değildir. Compile/verify medianı 17.667 µs, bytecode dispatch 19.030'dur.

Tüm benchmark process'i peak RSS 14.073.856 byte ve wall 1.812 s ölçüldü. Native
scope 100.000 işlem medianı 2.456 ms, active handle=0'dır.

- [54 satır özet](benchmarks/stage6.csv)
- [1230 ham örnek](benchmarks/stage6-samples.csv): 810 run, 405 compile, 15 native scope
- [Manifest ve SHA-256](benchmarks/stage6-process.json)
- [Native scope kaydı](benchmarks/stage6.stderr.txt)

Binary SHA-256 `179ba7be1e91cc5d7ae16b1ff2151d9a956dc19a352a5e91f7c78e9d804f4aee`
olarak manifest ve dosya üzerinde yeniden doğrulandı. Masaüstü yükü, frekans ve
affinity kontrolsüzdür; host allocator sayacı yoktur. Nihai kapsam
[benchmark planında](FINAL_BENCHMARK_PLAN.md) tanımlıdır.
