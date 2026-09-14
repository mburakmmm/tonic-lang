# ADR 0007 — Custom descriptor resolution without temporary methods

## Karar

Tonic kullanıcı instance'ları sınıflarında `__get__` ve/veya `__set__` tanımlayarak
descriptor olabilir. Instance attribute okuması şu sırayı izler: property, custom
data descriptor, instance shape/dict alanı, custom non-data descriptor, normal class
attribute/function binding. Class okuması `__get__(None, owner)` çağırır; inherited
descriptor'a verilen owner gerçek erişim sınıfıdır.

Özel metot descriptor instance'ından değil, onun sınıf/MRO tablosundan aranır.
Çözümleme callable ile örtük receiver'ı ayrı döndürür. Call binder bu receiver'ı
doğrudan yeni frame register'ına koyar; iki protokol argümanı sabit boyutlu inline
alanda taşınır. Böylece gözlemlenemeyen geçici `BoundMethod`, host `Vec`, guest
argument tuple veya keyword dict ayrılmaz. `__set__` dönüşü Python gibi yok sayılır.

## Invalidation ve GC

Her erişim güncel class tablolarından çözülür; dolayısıyla descriptor sınıfı veya
owner sınıfı rebinding'i hemen görünür ve şimdilik cache invalidation gerektirmez.
Callable, receiver, instance ve owner frame/argument register'larında kesin GC
kökleridir. İleride inline cache eklendiğinde hem owner class version hem descriptor
class version guard edilmelidir.

## Sınırlar

Metaclass descriptor'ları,
`__getattribute__` ve `__getattr__` henüz yoktur. Guest exception handler olmadığı
için descriptor üzerinde `hasattr` ve default'lu `getattr` açıkça
`UnsupportedFeature` verir.

## Doğrulama

CPython differential testleri data/non-data önceliğini, class/instance erişimini,
inheritance, `getattr/setattr`, setter dönüşünü ve çağrılamayan hook hata türlerini
default/stress GC altında karşılaştırır. Bir katman testi 1 ve 1.000 descriptor
okumasının aynı guest allocation sayısına sahip olduğunu doğrular.

Otomatik `__set_name__` class completion davranışı daha sonra
[ADR 0008](0008-descriptor-set-name.md) ile eklenmiştir.
Attribute/property deletion daha sonra [ADR 0009](0009-attribute-deletion.md) ile
eklenmiştir.
