# ADR 0055: Canonical builtin type nesneleri

## Durum

Kabul edildi ve uygulandı.

## Karar

VM, primitive ve temel heap değerleri için logical class handle'larından oluşan
tek bir `RuntimeTypes` tablosu tutar. Tablo precise GC root setine dahildir; nesne
adresleri, Rust enum düzeni veya CPython layout'u type kimliğine dönüşmez.
`runtime_class` hem tek argümanlı `type` hem `isinstance` için canonical kaynaktır.
Class değerleri metaclass'larına, instance'lar kendi class'larına; immediateler ve
temel heap türleri ilgili builtin class'a eşlenir.

`bool` class'ı `int` tabanlıdır. Böylece `isinstance(True, int)` ve
`issubclass(bool, int)` özel koşul olmadan C3 MRO üzerinden doğrudur. NoneType ve
function type nesneleri yalnızca introspection için köklenir; Python global
namespace'inde bulunmayan bu tip adları builtin olarak yayımlanmaz.

Int, float, bool, str, list, tuple, dict ve range class'ları callable
constructor'lardır. `range` adı ile `type(range(...))` aynı canonical class
handle'ını paylaşır; ayrı bir legacy builtin-function kimliği yoktur.
Bool custom `__bool__`/`__len__` için mevcut suspending truth continuation'ını
kullanır. Numeric parse/overflow hataları guest ValueError/OverflowError olur;
container constructor'ları yeni storage üretir ve alias paylaşmaz. `int` explicit
2..36/auto-detect base'i, list/tuple mevcut builtin iterable'ları ve `dict`
mapping, iterable-pair ve keyword girdilerini destekler.

## Sınırlar

Builtin class alt sınıfı oluşturmak mümkün olsa da native primitive storage üreten
subclass constructor semantiği henüz yoktur. User-defined `__iter__`/`__next__`
protokolü ve henüz bulunmayan bytes-like girdiler constructor kapsamını sınırlar.

## Doğrulama

Katman ve CPython differential testleri canonical type adlarını, bool/int MRO'sunu,
tuple classinfo'yu, custom truth continuation'ını, temel conversions ve hata
türlerini normal/stress GC ile interpreter/JIT yollarında kapsar. Corpus 275 çıktı
ve 96 exception vakasına genişletilmiştir.
