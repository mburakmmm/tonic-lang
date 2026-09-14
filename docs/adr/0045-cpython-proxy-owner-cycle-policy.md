# ADR 0045 — CPython proxy owner ve cycle politikası

Durum: kısmen uygulanmış. 13 Eylül 2026.

## Karar

`PyTonicProxy` hiçbir `Vm*` adresi tutmaz. C ABI'nin ref-counted opaque
`TonicRuntimeOwner` nesnesi yalnız runtime kimliği, dead durumu ve deferred
persistent-release kuyruğunu taşır. Proxy ayrıca yaratıldığı `Vm::run` execution
kimliğini kaydeder. Callback ancak thread-local aktif `TonicContext`, aynı owner ve
aynı execution birlikte doğrulanırsa Tonic'e girer. Böylece taşınmış/düşmüş VM
adresine erişim ve yeni programın code tablosunda eski callable yürütme engellenir.

Proxy Tonic tarafında ayrı adapter kimlikli bir `ForeignPyObject` içinde yaşar.
Normal acyclic yaşamda wrapper GC ile toplandığında `Py_DecRef` capsule destructor'ını
çalıştırır; destructor persistent token'ı owner kuyruğuna bırakır. VM native dönüşü,
GC ve shutdown safepoint'lerinde yalnız kendi kuyruğunu boşaltır. Runtime dead ise
destructor callback yapmaz; handle table zaten yok edilmiştir.

İki bağımsız collector arasındaki cycle otomatik toplanmış sayılmaz. Örneğin proxy'nin
tuttuğu persistent callable bir closure üzerinden aynı proxy wrapper'a dönebilir.
Bu sürümde açık politika, böyle bir graph kurulurken `python.close_proxy(proxy)`
çağırmaktır. Close idempotenttir, persistent anchor'ı doğru owner kuyruğuna bırakır
ve sonraki çağrı `PythonError` içinde "PyTonicProxy is closed" tanısı üretir.

## Kanıt

Integration testi proxy→persistent callable→closure/list→proxy döngüsünü kurar,
`close_proxy`yi iki kez çağırır, kapalı çağrının tanısını doğrular, yeni execution
sonrası major collection çalıştırır ve sıfır aktif handle ile exactly-once foreign
destructor gözler. Ayrı callback testi iki positional argümanı stress GC altında
Tonic frame'ine yönlendirir.

## Açık kalanlar

Weak proxy identity cache yoktur; aynı Tonic değerinin tekrarlı ihracı aynı Python
nesne kimliğini garanti etmez. CPython'ın proxy'yi dış bir container'da sakladığını
otomatik saptayan escape/cycle protocolü de yoktur. Bu nedenle explicit close
gerektiren cycle davranışı kullanıcıya görünür bir compatibility-mode kısıtıdır.
