# ADR 0100 — HPy scalar dönüşümleri ve public exception yüzeyi

## Durum

Kabul edildi ve H2c'de uygulandı.

## Bağlam

HPy Universal 0.9 context'i integer dönüşümlerini signed/unsigned 32/64-bit,
platform size türleri, mask, pointer ve double biçimlerinde ayrı slotlarla sunar.
Tonic integer'ları immediate veya heap bigint olabilir; dolayısıyla dönüşüm
yüzeyi host pointer'ı ya da CPython integer yerleşimini kullanamaz. HPy 0.9 aynı
zamanda `SetObject`, `ExceptionMatches` ve `NoMemory` sunar, fakat public
fetch/restore API'si içermez.

## Karar

Bütün integer girişleri Tonic'in `BigInt` değerine gider. Exact-width çıkışlar
checked daraltma yapar ve taşmada `OverflowError` üretir; mask çıkışları iki
üzeri bit genişliği modülünü uygular. Pointer dönüşümü `usize` sınırını doğrular.
Integer-to-double yalnız finite sonuç kabul eder. Bool singleton'ları immediate,
float değerleri Tonic heap değeridir ve HPy local handle tablosu yalnız logical
handle taşır.

Exception indicator `Diagnostic` olarak call scope'unda kalır. `SetObject`
mesajı Tonic `str` protokolünden alınır. `ExceptionMatches`, built-in exception
hiyerarşisini ve 32 seviye ile sınırlı nested tuple'ları destekler. Context'teki
built-in exception handle'ları gerekirse Tonic'in original built-in binding'ine
materialize edilir. `NoMemory` bir `MemoryError` indicator kurar. HPy 0.9'da
olmayan fetch/restore capability olarak yayımlanmaz.

## Sonuçlar

Gerçek HPy 0.9 C fixture scalar constructor/conversion'ları, unsigned 64-bit
bigint roundtrip'ini, mask ve pointer sınırlarını, exception hierarchy/tuple
matching'i, `SetObject` mesajını ve explicit no-memory fault'unu sınar. Başarı ve
hata yolları interpreter/JIT caller ile default/her-allocation GC matrisinde
çalışır; her dönüşte local handle sayısı sıfırdır. Custom `__index__`/`__float__`
protokolleri ve custom exception class'ları bu kararla desteklenmiş sayılmaz.
