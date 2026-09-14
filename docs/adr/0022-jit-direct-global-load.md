# ADR 0022 — JIT direct bound-global load

## Durum

Kabul edildi — 5 Eylül 2026.

## Karar

VM her program çalıştırmasının başında symbol tablosuyla aynı uzunlukta raw
`Value` dizisi kurar. Interpreter'daki her `STORE_GLOBAL`, asıl root slotuyla
birlikte bu aynanın aynı indeksini de günceller. JIT entry global slice pointer ve
uzunluğunu alır. `LOAD_GLOBAL` önce indeksi doğrular, bound değeri doğrudan yükler
ve yalnız sınır dışı veya `UNBOUND` durumda runtime helper'a gider.

Global slice native giriş boyunca salt-okunurdur. JIT tarafından desteklenmeyen
global mutation opcode'u native çalışırken yürüyemez; bir `CALL` side exit'i
native invocation'ı bitirir ve resume yeni invocation ile güncel aynayı görür.
Aynadaki heap handle'ları ayrıca root sayılmaz; `Vm::globals` kesin root kaynağıdır.

## Sonuç

Global kimliğine ilişkin compile-time varsayım olmadığı için rebinding ayrıca
invalidation gerektirmez. Missing-name yolu önceki exact `NameError` türü ve
bytecode PC'sini korur. `fib(20)` ara ölçümünde 21.876 helper çağrısı sıfıra indi;
JIT medyanı aynı geliştirme serisindeki yaklaşık 1,98 ms'den 1,91 ms'ye düştü.
Doğrudan Tonic-to-Tonic call ve sürümlü sabit global specialization ayrı kalır.
