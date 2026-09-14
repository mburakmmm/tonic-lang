# ADR 0001 — Bağımsız ilk yürütme dilimi

Durum: bootstrap; public stable ABI kararı değildir.
Bu kayıt ilk dilimi anlatır. Scope/call/heap kararlarının sonraki hali
[ADR 0002](0002-closures-calls-precise-gc.md) ile güncellenmiştir; aşağıdaki
arena ve GC-yok açıklamaları tarihsel durumdur.

## Problem ve seçenekler

Boş depodan Python sözdizimini koruyan, JIT/GC'ye genişletilebilir çalışır bir
runtime gerekiyor. El yazısı eksik bir Python grameri veya CPython'a execution
aktarmak ürün hedefini karşılamaz. RustPython **parser** 0.4.0 sabitlenmiştir;
RustPython VM kullanılmaz. Parser AST'si Tonic-owned AST'ye çevrilir.
Kaynak: https://docs.rs/rustpython-parser/0.4.0/rustpython_parser/
Bağımlılık MIT lisanslıdır; parser yalnızca compile yolundadır, VM hot path,
nesne layout veya bytecode formatını yönetmez. `num-bigint`/`num-traits`
(MIT/Apache-2.0) tam sayı fallback'ini sağlar; immediate işlemler bunlara girmez.
Tam çözülmüş sürümler Cargo.lock'tadır.

## Karar

Dört crate: core (AST/bytecode/tanı), compiler (parser/scope/lowering), runtime
(Value/heap/VM/native API), CLI. Conceptual aşamalar crate sayısından bağımsızdır.
8-byte instruction dört açık little-endian u16 word kullanır. Opcode numaraları
manuel sabittir; ham Rust layout serialize edilmez. Program doğrulaması
immutable VerifiedProgram üretir. Bytecode dosya formatı henüz yayımlanmaz.

Value düşük 3 bit etiketli u64'tür. 61-bit signed immediate integer, bool,
None ve unbound sentinel kullanılır; diğerleri logical heap index'tir.
Büyük tam sayılar arbitrary precision'a yükselir. Float başlangıçta heap'tedir.
NaN boxing ölçülmeden benimsenmemiştir. İç Value dış native Handle değildir.

VM açık frame ve register vektörleri kullanır; guest recursion Rust stack'ini
büyütmez. Fonksiyon parametreleri contiguous register penceresinden kopyalanır,
arg tuple/kwargs dict oluşturulmaz. Symbol ID ile global slot aranır; yerel
okuma/arithmetik yolunda string lookup yoktur. Builtin bağları değiştirilebilir.
Geçici registerlar statement sonunda yeniden kullanılır; tam CFG liveness yoktur.

BOOTSTRAP heap runtime ömürlü arena'dır: otomatik tracing/reclamation yoktur.
Bütün nesnelerin managed-edge ziyaretçisi vardır. İç heap slotları tekrar
kullanılmaz, native referanslar opaque scoped/persistent handle ile tutulur.
Heap relocation Vec yeniden ayırdığında logical index'i değiştirmez.
Bu generational/moving collector değildir ve sınırsız süreç için uygun değildir.

Native handle table her slot tahsisinde process-wide, tekrar kullanılmayan
u32 token verir; handle u64 içinde token/index taşır. Token tükenince hata,
wrap yoktur. Böylece ayrı runtime tablolarında aynı index stale/cross-runtime
alias olmaz. Table lookup indexed, local cleanup scope Drop, persistent cleanup
explicit'tir. Bu token politikası ABI sabitlenmeden tekrar değerlendirilecektir.

## Sonuçlar ve riskler

Küçük dikey dilim güvenli Rust ile denetlenebilir; unsafe yasaktır. Compile-time
AST recursion ve arena büyümesi bootstrap kaynak sınırlarıdır. Fonksiyon/frame
ve kaynak boyutu sınırları tanı üretir. Uzantı callback/reentry ve GC henüz yoktur.
Native C ABI ve CPython bağımlılığı eklenmez; Rust API binary-stable değildir.

Performans iddiası yapılmaz. Interpreter baseline benchmark'ı eklenecek;
Value seçenekleri/GC/JIT için daha sonra karşılaştırmalı deney gerekir.
