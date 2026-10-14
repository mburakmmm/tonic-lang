# ADR 0150 — Üç yönlü numeric union JIT

## Durum

Kabul edildi ve uygulandı.

## Bağlam

Tonic ayrı `int | bool`, `int | float` ve `bool | float` typed-JIT yollarına
sahiptir. Bunların `int | bool | float` birleşimi, Python'ın `bool` değerini
`int` alt türü sayan `isinstance` davranışını korurken üç farklı logical
representation'ı güvenle daraltmalıdır.

## Karar

Typed signature, scalar lattice ve runtime annotation planına
`IntOrBoolOrFloat` eklenir. Giriş ve arbitrary-PC guard exact small-int, exact
bool immediate veya exact Tonic float kabul eder. Exact builtin type testleri:

- `float`: true `Float`, false `IntOrBool`;
- `bool`: true `Bool`, false `IntOrFloat`;
- `int`: true `IntOrBool`, false `Float`.

Son kural Python'ın `isinstance(True, int)` davranışını korur. Guarded annotated
direct-call sonucu caller'a aynı fact ile taşınabilir. Union dışı gerçek değer,
yanlış callee sonucu veya yeniden bağlanmış builtin/type global'i exact PC'de
generic Python yürütmesine döner.

## Sonuçlar

Üç numeric türü kabul eden API'ler exact type testlerinden sonra mevcut unboxed
integer-like veya F64 yollarını kullanabilir. Native giriş float scratch slotu
ve canlı logical root taşır. Üç üyenin ayrılmadan tek karma operasyonda
kullanıldığı akışlar explicit materialization/lowering kanıtı gelene kadar
native kabul edilmez.
