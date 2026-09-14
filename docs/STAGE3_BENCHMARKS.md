# Sınıf, shape ve metot ara ölçümü

31 Ağustos 2026. **Nihai dil benchmarkı değildir.** macOS 26.6 ARM64, Rust stable
1.86.0, release thin LTO / tek codegen unit. Bu aşama sınıf semantiğini ekler;
adaptive inline cache veya JIT optimizasyonu içermez. CPython yalnızca doğruluk
oracle'ıdır, bu raporda hız karşılaştırması yapılmamıştır.

## Yöntem ve tekrar üretme

```sh
cargo bench -p tonic-runtime --bench interpreter --locked --offline --no-run
python3 benches/capture.py --output docs/benchmarks/stage3
```

[Harness](../crates/tonic-runtime/benches/interpreter.rs) 22 program içerir:
önceki 14 iş yükü ve 8 yeni instance/attribute/method programı. Her program için
parse/compile/verify ayrı ölçülür. Her GC modu ve compile için 2 ısınma + 15
kaydedilmiş örnek vardır. Otomatik GC aralığı 1024 allocation veya kapalıdır.
Her run yeni VM kullanır; compile, VM kuruluşu ve teardown run zamanının dışında,
Vm::run hazırlığı ve GC içindedir. Bu aynı VM'de JIT ısınma deneyi değildir.
Ana koşu, test ve differential süreçleri bittikten sonra alınmıştır.

İş yükleri sabit sırada çalışır. Masaüstü yükü, CPU modeli/frekansı/affinity ve
güç durumu kontrol edilmemiştir. Bu nedenle tablo tekrarlanabilir bir harness
kaydıdır; kontrollü makinede çoklu bağımsız oturum ölçümünün yerini tutmaz.

## Süre ve tahsisler

| İş yükü | Compile median µs | GC açık median µs | GC kapalı median µs | Guest allocation/run | Bytecode byte |
|---|---:|---:|---:|---:|---:|
| integer_loop | 29.125 | 4942.458 | 4528.166 | 0 | 152 |
| fib_40_x1000 | 20.083 | 2222.625 | 2226.166 | 1 | 336 |
| known_calls_10000 | 11.875 | 905.750 | 873.250 | 1 | 216 |
| native_calls_10000 | 9.000 | 1341.750 | 1312.083 | 0 | 176 |
| float_loop_10000 | 8.459 | 672.875 | 616.250 | 10002 | 152 |
| list_iteration_1000 | 14.375 | 204.542 | 207.417 | 1001 | 272 |
| closure_calls_10000 | 16.875 | 815.625 | 773.042 | 3 | 288 |
| closure_creation_5000 | 13.875 | 723.250 | 715.167 | 10001 | 256 |
| keyword_calls_10000 | 15.916 | 1130.583 | 1112.875 | 1 | 264 |
| variadic_calls_10000 | 15.542 | 3018.250 | 2598.291 | 30002 | 280 |
| expanded_calls_10000 | 18.041 | 2029.625 | 2082.250 | 10004 | 328 |
| dict_lookup_10000 | 13.125 | 871.583 | 870.000 | 5 | 256 |
| dict_insert_10000 | 9.500 | 1024.459 | 1010.042 | 1 | 168 |
| cyclic_garbage_10000 | 11.166 | 1320.584 | 1121.167 | 30000 | 168 |
| instance_creation_10000 | 8.333 | 649.625 | 612.250 | 10004 | 168 |
| initialized_instances_10000 | 13.333 | 1860.541 | 1743.458 | 20005 | 240 |
| attribute_load_10000 | 13.875 | 552.209 | 570.833 | 5 | 248 |
| attribute_store_10000 | 12.542 | 712.208 | 686.250 | 5 | 232 |
| bound_method_calls_10000 | 14.167 | 1097.875 | 1098.333 | 10006 | 272 |
| saved_method_calls_10000 | 15.791 | 810.375 | 806.334 | 7 | 288 |
| inherited_attribute_10000 | 15.417 | 828.833 | 807.417 | 9 | 296 |
| dictionary_attribute_10000 | 22.625 | 639.833 | 614.083 | 79 | 424 |

Bytecode byte, instruction sayısı × 8'dir; constant/span/call metadata dahil
değildir. Compile süresi parse + HIR/lowering + verification içerir; JIT compile
süresi değildir. Guest allocation sayısı run içindeki managed nesne tahsisidir;
VM kurulurken ayrılan builtins ve host allocator çağrıları bu sayıya dahil değildir.

Küçük integer döngüsünde allocation sayısı hâlâ sıfırdır. Normal/default/keyword
çağrılar geçici arg tuple/dict üretmez. Variadic callee kendi `*args/**kwargs`
nesnelerini oluşturur. Float aritmetiği henüz boxed sonuç, generic iteration
ise iterator nesnesi üretir. Dictionary/slot vektörü büyümesi host allocation
yapabilir; tek managed container allocation'ı toplam malloc sayısı değildir.

## Yeni nesne yollarının throughput'u

| Yeni iş yükü | Milyon iterasyon/s | Milyon dispatch/s | Peak register | GC açık peak heap byte |
|---|---:|---:|---:|---:|
| instance_creation_10000 | 15.393 | 184.742 | 7 | 84139 |
| initialized_instances_10000 | 5.375 | 102.129 | 11 | 100739 |
| attribute_load_10000 | 18.109 | 253.565 | 8 | 2339 |
| attribute_store_10000 | 14.041 | 196.598 | 8 | 2339 |
| bound_method_calls_10000 | 9.109 | 163.969 | 13 | 84318 |
| saved_method_calls_10000 | 12.340 | 209.805 | 12 | 2566 |
| inherited_attribute_10000 | 12.065 | 168.945 | 9 | 3035 |
| dictionary_attribute_10000 | 15.629 | 220.493 | 15 | 16854 |

İterasyon/s = 10.000 / tüm programın run süresidir. Class tanımı, başlangıç
atamaları, döngü kontrolü ve dictionary-mode hazırlığı bu süreye dahildir;
yalnız bir attribute load veya call için izole latency değildir. Dispatch/s,
VM opcode sayısıdır, CPU instruction veya Python işlem sayısı değildir.
`attribute_store` bir read-modify-write (`c.x += 1`) ölçer. `integer_loop`
100.000 module-scope iterasyonudur; saf local-register ölçümü değildir.

10.000 boş instance oluşturma 649.625 µs, `__init__` ile field atama 1860.541 µs
median sürmüştür. İlki 10.004, ikincisi 20.005 guest allocation üretir. İkinci
yolda instance yanında `__init__` bound-method nesnesi de oluşturulur; kalan
küçük fark class-body/function/metadata kuruluşundandır. Implicit `self`
argümanı ayrıca tuple/dict materialize etmez.

Her iterasyonda `c.add(i)` yapan program 1097.875 µs ve 10.006 allocation;
`f=c.add` ile metodu bir kez alıp `f(i)` yapan program 810.375 µs ve 7 allocation
ölçmüştür. Kaydedilmiş metot receiver/function kimliğini tutar. Bu fark gelecekte
method-call specialization için somut baseline'dır; henüz bu optimizasyon
uygulanmış değildir ve iki farklı kaynak programının karşılaştırmasıdır.

## Bellek ve GC

| İş yükü | Run sonu heap byte açık / kapalı | Ayrılmış nesne açık / kapalı | Collection | Toplanan nesne | Toplam pause median µs | En uzun tek pause µs |
|---|---:|---:|---:|---:|---:|---:|
| instance_creation_10000 | 66459 / 802227 | 822 / 10019 | 9 | 9197 | 42.665 | 6.250 |
| initialized_instances_10000 | 56611 / 1922411 | 584 / 20020 | 19 | 19436 | 148.460 | 15.750 |
| bound_method_calls_10000 | 66798 / 802486 | 825 / 10021 | 9 | 9196 | 35.127 | 5.541 |
| saved_method_calls_10000 | 2566 / 2566 | 22 / 22 | 0 | 0 | 0.000 | 0.000 |
| float_loop_10000 | 65908 / 801756 | 819 / 10017 | 9 | 9198 | 36.416 | 6.792 |
| cyclic_garbage_10000 | 31580 / 2801596 | 336 / 30015 | 29 | 29679 | 238.956 | 15.042 |

`resident_objects` run sonundaki ayrılmış heap girişleridir; erişilemez fakat
sonraki collection'ı bekleyen nesneleri de içerir. Run sonunda zorunlu final
collection yoktur. `gc_reclaimed` gerçekten serbest bırakılmış nesneleri sayar.

`gc_pause_median_us`, 15 run'ın **toplam GC sürelerinin median'ıdır**; tek
collection median'ı değildir. `gc_pause_max_us`, bütün ölçülmüş run'lardaki
en uzun tek collection'dır. Süre root toplama ve mark/compact işlemini içerir.
Diğer GC/bellek sayaçları median run örneğinden alınır. Nursery/old-generation,
write barrier ve bounded-pause garantisi henüz yoktur.

Tahmini heap byte, managed Object ve takip edilen payload kapasitesini içerir.
Allocator overhead, bütün hash-key/BigInt payload'ları, heap/handle/frame/register
tabloları, parser ve shared-shape metadata dahil değildir. Shape metadata
VM ömrünce kalır; 4096 transition, shape başına 64 field ve isim başına 1024 byte
bütçeleri aşılınca instance dictionary moduna geçer. Metadata limiti guest
attribute kaybına yol açmaz. Tahmini heap byte RSS yerine kullanılamaz.

Benchmark child process peak RSS **14.139.392 byte**; wall **1.422 s**, user
**1.031 s**, system **0.011 s**. Bunlar parser, bütün workload'lar, VM
kuruluş/yıkımları ve GC kapalı örnekleri içerir; iş yükü başına bellek değildir.
Cargo/rustc dahil değildir. 100.000 native scope create/resolve/drop için median
**2.715 ms**, min 2.612, max 2.741 ms; her örneğin sonunda active handle=0.

## Önceki 14 iş yükünde regresyon kontrolü

Sınıf değişikliğinden hemen önce aynı 14 programla alınan
[stage3-before.csv](benchmarks/stage3-before.csv) referanstır. Eski tarihsel
Stage 2 raporuyla karıştırılmamalıdır. Aşağıdaki iki koşuda otomatik GC açıktır;
pozitif oran daha uzun süreyi, yani yavaşlamayı gösterir.

| İş yükü | Önce median µs | Sonra median µs | Süre değişimi |
|---|---:|---:|---:|
| integer_loop | 3654.417 | 4942.458 | +35.2% |
| fib_40_x1000 | 1810.458 | 2222.625 | +22.8% |
| known_calls_10000 | 677.833 | 905.750 | +33.6% |
| native_calls_10000 | 1169.083 | 1341.750 | +14.8% |
| float_loop_10000 | 531.625 | 672.875 | +26.6% |
| list_iteration_1000 | 173.709 | 204.542 | +17.7% |
| closure_calls_10000 | 651.625 | 815.625 | +25.2% |
| closure_creation_5000 | 616.708 | 723.250 | +17.3% |
| keyword_calls_10000 | 891.667 | 1130.583 | +26.8% |
| variadic_calls_10000 | 2923.209 | 3018.250 | +3.3% |
| expanded_calls_10000 | 1903.458 | 2029.625 | +6.6% |
| dict_lookup_10000 | 787.208 | 871.583 | +10.7% |
| dict_insert_10000 | 925.375 | 1024.459 | +10.7% |
| cyclic_garbage_10000 | 1170.500 | 1320.584 | +12.8% |

**Bu koşuda bütün eski iş yükleri yavaşlamıştır.** Integer loop +%35.2, normal
function çağrısı +%33.6 dikkat gerektirir. GC kapalı integer loop da
3508.625 → 4528.166 µs olduğundan fark yalnız collection süresine bağlanamaz.
Eski iş yüklerinin bytecode/dispatch ve run içi guest allocation sayıları
korunmuştur. VM başlangıcındaki object/builtin metadata tahmini heap'i
611 → 1596 byte, ayrılmış girişleri 6 → 15 yapmıştır.

Örnekler gürültülüdür: yeni integer-loop GC-açık min/max 4534.041–8159.125 µs.
Buna rağmen regresyonu yalnız gürültü kabul edip geçmiyoruz. Bu dilim bir semantik
genişletmedir; performans iyileştirmesi olarak sunulmaz. Sınıf/call/frame ve
dispatch düzenindeki değişiklikler ile ortam etkisinin payı, kontrollü tekrar
ve gerçek profiler verisi olmadan ayrılamaz. Profiler/perf-counter ölçümü bu
raporda yoktur. Sonraki hot-path değişikliğinden önce regresyon tekrar üretilip
profile edilmelidir; ölçülmemiş unsafe dispatch/cache değişikliği eklenmemiştir.

## Doğrulama ve ham kayıtlar

108 Rust testi debug/release, 247 stdout + 45 exception vakası dört modda
(debug/release × normal/stress GC) geçti. Class scope, C3, binding, private names,
rebinding, GC root/edge/cycle ve shape fallback testleri vardır. Bunlar tam
Python conformance değildir; [doğrulama kapsamı](VALIDATION.md) ayrıca açıklanır.

- [44 satır özet CSV](benchmarks/stage3.csv).
- [1005 ham örnek](benchmarks/stage3-samples.csv): 660 run, 330 compile, 15 native scope.
- [Ortam/process manifesti](benchmarks/stage3-process.json): binary ve Cargo.lock SHA-256, RSS, wall/user/system.
- [Native scope kaydı](benchmarks/stage3.stderr.txt).
- [Değişiklik öncesi CSV](benchmarks/stage3-before.csv), [645 ham örnek](benchmarks/stage3-before-samples.csv), [manifest](benchmarks/stage3-before-process.json).

Henüz ölçülmeyenler: host allocation count/byte, kontrollü macrobenchmark,
descriptor/exception/generator yolları, quickening, JIT compile/code-size/deopt,
buffer/FFI/callback/CPython bridge. Olmayan alt sistemler sıfır maliyetli sayılmaz.
[Nihai benchmark planı](FINAL_BENCHMARK_PLAN.md), bu kabul kapıları kapandıktan
sonra uygulanacaktır. Sınıf modelinin sınırları [ADR 0003](adr/0003-classes-shapes.md)
içindedir.
