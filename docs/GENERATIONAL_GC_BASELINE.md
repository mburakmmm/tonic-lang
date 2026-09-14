# Generational GC ara baseline

Bu ölçüm M5 generational kabul kapısı içindir; dil tamamlandıktan sonra alınacak
Python karşılaştırmalı nihai benchmark değildir.

## Yöntem

9 Eylül 2026'da macOS ARM64 üzerinde Rust stable 1.86.0 ile
`cargo bench -p tonic-runtime --bench generational_gc --locked --offline`
çalıştırıldı. Her vaka 3 warmup ve 15 ölçüm örneği kullanır. VM başına collection
aralığı 128 allocation'dır. Tablodaki süre 15 örneğin medyanıdır; min/max aynı
koşunun gözlenen sınırlarıdır. Sayaçlar medyan süreli örnekten alınır.

Karşılaştırılan eski sürüm her collection'da bütün heap'i tarıyordu. Yeni sürüm
31 minor ardından bir major çalıştırır; nursery survivor'larını ilk minor'da old
alana terfi ettirir.

## Wall time

| İş yükü | Eski full-heap medyan | Generational medyan | Değişim |
|---|---:|---:|---:|
| 100.000 geçici tek elemanlı list | 14.152 ms | 12.073 ms | %14,7 daha hızlı |
| 100.000 old-list → young-list yazımı | 15.097 ms | 13.964 ms | %7,5 daha hızlı |
| 100.000 erişilemeyen self-cycle | 25.550 ms | 20.934 ms | %18,1 daha hızlı |

## Generational sayaçlar

| İş yükü | Toplama | Minor | Major | Terfi | Reclaimed | Moved |
|---|---:|---:|---:|---:|---:|---:|
| Geçici list | 781 | 757 | 24 | 1.581 | 99.921 | 1.565 |
| Old→young slot | 781 | 757 | 24 | 1.582 | 99.920 | 1.566 |
| Self-cycle | 2.343 | 2.270 | 73 | 6.267 | 299.863 | 6.251 |

## Pause ölçümü

| İş yükü | Eski toplam pause | Yeni toplam pause | Yeni max pause |
|---|---:|---:|---:|
| Geçici list | 1.426 ms | 1.350 ms | 3,250 µs |
| Old→young slot | 1.412 ms | 1.396 ms | 9,500 µs |
| Self-cycle | 4.249 ms | 3.954 ms | 12,208 µs |

Max değerleri yalnız 15 örnekli tek yerel koşunun uç değeridir; p95/p99 veya
bounded-pause garantisi değildir. Ana sonuç, üç iş yükünde de wall time ve toplam
collector pause'un azalmasıdır. Old→young vaka write barrier ile remembered-set
tarama maliyetini içerir; self-cycle vaka periyodik major collection'ın old-space
çöpünü gerçekten topladığını doğrular.

Son koşunun ham çıktısı:

```text
case,median_us,min_us,max_us,collections,minor,major,promoted,reclaimed,moved,pause_us,max_pause_us
ephemeral_lists_100000,12073.209,11835.167,12488.417,781,757,24,1581,99921,1565,1350.091,3.250
old_to_young_slot_100000,13964.166,13760.625,15932.959,781,757,24,1582,99920,1566,1396.028,9.500
cyclic_garbage_100000,20934.041,20331.875,21975.542,2343,2270,73,6267,299863,6251,3954.093,12.208
```
