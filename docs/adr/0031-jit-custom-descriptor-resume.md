# ADR 0031 — Custom descriptor `ATTR` resume ve returned-leaf inlining

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

Custom descriptor `__get__` semantiği JIT helper içinde yeniden uygulanmaz.
Profille kararlı bir `ATTR(...)->CALL` dizisinde generic `ATTR`, exact bytecode
PC'siyle interpreter'a yan çıkar. Interpreter property/custom descriptor/MRO
kurallarını normal VM frame'leriyle yürütür. Sonuç caller register'ına yazıldığında
parent frame `pc + 1` konumundan native koda döner; descriptor'ın döndürdüğü exact
Tonic function guard'ı geçerse leaf call ADR 0026/0027 yolunda inline edilir.

Generic `ATTR` desteği yalnız sonucu bir guarded direct-call site tarafından
tüketilen, arada callee register'ını değiştirmeyen `CONST/MOVE` hazırlığı bulunan
sitelerde derlenir. Direct-call profili yoksa code object unsupported kalır. Bu
kârlılık kapısı, hem `ATTR` hem generic `CALL` için iki side exit ödeyen kodun
adaptive interpreter'dan daha yavaş çalışmasını engeller.

## Deopt, hata ve GC

Descriptor getter allocation, exception, nested call veya class mutation yapabilir;
bunların tümü interpreter'ın mevcut continuation ve precise root mekanizmasını
kullanır. Getter tamamlandıktan sonra returned function değişmişse exact callee
guard'ı `CALL` PC'sine deopt eder ve güncel callable generic binder ile çağrılır.
Bu yol descriptor nesnesini ya da sonucunu persistent native pointer olarak tutmaz.
Stress GC testi her allocation sınırında collection ile getter frame, caller frame
ve dönen callable root'larını birlikte doğrular.

## Ölçüm

100.000 iterasyonda `Forward.__get__` global `add` function'ını döndürür ve
`math.op(total,b=1)` çağrılır. Değişiklik öncesinde JIT code üretmiyordu; son
aynı-koşu A/B ölçümü:

- adaptive interpreter: 51,424 ms;
- direct-call kapalı JIT: 54,859 ms, code üretilmedi;
- descriptor resume + direct leaf: 41,565 ms;
- native code: 1.408 byte;
- 99.937 `ATTR` side exit/resume ve 99.937 direct leaf call;
- sıfır deopt, sıfır direct-method site.

Adaptive kontrole göre süre yaklaşık %19,2 azalmıştır (1,24×). Ayrı mutation testi
getter'ın döndürdüğü global function değiştiğinde exact `CALL` deopt'unu ve yeni
sonucu doğrular.

## Sınırlar

Descriptor getter'ın kendisi native değildir ve çağrı başına bir side exit/resume
maliyeti vardır. Direct leaf olmayan descriptor sonuçları bu kârlılık kapısından
geçmez. Property/custom descriptor sonucuna yapılan variadic/expanded çağrılar,
`super` ve callable-instance zincirleri generic yolda kalır.
