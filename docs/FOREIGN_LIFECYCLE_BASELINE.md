# Foreign wrapper yaşam döngüsü ara ölçümü

12 Eylül 2026; Apple Silicon arm64, macOS 26.6, Rust 1.86.0, release LTO.
Bu sonuç tamamlanma sonrası nihai benchmark değildir.

Komut:

```sh
cargo bench -p tonic-runtime --bench foreign_lifecycle --locked --offline
```

Her varyant 100.000 geçici managed nesne üretir; parse/compile timer dışındadır,
otomatik GC kapalıdır. `managed_list` bir öğeli Tonic listeleri oluşturur.
`owned_foreign` her çağrıda C function-table yolundan bir managed foreign wrapper
ve ayrı `Box<u64>` payload oluşturur. İkinci timer full major collection ile bütün
geçici nesneleri toplar; foreign kuyruğu 10.000 destructor'ı collector sweep'i
bittikten sonra çalıştırır. Üç warmup ve 15 örnekten toplam sürenin medyanı alınır.

| Tür | Oluşturma | Collection/finalization | Toplam | Guest allocation | Native call | Destructor |
|---|---:|---:|---:|---:|---:|---:|
| Managed list | 17,375 ms | 2,295 ms | 19,670 ms | 100.000 | 0 | 0 |
| Owned foreign | 29,368 ms | 4,307 ms | 33,675 ms | 100.000 | 100.000 | 100.000 |

Bu kontrollü no-trace payload koşulunda owned foreign toplamı list baseline'ından
yaklaşık %71,2 daha uzun sürer. Bu fark C function-table geçişini, `Box` host
tahsisini ve deferred destructor çağrısını birlikte içerir. Ölçüm host allocator
byte sayısını ayırmaz ve gerçek
bir dış runtime destructor'ının maliyetini temsil etmez; yalnız Tonic wrapper,
C callback, queue ve tam-bir-kez cleanup taban maliyetini gösterir. Trace edilen
managed kenarlar ayrıca correctness/stress-GC testlerinde ölçülür.
