# ADR 0149 — Optional bool-float union JIT

## Durum

Kabul edildi ve uygulandı.

## Bağlam

`bool | float` typed-JIT yolu exact builtin type testiyle immediate bool ve F64
state'ini ayırır. Optional API'ler aynı değere `None` eklediğinde generic yola
dönmek, annotation-guided first-call zincirini gereksiz yere keser. Üç logical
representation'ı tek makine türü saymak ise tag ve GC kök güvenliğini bozar.

## Karar

Typed signature, scalar lattice ve runtime annotation planına
`BoolOrFloatOrNone` eklenir. Giriş ve arbitrary-PC guard yalnız exact bool,
exact Tonic float veya `None` kabul eder. `is None`/`is not None` refinement'ı
non-None kenarında `BoolOrFloat` üretir. Exact builtin
`isinstance(value, float)` ve `isinstance(value, bool)` bundan sonra mevcut
bool/F64 dal lowering'ini kullanır.

Guarded direct-call sonucu caller'a aynı üç üyeli fact ile taşınabilir. Yanlış
annotation girdisi veya sonucu ve yeniden bağlanmış builtin/type global'i exact
PC'de generic Python yürütmesine döner. Standart annotation runtime type check
değildir; exact int gibi union dışı değerler advisory semantiği korur.

## Sonuçlar

Optional flag-or-measurement fonksiyonları None kontrolünden sonra native bool
ve float dallarında kalabilir. Float kontrolü için scratch slot ve canlı logical
root korunur. Üç üyenin type testi olmadan tek operasyonda birleştiği akışlar
explicit materialization kanıtı gelene kadar native kabul edilmez.
