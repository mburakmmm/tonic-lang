# ADR 0038 — Profilli native float direct leaf

## Durum

Kabul edildi — 11 Eylül 2026.

## Karar

Exact Tonic function ve bütün supplied argümanların exact `Float` olduğu ordinary
`CALL` sitesi sekiz kararlı gözlemden sonra özel direct plan seçebilir. Target;
parametrelerden türeyen `Move`, `Add`, `InplaceAdd`, `Sub`, `Mul` ve tek `Return`
dataflow alt kümesinde olmalıdır. Closure, cell, class body, variadic parametre,
expanded call ve method receiver bu ilk kapsamın dışındadır.

Generated caller callee identity guard'ından sonra her boxed argümanı `UnboxFloat`
runtime op'uyla bir kez çözer. Helper 16 baytlık native scratch alana IEEE-754
bitlerini ve ayrı presence bayrağını yazar; bu alan managed root sayılmaz. Presence
miss hiçbir target işlemi çalışmadan özgün `CALL` PC'sine deopt eder. Target'ın
doğrulanmış aritmetik dataflow'u Cranelift F64 SSA değerleriyle yürür.

`Return`, F64 bitlerini `BoxFloat` helper'ına verir. Helper bütün guest register ve
JIT-private root tamponuyla safepoint yaptıktan sonra tek `Object::Float` ayırır ve
sonucu hidden precise root'a yazar. Native kod bu root'u caller destination'a taşır.

## Materialized sabitler

Float ve string sabitleri immediate `Value` değildir. VM, derlenen code object'in
doğrulanmış `CONST` PC'lerini önceden materialize edilmiş logical handle'larla
eşler. JIT bu opaque word'ü yükler; native heap adresi gömmez. Program constant
tablosu handle'ı dış köklerde tutar, register'a yüklendikten sonra JIT root tamponu
da safepoint görünürlüğünü sağlar.

Exact-float argümanlarla çağrılan straight-line float leaf eski integer leaf tier'ına
tek başına gönderilmez. Bu, ebeveyn OSR direct planı hazır olmadan sekiz gereksiz
integer-tag deopt'unu önler. Generic çağrı semantiği ve mixed-type fallback aynen
korunur.

## Ölçüm ve kabul

100.000 çağrılık üç-op leaf'te önceki JIT medyanı 24,435 ms, 300.004 allocation ve
sekiz deopt idi. Yeni yol 2,988 ms, 100.130 allocation, sıfır deopt/side exit ve
99.937 direct call üretir. Güncel adaptive interpreter 22,523 ms, generic
interpreter 23,582 ms'dir; direct yol sırasıyla 7,54× ve 7,89× hızlıdır.

Stress-GC testi boxed dönüşün köklendiğini doğrular. Ayrı mixed-type testi float
profilden sonra int argümanın target etkisinden önce deopt ettiğini gösterir. `inf`,
`nan` ve signed zero sonuçları interpreter çıktısıyla differential olarak aynıdır.

## Sınırlar

Bu karar çağrı sınırında unbox/box yapar. Loop-carried float accumulator'ı native
registerda safepointler boyunca tutmak, JIT stack map üretmek, side exit'te bütün
interpreter register durumunu yeniden kurmak ve arithmetic içindeki polymorphic
float/int SSA bir sonraki stack-map ve tam-deopt işidir.
