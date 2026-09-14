# Scope, çağrı, dict ve GC ara ölçümü

31 Ağustos 2026. **Nihai dil benchmarkı değildir.** macOS 26.6 ARM64, Rust stable 1.86.0, release thin LTO/tek codegen unit. Cranelift veya CPython hız karşılaştırması içermez.

## Tekrar çalıştırma

```sh
cargo bench -p tonic-runtime --bench interpreter --locked --offline --no-run
python3 benches/capture.py --output docs/benchmarks/stage2
```

14 iş yükü; GC aralığı 1024 ve otomatik GC kapalı modlar. Her kombinasyonda 2 warmup + 15 örnek. Parse/compile/verify ayrı ölçülür. Run ölçümünde compile, VM creation ve teardown dışarıda; run hazırlığı ve collection içeridedir. Her örnek fresh VM kullanır. Bu bir aynı-VM JIT warmup deneyi değildir. İş yükü sırası sabittir.

## Interpreter sonuçları

| İş yükü | GC açık median µs | GC kapalı median µs | Guest allocation / run | Toplanan nesne | GC açık peak tahmini byte |
|---|---:|---:|---:|---:|---:|
| integer_loop | 5140.709 | 5253.417 | 0 | 0 | 611 |
| fib_40_x1000 | 2561.917 | 2695.000 | 1 | 0 | 691 |
| known_calls_10000 | 1063.167 | 1019.541 | 1 | 0 | 691 |
| native_calls_10000 | 1753.708 | 1710.584 | 0 | 0 | 611 |
| float_loop_10000 | 827.916 | 771.500 | 10002 | 9206 | 82851 |
| list_iteration_1000 | 262.708 | 250.917 | 1001 | 0 | 80731 |
| closure_calls_10000 | 1009.750 | 975.417 | 3 | 0 | 859 |
| closure_creation_5000 | 978.917 | 915.333 | 10001 | 9206 | 86955 |
| keyword_calls_10000 | 1406.417 | 1339.000 | 1 | 0 | 723 |
| variadic_calls_10000 | 4684.750 | 4361.167 | 30002 | 29742 | 149291 |
| expanded_calls_10000 | 3414.291 | 3242.916 | 10004 | 9206 | 83052 |
| dict_lookup_10000 | 1343.500 | 1291.667 | 5 | 0 | 1199 |
| dict_insert_10000 | 1600.917 | 1555.625 | 1 | 0 | 836275 |
| cyclic_garbage_10000 | 2029.584 | 1654.375 | 30000 | 29687 | 96483 |

`known_calls` burada tanımı bilinen bir Tonic fonksiyonunu tekrar çağırır; henüz inline-cache/CALL_TONIC_FUNCTION specialization uygulanmış değildir. `integer_loop` module-scope döngüdür; yalnızca local register microbenchmarkı sayılmaz.

## Bellek ve GC

| İş yükü | Run sonu heap byte, GC açık / kapalı | Heap'te kalan nesne, açık / kapalı | Collection | Toplam GC pause median µs | En uzun tek pause µs |
|---|---:|---:|---:|---:|---:|
| float_loop_10000 | 64291 / 800771 | 802 / 10008 | 9 | 51.875 | 12.959 |
| closure_creation_5000 | 67387 / 840691 | 801 / 10007 | 9 | 100.669 | 12.791 |
| variadic_calls_10000 | 38010 / 4330772 | 266 / 30008 | 29 | 708.086 | 88.292 |
| cyclic_garbage_10000 | 29843 / 2800611 | 319 / 30006 | 29 | 419.456 | 31.458 |

Benchmark process peak RSS: **14.450.688 byte**. Parse, bütün VM kuruluş/yıkımları ve GC kapalı örnekler dahil; workload başına veya yalnızca GC açık RSS değildir. Cargo/rustc bu child ölçümüne dahil değildir.

`resident_objects`, o anda heap'te ayrılmış giriş sayısıdır; sonraki safepoint'i bekleyen çöpü de içerir. Run sonunda zorunlu son collection yoktur. `gc_reclaimed` gerçekten toplanan nesneleri sayar. Peak/live tahmini byte yalnızca Object boyutları ve takip edilen payload kapasitesidir: hash key string/BigInt malzemesinin tüm payload'ı, allocator overhead, heap/slot tablosu yedek kapasitesi, register/frame/native tabloları ve parser belleği dahil değildir. Bu yüzden yaklaşık heap byte, RSS yerine kullanılamaz.

GC pause ölçümü root toplama + mark/compact aşamalarını kapsar. `gc_pause_median_us` 15 run'ın **toplam GC sürelerinin median'ıdır**; tek collection median'ı değildir. `gc_pause_max_us` bütün ölçülmüş run'lardaki en uzun tek collection'dır. Stop-the-world collector'ın bounded pause garantisi yoktur.

Küçük integer döngüsünde per-iteration guest allocation yoktur. Keyword çağrısı yalnızca tanımdaki function nesnesini ayırır. Variadic callee ise dilin istediği tuple/dict'i gerçekten oluşturur. Expanded çağrılar henüz generic iterator tahsis eder. Dict'e 10.000 immediate değer eklemek yalnızca bir managed Dict nesnesi üretir; iç Vec/hash tablo host allocation'ları bu sayaçta **yoktur**.

## Ham veri ve sınırlamalar

- [Özet CSV](benchmarks/stage2.csv): süreler, dispatch, allocations, register/code büyüklüğü ve GC sayaçları.
- [645 ham örnek](benchmarks/stage2-samples.csv): 420 run, 210 compile ve 15 native-scope örneği.
- [Ortam/process manifesti](benchmarks/stage2-process.json): binary ve Cargo.lock SHA-256, platform, process wall/user/system ve RSS.
- [Native scope kaydı](benchmarks/stage2.stderr.txt): 100.000 create/resolve/drop için 15 örnek; sonunda active handle=0.
- [Scope öncesi](benchmarks/stage2-before-scopes.csv) ve [GC öncesi](benchmarks/stage2-before-gc.csv) eski 6-workload harness kayıtları. Önceki core/object maliyetleri ve ölçüm zamanı farklıdır.
- [Eşzamanlı testlerle çakışan koşu](benchmarks/stage2-concurrent.csv) ve [manifesti](benchmarks/stage2-concurrent-process.json) saklanmıştır; ana tablo test süreçleri bittikten sonra yeniden alınmıştır.

Bu masaüstü ortamında CPU modeli/frekansı/affinity/güç durumu kontrol edilmemiştir. Aynı oturumdaki süreler belirgin değişebilmektedir. Önceki baseline'a göre bazı workload'lar yavaştır; farkın tamamını tek bir yeni özelliğe bağlamak veya bu tabloyla genel hızlanma oranı ilan etmek doğru değildir. GC kapalı modu bile yeni handle tablosunu/call binder'ı kullandığından eski arena uygulamasıyla aynı değildir.

Ölçüm tam Python conformance veya production hazır olma kanıtı değildir. Native-scope host allocation'ları sayılmamış, profiler/perf-counter verisi toplanmamış, shapes/methods/exceptions/generators/JIT/bridge iş yükleri çalıştırılmamıştır. Bunlar [nihai benchmark kabul planında](FINAL_BENCHMARK_PLAN.md) bekleyen işlerdir.
