# ADR 0020 — Adaptive loop OSR

## Durum

Kabul edildi ve uygulanmıştır.

## Karar

Backedge içeren fonksiyonlar ilk girişte doğrudan JIT edilmez. İlk giriş ve ilk
63 backedge adaptive interpreter'da type feedback toplar. Altmış dördüncü hotness
gözleminde aktif frame'in register'ları korunur, code object Cranelift ile derlenir
ve loop hedefinin exact bytecode PC'sinden native yürütmeye geçilir.

`Vm::jit_osr_threshold` varsayılan 64'tür ve test/ölçüm için değiştirilebilir.
Eşiğin altında biten loop compile edilmez. Unsupported code object eşikte bir kez
denenir ve kalıcı generic fallback olarak işaretlenir. Fuel açıkken JIT ve OSR
devre dışı kalır, böylece instruction budget semantiği korunur.

Cranelift code object'i backedge veya resumable `CALL` içeriyorsa giriş switch'i
verified instruction PC'lerinin tamamını hedefleyebilir. Call continuation ve OSR
aynı arbitrary-PC entry ABI'sini paylaşır. Loop içindeki bütün boxed değerler ABI
register dizisinde materialized olduğundan OSR için ek nesne materialization'ı
gerekmez.

## Doğrulama ve ölçüm

Cold `sum_to(10)` compile üretmez. `sum_to(100)` eşiği geçer ve testte
`jit_osr_entries == 1` olur. Bir milyon iterasyonluk benchmarkta interpreter
instruction sayısı 13.000.021'den 835'e, adaptive süre 90,684 ms'den 3,699 ms'ye
iner. Bu koşu compile süresini de içerir.

OSR sonrası native backedge'ler ADR 0019'daki periyodik poll safepoint'ini kullanır.
Module-level region OSR, exception handler entry'leri, generator resume ve unboxed
deopt materialization henüz desteklenmez.
