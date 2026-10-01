# Interpreter başlangıç ölçümleri

Bu dosya ilk dilimin tarihsel baseline'ıdır.
[PYTHON_COMPARISON_STAGE8.md](PYTHON_COMPARISON_STAGE8.md) Tonic–CPython ara
karşılaştırmasını içerir. Güncel descriptor ara baseline'ı
[STAGE7_BENCHMARKS.md](STAGE7_BENCHMARKS.md), slice ara baseline'ı
[STAGE6_BENCHMARKS.md](STAGE6_BENCHMARKS.md), property ara ölçümü
[STAGE5_BENCHMARKS.md](STAGE5_BENCHMARKS.md), decorator/method descriptor ölçümü
[STAGE4_BENCHMARKS.md](STAGE4_BENCHMARKS.md), önceki class/shape/method
ölçümü [STAGE3_BENCHMARKS.md](STAGE3_BENCHMARKS.md), closure/call/dict/GC ölçümü
[STAGE2_BENCHMARKS.md](STAGE2_BENCHMARKS.md),
CPython iki-collector graph tarama maliyeti
[CROSS_COLLECTOR_BASELINE.md](CROSS_COLLECTOR_BASELINE.md),
nihai benchmark kabul planı [FINAL_BENCHMARK_PLAN.md](FINAL_BENCHMARK_PLAN.md)
içindedir. Aşağıdaki arena/GC-yok durumu güncel runtime'ı anlatmaz.

31 Ağustos 2026, yerel macOS 26.6 ARM64, `rustc 1.86.0` stable.
Release: thin LTO, tek codegen unit. CPU modeli sandbox içinden okunamadı.
Bu tarihsel ilk kayıtta başka makineler veya Python ile hız karşılaştırması yapılmamıştır.

Komut:

```sh
cargo bench -p tonic-runtime --bench interpreter --locked
```

Kaynaklar bir kez compile/verify edilir. Her örnek fresh VM'de çalışır; compile,
VM kuruluşu ve destruction zaman dışında, `Vm::run` hazırlığı zaman içindedir.
İlk 2 örnek ısınma, sonraki 15 örneğin median/min/max değerleri kaydedilir.
Zaman aralığında başka uygulamalar çalışabilir; kontrollü laboratuvar ölçümü değildir.

| İş yükü | Median µs | Bytecode dispatch | Guest heap tahsisi |
|---|---:|---:|---:|
| 100.000 integer loop iterasyonu | 3498.416 | 1.300.010 | 0 |
| fib(40), 1.000 çağrı | 1681.250 | 623.010 | 1 |
| 10.000 Tonic fonksiyon çağrısı | 547.250 | 190.010 | 1 |
| 10.000 native fastmath çağrısı | 1230.375 | 160.010 | 0 |
| 10.000 float loop iterasyonu | 423.625 | 130.010 | 10.002 |
| 1.000 beş elemanlı list iterasyonu | 146.417 | 47.020 | 1.001 |

Tam tablo: [baseline.csv](baseline.csv). `dispatches_per_s` Python işlemi/s
veya native CPU instruction/s değildir; çalıştırılan VM bytecode instruction/s'dir.
List iteratörleri henüz generic heap nesnesidir. Float arithmetic henüz heap'e
boxed sonuç üretir. Bu sayılar tamamlanmış specialization/JIT izlenimi vermemelidir.

100.000 local scope create/resolve/drop: ana ölçümde 2.504 ms,
sonunda active handle = 0. Bu tek toplam süre olup 15-sample median değildir.
Native sınırdaki host handle vektörü tahsisleri **guest allocation** sayısına
katılmaz. İş yüklerindeki küçük integer aritmetiği guest nesnesi oluşturmaz;
bu, VM kuruluşu/register reserve gibi bütün host tahsislerinin sıfır olduğu
anlamına gelmez.

## Bellek ve ölçüm kapsamı

`estimated_heap_bytes`: managed Object boyutları ve bazı payload kapasite
hesaplarının yaklaşık toplamı. Allocator overhead, Vec arena yedek kapasitesi,
handle/frame tabloları, parser maliyeti ve bütün native modül metadata'sını ölçmez.
`peak_registers`: o run boyunca aynı anda ayrılmış VM register slot sayısı.
`bytecode_bytes`: instruction sayısı × 8; constants/spans/call metadata hariç.

Ayrı process ölçümü [baseline-process.json](baseline-process.json): benchmark
child process peak RSS 3.817.472 byte. Cargo/rustc hariç, parser/VM kuruluşu ve
bütün workload örnekleri dahildir; iş yükü başına bellek değildir. macOS'ta
`resource.getrusage(RUSAGE_CHILDREN).ru_maxrss` kullanılmıştır. `/usr/bin/time -l`
denemesinde `sysctl kern.clockrate` sandbox tarafından reddedildi; ham kayıt
[baseline-process.txt](baseline-process.txt) içinde tutuldu. RSS bunun yerine
child process resource usage API'siyle ölçüldü.

Henüz ölçülmeyenler: tam host allocation sayısı/byte, GC pause/throughput,
JIT compile time, native code size, cold/warm tier geçişi, shape/dict/closure/
exception/CPython bridge workload'ları. Olmayan subsystem için sıfır sonuç yazılmaz.

## İlk inceleme ve karşılaştırma

`+=` tip kontrolünün immediate değerde geçici TypeError oluşturan getter'ı
çağırdığı görüldü. Tip-probe yolu doğrudan tag/slot denetimiyle değiştirildi;
bu bir semantik değişiklik değildir. Önceki kayıt
[baseline-before-type-probe.csv](baseline-before-type-probe.csv) içindedir.
Integer-loop median 4020.125 → 3498.416 µs; ancak min değerleri benzer ve
örnekler arası gürültü yüksektir. Bu koşu tek başına belirli bir hızlanma oranını
kanıtlamaz. Sonraki ciddi optimization öncesi stabil benchmark ve profiler gerekir.

## `sum` builtin ara kabul ölçümü

1 Ekim 2026'da ADR 0088 için aynı kontrolsüz macOS ARM64 hostunda iki kalıcı
iş yükü eklendi. Her ikisi 1.000 öğelik diziyi 100 kez toplar. Bu bir nihai dil
benchmark'ı değildir; tamamlanmış yol haritası sonrasında
[FINAL_BENCHMARK_PLAN.md](FINAL_BENCHMARK_PLAN.md) ayrıca uygulanacaktır.

Interpreter harness'ında integer builtin yolu 1.006,667 µs median verirken aynı
işi guest `for` döngüsüyle yapan kontrol 12.882,500 µs verdi. Oran 12,80×'dir;
bytecode dispatch sayısı 701.218'den 1.618'e iner. İki iş yükü de kaynak liste ve
VM kurulumu dahil 103 guest allocation raporladı; builtin integer öğeleri için
öğe başına managed sayı ayırmadı. Compensated-float builtin yolu 1.759,333 µs ve
1.306 guest allocation verdi; bunların 1.000'i kaynak comprehension değerleri,
geri kalanı VM/runtime kurulumu ile 100 materialized sonuçtur.

| İş yükü/faz | Tonic median µs | CPython 3.14.6 median µs | Tonic / CPython |
|---|---:|---:|---:|
| integer `sum`, warm run | 1.012,021 | 208,167 | 4,8616× |
| float `sum`, warm run | 1.783,750 | 218,063 | 8,1800× |
| integer `sum`, cold CLI | 3.906,166 | 16.742,000 | 0,2333× |
| float `sum`, cold CLI | 4.734,125 | 16.824,167 | 0,2814× |

Warm throughput'ta CPython hâlâ belirgin biçimde öndedir; bu sonuç sonraki
profiling/JIT builtin çalışmalarına açık bir hedef verir. Cold CLI oranı Tonic'in
daha kısa process başlangıcını ölçer ve warm runtime üstünlüğü olarak okunmamalıdır.
Ham 30 warm ve 15 cold örnek, p95 değerleri, binary/source hash'leri ve sınırlamalar
[sum-stage-0088](benchmarks/sum-stage-0088/) altında saklanır.

## `any`/`all` builtin ara kabul ölçümü

ADR 0089'un kalıcı iş yükü, 1.000 `False` ve 1.000 `True` öğeyi `any` ve
`all` ile 100 kez tam tarar. Release interpreter medianı 6.596,541 µs'dir;
eşdeğer konuk `for` döngüleri 28.471,083 µs sürer. Builtin yol 4,32× daha hızlıdır
ve bytecode dispatch sayısını 1.114.740'tan 14.340'a indirir. Her iki yol da
kaynak listeler ve VM kurulumu dahil 208 guest allocation raporlar; builtin
tarama öğe başına managed nesne üretmez.

| İş yükü/faz | Tonic median µs | CPython 3.14.6 median µs | Tonic / CPython |
|---|---:|---:|---:|
| `any` + `all`, warm run | 6.788,479 | 341,438 | 19,8821× |
| `any` + `all`, cold CLI | 10.046,125 | 16.846,625 | 0,5963× |

Warm taramada CPython belirgin biçimde öndedir. Cold CLI sonucu süreç başlatma,
parse, compile ve yürütmeyi birlikte ölçer; runtime throughput üstünlüğü olarak
yorumlanmaz. Bu ölçüm nihai dil benchmark'ı değildir. Ham örnekler, p95 değerleri,
binary/source hash'leri ve sınırlamalar
[any-all-stage-0089](benchmarks/any-all-stage-0089/) altında saklanır.

## `min`/`max` builtin ara kabul ölçümü

ADR 0090 iş yükü 1.000 integer öğenin minimum ve maksimumunu 100 kez tarar.
Release interpreter builtin medianı 11.838,292 µs, eşdeğer konuk döngüsü
56.080,458 µs'dir; builtin 4,74× daha hızlıdır. Dispatch 1.402.018'den 2.318'e
iner. Kaynak/VM dahil builtin 203 allocation raporlar ve öğe başına tahsis yapmaz.

| Faz | Tonic median µs | CPython 3.14.6 median µs | Tonic / CPython |
|---|---:|---:|---:|
| warm run | 12.168,771 | 1.377,188 | 8,8360× |
| cold CLI | 15.571,042 | 18.265,125 | 0,8525× |

Warm CPython üstünlüğü sonraki profiling/JIT builtin çalışması için açık hedeftir.
Cold CLI süreç başlangıcı dahil toplamdır. Ham örnekler ve provenance
[min-max-stage-0090](benchmarks/min-max-stage-0090/) altındadır.
