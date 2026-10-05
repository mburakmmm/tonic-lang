# ADR 0102 — Per-runtime HPyGlobal ve precise HPyField kenarları

## Durum

Kabul edildi ve H3a'da uygulandı.

## Bağlam

HPy Universal extension'daki `HPyGlobal` declaration storage'ı shared library'ye
aittir; Tonic değeri doğrudan bu C word'üne yazılırsa iki VM aynı root'u paylaşır
ve moving GC logical-handle sınırı bozulur. `HPyField` ise bağımsız bir root
değildir. Alanın değeri yalnız owner yaşadığı sürece trace edilmeli, old→young
mutation write barrier'a girmeli ve field-only cycle toplanabilmelidir.

## Karar

Module definition'ın null-terminated global declaration listesi load sırasında
doğrulanır. C storage'a yalnız module-local opaque slot yazılır. Asıl değer VM'de
`(module identity, slot)` anahtarıyla tutulur ve explicit root listesine katılır.
Global clear root'u kaldırır; VM shutdown bütün native global root'larını final
collection öncesinde temizler.

`HPyField_Store` sıfır alana process-unique logical field token'ı verir. Runtime
değeri heap içindeki `(owner Value, field token)` metadata kenarında saklar. Owner
ve değer raw adres değil generation doğrulamalı logical `Value` olduğu için heap
compaction tabloyu yeniden yazmaz. Store ortak heap write barrier'ını çağırır.
Major/minor marker, object içi trace kenarlarına ek olarak owner'ın external field
kenarlarını gezer; sweep ölü owner'ların field metadata'sını siler. Clear kenarı
kaldırıp public `HPyField_NULL` gözlenebilirliğini geri getirir.

## Sonuçlar

Aynı module state'i kullanan iki Tonic VM'nin global değerleri ayrıdır. Global
değer cycle olsa bile store süresince root'tur ve clear sonrasında toplanır.
Field değeri owner üzerinden moving GC'den çıkar; old owner'dan nursery değere
store remembered set'e girer. Owner↔field-value cycle'ı dış root yoksa toplanır.

Bu aşama extension type/native payload yerleşimi ilan etmez. H4 gelene kadar field
owner'ı mevcut bir Tonic heap nesnesidir. `HPyTracker`, non-zero module C state ve
payload teardown ayrı açık kabul kapısıdır.
