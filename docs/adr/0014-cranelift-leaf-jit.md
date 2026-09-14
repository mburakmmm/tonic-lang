# ADR 0014 — Cranelift leaf JIT ve deopt ABI'si

## Durum

Kabul edildi; ilk JIT dilimi, hotness tiering, allocation üreten ilk runtime helper
ve resumable Tonic call uygulanmıştır. Doğrudan hızlı call, invalidation, OSR ve
machine stack map ayrı açık işlerdir.

## Karar

Cranelift entegrasyonu `tonic-jit` crate'inde tutulur ve doğrudan bağımlılıklar
0.119.0 sürümüne sabitlenir. Compiler veya runtime Cranelift tiplerini public
API'sine taşımaz.

Bir derlenmiş leaf fonksiyon şu dahili ABI'yi kullanır:

```text
extern "C" fn(
    registers: *mut u64,
    runtime_context: *mut c_void,
    runtime_helper: extern "C" fn(...),
    start_pc: usize,
) -> u64
```

Register sayısı verified `CodeObject` metadata'sından gelir. Runtime tam bu
uzunlukta geçici bir raw register dizisi verir. Status'un üst biti native dönüşü,
ikinci üst biti helper hatasını, üçüncü üst biti resumable side exit'i; kalan bitleri dönüş/deopt/hata bytecode PC'sini
gösterir. Dönüş değeri register
0'a yazılır. Guard failure öncesinde interpreter'ın ihtiyaç duyduğu değerler aynı
dizide materialized kalır; runtime diziyi VM frame'ine geri kopyalayıp bildirilen
generic opcode'dan yürütür.

İlk destek kümesi immediate `int`, `bool` ve `None` sabitleri; move; integer
`+ - * // %`; runtime üzerinden `/`; unary `+ - not`; integer karşılaştırmaları;
koşullu/koşulsuz branch, loop ve return'dür. Integer işlemleri exact-tag guard'ı taşır. Çarpma hem makine
signed-overflow hem Tonic immediate aralığını, toplama/çıkarma ve floor işlemleri
immediate aralığını korur. Sıfıra bölme native trap üretmez; deopt ile generic
runtime'ın `ZeroDivisionError` yoluna döner. `//` ve `%`, negatif operandlarda
Python floor/remainder işaret semantiğini native kodda uygular. `/`, raw nesne
adresini açığa çıkarmayan logical `Value` sözcükleriyle runtime helper'a gider;
float allocation ve generic numeric semantik Tonic heap'inde kalır. Helper hatası
exact bytecode PC, statik hata türü ve mesajla döner.

Backedge içeren desteklenebilir leaf fonksiyon ilk girişte; düz leaf fonksiyon
varsayılan olarak sekizinci girişte derlenir. Eşik `Vm::jit_threshold` ile test ve
ölçüm amacıyla ayarlanabilir. Ölçülen native bridge maliyeti nedeniyle yedi
instruction'dan küçük düz fonksiyonlar varsayılan olarak adaptive interpreter'da
kalır; `Vm::jit_min_instructions` bu kârlılık eşiğini testlerde değiştirebilir.
Bir code object closure, class state, heap constant veya desteklenmeyen opcode
içerirse kısmi native yürütme yapılmaz; fonksiyon interpreter'da kalır. Desteklenen
fonksiyondaki dinamik type/overflow guard failure ise exact PC deopt'udur.

## GC ve güvenlik

Allocation yapabilen runtime helper çağrısı safepoint'tir. Generated code bütün virtual
register'ları çağrı öncesinde raw register dizisinde materialize eder. Runtime bu
diziyi, aktif JIT frame aralığı çıkarılmış diğer VM roots ile birleştirir ve GC
gerekiyorsa helper işlemi öncesinde collection yapar. Handle'lar logical ve stable
olduğu için compaction sonrasında pointer düzeltme gerekmez. Ardışık `/` helper'ları
arasındaki tek canlı heap değerinin stress collection'dan sağ çıktığı test edilir.
Bu explicit root ABI mevcut boxed değerler için stack map gerektirmez. Gelecekte
yalnız machine register/stack'te tutulan unboxed managed değerler, Tonic calls ve
backedge safepoint'leri kesin stack map gerektirir.

`unsafe`, `tonic-jit` crate'i içinde finalized code pointer dönüşümü ile trampoline'ın
senkron çağrı bağlamı/register/output pointer erişimlerine sınırlıdır. Her blok
invariant'ını açıklar. Trampoline `catch_unwind` ile Rust panic'inin native FFI
sınırını aşmasını engeller. Sahip `JITModule`, entry pointer'dan uzun yaşar;
register erişimleri bytecode verifier'ın doğruladığı indeksler ve runtime'ın exact
uzunluk kontrolü üzerinden yapılır. `tonic-runtime` unsafe-free kalır.

Instruction fuel açıkken JIT devre dışıdır. Native loop henüz fuel poll
içermediğinden aksi davranış yürütme bütçesini anlamsızlaştırır.

## Ölçüm ve doğrulama

Runtime compile attempt, başarılı compile, native call/return, deopt, fallback,
bounded kararsız-site de-specialization, compile nanosaniyesi ve code byte
sayaçlarının yanında helper call, safepoint, helper-triggered collection ve runtime
error sayaçlarını raporlar. Aynı code object sekiz guard failure ürettiğinde native
girdi bırakılır ve sonraki çağrılar generic interpreter'da kalır. Aynı differential corpus
interpreter ve `TONIC_JIT=1` ile, normal ve stress GC altında çalıştırılır.
Production-ready kabulü için doğrudan hızlı Tonic call, invalidation, OSR,
native backedge safepoint'leri, gerekli machine stack map'leri ve daha geniş deopt
state testleri tamamlanmalıdır.

Exact-callee/arity yan etkisiz integer leaf alt kümesi daha sonra
[ADR 0026](0026-jit-direct-leaf-call.md) ile native caller gövdesine alınmıştır;
exact function default/keyword binding planı [ADR 0027](0027-jit-direct-call-binding.md)
ile, plain bound-instance method fusion [ADR 0028](0028-jit-bound-method-fusion.md)
ile, staticmethod fusion [ADR 0029](0029-jit-staticmethod-fusion.md) ile eklenmiştir.
Classmethod ve class-level method fusion
[ADR 0030](0030-jit-classmethod-fusion.md), custom descriptor returned-leaf resume
[ADR 0031](0031-jit-custom-descriptor-resume.md), resumable expanded-call segmenti
[ADR 0032](0032-jit-expanded-call-segment.md), gözlenmeyen boş variadic fast path
[ADR 0033](0033-jit-unobserved-variadics.md), guarded sequence/named expansion
[ADR 0035](0035-jit-positional-sequence-expansion.md) ve exact-dict mapping
expansion [ADR 0036](0036-jit-mapping-expansion.md) ile eklenmiştir. Ordinary
call'da gözlenen variadic materialization
[ADR 0037](0037-jit-materialized-variadics.md) ile eklenmiştir; expanded/method
varyantları açık kalır.
Method dependency lookup amortization ve owner guard'ı
[ADR 0034](0034-jit-method-entry-cache.md) ile eklenmiştir.
