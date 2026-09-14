# ADR 0039 — Unboxed float loop deopt map'i

## Durum

Kabul edildi — 12 Eylül 2026.

## Karar

Runtime, JIT compile anındaki exact-float function parametre slotlarını execution
profile olarak Cranelift katmanına verir. CFG dataflow `Move`, `Add`, `InplaceAdd`,
`Sub` ve `Mul` boyunca kesin float bilgisini sabit noktaya taşır; farklı tiplerin
birleştiği veya float'ın desteklenmeyen bir operasyonda kullanıldığı fonksiyon eski
boxed lowering'e düşer.

Kanıtlanan float register'lar Cranelift explicit native stack slotlarında F64 olarak
tutulur. Float constant doğrudan IEEE bitleriyle slota yazılır. Arithmetic ara
sonuçları guest heap'e ayrılmaz. Float `Return` tek `BoxFloat` safepoint'iyle hem
kaynak guest register'ını hem JIT dönüş slotu register 0'ı materialize eder.

## Stack/deopt map ve OSR

Her erişilebilir bytecode PC'si için toplam guest register sayısı ve unboxed F64
register listesi kaydedilir. Listede olmayan register'lar precise root tamponunda
zaten materialized durumdadır. Bunlar Tonic deoptimization stack map'leridir;
Cranelift'in GC pointer bitmap'i değildir. F64 bir managed pointer olmadığından GC
root olmaz; bütün managed `Value` handle'ları mevcut explicit root tamponunda kalır.

Native fonksiyon arbitrary `start_pc` alabilir. Entry prologue, seçilen PC'nin
map'ine göre gereken her F64 slotunu opaque `UnboxFloat` helper'ıyla boxed VM
register'ından kurar. Type miss hiçbir guest işlemi yürütmeden aynı `start_pc`ye
deopt eder ve özgün register tamponunu değiştirmez.

Backedge poll helper'ının sıfır olmayan sonucu deopt isteğidir. Generated code hedef
PC map'indeki her canlı F64 slotunu `BoxFloat` ile aynı numaralı guest register'a
yazar; her allocation exact root tamponuyla safepoint yapar. Bütün map
materialize edildikten sonra hedef PC döndürülür. Interpreter bu tampon ve PC ile
doğrudan devam edebilir.

## Doğrulama ve ölçüm

JIT unit testi `accumulate` fonksiyonunda PC=8 map'ini `[0, 1, 7, 8]` olarak
doğrular. Baştan native çalışma 5.0 üretir; boxed `value=2.5`, `step=0.5`, `i=5`
durumundan PC=2 resume yine 5.0 üretir. Yanlış step tipi PC=2'ye tamponu değiştirmeden
deopt eder. İstekli poll 1.024'üncü backedge'de canlı `value` ve `step` slotlarını
iki ayrı boxed register olarak kurup PC=2'ye döner.

100.000 iterasyonluk release benchmarkında boxed JIT 3,115 ms ve 100.003 allocation,
unboxed loop 1,969 ms ve 67 allocation üretmiştir. Güncel adaptive interpreter
10,766 ms'dir. Runtime helper sayısı integer kontrol işlemlerinin güvenli generic
yola alınması nedeniyle 199.975'e çıkmıştır; buna rağmen net hızlanma 5,47×'dir.

## Sınırlar

İlk dataflow float karşılaştırma, division, unary float ve call/attribute içeren
loop'u specialize etmez. Bu fonksiyonlar doğru boxed JIT/interpreter yolunda kalır.
Deopt map formatı bugün yalnız F64 machine değerini tanır; gelecekte unboxed integer
ve scalar-replaced aggregate türleri yeni açık location varyantları gerektirebilir.
