# ADR 0035 — Guarded sequence/named expansion ve direct leaf

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

Düz bir `BEGIN_ARGS` → `CALL_EXPANDED` segmenti yalnız `CONST`, `MOVE`,
`ARG_POS`, `ARG_STAR` ve `ARG_NAMED` içeriyorsa profillenir. Her `ARG_STAR` sitesi exact
built-in list/tuple uzunluğunu, çağrı sitesi exact Tonic function kimliğini en az
sekiz kararlı gözlemle kaydeder. Hedef closure/class-body olmayan ve mevcut
integer direct-leaf alt kümesine giren tam positional imzaysa Cranelift segmenti
native koda alır.

Normal positional ve named argümanlar absolute caller register'ından okunur. Named
değerler ordinary binder'ın positional-only, keyword-only, duplicate ve default
slot kurallarıyla compile time'da target slotlarına bağlanır. Star öğeleri
opaque `LoadSequenceItem` runtime helper'ıyla alınır. Helper yalnız exact
list/tuple ve profilli uzunluk eşleştiğinde güncel öğeyi döndürür. Böylece aynı
uzunluktaki item mutation hemen görünür; tür veya uzunluk değişimi guard miss
üretir. Function ve argument-tag guard'larıyla birlikte bütün miss'ler özgün
`BEGIN_ARGS` PC'sine deopt eder.

Segmentte yeniden yürütülebilir olmayan opcode kabul edilmez. Bu sayede deopt
argument ifadelerinin observable etkisini iki kez çalıştırmaz. Nested builder
önceki generic side-exit/resume yolunda kalır. `**mapping` daha sonra
[ADR 0036](0036-jit-mapping-expansion.md) ile guarded direct yola eklenmiştir.

## GC ve ABI

Helper'a guest register'lar ile JIT-private cache alanlarının tamamını içeren
`root_count` dilimi verilir. Sequence lookup allocation veya collection yapmaz;
heap adresi native koda sızdırmaz ve yalnız logical `Value` döndürür. Her dinamik
öğe guest register önekinin ardındaki ayrı JIT-private root'a materialize edilir.
Bu düzen birden fazla öğede source container register'ını korur ve sonraki
safepoint'lerin daha önce alınan öğeleri kesin kök olarak görmesini sağlar.

## Ölçüm

`add(total, *values)` ve tek öğeli `values` ile 100.000 çağrılık aynı A/B koşusu:

| Mod | Medyan | Interpreter instruction | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|
| Adaptive interpreter | 21,524 ms | 2.100.027 | 0 | 0 | 0 / 0 |
| JIT generic segment | 18,747 ms | 1.000.715 | 99.937 / 99.937 | 1.392 | 0 / 0 |
| JIT guarded expansion | 1,080 ms | 1.345 | 0 | 1.908 | 1 / 99.937 |

Guarded yol bu koşuda generic segment JIT'ten yaklaşık 17,37×, adaptive interpreter'dan
19,94× hızlıdır. Stress-GC testi 5.000 çağrıda sıfır side exit ve 4.900'den fazla
direct call doğrular. Ayrı test aynı uzunluktaki item mutation'ı gözler; uzunluk
değişiminde deopt sonrası generic binder'ın `TypeError` sonucunu korur.

`add(total, *values, bias=0)` named/default varyantı 100.000 çağrıda adaptive
29,728 ms, generic segment JIT 26,342 ms ve guarded direct yol 1,236 ms ölçer.
Direct yol generic JIT'ten 21,31×, adaptive tier'dan 24,05× hızlıdır; 99.937
çağrının tamamı native kalır.

## Sınırlar

Genel iterable protokolü, nested expansion ve target'ın gözlediği materialized
`*args/**kwargs` bu karara dahil değildir. Mapping kapsamı ADR 0036'dadır; kalanlar
ayrı profil ve materialization sözleşmesi gerektirir.
