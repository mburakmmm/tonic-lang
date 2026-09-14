# ADR 0046 — CPython container dönüşümü ve proxy protokolleri

## Durum

Kabul edildi — 14 Eylül 2026.

## Karar

Native ABI iki ek capability ile genişletilir. `CONTAINER_ACCESS_V1`, logical
identity, bool/decimal BigInt, list/tuple/dict oluşturma ve içerik erişimini opaque
handle'larla sunar. `PROTOCOL_ACCESS_V1`, keyword dict alan callback, attribute
get/set ve repr işlemlerini normal VM semantiğine yönlendirir. Yeni girişler ABI
tablosunun sonuna eklenir; önceki prefix düzeni değişmez.

Tonic ile CPython arasında None/bool/int/BigInt/float/UTF-8 string doğrudan çevrilir.
List, tuple ve dict materialization'ı çağrıya özel identity memo tablosu kullanır.
Tonic list/dict ve CPython list/dict döngüleri korunur. Henüz staged immutable tuple
oluşturma bulunmadığından tuple üzerinden geri dönen cycle kesin `ValueError`dır.
Arbitrary CPython nesnesinin owned referansı yalnız başarılı `ForeignPyObject`
oluşturulduğunda açık transfer setine kaydedilir; cleanup kararı sonuç türünden
tahmin edilmez.

`PyTonicProxy`, `PyCFunction`/capsule bileşimi yerine CPython 3.12+
`PyType_FromSpec` negative-basicsize API'siyle gerçek heap type olarak kurulur.
Özel trailing storage yalnız `PyObject_GetTypeData` ile erişilen bir
`ProxyPayload*` taşır; bridge `PyObject_HEAD` düzenine bağımlı değildir.
`tp_call`, `tp_getattro`, `tp_setattro` ve `tp_repr` slotları persistent Tonic
handle'ını aktif runtime/execution scope'unda ödünç alır. Call positional ve keyword
değerlerini ordinary Tonic binder'a; attribute işlemleri property/custom descriptor
continuation'larına gönderir. Python'dan geri gelen aynı proxy özgün logical Tonic
handle'a açılır.

Keyword bulunmayan proxy çağrısı eski positional `call` ABI'sini kullanır. Ölçümde
her callback için boş Tonic dict yaratılması 100.000 guest allocation üretmişti;
bu dal ile sayı tekrar iki başlangıç allocation'ına iner. Keyword yolu semantik
gereği dict materialization maliyetini öder.

## Doğrulama

Gerçek libpython testleri BigInt/container dönüşümünü, insertion order'ı,
alias/list cycle'ını, named ve keyword çağrılarını; proxy property get, attribute
set, repr, keyword callback, logical-handle roundtrip, hata sınıfı ve lifecycle'ı
kapsar. Ayrı C ABI testi yeni capability tablosundan container ve protocol
girişlerini çalıştırır. Release bridge benchmarkı positional, keyword ve attribute
yollarını allocation sayaçlarıyla kaydeder.

## Sınırlar

Doğrudan proxy weak identity/root demotion protokolü
[ADR 0047](0047-cpython-weak-identity-cycle-demotion.md) ile eklenmiştir. Arbitrary
`ForeignPyObject` iç grafiklerinin genel cycle taraması bu kararın kapsamında
değildir. Explicit `close_proxy` politikası ADR 0045'te kalır.
