# ADR 0044 — İzole CPython bridge ilk dilimi

Durum: kısmen uygulanmış. 13 Eylül 2026.

## Karar

CPython kodu `tonic-cpython` adlı ayrı crate'te kalır. Crate build sırasında
`PYTHON_CONFIG` veya `python3-config --ldflags --embed` çıktısıyla CPython 3.12+
bağlanır; tonic-runtime ve JIT hiçbir `PyObject` tipi, refcount işlemi veya CPython
execution lock'u içermez.

Her CPython girişi lazy serialized initialization ardından
`PyGILState_Ensure/Release` guard'ı kurar. Embedded interpreter süreç sonuna kadar
yüklü tutulur; bu aşamada `Py_Finalize` çağrılmaz. Tonic int/float/UTF-8 string
değerleri sırasıyla gerçek `PyLong`, `PyFloat` ve `PyUnicode` üzerinden iki yönlü
dönüştürülür. İlk callable örneği `PyNumber_Absolute` kullanır.

CPython-owned arbitrary nesne, adapter kimliği sabit bir generic foreign wrapper
olarak Tonic'e girer. Wrapper bir owned `PyObject*` referansı taşır; GC/shutdown
finalization kuyruğunda CPython execution state'i altında `Py_DecRef` edilir.
`foreign_borrow_payload` yalnız beklenen adapter kimliğiyle ve GC çalışamayan aktif
native scope boyunca raw payload borrow verir. Örnek gerçek bir `PyList` üretir ve
`PyObject_Length` ile yeniden CPython'a geçirir.

C ABI ayrı `TonicPersistentHandle` create/borrow/release işlemleri taşır. İlk
`PyTonicProxy` gerçek bir CPython callable'dır. Self capsule'ı persistent token,
VM adresi taşımayan ref-counted runtime owner ve execution kimliği tutar. Proxy
Tonic heap'inde `ForeignPyObject` olarak yaşayabilir; çağrıldığında thread-local
aktif bridge context'ini, runtime owner'ı ve execution kimliğini guard edip
positional primitive/foreign argümanları `api.call` ile Tonic frame'ine geri döner.
Capsule destructor'ı release'i owner'ın
kuyruğuna bırakır; owning VM bu kuyruğu native/GC/shutdown safepoint'lerinde boşaltır.

`python.call1(module, name, value)` `None`, bool, i64, float, UTF-8 string ve mevcut
`ForeignPyObject` girdilerini CPython'a taşır. Aynı primitive sonuçları Tonic'e
çevirir; diğer owned `PyObject*` sonuçlarını generic foreign wrapper'a aktarır.

## Kanıt

Integration testi integer/float/Unicode dönüşümünü, CPython call sonucunu,
`ForeignPyObject` adapter guard'ını ve Tonic GC sonrası deferred `Py_DecRef`
sayacını kapsar. CPython exception indicator owned exception nesnesine alınır,
`traceback.format_exception` ile tür/traceback metnine, ardından `PythonError`
tanısına çevrilir ve sonraki bridge çağrısı temiz state ile çalışır. Callback'teki
Tonic exception da CPython `RuntimeError` üzerinden geri çevrilir. 100.000 callback
41,807 ms, genel dönüşümlü isimli CPython çağrısı 69,301 ms medyandır. Yöntem
[`CPYTHON_BRIDGE_BASELINE.md`](../CPYTHON_BRIDGE_BASELINE.md) içindedir.

## Sonraki karar

BigInt/container/keyword dönüşümü ve proxy attribute/set/repr protokolleri
[ADR 0046](0046-cpython-container-proxy-protocols.md) ile tamamlanmıştır. Weak
identity cache ve iki collector arasındaki otomatik cycle/finalizer politikası
ayrıca açık kalır.
