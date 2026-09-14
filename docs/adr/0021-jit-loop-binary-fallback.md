# ADR 0021 — JIT loop binary fallback

## Durum

Kabul edildi — 5 Eylül 2026.

## Karar

Backedge içeren JIT code object'lerinde `+`, `+=`, `-`, `*`, `//` ve `%` önce
iki operand için exact immediate-integer guard'ı çalıştırır. Guard başarılıysa
mevcut allocation-free Cranelift integer yolu kullanılır. Tür guard'ı başarısızsa
instruction deopt etmez; bütün virtual register'lar materialized haldeyken opak
runtime helper generic Tonic işlemini aynı bytecode PC'sinde yürütür.

Helper allocation yapabileceği için register dizisi ve diğer VM kökleri kesin GC
roots olarak verilir. Runtime hata türü ve PC normal JIT helper hata yoluyla
korunur. Integer taşması ve sıfıra bölme şimdilik generic interpreter'a exact-PC
deopt eder. Backedge içermeyen küçük leaf fonksiyonlar ölçülen compile/bridge
maliyeti nedeniyle eski guard/deopt ve kârlılık politikasını korur.

## Gerekçe ve ölçüm

Bu yol kayan nokta ve diğer generic operandlarla çalışan sıcak döngülerde native
dispatch avantajını korurken nesne semantiğini runtime'da merkezde tutar. 100.000
float `+=` iterasyonunda adaptive interpreter medyanı 10,713 ms, JIT medyanı
2,741 ms ölçüldü; 3,91× hızlanma ve sıfır deopt elde edildi. Aynı koşudaki bir
milyon integer toplam döngüsü 3,752 ms ile adaptive tier'ın 89,335 ms sonucuna
karşı 23,81× hızlı kaldı.

Bu karar native unboxed float aritmetiği değildir. Her float işlem helper çağrısı
ve sonuç allocation'ı öder; typed float fast path ayrı profil ve guard çalışmasıdır.
