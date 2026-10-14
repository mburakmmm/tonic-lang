# ADR 0147 — Optional karma numeric union JIT

## Durum

Kabul edildi ve uygulandı.

## Bağlam

`int | float` typed-JIT yolu iki sayısal representation'ı exact builtin
`isinstance` ile ayrı native dallara ayırabiliyordu. `float | None` yolu ise
None identity refinement ve heap-backed F64 unboxing sağlıyordu. Gerçek Python
API'lerinde sık kullanılan `int | float | None` bu iki davranışı aynı control
flow içinde birleştirmelidir. Union'ı doğrudan tek makine representation'ı
saymak integer tag'ini, float logical handle'ını veya `None` sentinel'ını yanlış
yorumlayarak bellek ve semantik güvenliğini bozabilir.

## Karar

Public typed signature ve runtime annotation planına ayrı
`IntOrFloatOrNone` scalar türü eklenir. Normal ve arbitrary-PC giriş guard'ı
yalnız exact small-int immediate, exact Tonic float handle veya `None` kabul
eder. `x is None` ve eşdeğer ters operand biçimleri non-None kenarında fact'i
`IntOrFloat`a daraltır. Exact builtin `isinstance(x, float)` veya
`isinstance(x, int)` bundan sonra mevcut i64/F64 edge refinement'ını kullanır.

Annotated exact callee'nin sonucu generic call side-exit'inden sonra aynı union
guard'ıyla caller'a taşınabilir. Callee sonucu annotation dışındaysa caller aynı
successor PC'de deopt eder ve gerçek Python değeriyle generic yürütür. Builtin
callee veya type nesnesi yeniden bağlanırsa type-test sonucu tahmin edilmez;
identity guard failure yine successor PC'de generic branch'e döner.

Integer ile float'ın aynı register'da birleşip tür testi olmadan tüketildiği
akışlar native kabul edilmez. Bu akışlar ancak explicit tagged materialization
ve doğru deopt metadata'sı eklendiğinde genişletilebilir. Python `bool`, `int`
annotation'ının zorunlu runtime kontrolü sayılmaz; exact typed giriş guard'ını
kaçırır ve advisory generic semantik `bool <: int` davranışını korur.

## Sonuçlar

Optional karma numeric kod None kontrolünden sonra tek bir guarded type testiyle
i64 veya F64 native aritmetiğe geçebilir. Giriş, dönüş ve direct-call sonucu aynı
lattice sözleşmesini paylaşır. Ek maliyet üç üyeli guard, float scratch slotu ve
branch fact metadata'sıdır. Desteklenmeyen birleşimler ya da yanlış annotation
program hatası olmaz; yalnız typed specialization reddedilir veya deopt edilir.
