# ADR 0009 — Attribute deletion and descriptor deleters

## Karar

Python `del owner.name` ifadeleri Tonic AST'de yalnız attribute hedefleri olarak
saklanır ve bytecode v5 `DelAttr(owner, symbol)` talimatına düşürülür. Çoklu hedefler
soldan sağa ayrı talimatlarla değerlendirilir. Name, item ve slice deletion bu
dilimde açık `UnsupportedSyntax` verir.

Instance deletion önceliği property deleter, custom data-descriptor `__delete__`,
sonra normal instance alanıdır. Yalnız `__delete__` tanımlayan descriptor assignment
için eksik `__set__`; yalnız `__set__` tanımlayan descriptor deletion için eksik
`__delete__` hatası üretir. Böylece iki hook da data-descriptor statüsünü belirler.
Class attribute deletion descriptor instance'ını kaldırır; instance `__delete__`
çağırmaz ve class version'ı artırır.

Slot tabanlı instance ilk silmede dictionary moda geçer; kalan değerler korunur.
Bu basit ve doğru bootstrap yolu shape'i geriye doğru değiştirmez. Property nesnesi
`fdel` ve `.deleter` sunar; üç positional argümanlı `property(fget, fset, fdel)`
desteklenir. Deleter dönüş değeri yok sayılır.

## Güvenlik ve doğrulama

`DelAttr` doğrulayıcısı owner register, symbol ve ayrılmış operandı kontrol eder.
Deleter normal frame/call binder yolunu kullanır; custom hook receiver ve owner
sabit boyutlu inline argüman alanında taşınır. CPython differential testleri custom
ve property deleter, normal instance/class deletion, çoklu hedef sırası ve hata
türlerini default/stress GC altında karşılaştırır.

## Sınırlar

`del name`, item/slice deletion, `delattr` builtin'i, metaclass descriptor deletion
ve custom `__delattr__` henüz yoktur.
