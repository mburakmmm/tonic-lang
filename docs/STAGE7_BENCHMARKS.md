# Custom descriptor ara baseline'ı

1 Eylül 2026. **Nihai dil benchmarkı değildir.** macOS 26.6 ARM64, Rust stable
1.86.0, release thin LTO / tek codegen unit; JIT ve CPython hız karşılaştırması yoktur.

29 program × iki GC modu, kombinasyon başına 2 warmup + 15 örnek; compile ayrıca
ölçülür ve her run fresh VM'dir. Yeni iş yükleri aynı instance üzerinde 10.000
custom `__get__` veya `__set__` çağrısı yapar.

| İş yükü | GC açık median µs | GC kapalı median µs | Dispatch/s | Guest allocation/run | Peak heap byte |
|---|---:|---:|---:|---:|---:|
| descriptor_load_10000 | 1925.208 | 1848.125 | 88.318 milyon | 11 | 3.570 |
| descriptor_store_10000 | 1754.292 | 1719.959 | 96.922 milyon | 11 | 3.570 |

Her iki iş yükünde 11 allocation yalnızca sınıf, fonksiyon, descriptor ve instance
kuruluşuna aittir; 10.000 erişim allocation sayısını artırmaz ve collection
tetiklemez. Callable/receiver ayrımı geçici `BoundMethod` üretmez; iki protokol
argümanı call binder'ın sabit boyutlu inline alanında taşınır, host `Vec` de ayrılmaz.
Yine de her erişim generic MRO lookup ve normal frame çağrısı yapar; inline cache,
quickening veya JIT henüz yoktur.

Compile/verify medianları load için 20.625 µs, store için 19.666 µs'dir. Tüm
benchmark process'i peak RSS 14.041.088 byte ve wall 1.921 s ölçüldü. Native scope
100.000 işlem medianı 2.801 ms, active handle=0'dır.

- [58 satır özet](benchmarks/stage7.csv)
- [1320 ham örnek](benchmarks/stage7-samples.csv): 870 run, 435 compile, 15 native scope
- [Manifest ve SHA-256](benchmarks/stage7-process.json)
- [Native scope kaydı](benchmarks/stage7.stderr.txt)

Binary SHA-256 `31fe09045cdb5eae93a875c268ddfafc12d06b68185e005b72b9e5482ab43f3b`
olarak manifest ve dosya üzerinde yeniden doğrulandı. Masaüstü yükü, frekans ve
affinity kontrolsüzdür; host allocator sayacı yoktur. Nihai kapsam
[benchmark planında](FINAL_BENCHMARK_PLAN.md) tanımlıdır.
