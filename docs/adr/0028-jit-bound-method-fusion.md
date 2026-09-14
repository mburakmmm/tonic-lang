# ADR 0028 — Plain bound-method lookup ve leaf-call fusion

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

Adaptive yürütmede kararlı biçimde plain Tonic instance method üreten bir `ATTR`,
sonucunu callee olarak kullanan `CALL` ile JIT derleme anında kaynaştırılabilir.
Kabul için:

- owner exact Tonic instance olmalı;
- aynı isim instance alanında bulunmamalı;
- güncel C3 class lookup sonucu doğrudan bir Tonic `Function` olmalı;
- `ATTR` sonucu yalnız ilgili `CALL` callee'si olarak kullanılmalı;
- arada yalnız tekrar yürütülmesi yan etkisiz `CONST/MOVE` argüman hazırlığı olmalı;
- owner ve callee register'ları bu aralıkta değiştirilmemeli;
- hedef, ADR 0026/0027'nin inlineable leaf ve bağlama koşullarını sağlamalıdır.

Generated `ATTR`, opaque runtime helper üzerinden güncel plain-method lookup yapar.
Helper heap düzenini JIT'e açmaz, allocation ve collection yapmaz; underlying
function logical handle'ını döndürür. Native kod bunu profildeki exact function
handle'ıyla karşılaştırır. Başarıda bound-method nesnesi materialize edilmez;
owner register'ı target'ın implicit `self` slotuna, açık positional/keyword/default
argümanlar kalan slotlara bağlanır.

Lookup sonucu değişirse veya sonraki operand guard'ı kaçarsa deopt noktası `CALL`
değil özgün `ATTR` PC'sidir. Interpreter böylece gerçek bound-method nesnesini
yeniden kurar ve saf `CONST/MOVE` dizisini tekrarlar. Bu atomiklik, kısmen
materialize edilmiş underlying function değerinin generic çağrı yoluna sızmasını
önler.

## Invalidation ve GC

Helper her native method girişinde instance shadowing ile güncel C3 lookup
sonucunu tekrar okur. Class/base mutation mevcut dependency-version zincirini
günceller; helper yeni function, descriptor veya eksik sonuç görür ve exact guard
kaçırır. Instance alanına aynı isim yazılması da lookup'u başarısız yapar. Böylece
doğruluk persistent native heap pointer'a veya yalnız compile-time class sürümüne
bağlı değildir.

Profile ve generated code function logical handle'ını zayıf varsayım olarak
tutar. Function class namespace'inde kaldığı sürece normal GC edge'iyle köklüdür;
rebind sonrasında toplanabilirse slot generation değişimi stale guard eşleşmesini
engeller. Helper allocation yapmadığından yeni safepoint değildir; caller'ın
backedge poll ve diğer helper safepoint'leri materialized register roots'u korur.

## Ölçüm

100.000 iterasyonluk `counter.add(total,b=1)` iş yükünde değişiklik öncesi JIT
`ATTR` opcode'unu desteklemediği için native code üretmiyor; adaptive medyan
23,027 ms, JIT seçeneği 23,793–25,030 ms aralığındaydı. Fusion sonrasında:

- medyan 2,024 ms;
- native code 1.604 byte;
- 99.937 direct method call;
- sıfır side exit/resume;
- bir guarded method site.

Adaptive kontrole göre yaklaşık 11,38× hızlanma elde edilmiştir. Receiver-identity
A/B testi 5.000 çağrıda aynı sonucu üretir ve generic method yoluna göre en az
4.800 geçici heap allocation'ı kaldırır. Ayrıntı
[JIT_DIRECT_CALL_BASELINE.md](../JIT_DIRECT_CALL_BASELINE.md) içindedir.

## Sınırlar

`staticmethod` daha sonra [ADR 0029](0029-jit-staticmethod-fusion.md) ile instance
erişimi için; `classmethod` ve class-level erişim
[ADR 0030](0030-jit-classmethod-fusion.md) ile eklenmiştir. Custom descriptor/property,
callable instance, `super`, variadic/expanded call ve argument hazırlığında
yan etkili/guard'lı opcode bu fusion'a alınmaz. Her çağrıda helper lookup maliyeti
vardır; ileride class/shape/version guard'larını doğrudan native kodda okuyacak
dependency metadata bu maliyeti azaltabilir. Inline hedef hâlâ düz, yan etkisiz
integer leaf alt kümesidir.

Per-call helper maliyeti daha sonra [ADR 0034](0034-jit-method-entry-cache.md)
ile invocation-local lazy cache ve exact owner guard'ına dönüştürülmüştür.
