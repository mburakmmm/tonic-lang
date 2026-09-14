# ADR 0030 — Classmethod ve class-level method leaf-call fusion

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

ADR 0028/0029'daki allocation-free method profili üç bağlama türünü ayırır:
`Static`, `Instance` ve `Class`. Instance veya class üzerinden yapılan ordinary
attribute lookup exact Tonic function, `staticmethod` ya da `classmethod` ile
sonuçlanırsa ve hedef ADR 0026/0027'nin leaf-call koşullarını sağlarsa `ATTR` ile
onu tüketen `CALL` tek native yol olarak derlenebilir.

Classmethod receiver'ı compile-time profilden gömülmez. Bu, base class'ta tanımlı
descriptor'a `Sub.identity()` veya `Sub().identity()` ile erişildiğinde alıcının
dinamik `Sub` olması için gereklidir. Opaque lookup helper her girişte güncel C3
lookup'u yapar ve JIT'in kesin kök buffer'ındaki method cache'e iki raw `Value`
yazar:

```text
word 0 = underlying exact function
word 1 = gerçek implicit receiver (`self` veya `cls`)
```

Generated kod function handle ve binding türünü guard eder. `Instance` ve `Class`
planlarında target'ın ilk parametresi root cache'teki receiver'dan, `Static`
planında doğrudan açık argümanlardan beslenir. Class üzerinden plain function
erişimi Python'ın unbound davranışına uygun biçimde `Static` sayılır ve receiver
eklemez.

## Deopt ve GC invariantı

Lookup miss'i, binding-kind değişimi veya sonraki operand guard miss'i özgün
`ATTR` PC'sine atomik deopt eder. Interpreter güncel descriptor semantiğini ve
argüman hazırlığını baştan yürütür; yarım bağlanmış function/receiver generic
`CALL` yoluna sızmaz.

Function, receiver ve exact owner guest register'ların sonundaki JIT-private root
kuyruğunda tutulur. Helper ve backedge poll toplam `root_count` değerini gördüğü
için bu kelimeler her collection'da kesin köktür. Persistent heap adresi tutulmaz;
değerler 64-bit logical slot+generation `Value` olarak taşınır. Bu tasarım
[ADR 0034](0034-jit-method-entry-cache.md) ile kalıcı hale getirilmiştir.

## Invalidation

Helper her native girişte instance shadowing'i ve güncel class lookup sonucunu
yeniden çözer. Aynı function handle'ı classmethod wrapper'dan plain function'a
taşınsa bile binding-kind selector guard'ı miss üretir. Class/base mutation ve
stale generation bu nedenle yanlış receiver ile çağrıya dönüşmez.

## Ölçüm ve doğrulama

100.000 inherited classmethod çağrılı aynı koşuda:

- adaptive interpreter: 36,416 ms;
- JIT classmethod fusion: 3,828 ms;
- native code: 1.716 byte;
- bir method site, 99.937 direct call, sıfır side exit/resume.

Yaklaşık hızlanma 9,51×'dir. Instance ile inherited subclass receiver, class ile
plain function ve inherited classmethod, wrapper-to-plain binding değişimi ve
`gc_interval=1` ayrı integration testlerinde doğrulanır. Debug/release toplam
177 test ve debug/release × interpreter/JIT × normal/stress GC differential
matrisinin sekiz koşusu geçer.

## Sınırlar

Custom descriptor/property, `super`, callable instance, variadic/expanded çağrı
ve safepoint içeren argüman hazırlığı generic yolda kalır. Helper lookup maliyeti
her çağrıda ödenir; ölçümle gerekçelendirilirse class/shape/dependency-version
guard'larıyla daha doğrudan native lookup eklenebilir.

Bu helper maliyeti daha sonra [ADR 0034](0034-jit-method-entry-cache.md) ile
invocation-local lazy cache'e indirilmiş; exact owner guard'ı eklenmiştir.
