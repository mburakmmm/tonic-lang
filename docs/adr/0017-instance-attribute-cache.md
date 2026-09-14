# ADR 0017 — Monomorphic instance attribute cache

## Durum

Kabul edildi; slot-backed instance attribute okumaları için uygulanmıştır.

## Karar

Bir `Attr` sitesi sekiz kez aynı class, shape, slot ve class dependency version ile
normal instance alanı okursa `AttrSlot` olur. Fast path şu guard'ları taşır:

- owner exact instance'dır ve class logical handle'ı cache ile aynıdır;
- instance hâlâ slot storage kullanır ve shape ID aynıdır;
- slot indeksi mevcut values alanındadır;
- exact class dependency version değeri cache ile aynıdır.

Guard'lar geçerse okuma bir shape karşılaştırması ve slot load'dur. Cache yalnız
normal `heap.attr` yolundan gerçekten instance slotu döndükten sonra kurulur.
Property, custom data/non-data descriptor, `super`, class/module ve dictionary-mode
okumaları cache kurmaz.

Her class attribute set/delete owner ve mevcut descendant class version'larını
artırır. Weak descendant metadata ve GC pruning ayrıntıları sonradan kabul edilen
[ADR 0024](0024-class-dependency-invalidation.md) ile bu ADR'yi granüler
invalidation yönünde değiştirir. Guard failure mevcut okumayı baştan generic descriptor önceliğiyle
yürütür. Test, cache kurulduktan sonra class'a aynı isimde property ekleyip instance
slotunun artık property'yi gölgeleyemediğini doğrular.

Cached class handle root değildir. Canlı instance class'ını zaten trace eder;
toplanmış handle generation değişimi nedeniyle yeni class ile yanlış eşleşmez.
İki girişli polymorphic cache [ADR 0023](0023-two-entry-pic.md) ile eklenmiştir.

## Ölçüm

100.000 instance slot okuması release A/B ölçümünde generic 13.632 ms'den
adaptive 9.522 ms'ye indi; süre yaklaşık %30,2 azaldı. Ölçümde guest allocation
sayısı değişmedi.
