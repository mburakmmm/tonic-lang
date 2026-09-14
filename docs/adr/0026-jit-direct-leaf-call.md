# ADR 0026 — Guard'lı exact-callee leaf inlining

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

Adaptive interpreter'da monomorphic `TonicCall` durumuna ulaşmış bir `CALL`,
caller JIT derlenirken profile-backed doğrudan çağrı adayıdır. Callee şu dar ve
kanıtlanabilir koşulların tamamını sağlıyorsa gövdesi caller native koduna alınır:

- exact positional arity; keyword, default, vararg ve kwarg yoktur;
- closure/free/cell veya class-body state'i yoktur;
- ilk erişilebilir `RETURN` öncesinde yalnız immediate `CONST`, `MOVE` ve exact-int
  `ADD`/`INPLACE_ADD`/`SUB`/`MUL` vardır;
- bu işlemler guest-visible yan etki ve allocation üretmez.

Generated code callee'nin raw logical handle değerini profildeki exact handle ile
karşılaştırır. Argümanların bound olduğu ve arithmetic operandlarının exact
immediate integer olduğu korunur; sonuç immediate aralığı ve multiplication
overflow ayrıca kontrol edilir. Herhangi bir guard kaçarsa callee gövdesindeki
ara sonuçlar caller register'larına yazılmadan özgün caller `CALL` PC'sine deopt
edilir. Generic VM çağrı yolu binder'ı ve normal traceback/error semantiğini tek
doğruluk kaynağı olarak korur.

Callee handle'ı guest heap adresi değildir. Slot + generation içeren logical
`Value` guard'ı heap compaction sonrasında değişmez; slot reuse generation'ı
artırdığı için farklı bir nesne eski guard'ı geçemez. Inlined gövde allocation
yapmadığından yeni bir GC safepoint gerektirmez. Caller'ın var olan backedge poll
safepoint'leri bütün materialized caller register'larını köklemeye devam eder.

Call-depth sınırı korunur: direct-call site'ı içeren compiled entry, mevcut frame
sayısı yeni çağrıyı kabul etmeyecek düzeydeyse çalıştırılmaz ve interpreter'a
bırakılır. Logical `Stats::calls` sayacı generated entry'nin ayrı bir `u64`
counter pointer'ına yazdığı başarılı inline-call sayısıyla tamamlanır;
`jit_direct_calls` bu alt kümeyi ayrıca raporlar.

## Profil ve invalidation

Straight-line caller'ın sekizinci girişinde JIT eşiği ile call quickening eşiği
çakışabilir. Site yedinci kararlı gözlemdeyse derleme bir giriş ertelenir; dokuzuncu
girişte exact-callee profili native koda alınır. Loop OSR'da profil zaten hotness
eşiğinden önce oluşur.

Global veya local callee rebinding compile edilmiş kodu sessizce yanlış hedefe
götürmez. Exact guard miss `CALL` PC'sine deopt eder. Mevcut bounded deopt sayacı
tekrarlanan kararsızlıkta code object'i adaptive interpreter'a indirir. Bu sürüm
PIC biçiminde iki native inline target veya yeniden derleme yapmaz.

## Ölçüm

100.000 `add(total, 1)` çağrılı loop'ta aynı binary içindeki A/B sonucu:

- adaptive interpreter: 13,368 ms;
- JIT, direct inlining kapalı: 8,019 ms ve 99.937 side exit/resume;
- JIT, direct inlining açık: 0,760 ms, sıfır side exit/resume ve 99.937 inline call.

Yeni yol eski resumable JIT çağrı yolundan 10,55×, adaptive interpreter'dan
17,58× hızlıdır. Compiled code 1.356 byte'tan 1.780 byte'a çıkmıştır. Ayrıntılı
yöntem ve ham çıktı [JIT_DIRECT_CALL_BASELINE.md](../JIT_DIRECT_CALL_BASELINE.md)
dosyasındadır.

## Sınırlar

Bu kararın keyword/default sınırı [ADR 0027](0027-jit-direct-call-binding.md) ile
exact function profilleri için, plain bound-instance method sınırı
[ADR 0028](0028-jit-bound-method-fusion.md) ile ve instance staticmethod sınırı
[ADR 0029](0029-jit-staticmethod-fusion.md) ile, classmethod ve class erişimi sınırı
[ADR 0030](0030-jit-classmethod-fusion.md) ile kaldırılmıştır. Genel calling
convention yine tamamlanmış değildir. Custom descriptor,
variadic/expanded,
closure, allocation veya exception helper'ı kullanan ve control-flow içeren
callee'ler mevcut explicit-frame side exit/resume yolunda kalır. Recursive çağrı
native stack recursion'a çevrilmez. Daha genel direct call için precise stack map,
deopt frame reconstruction ve callee dependency/invalidation tasarımı gereklidir.
