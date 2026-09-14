# Security Policy / Güvenlik Politikası

## Supported versions / Desteklenen sürümler

Tonic is currently an alpha-stage runtime and has no supported stable release.
Security fixes are applied to the latest `main` revision only.

Tonic şu anda alfa aşamasındadır ve desteklenen kararlı bir sürümü yoktur.
Güvenlik düzeltmeleri yalnızca en güncel `main` revizyonuna uygulanır.

## Reporting a vulnerability / Güvenlik açığı bildirme

Please use GitHub's **Report a vulnerability** private reporting flow when it is
available for this repository. Do not open a public issue for a suspected memory
safety problem, bytecode verifier escape, handle-forging path, sandbox escape, or
credential disclosure.

Bu depoda kullanılabiliyorsa GitHub **Report a vulnerability** özel bildirim
akışını kullanın. Olası bellek güvenliği sorunu, bytecode verifier atlatma,
handle üretme yolu, sandbox kaçışı veya kimlik bilgisi sızıntısı için herkese
açık issue oluşturmayın.

Include the affected revision, platform, reproduction steps, expected impact,
and whether the report contains proof-of-concept code. You should receive an
initial acknowledgement through GitHub within seven days.

Etkilenen revizyonu, platformu, tekrar üretme adımlarını, beklenen etkiyi ve
bildirimin proof-of-concept kod içerip içermediğini ekleyin. GitHub üzerinden
yedi gün içinde ilk yanıt hedeflenir.

## Scope / Kapsam

High-priority areas include unsafe/FFI boundaries, bytecode verification, moving
GC roots and write barriers, JIT guards/deoptimization, native handle ownership,
CPython bridge lifetime rules, and malformed guest input.

Öncelikli alanlar unsafe/FFI sınırları, bytecode doğrulama, moving GC kökleri ve
write barrier'lar, JIT guard/deoptimization, native handle sahipliği, CPython
köprüsü yaşam süreleri ve bozuk guest girdisidir.

Tonic does not yet claim production hardening or resource-isolation sandboxing.
Reports that demonstrate memory unsafety or host compromise are still in scope.

Tonic henüz production hardening veya kaynak izolasyonlu sandbox garantisi
vermez. Bellek güvensizliği veya host ele geçirilmesi gösteren bildirimler yine
kapsamdadır.
