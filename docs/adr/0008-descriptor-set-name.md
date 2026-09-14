# ADR 0008 — Descriptor set_name class continuation

## Karar

Class body başarıyla döndüğünde namespace önce gerçek Class nesnesine çevrilir.
Sınıfın kendi final attribute tablosundaki descriptor instance'ları tanım sırasıyla
taranır. Descriptor sınıf/MRO'sunda `__set_name__` varsa normal call binder üzerinden
`descriptor.__set_name__(owner, name)` çağrılır. Bütün callback'ler bittikten sonra
class decorator ve dış bytecode yürümeye devam eder.

Inherited attribute'lar yeniden çağrılmaz. Class oluşturulduktan sonraki attribute
rebind otomatik callback üretmez. Aynı descriptor birden fazla ad altında bulunursa
her final ad için sırayla çağrılır. Callback dönüş değeri yok sayılır; hata normal
guest çağrı hatası olarak class statement'tan dışarı yayılır.

## Continuation ve GC

Callback Tonic function olduğunda interpreter yeni frame açar. Frame'in
`SetNames` return action'ı tamamlanmış class ile kalan callback listesini taşır.
Precise root taraması class, future callable, optional receiver ve name değerlerinin
tamamını ziyaret eder. Böylece callback allocation yapıp moving collection
tetiklese bile sonraki işler stale handle içermez. Senkron builtin/native callback
aynı continuation helper'ında döngüsel olarak tamamlanır.

## Sınırlar

Bu karar metaclass `__new__/__init__`, mappingproxy
ve class namespace customization sağlamaz. Callback listesinin hazırlanması class
attribute tablosunun final snapshot'ını kullanır.

## Doğrulama

Katman ve CPython differential testleri body/callback/decorator sırasını, birden
fazla descriptor'ı, inheritance, geç rebinding ve hata yayılımını doğrular. Katman
testindeki callback'ler stress GC altında heap allocation yaparak pending continuation
köklerini hareketli collection boyunca sınar.

Descriptor/property deletion daha sonra [ADR 0009](0009-attribute-deletion.md)
ile eklenmiştir.
