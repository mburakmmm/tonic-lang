# ADR 0041 — Non-moving typed buffer ve owner descriptor

Durum: uygulanmış. 12 Eylül 2026.

## Karar

İlk buffer storage türü C-contiguous f64'tür. Data `Box<[f64]>`, shape
`Box<[usize]>`, byte strides `Box<[isize]>` olarak GC heap nesnesinden ayrı tahsis
edilir. GC küçük owner nesnesini taşıyabilir; üç backing pointer değişmez. Shape
çarpımı, byte stride taşması ve rank sınırı allocation öncesinde doğrulanır.

Public descriptor fixed-width dtype kimliği, data/byte length/item size,
rank/shape/stride, writable ve C-contiguous flags ile opaque owner handle taşır.
Her C export ayrı `BufferOwner` handle üretir. Release yalnız bu handle sınıfını
kabul eder, descriptor'ı temizler ve stale/double release'i tanıya çevirir.
Read-only storage writable olarak export edilemez. Bu sürümde native scope boyunca
reentry ve GC yasak olduğundan view pointer'ları scope sonuna kadar geçerlidir.

Rust native facade lifetime bağlı `F64BufferView` döndürür. `fastmath.array`
liste/tuple sayıları bir kez buffer'a kopyalar; `fastmath.sum` typed slice'ı
eleman boxing/unboxing ve yeni buffer copy olmadan tarar. Copy ve export sayaçları
interop maliyetini görünür kılar.

## Ölçüm

1.024 elemanın 10.000 kez toplamında boxed liste 46,920 ms, typed buffer 9,401 ms
medyan verdi; yaklaşık 4,99× hızlanmadır. 10.000 export için bir ilk copy vardır.
Ayrıntı [`BUFFER_BASELINE.md`](../BUFFER_BASELINE.md) dosyasındadır.

## Sınırlar

Diğer dtype storage'ları, non-contiguous/sliced view üretimi, external owner ve
callback boyunca uzun ömür sonraki capability sürümleridir. ABI dtype kimlikleri
Tonic class/type ID'lerinden bağımsız kalır.
