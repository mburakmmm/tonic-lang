# ADR 0148 — Bool-float union JIT

## Durum

Kabul edildi ve uygulandı.

## Bağlam

Tonic exact `bool` değerlerini immediate olarak taşır ve Python'ın sayısal
protokolünde 0/1 gibi işler. Float değerleri ise moving-GC uyumlu logical handle
üzerinden F64 state'e unbox edilir. `bool | float` annotation'ını tek bir
representation sanmak bu iki yolu karıştırır; bütünü generic bırakmak da sık
kullanılan flag-or-measurement API'lerinde annotation hızlı-yolunu keser.

## Karar

Typed signature, scalar lattice ve runtime annotation planına ayrı
`BoolOrFloat` türü eklenir. Normal ve arbitrary-PC giriş guard'ı yalnız exact
bool immediate veya exact Tonic float handle kabul eder. Exact builtin
`isinstance(value, float)` union'ı F64/bool; `isinstance(value, bool)` ise
bool/F64 kenarlarına ayırır. Bool dalı mevcut Python integer-like decode yolunu
kullanır ve `False == 0`, `True == 1` davranışını korur.

Guarded annotated direct-call sonucu caller'a `BoolOrFloat` fact'i olarak
taşınabilir. Union dışı sonuç, exact int/string girdi veya yeniden bağlanmış
`isinstance`/type global'i exact PC'de generic yürütmeye döner. Standart
annotation runtime type check değildir.

## Sonuçlar

Type testiyle ayrılan bool-float fonksiyonları iki dalda da native aritmetik ve
kanıtlı dönüş kullanabilir. Giriş guard'ı float helper scratch slotu gerektirir;
bu slot managed root yerine geçmez. Bool ile float'ın ayrılmadan birleşik bir
operasyonda tüketildiği akışlar explicit materialization veya ek lowering
kanıtı gelene kadar native kabul edilmez.
