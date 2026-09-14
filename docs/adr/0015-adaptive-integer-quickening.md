# ADR 0015 — Adaptive integer arithmetic quickening

## Durum

Kabul edildi; `Add`, `InplaceAdd`, `Sub` ve `Mul` için uygulanmıştır.

## Karar

Verified bytecode değişmez ve serialize edilen opcode değeri değiştirilmez.
Runtime her run için instruction'larla aynı uzunlukta ayrı bir adaptive-state
tablosu oluşturur. Bir site sekiz ardışık immediate integer sonucu gözlediğinde
`IntBinary` olur. Bu durumda tag decode, checked makine işlemi ve immediate-range
kontrolü VM içinde yapılır; heap'in generic numeric yoluna girilmez.

Guard failure site'ı hemen `Generic` durumuna döndürür ve aynı instruction generic
runtime'da yeniden yürütülür. Böylece float, BigInt, list `+=` veya hata semantiği
değişmez. Sekiz yeni kararlı integer gözlemi site'ı yeniden specialize edebilir.
Sayaç taşması `saturating_add` ile davranışı değiştirmez.

State tablosu programlar arasında korunmaz. `Vm::run` globals, JIT cache ve
profiling state ile birlikte tabloyu da yeniden kurar. `adaptive_specialization`
alanı yalnız A/B benchmark ve doğrulama için mekanizmayı kapatabilir.

## Ölçüm

Aynı release binary içindeki generic/adaptive A/B ölçümü numeric loop'ta %2,3,
mul ağırlıklı işte %1,4 kazanç; kısa add-call işinde %0,35 ve float işinde %0,7
kayıp göstermiştir. Kapsam bu nedenle küçük tutulur. Call ve attribute cache,
shape/type version guard'ları ayrıca ölçülmeden bu mekanizmaya eklenmez.
