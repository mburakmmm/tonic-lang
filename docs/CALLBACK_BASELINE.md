# Persistent callback/reentry ara ölçümü

12 Eylül 2026; Apple Silicon arm64, macOS 26.6, Rust 1.86.0, release LTO.
Bu sonuç tamamlanma sonrası nihai benchmark değildir.

Komut:

```sh
cargo bench -p tonic-runtime --bench callback --locked --offline
```

Her iki varyant `add(value): return value+1` gövdesini 10.000 kez çalıştırır. Guest
varyantı çağrı ve sayacı Tonic while loop'unda yürütür. Host reentry varyantı
callable ve argument için persistent handle kurulumunu timer dışında yapar; her
ölçülen çağrıda `Vm::call_persistent` ile frame/root scope kurar, persistent result
üretir ve sonucu release eder. Parse/compile iki yolda da timer dışındadır. Üç
warmup ve 15 örnek alınır.

| Giriş | Medyan | Min–maks | Çağrı/s | Callback sayacı |
|---|---:|---:|---:|---:|
| Tonic guest loop | 2,207 ms | 1,719–3,552 ms | 4.530.697 | 0 |
| Host persistent reentry | 1,106 ms | 0,999–1,305 ms | 9.042.270 | 10.000 |

Bu tablo callback'in guest call'dan iki kat ucuz olduğu anlamına gelmez: guest
varyantında 10.000 while karşılaştırması, branch ve sayaç güncellemesi de ölçülür;
host varyantında loop Rust'tadır. Sonuç callback yolunun tek başına yaklaşık
110,6 ns/call medyan throughput verdiğini ve persistent result release dahilken
10.000 çağrıda kararlı kaldığını gösterir. Native→Tonic aktif nested reentry ayrıca
stress-GC correctness testidir; bu mikrobenchmark idle-runtime callback yolunu ölçer.
