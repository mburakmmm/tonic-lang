# ADR 0048 — CPython cross-collector graph tracing

## Durum

Kabul edildi — 14 Eylül 2026.

## Problem

Doğrudan `PyTonicProxy` wrapper'ı kendi Tonic kenarını bildirebiliyordu; fakat
arbitrary bir `ForeignPyObject` içindeki transitif proxy görünmüyordu. Örneğin

```text
Tonic Box -> ForeignPyObject(SimpleNamespace) -> PyTonicProxy -> Tonic Box
```

halkasında iki collector da yalnız kendi yarısını gördüğü için persistent proxy
kökü bütün halkayı yaşatıyordu.

## Karar

`ForeignPyObject` payload'ı CPython nesnesi yanında runtime kimliğini taşır. Her
major/minor GC öncesi adapter, GIL altında public `Py_tp_traverse` slotunu çağırarak
runtime'a ait foreign köklerden erişilen CPython grafiğini tarar. Aynı runtime'a
ait `PyTonicProxy` nesnelerinin non-rooting foreign-reference token'ları
`TonicTraceVisitor.visit_borrowed` ile Tonic graph edge olarak bildirilir.

Tarayıcı trial-deletion benzeri bir dış-kök testi uygular. Her düğümün CPython
reference count'undan taranan iç kenarlar, Tonic'in sahip olduğu foreign root
referansları ve doğrudan proxy wrapper referansları çıkarılır. Refcount, adapter
ile aynı `Python.h` sürümüne karşı derlenen dar bir C shim üzerinden `Py_REFCNT`
ile okunur. Böylece `sys.getrefcount` çağrısının sürüme göre değişen geçici
referansları doğruluk kararına girmez; CPython object layout Rust'a veya Tonic core
runtime'a taşınmaz. Artık referans kalmazsa proxy persistent kökü deferred olarak
düşürülür. Dış CPython referansı
bulunursa `TonicTraceVisitor.promote` non-rooting token'dan yeni persistent handle
üretir. Böylece daha önce zayıflatılmış bir proxy Python koduyla dışarı taşındığında
hedefi tekrar güçlü biçimde korunur.

Borrowed trace token'ı proxy payload'ına aittir. Managed foreign wrapper onu
yenilemeyi bıraktığında veya toplandığında release etmez; proxy `tp_dealloc`
callback'i doğru runtime-owner kuyruğuna tam bir kez bırakır. ABI v1 tablosuna
sondan eklenen `runtime_identity` ve `foreign_reference_release_deferred` alanları
ile `TONIC_CAP_CROSS_COLLECTOR_V1` bu sözleşmeyi görünür kılar.

Foreign finalization sırası da kesinleştirilmiştir:

1. wrapper collector tarafından logical dead yapılır;
2. payload destructor'ı traced Tonic kenarları hâlâ finalization root iken çalışır;
3. destructor döndükten sonra wrapper-owned foreign-reference handle'ları bırakılır;
4. deferred persistent/borrowed release kuyrukları güvenli VM sınırında boşaltılır.

## Güvenli sınırlar

Tarama 4.096 düğüm ve 16.384 kenarla sınırlıdır. Limit, traversal hatası veya başka
runtime'a ait proxy görüldüğünde collector güçlü kökü korur. Type, module ve Python
function altyapı grafikleri global interpreter durumuna açıldıkları için traversal
sınırıdır. Bu nesnelerde veya limiti aşan özel extension graph'larında davranış
conservative retention'dır; yanlış erken toplama yapılmaz.

## Kanıt

Testler transitif `SimpleNamespace -> proxy` halkasının explicit close olmadan tek
major koleksiyonda toplandığını ve `sys` modülündeki dış referansın hedefi güçlü
tutup silindiğinde serbest bıraktığını doğrular. Mevcut doğrudan proxy identity,
callback, exception, attribute ve shutdown testleri aynı sözleşmede geçer.

Ölçüm ve tekrar üretim komutu
[CROSS_COLLECTOR_BASELINE.md](../CROSS_COLLECTOR_BASELINE.md) içindedir.
