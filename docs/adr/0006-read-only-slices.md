# ADR 0006 — Read-only slice values and sequence slicing

## Karar

Parser slice bileşenlerini Tonic AST içinde `Slice { start, stop, step }` olarak
saklar. Lowering eksik bileşenleri `None` yapar ve üç ardışık register üzerinden
`Slice` bytecode talimatına verir. Bytecode formatı bu yeni açık opcode nedeniyle
sürüm 4'tür; doğrulayıcı register penceresini ve ayrılmış operandı denetler.

Runtime slice'ı üç `Value` kenarı olan yönetilen bir nesne olarak taşır. Bu seçim
büyük tamsayı bileşenlerinin moving GC sırasında izlenmesini sağlar ve heap adresini
bytecode'a ya da guest semantiğine sızdırmaz. List, tuple ve Unicode string slicing
Python'ın açık uç, negatif indeks, clamp ve negatif adım kurallarıyla yeni bir
sequence üretir. String indeksleri UTF-8 byte konumu yerine Unicode scalar value
sırasını kullanır; mevcut string indeksleme ve `len` davranışıyla aynıdır.

## Sınırlar

Bu aşama read-only list/tuple/string slice içindir. Slice assignment parser'da açık
`UnsupportedSyntax` verir. Range slicing, custom `__getitem__`, slice builtin'i ve
slice'ın dict anahtarı olarak kullanımı sonraki object-protocol çalışmasına aittir.
Slice adımı sıfırsa `ValueError`, bileşen integer/None değilse `TypeError` üretilir.

## Doğrulama

Katman testleri AST sahipliğini, opcode üretimini, verifier sınırlarını, Unicode'u,
çok büyük sınırları ve hata türlerini kapsar. Differential corpus default ve her
allocation'da GC kiplerinde CPython 3.14.6 ile aynıdır; ek olarak 7.425 üretilmiş
list/tuple/string kombinasyonu karşılaştırılmıştır.
