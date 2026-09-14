# ADR 0033 — Gözlenmeyen boş variadic parametrelerin direct leaf yolu

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

Bir exact Tonic leaf function `*args` veya `**kwargs` tanımlasa da ordinary call
bu parametrelere hiçbir eleman taşımıyorsa ve target bytecode parametre
register'larını hiç okumuyorsa direct-call planına alınabilir. Hot path boş tuple
ve dict materialize etmez; named positional/keyword/default slotları ADR 0027 ile
aynı şekilde bağlanır.

Kabul statik ve muhafazakârdır. Direct-leaf alt kümesinde `MOVE`, arithmetic veya
`RETURN` operandı variadic register'a değerse hedef reddedilir. Bilinmeyen opcode
da false döner. Call site'ta positional sınırını aşan veya named parametreyle
eşleşmeyen keyword mevcut generic binder kurallarıyla direct profile oluşmasını
engeller. Böylece atlanan boş koleksiyon guest kod tarafından gözlenemez.

## Ölçüm

`add(a,b,*rest,**kw): return a+b` 100.000 kez iki positional argümanla çağrılır:

- adaptive interpreter: 16,171 ms;
- side-exit/resume JIT: 9,510 ms;
- direct variadic leaf: 0,821 ms;
- 99.937 direct call, sıfır side exit/resume;
- native code: 1.780 byte.

Direct yol aynı koşudaki side-exit JIT'ten 11,58×, adaptive tier'dan 19,70×
hızlıdır. Pozitif test moving stress GC altında sonucu ve sayaçları; negatif test
`return rest` hedefinin direct site üretmeyip `(2, 3)` tuple semantiğini koruduğunu
doğrular.

## Sınırlar

Kullanılan `*args/**kwargs`, bu koleksiyonlara eleman taşıyan çağrılar ve
`CALL_EXPANDED` direct inlining kapsam dışıdır. Bunlar ADR 0032'nin generic segment
ve binder yolunda kalır. İleride materialized variadic specialization eklenirse
allocation, escape ve deopt rekonstrüksiyonu ayrıca ölçülmelidir.
