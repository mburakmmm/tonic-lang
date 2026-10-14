# ADR 0151 — Optional tam numeric union JIT

## Durum

Kabul edildi ve uygulandı.

## Bağlam

Tonic `int`, `bool`, `float` ve `None` için tekli, ikili ve üçlü typed scalar
union yollarına sahiptir. Dört üyeli `int | bool | float | None`, optional API
sınırında bu fact'lerin tamamını birleştirir ve eksik kalırsa typed çağrı
grafiğini generic yola keser.

## Karar

Typed signature, scalar lattice ve runtime annotation planına
`IntOrBoolOrFloatOrNone` eklenir. Giriş ve arbitrary-PC guard exact small-int,
exact bool, exact Tonic float veya `None` kabul eder. None identity refinement
non-None kenarında `IntOrBoolOrFloat` üretir. Exact builtin float/bool/int
testleri ADR 0150'deki edge kurallarını kullanır; `int` testi `bool <: int`
semantiğini korur.

Guarded annotated direct-call sonucu caller'a aynı dört üyeli fact ile
taşınabilir. Union dışı girdi/sonuç veya yeniden bağlanmış builtin/type global'i
exact PC'de generic Python yürütmesine döner.

## Sonuçlar

Temel `int/bool/float/None` union kombinasyon ailesi typed giriş, return ve call
result sınırlarında tamamlanmıştır. Float scratch state ve logical managed root
birlikte korunur. User-class, shape ve container üyeli genel union narrowing bu
karardan ayrı kalır ve kendi version/alias/materialization kanıtlarını gerektirir.
