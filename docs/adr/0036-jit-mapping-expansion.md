# ADR 0036 — Guarded `**mapping` expansion ve kesin JIT argument roots

## Durum

Kabul edildi — 11 Eylül 2026.

## Karar

Düz `BEGIN_ARGS` → `CALL_EXPANDED` segmentindeki `ARG_MAPPING`, aynı exact
built-in dict ve aynı sıralı string-key kümesi en az sekiz kez gözlendiğinde
direct integer leaf bağlama planına katılabilir. Profil yalnız programın symbol
tablosunda bulunan string anahtarları kabul eder. Boş dict ve genel mapping
protokolü bu sürümde generic expanded-call yolunda kalır.

Compile-time binder, profilli anahtarları ordinary keyword kurallarıyla target
slotlarına yerleştirir. Positional-only ihlali, duplicate değer, bilinmeyen keyword
ve eksik parametre varsa site derlenmez. Her profilli anahtar için
`LoadMappingItem` helper'ı current dict değerini okur. Helper şu guard'ları uygular:

- owner exact Tonic dict olmalı;
- current entry sayısı profilli sayıyla aynı olmalı;
- istenen string anahtar current dict içinde bulunmalı.

Herhangi bir guard kaybı yeniden yürütülebilir segmentin `BEGIN_ARGS` PC'sine
deopt eder. Böylece key set değişimi generic binder'ın güncel `TypeError` veya
normal çağrı semantiğine gider; aynı key'in value mutation'ı ise deopt etmeden
hemen görülür. Count guard ile bütün profilli anahtarların bulunması birlikte aynı
key kümesini kanıtlar.

## GC ve geçici argument roots

Sequence ve mapping helper sonuçları guest source register'ının üzerine yazılmaz.
JIT her dinamik expanded öğe için guest register önekinin ardında ayrı bir kesin
root kelimesi ayırır. Helper sonucu bu yuvaya yazar ve inlined target o kökten
okur. Böylece birden fazla `*sequence` veya `**mapping` öğesi kaynak container'ı
bozmaz; önce alınan managed değerler sonraki helper safepoint'lerinde de canlıdır.

`Metadata::register_count` yalnız guest-visible öneki, `root_count` ise method
cache ve expanded-argument köklerini de içerir. Runtime bütün root dilimini helper
ve GC poll'larına verir, native dönüşte yalnız guest önekini VM frame'ine kopyalar.
Helper heap adresi sızdırmaz, allocation/collection yapmaz ve yalnız logical
`Value` döndürür.

## Ölçüm

`add(total, **mapping)` iki anahtar (`b`, `bias`) ile 100.000 kez çağrıldı. Harness
source'u bir kez compile eder; her mod 3 warmup ve 15 ayrı VM örneği kullanır:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 33,476 ms | 33,133 ms | 34,091 ms | 2.300.033 | 0 | 0 | 0 / 0 |
| JIT generic segment | 30,284 ms | 29,494 ms | 30,905 ms | 700.756 | 99.937 / 99.937 | 1.696 | 0 / 0 |
| JIT guarded mapping | 2,761 ms | 2,699 ms | 2,999 ms | 1.197 | 0 | 2.524 | 1 / 99.937 |

Guarded mapping aynı koşudaki generic segment JIT'ten 10,97×, adaptive
interpreter'dan 12,13× hızlıdır. İki profilli key her logical çağrıda güncel
okunduğundan ölçüme 199.874 allocation-free mapping helper çağrısı dahildir.

Stress-GC integration testi iki anahtarı 5.000 iterasyonda bağlar, sonra `bias`
değerini değiştirip yeni native invocation'da güncel sonucu ve sıfır deopt'u
doğrular. Ayrı test key sayısı/kümesi değişince `BEGIN_ARGS` deopt'unu ve generic
`TypeError` sonucunu kontrol eder. Çok öğeli sequence testi de yeni ayrı-root
düzeninin source list'i koruduğunu doğrular.

## Sınırlar

Boş dict, dict subclass/genel mapping protokolü, program symbol tablosuna alınmamış
string key, nested argument builder ve target'ın gerçekten okuduğu materialized
`*args/**kwargs` generic yolda kalır. Profil key sırasına duyarlıdır; farklı insertion
order güvenli biçimde yeniden profil ister. Daha geniş mapping PIC ancak ölçüm bunu
gerekçelendirirse eklenir.
