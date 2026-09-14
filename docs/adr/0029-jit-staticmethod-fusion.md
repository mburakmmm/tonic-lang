# ADR 0029 — Instance staticmethod lookup ve leaf-call fusion

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

ADR 0028'in allocation-free method lookup profili, instance üzerinden erişilen
exact Tonic `staticmethod` descriptor'ını da tanır. Profil underlying function
handle'ıyla birlikte binding türünü saklar. JIT runtime helper seçicisi iki bilgiyi
taşır:

```text
low 32 bit  = attribute SymbolId
high bit 32 = 0 static / 1 instance receiver
```

Helper güncel lookup sonucu hem aynı exact function hem de beklenen binding türü
ise function handle'ını döndürür. Static türünde target parametre planına owner
eklenmez; açık positional/keyword/default argümanlar slot 0'dan başlayarak bağlanır.
Instance türünde ADR 0028'deki implicit `self` davranışı sürer.

Binding türü exact function guard'ından ayrı bir semantik varsayımdır. Örneğin
`raw = Math.add; Math.add = raw` aynı function handle'ını static wrapper'dan plain
class function'a taşır. Yalnız handle guard'ı bu değişimi göremez ve receiver'ı
yanlış bağlar. Helper selector guard'ı bu durumda `UNBOUND` sonucu üretir;
generated kod özgün `ATTR` PC'sine deopt eder ve generic descriptor/binder kesin
`TypeError` davranışını yürütür.

Static lookup allocation veya collection yapmaz. Exact handle'lar hareketli GC
için logical slot+generation değerleridir; persistent object adresi native koda
girmez. Argument replay ve owner-register değişmezliği ADR 0028 ile aynı compile
time doğrulamalarına tabidir.

## Ölçüm

100.000 iterasyonluk `math.add(total,b=1)` iş yükünde değişiklik öncesi `ATTR`
nedeniyle native code üretilmemiştir:

- adaptive interpreter: 27,471 ms;
- direct seçeneği açık fakat unsupported: 30,207 ms, sıfır code byte.

Fusion sonrası ayrı koşuda:

- adaptive interpreter: 20,044 ms;
- JIT method fusion: 2,379 ms;
- native code: 1.600 byte;
- 99.937 direct call, bir method site, sıfır side exit/resume.

Son koşudaki adaptive kontrole göre yaklaşık 8,43× hızlanma vardır. Koşular arası
host yükü değiştiği için önce/sonra adaptive süreleri birbirine oranlanmaz; karar
aynı son koşudaki A/B oranına dayanır. Ham veriler
[JIT_DIRECT_CALL_BASELINE.md](../JIT_DIRECT_CALL_BASELINE.md) içindedir.

## Sonraki genişletme ve sınırlar

Class üzerinden function/staticmethod erişimi ile classmethod receiver bağlama
[ADR 0030](0030-jit-classmethod-fusion.md) kapsamında eklenmiştir. Custom
descriptor/property, `super`, callable instance ve variadic/expanded çağrılar
generic yolda kalır. Helper her çağrıda lookup yapar; doğrudan class/shape/version
guard'lı native load gelecekte ölçülebilir bir iyileştirmedir.
Per-call lookup helper'ı daha sonra
[ADR 0034](0034-jit-method-entry-cache.md) ile native giriş başına bir lazy
lookup'a indirilmiştir.
