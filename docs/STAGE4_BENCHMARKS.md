# Decorator ve method descriptor ara ölçümü

1 Eylül 2026. **Nihai dil benchmarkı değildir.** macOS 26.6 ARM64, Rust stable
1.86.0, release thin LTO / tek codegen unit. CPython hız karşılaştırması ve JIT
yoktur. Yöntem, GC/bellek sayaçlarının kapsamı ve kontrollü ortam sınırlamaları
[Stage 3 raporuyla](STAGE3_BENCHMARKS.md) aynıdır.

## Tekrar üretme

```sh
cargo bench -p tonic-runtime --bench interpreter --locked --offline --no-run
python3 benches/capture.py --output docs/benchmarks/stage4
```

24 program × iki GC modu çalıştırılmıştır. Her kombinasyonda 2 ısınma + 15
kaydedilmiş run; her program için ayrıca 2 + 15 parse/compile/verify örneği vardır.
Her run fresh VM kullanır. Özet 48 satır, ham kayıt 1095 örnektir: 720 run,
360 compile ve 15 native scope.

## Yeni method iş yükleri

| İş yükü | GC açık median µs | GC kapalı median µs | Dispatch/s | Guest allocation/run | GC collection | Peak tahmini heap byte |
|---|---:|---:|---:|---:|---:|---:|
| static_method_calls_10000 | 957.458 | 946.333 | 188.017 milyon | 6 | 0 | 2646 |
| class_method_calls_10000 | 1414.417 | 1401.250 | 134.345 milyon | 10006 | 9 | 84482 |

İki program class tanımı ve 10.000 çağrılık module-scope while döngüsünü ölçer.
Bu değerler yalnız tek lookup/call latency'si değildir. Static erişim wrapper'ı
açıp function döndürür; iterasyon başına managed allocation yoktur. Classmethod
erişimi her turda erişilen class'a bağlı BoundMethod ayırır. Bu yüzden toplam
10.006 allocation ve 9 full-heap collection görülür. Bu, gelecekte ölçülecek
descriptor/call specialization için açık maliyet baseline'ıdır.

## Ortak iş yükü kontrolü

| İş yükü | Stage 3 GC açık µs | Stage 4 GC açık µs | Süre değişimi |
|---|---:|---:|---:|
| integer_loop | 4942.458 | 4933.709 | -0.2% |
| known_calls_10000 | 905.750 | 846.042 | -6.6% |
| attribute_load_10000 | 552.209 | 574.916 | +4.1% |
| bound_method_calls_10000 | 1097.875 | 1122.000 | +2.2% |
| saved_method_calls_10000 | 810.375 | 851.625 | +5.1% |

Ortak programların bytecode dispatch ve run içi guest allocation sayıları
değişmemiştir. Sonuçlarda tek yönlü genel regresyon görünmez; yaklaşık ±%7 aralığındaki
farklar kontrolsüz masaüstü koşusunda ayrı bir optimizasyona bağlanamaz. Bunlar
hızlanma iddiası değildir. Stage 3'te görülen önceki genel regresyon ayrıca
profile edilmesi gereken açık iş olarak kalır.

Benchmark process peak RSS **13.991.936 byte**; wall **1.434 s**, user **1.125 s**,
system **0.010 s**. Bütün workload'lar ve GC kapalı örnekler dahildir, workload
başına RSS değildir. Native scope 100.000 create/resolve/drop median **2.637 ms**,
min 2.598, max 2.700 ms; sonunda active handle=0.

## Ham kayıtlar ve doğrulama

- [Özet CSV](benchmarks/stage4.csv)
- [Ham örnekler](benchmarks/stage4-samples.csv)
- [Binary/Cargo.lock SHA-256 ve process manifesti](benchmarks/stage4-process.json)
- [Native scope stderr kaydı](benchmarks/stage4.stderr.txt)

110 Rust testi debug/release geçti. Differential corpus 251 stdout + 48 exception
vakasıdır ve debug/release × normal/stress-GC matrisinde çalıştırılır. Bu ara
ölçüm host malloc sayaçları, macrobenchmark, property/custom descriptor, JIT,
buffer/FFI veya CPython bridge sonucu içermez. Nihai kapsam
[benchmark kabul planında](FINAL_BENCHMARK_PLAN.md) tanımlıdır.
