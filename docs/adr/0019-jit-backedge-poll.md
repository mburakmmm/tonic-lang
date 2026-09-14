# ADR 0019 — Periyodik native backedge safepoint'i

## Durum

Kabul edildi ve uygulanmıştır.

## Karar

Her derlenmiş backedge üzerinde native stack slot'ta tutulan sayaç azaltılır.
Sayaç 1024'te bir sıfıra ulaştığında generated code `Poll` runtime helper'ını
çağırır ve sayacı yeniler. Ara 1023 geçiş yalnız stack load/store, çıkarma ve dal
maliyetini öder.

Poll allocation safepoint'idir. Bütün virtual register'lar ABI register dizisinde
materialized olduğundan helper kesin root slice'ını collector'a verir. VM, aktif
JIT frame register aralığını genel root listesinden çıkarır ve güncel native diziyle
değiştirir. Diğer frames, cells, globals, constants, continuations ve native
handles aynı collection'a eklenir.

## Gerekçe ve ölçüm

Her backedge'de FFI helper çağırmak sıcak numeric loop maliyetini gereksiz büyütür.
Inline sayaç bounded safepoint aralığı verir. `sum_to_1000000` 976 poll üretir;
poll öncesi son ölçüm 3,913 ms, poll sonrasında 4,092 ms'dir. Adaptive interpreter
91,393 ms olduğundan native yol 22,34× hızlı kalır.

Doğrudan JIT testinde heap-tag'li bir değer 2.500 iterasyon boyunca yalnız virtual
register'da tutulur; iki poll callback'i de exact raw root'u görür. Runtime helper
panikleri FFI sınırını geçemez ve poll hatası exact backedge PC'siyle döner.

## Açık sınırlar

Fuel kullanımı JIT'i kapatıp interpreter'a düşürmeye devam eder; native instruction
accounting henüz yoktur. Unboxed managed değerler eklendiğinde machine stack map
gerekir. OSR, dış interrupt handle'ı ve pause-latency tuning ayrı ölçümlü işlerdir.
