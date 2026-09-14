# ADR 0032 — Resumable expanded-call segmenti

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

`BEGIN_ARGS` ile eşleşen dış `CALL_EXPANDED` arasındaki argument-builder bölgesi
tek generic VM segmenti olarak çalışır. JIT `BEGIN_ARGS` PC'sinde side exit eder,
caller frame o andaki global expanded-argument stack derinliğini kaydeder ve
`jit_resume` işaretini geçici olarak kapatır. Interpreter şu opcodları mevcut
semantiğiyle yürütür:

```text
BEGIN_ARGS, ARG_POS, ARG_STAR, ARG_NAMED, ARG_MAPPING, CALL_EXPANDED
```

Bir `CALL_EXPANDED` builder'ı pop ettikten sonra stack derinliği kaydedilen dış
derinliğe dönerse parent frame native koda sonraki PC'den devam eder. İç içe
expanded çağrılar bu nedenle erken resume üretmez. Callee normal VM frame'i,
builtin veya başka bir JIT tier olabilir.

## Neden opcode başına resume yok

Her argument-builder opcode'unda JIT↔VM geçişi yapmak star iterator, duplicate
keyword ve mapping sırası semantiğini korusa da geçiş maliyetini katlar. Segment
sınırı aynı generic kodu bir kez çağırır ve pending `ExpandedArgs` değerlerini
VM'in mevcut precise root taramasında tutar. `*args`, `**kwargs`, deferred star,
yanlış keyword ve soldan sağa değerlendirme davranışı tek bir uygulamada kalır.

## GC ve hata davranışı

Argument stack VM-owned olduğu için star expansion veya nested call sırasında
allocation/collection olduğunda pending positional/keyword değerleri normal root
kaynağıdır. Hata segment içinde doğrudan normal traceback/PC yoluyla yayılır.
Başarılı dış call öncesinde resume işareti parent frame'e yazılır; child frame
döndükten sonra caller güvenli biçimde devam eder.

## Ölçüm ve doğrulama

100.000 kez `add(total,*values)` çağıran loop değişiklik öncesinde code üretmiyor
ve adaptive interpreter'da 28,760 ms sürüyordu. Son aynı-koşu A/B ölçümü:

- adaptive interpreter: 24,762 ms;
- resumable expanded JIT: 21,032 ms;
- native code: 1.392 byte;
- 99.937 segment side exit/resume;
- interpreter instruction: 2.100.027 → 1.000.715.

Süre yaklaşık %15,1 azalır (1,18×). Unit test exact `BEGIN_ARGS` PC side exit'ini;
integration testleri normal ve nested `*`/`**` builder'larını, matching-depth
resume'u ve her-allocation stress GC'yi doğrular.

## Sınırlar

Expanded çağrının bağlanması ve callee frame kurulması generic kalır; direct
variadic/expanded inlining yapılmaz. Segment başına bir side exit/resume maliyeti
vardır. İleride sabit expanded shape profili ve allocation davranışı ölçülmeden
argument tuple/dict materialization kaldırılmayacaktır.
