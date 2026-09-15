# ADR 0049 — Tonic HPy Universal host

## Durum

Kabul edildi — 15 Eylül 2026. Uygulama aşamalı olarak bekliyor.

## Problem

Tonic-native C ABI hızlı ve moving-GC uyumludur, fakat yalnız Tonic API'sine göre
derlenen uzantıları çalıştırır. Mevcut CPython bridge geniş bir kaçış yolu sağlar;
libpython, `PyObject` ownership, conversion ve proxy maliyetini taşır. Cython tabanlı
native ekosistemi Tonic nesne düzenini CPython'a çevirmeden çalıştıracak taşınabilir
bir uzantı yolu eksiktir.

## Karar

Tonic, ayrı `tonic-hpy` adapter/loader katmanında HPy Universal ABI host etmeyi
hedefler. `.hpy0` uzantıları Tonic'in sürümlenmiş `HPyContext` uygulamasını alır;
context işlemleri Tonic'in scoped handle, persistent root, object protocol,
exception, buffer ve native type kayıt API'lerine çevrilir.

aHPy tarafından `--runtime-backend=hpy-universal` ile üretilen uzantılar ana
uyumluluk ve gerçek-dünya pilot kaynağıdır. Handwritten HPy örnekleri adapter
hatalarını aHPy codegen hatalarından ayıran alt katman oracle'ı olarak korunur.

Tonic core `Value` ve heap layout'ı HPy tiplerine dönüştürülmez. HPy isimleri ve
layout'ları adapter crate'inin dışında kullanılmaz. `HPy` ile `TonicHandle` binary
eşitliği varsayılmaz; tek-indirection eşleme yalnız sürüm, lifetime, null, runtime
identity ve Debug Mode testleri kanıtlarsa seçilir.

## Öncelik

1. Tonic-native ABI;
2. HPy Universal host;
3. aHPy cross-runtime uyumluluk;
4. gerekirse Limited API/abi3 facade;
5. legacy full CPython ABI için mevcut bridge veya ayrı compatibility modu.

Full CPython object layout emülasyonu HPy milestone'unun parçası değildir.

## Kabul kapıları

- Minimal `.hpy0` modül libpython olmadan yüklenir ve scalar Fibonacci çalıştırır.
- Local/Dup/Close, exception ve module teardown failure-path testleri geçer.
- `HPyGlobal` runtime izolasyonunu; `HPyField` gerçek moving-GC ve write barrier
  davranışını korur.
- Pure type payload, trace, inheritance, finalizer ve shutdown testleri geçer.
- Normal/Trace/Debug context, malformed binary ve ABI version testleri geçer.
- Aynı özellik handwritten ve aHPy-generated extension ile doğrulanır.
- Native Tonic ABI, HPy Universal, aHPy Universal ve CPython bridge maliyetleri
  aynı semantik workload üzerinde ayrı raporlanır.

Ayrıntılı aşamalar ve kapsam dışı iddialar
[HPY_AHPY_STRATEGY.md](../HPY_AHPY_STRATEGY.md) içindedir.
