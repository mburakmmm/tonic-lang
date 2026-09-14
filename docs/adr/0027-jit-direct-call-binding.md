# ADR 0027 — JIT direct-call parametre bağlama planı

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

Exact Tonic function profili, closure/cell, `*args` veya `**kwargs` gerektirmiyorsa
positional-only, normal keyword, keyword-only ve default parametreleri doğrudan
leaf inlining yolunda kullanabilir. Runtime çağrı sitesi ile hedef `Signature`
metadata'sını ordinary binder kurallarıyla bir kez eşler ve target parametre
yuvası sırasındaki bir plan üretir:

```text
target slot 0 <- caller argument window + 0
target slot 1 <- exact function default 0
target slot 2 <- caller argument window + 1 (keyword)
```

Generated code bu planı SSA değerlerine uygular; positional tuple veya keyword
dict ayırmaz ve target'ın ara register'larını caller'ın görünür register dizisine
yazmaz. Eksik, fazla, duplicate veya positional-only keyword içeren düzenler
uzmanlaşma adayı olmaz; ordinary binder kesin hata ve değerlendirme sırasının tek
doğruluk kaynağı olarak kalır.

Default değerleri code object'ten değil exact function nesnesinden alınır. Native
kod callee'nin slot+generation logical handle'ını önce doğruladığı için aynı guard
hem code identity hem de o function nesnesinin definition-time default setini
korur. Heap compaction logical handle'ı değiştirmez; slot reuse generation'ı
değiştirir. Caller veya default operandı inline exact-int yoluna uymuyorsa özgün
caller `CALL` PC'sine atomik deopt edilir.

Adaptive interpreter'ın monomorphic call cache'i aynı geçerli düzenleri frame'e
doğrudan bağlar. Yalnız ilk profil gözlemlerinde veya generic fallback'te genel
binder çalışır. Bu yol da guest tuple/dict ayırmaz.

## Ölçüm

100.000 çağrılı `add(total,bias=0)` iş yükünde `b=1` default'u ve `bias` keyword-only
slotu kullanılmıştır. Değişiklik öncesi direct-call seçeneği uzmanlaşamadığı için
99.937 side exit/resume ve 12,405 ms medyan üretmiştir. Sonrasında:

- adaptive interpreter: 14,850 ms;
- inlining kapalı JIT: 10,061 ms, 99.937 side exit/resume;
- binding planlı JIT: 1,120 ms, sıfır side exit/resume, 99.937 inline call.

Planlı yol aynı koşudaki side-exit JIT'ten yaklaşık 8,99× hızlıdır. Native code
1.660 byte'tan 2.284 byte'a çıkar. Ham sonuçlar ve positional kontrol iş yükü
[JIT_DIRECT_CALL_BASELINE.md](../JIT_DIRECT_CALL_BASELINE.md) içindedir.

## Sınırlar

Plain bound-instance method çağrısı daha sonra [ADR 0028](0028-jit-bound-method-fusion.md)
ile, staticmethod [ADR 0029](0029-jit-staticmethod-fusion.md), classmethod ve
class-level erişim [ADR 0030](0030-jit-classmethod-fusion.md) ile eklenmiştir.
Custom descriptor yolu bu kararın kapsamında değildir. `*args`, `**kwargs` ve `CALL_EXPANDED` materialization
semantiği generic yolda kalır. Inline callee hâlâ side-effect-free düz integer leaf
alt kümesiyle sınırlıdır; control-flow, allocation ve exception-producing helper
çağrıları için frame/deopt rekonstrüksiyonu gereklidir.
