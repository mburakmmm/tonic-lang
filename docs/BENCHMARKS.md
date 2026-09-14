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
