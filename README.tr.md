# Tonic

[![CI](https://github.com/mburakmmm/tonic-lang/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/mburakmmm/tonic-lang/actions/workflows/ci.yml)
[![Rust 1.86+](https://img.shields.io/badge/Rust-1.86%2B-000000?logo=rust)](rust-toolchain.toml)
[![Durum: Alfa](https://img.shields.io/badge/durum-alfa-orange)](docs/ROADMAP.md)

[English](README.md) · **Türkçe**

Python sözdizimi kullanan, Rust ile yazılan bağımsız bir dil/runtime.
**Bu sürüm geliştirme aşamasındadır; tam Python uygulaması değildir.**
Çekirdekte CPython, Python interpreter subprocess'i veya RustPython VM yoktur.

## Çalıştırma

Rust stable (MSRV 1.86) gerekir. İlk derleme Cargo bağımlılıklarını indirir.
Core CLI için CPython gerekmez. Bütün workspace'i veya `tonic-cpython` crate'ini
derlemek `python3-config --ldflags --embed` sağlayan CPython 3.12+ development/runtime
kurulumu ister; farklı executable `PYTHON_CONFIG` ile seçilebilir.

```sh
cargo build --workspace --locked
cargo run -p tonic-cli -- examples/fib.tonic
# 102334155
cargo run -p tonic-cli -- examples/fastmath.tonic
# 42
cargo run -p tonic-cli -- -c 'print(20 + 22)'
cargo run -p tonic-cli -- --check examples/fib.tonic
cargo run -p tonic-cli -- --dump-bytecode examples/fib.tonic
cargo run -p tonic-cli -- --stats --fuel 1000000 examples/fib.tonic
cargo run -p tonic-cli -- --jit --stats -c 'def sum_to(n):\n total=0\n while n:\n  n-=1\n  total+=n\n return total\nprint(sum_to(100))'
cargo run -p tonic-cli -- --gc-every 1 examples/closures.tonic
cargo run -p tonic-cli -- --gc-every 1 examples/classes.tonic
```

`tonic -` stdin'den kaynak alır. `.py` ve `.tonic` dosyaları aynı pipeline'ı
kullanır. `--check` yalnızca parse etmez: compile ve bytecode doğrulaması da yapar.
Hatalar dosya/satır/sütun ve fonksiyon zinciriyle stderr'e yazılır.
Çıkış kodları: başarı 0, dil/runtime hatası 1, CLI/dosya hatası 2.

## Mevcut durum

| Alan | Durum |
|---|---|
| M0 temel | Cargo workspace, CLI, tanılar, benchmark harness |
| M1 yürütme | sabitler, isimler, atama, aritmetik, `print` |
| Fonksiyonlar | `def`, nested functions, closure/cells, `global/nonlocal`, recursion; defaults, positional-only/keyword-only, keywords, `*args/**kwargs` |
| Lambda | bağımsız lexical scope, closure/cells, defaults ve tam mevcut call binder |
| Kontrol akışı | return, if/while/for, break/continue/else |
| Ek ifadeler | bool/None, bigint/float/string, tuple/list/dict, unpack, indexing/item assignment, kısa devre, zincirli karşılaştırma, koşullu ifade |
| VM | 8-byte register instructions, verifier, yeniden kullanılan geçici registerlar, açık frame stack, fuel/recursion limitleri |
| Interop | ABI v1 C table/panic guard; typed buffer; callback/reentry; foreign wrapper/vtable, precise trace ve deferred exactly-once destructor; staged shutdown |
| Sınıflar | class scope, `__init__`, bound/unbound metot, private mangling, C3 multiple inheritance, class attribute rebinding |
| Özel protokoller | instance `__call__`, `__len__`, `__bool__`; class MRO lookup ve askıya alınabilir VM continuation |
| Decorator/descriptor dilimi | function/class decorators, `staticmethod`, `classmethod`, property, custom `__get__/__set__/__delete__`, otomatik `__set_name__`, metaclass seçimi ve `__prepare__/__new__/__init__` zinciri |
| M3 | class/instance, ortak shapes + slotlar, dictionary fallback, canlı mappingproxy ve canonical builtin type nesneleri |
| Bellek | precise generational tracing, nursery/old ayrımı, write barrier, remembered set, cycle collection, compaction ve stress GC |
| JIT ilk dilim | Cranelift 0.119; integer native yolları, runtime-helper true division, resumable recursive calls, loop OSR, allocation/call/backedge safepoint'leri |
| M4 interpreter | integer aritmetik quickening; monomorphic ve iki girişli basit function-call ile class/shape/slot/dependency-version guard'lı instance attribute cache'leri |
| M4–M7 | expanded sequence/mapping, observed variadic, exact-float direct ve loop-carried F64 yolları; PC-indexli deopt map ve tam register rekonstrüksiyonu |
| İstisnalar | managed exception nesneleri ve traceback state, typed/tuple/bare `try/except/else`, bare reraise, `raise from`, cause/context zinciri, frame unwind, `finally` ve senkron `with` |
| Geniş dil | comprehension, generator, async, match, f-string vb. henüz yok |
| CPython bridge | ayrı `tonic-cpython` crate; bigint/primitive/list/tuple/dict/foreign dönüşüm, GIL state guard, alias/cycle-aware materialization, runtime/execution guard'lı gerçek `PyTonicProxy` heap type, positional/keyword callback, attribute/set/repr forwarding, weak identity cache ve bounded iki-collector graph/cycle taraması |
| HPy/aHPy | HPy Universal `.hpy0` host ve aHPy cross-runtime hattı proje kapsamına alındı; loader/context/field/type uygulaması henüz yok |
| Diğer interop | shared-library loader henüz yok; graph limitini aşan veya global Python altyapısına giren bridge graph'ları conservative retention kullanır |

Aritmetik: `+ - * / // %` ve bunların desteklenen tiplerde augmented assignment
biçimleri; unary `+ - not`. Karşılaştırmalar `== != < <= > >=`.
`and/or` operand döndürür ve kısa devre yapar. Builtin isimleri yeniden bağlanabilir.
`print`, `range`, `len`, `abs`, `object`, `isinstance`, `issubclass`, `getattr`,
`setattr`, `hasattr` sağlanır. Type-check builtin'lerinin classinfo argümanı
Tonic kullanıcı sınıfları, `object` ve bunlardan oluşan tuple'lardır; henüz
`int/str/type` gibi genel builtin type nesneleri yoktur.
`import` yalnızca kayıtlı native modülleri
bulur; `import fastmath as fm` desteklenir.
`fastmath.array(list_or_tuple)` sayıları tek seferde non-moving C-contiguous f64
buffer'a çevirir; `fastmath.sum(buffer)` sonrasında eleman boxing veya buffer copy
yapmadan typed slice üzerinde çalışır. `fastmath.sum` liste/tuple için generic
boxed-element fallback'ini de korur.
`print` için `sep/end` keywordleri vardır; `file/flush` henüz yoktur.
Defaults fonksiyon tanımında bir kez değerlendirilir; mutable defaults paylaşılır.
Dict insertion order korunur; bool/int/integral-float eşdeğer anahtarları aynı
girdiye erişir. List/dict item assignment ve `d[key] += value` desteklenir.
Generic metot okuması bir bound-method nesnesi ayırır. Profille kararlı doğrudan
çağrılarda JIT plain instance method, staticmethod ve classmethod lookup ile leaf
call'u kaynaştırır; geçici bound-method ayırmadan `self`/`cls` bağlar. Custom
descriptor getter generic VM frame'inde çalışır; exact leaf sonucu varsa caller
native `CALL` öncesinde devam eder.

Desteklenmeyen yapılar konumlu `UnsupportedSyntax` tanısıyla reddedilir;
kaynak sessizce atlanmaz. Parser bağımlılığının gramer kapsamı, Tonic'in yürütme
kapsamından geniştir. Hiçbir Python sürümüne tam conformance sözü verilmez.

## Doğrulama

Güncel yerel matris 266 Rust testi ile 284 stdout ve 109 exception türü
diferansiyel vakasını debug/release × interpreter/JIT × normal/stress-GC
modlarında çalıştırır. CI ayrıca JIT'i Linux x86-64 ve macOS AArch64 üzerinde
debug/release olarak, iki fuzz hedefini de her iki mimaride AddressSanitizer ile
kapılar.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 tests/differential/run.py
TONIC_GC_EVERY=1 python3 tests/differential/run.py
TONIC_JIT=1 python3 tests/differential/run.py
TONIC_JIT=1 TONIC_GC_EVERY=1 python3 tests/differential/run.py
cargo bench -p tonic-runtime --bench interpreter --locked
cargo bench -p tonic-runtime --bench jit --locked
cargo bench -p tonic-runtime --bench jit_direct_call --locked
cargo bench -p tonic-runtime --bench generational_gc --locked
cargo bench -p tonic-runtime --bench native_c_abi --locked
cargo bench -p tonic-runtime --bench buffer --locked
cargo bench -p tonic-runtime --bench callback --locked
cargo bench -p tonic-runtime --bench foreign_lifecycle --locked
cargo bench -p tonic-cpython --bench bridge --locked
cargo bench -p tonic-cpython --bench cross_runtime_gc --locked
cc -std=c11 -Wall -Wextra -Werror -Iinclude -fsyntax-only tests/c_header_smoke.c
```

Python differential test oracle'ı, isteğe bağlı karşılaştırma benchmarkı ve ayrı
`tonic-cpython` uyumluluk crate'i için kullanılır; core Tonic binary'sini çalıştırmak
için gerekmez. Kaydedilen ölçümler ve
sınırlamaları [BENCHMARKS.md](docs/BENCHMARKS.md), güncel CPython karşılaştırması
[PYTHON_COMPARISON_STAGE8.md](docs/PYTHON_COMPARISON_STAGE8.md) içindedir.
JIT'e özel ara baseline [JIT_BASELINE.md](docs/JIT_BASELINE.md) dosyasındadır;
direct leaf-call A/B sonucu [JIT_DIRECT_CALL_BASELINE.md](docs/JIT_DIRECT_CALL_BASELINE.md),
native float baseline ve sonucu [JIT_FLOAT_BASELINE.md](docs/JIT_FLOAT_BASELINE.md),
native C ABI maliyeti [NATIVE_C_ABI_BASELINE.md](docs/NATIVE_C_ABI_BASELINE.md),
zero-copy buffer ölçümü [BUFFER_BASELINE.md](docs/BUFFER_BASELINE.md),
callback/reentry ölçümü [CALLBACK_BASELINE.md](docs/CALLBACK_BASELINE.md),
foreign wrapper/finalization ölçümü [FOREIGN_LIFECYCLE_BASELINE.md](docs/FOREIGN_LIFECYCLE_BASELINE.md),
CPython bridge sınır maliyeti [CPYTHON_BRIDGE_BASELINE.md](docs/CPYTHON_BRIDGE_BASELINE.md),
cross-collector GC maliyeti [CROSS_COLLECTOR_BASELINE.md](docs/CROSS_COLLECTOR_BASELINE.md),
generational GC ölçümü [GENERATIONAL_GC_BASELINE.md](docs/GENERATIONAL_GC_BASELINE.md)
dosyasındadır. Bunlar tamamlanma sonrası alınacak nihai benchmark değildir.

HPy Universal host ve aHPy geliştirme sırası, kabul kapıları ve dürüst paket
uyumluluğu sınırları [HPY_AHPY_STRATEGY.md](docs/HPY_AHPY_STRATEGY.md) içinde
tanımlanmıştır. Bu bir uygulama planıdır; mevcut sürüm `.hpy0` yüklemez.

## Mimari ve sınırlar

```text
RustPython parser (yalnızca syntax)
    → Tonic AST → scope resolution → register bytecode → verifier → Tonic VM
                                                    ↘ Cranelift adaptive JIT
                                                    ↘ Rust-native Context/Handle
```

- `tonic-core`: Tonic AST, bytecode, tanı/span; parser veya runtime bağımlılığı yok.
- `tonic-compiler`: parser adapter, bootstrap scope analysis, lowering.
- `tonic-runtime`: 64-bit Value, heap, açık frame/register VM, native boundary.
- `tonic-cpython`: ayrı libpython adapter'ı, primitive conversions ve `ForeignPyObject`.
- `tonic-jit`: Cranelift backend, exact-tag guard'ları ve bytecode-PC deopt ABI'si.
- `tonic-cli`: dosya/stdin/komut satırı girişi ve çıktı.

**Bellek:** collector canlı `Value` köklerinden iz sürer; erişilemeyen nesneleri
ve döngüleri toplar, canlı heap girişlerini sıkıştırır. Logical slot + generation
taşımadan etkilenmez; tekrar kullanılan slot eski referansı geçerli yapmaz.
Her nesne nursery'de başlar ve ilk minor collection'da ya toplanır ya old alana
terfi eder. Owner-aware mutation API'leri old→young kenarlarını remembered set'e
yazar; her 32. otomatik collection full-heap major collection'dır. Varsayılan
collection aralığı 1024 allocation'dır. `--gc-every 1` stress, `--no-gc`
karşılaştırma içindir. Collection instruction/JIT safepoint sınırlarında çalışır,
native Context scope'u içinde çalışmaz. Henüz bounded-pause garantisi veya
user-language finalizer semantiği yoktur. Foreign payload destructor'ları sweep
sonrasında ayrı queue'da çalışır. Float ve büyük integer sonuçları heap'e ayrılır;
küçük integer döngüleri ayrılmaz. Kalıcı native handle açıkça serbest bırakılmalıdır.
Heap metadata kapasitesi bırakılmayabilir; native kod uzun çalışırken GC gecikir.
Bu sürüm kaynak sınırlı sandbox veya production runtime garantisi vermez.

**JIT:** `--jit`, closure/class gövdesi olmayan desteklenebilir fonksiyonlarda immediate
integer `+ - * // %`, unary işlemler, karşılaştırma, doğruluk, branch ve döngüleri
Cranelift ile native koda çevirir. `/` opak runtime-helper ABI'sinden generic
numeric semantiğe ve float allocation'a gider. Döngülü kodda integer guard'ını
kaçıran `+ += - * // %` aynı ABI üzerinden genel numeric/container semantiğine
devam eder; integer fast path native kalır. Allocation yapabilen helper çağrısı bütün JIT kayıtlarını
materialize eden kesin GC safepoint'idir; hata türü ve bytecode PC'si VM'e döner.
Exact-tag ve immediate taşma guard'ları generic interpreter'a döner; unsupported
opcode fonksiyonun tamamını güvenli biçimde interpreter'a bırakır. Genel `CALL`, exact
PC side exit'iyle explicit VM frame kurar; child dönüşünde caller sonraki PC'den
native koda devam eder. Profille kararlı, closure içermeyen ve yalnız yan etkisiz
immediate-int işlemleri yapan leaf callee doğrudan caller native gövdesine alınır.
Positional-only, keyword-only ve default parametreler compile-time binding planıyla
tuple/dict ayırmadan bağlanır. Plain `instance.method(...)` dizisi de profilli
lookup helper'ı, exact function guard'ı ve implicit receiver planıyla tek native
yola kaynaştırılır; sıcak yolda bound-method nesnesi ayırmaz. Instance üzerinden
`staticmethod` erişimi aynı lookup'ta binding-kind guard'ıyla ve receiver eklemeden
kaynaştırılır. `classmethod` dinamik instance/class receiver'ıyla aynı guarded
yoldadır; class üzerinden plain function ve staticmethod erişimi de kapsanır.
Lookup ilk gerçek `ATTR` noktasında native giriş kapsamlı cache'e alınır; sonraki
iterasyonlar exact owner/function guard'larıyla helper çağırmadan ilerler.
Custom descriptor getter generic VM'de çalışıp exact leaf sonucu için native
caller'a döner. Expanded argument builder da dış `CALL_EXPANDED` tamamlanana kadar
tek generic segmentte çalışır ve ardından native caller'a döner. Exact logical-callee
ve operand-tag guard'ı kaçarsa hiçbir guest
yan etkisi oluşmadan özgün `CALL` PC'sine deopt edilir. Bound `LOAD_GLOBAL`, VM'in sabit boyutlu symbol tablosuyla
eşzamanladığı materialized değer dizisini doğrudan okur; `UNBOUND` hata yolu exact-PC
helper'ını kullanır. Böylece recursive `fib` host recursion olmadan JIT'te çalışır ve
global rebinding'i görür. `--fuel` instruction bütçesinin native döngüde delinmemesi
için JIT'i kapatır. Native backedge'ler inline sayaçla 1024 geçişte bir kesin-root
poll helper'ına gider. Düz `*list/*tuple` + named/default expansion kararlı uzunluk
profiliyle direct leaf'e gider. Exact built-in dict `**mapping` de kararlı string-key
profili, güncel value lookup ve key-count/presence guard'larıyla aynı yola alınır.
Ordinary direct leaf'in gerçekten okuduğu `*args/**kwargs` tuple/dict'i allocation
helper'larında kesin JIT roots'a materialize edilir. Exact-float profilli,
yan etkisiz direct leaf çağrıları argümanlarını bir kez unbox eder; `+ - * +=`
sonuçlarını Cranelift F64 SSA'da tutar ve yalnız dönüşü hidden precise root'a box
eder. Guard miss özgün `CALL` PC'sine atomik deopt olur. Profilli float parametreli
numeric loop'lar loop-carried F64 değerlerini native stack slotlarında tutar. Her
erişilebilir bytecode PC'sinin deopt map'i materialized ve unboxed register'ları
ayırır; arbitrary-PC OSR girişinde gereken F64 slotları boxed VM durumundan kurulur.
Poll deopt'u bütün canlı F64 slotlarını kesin root tamponundaki interpreter
register'larına box ederek exact hedef PC'den devam edilebilir durum üretir.
Backedge içeren fonksiyon 64 interpreted hotness
gözleminden sonra exact loop PC'sinde OSR yapar; düz leaf
fonksiyon varsayılan sekizinci girişte derlenir; sekiz guard kaybı site'ı yeniden
interpreter'a indirir. Yedi instruction'dan küçük düz fonksiyonlar ölçülen bridge
maliyeti nedeniyle adaptive interpreter'da kalır.

**Kapsam:** annotation execution, list/dict metotları,
slice assignment, range/custom-object slicing, string repetition, general filesystem
import/stdlib ve REPL yoktur. List, tuple ve Unicode string üzerinde read-only slice;
açık uçlar, negatif sınırlar ve negatif adım desteklenir.
Descriptor `__get__`/`__set__`/`__delete__` data ve non-data önceliğiyle,
`__set_name__` class body sonrasında tanım sırasıyla çalışır. Metaclass seçimi,
dict tabanlı `__prepare__`, `__new__/__init__` zinciri, canlı salt okunur class
`__dict__` mappingproxy, `for` için custom `__iter__/__next__` ve dict dışı
class namespace mapping'leri desteklenir; kalan numeric/operator protokolleri
açıktır.
Senkron context manager `__enter__/__exit__` özel-metot lookup'u, nested unwind,
exception suppression, managed traceback aktarımı ve return/break/continue
temizliğiyle desteklenir. `raise ... from ...`, örtük `__context__`, explicit
`__cause__`, `from None` suppression ve `__traceback__` exception state'i
desteklenir; traceback nesnesinin ayrıntılı frame-introspection API'si henüz
bootstrap kapsamı dışındadır.
`object.__new__`, custom/inherited `__new__`, instance dışı dönüşte `__init__`
atlama ve otomatik static binding desteklenir. Method ve class-scope lambda
gövdeleri hareketli-GC uyumlu örtük
`__class__` hücresini yakalar. `super()` ve `super(type, receiver)` C3 MRO üzerinde
function/classmethod/staticmethod/property/custom descriptor bağlar; tek argümanlı
unbound `super(type)` bootstrap kapsamında değildir. Desteklenmeyen sınıf protokolleri
`UnsupportedFeature` ile reddedilir; sessizce yok sayılmaz. `object` temelinde
yalnızca varsayılan oluşturma/type-check vardır; `object.__init__` gibi açık
protokol metotları henüz sunulmaz. `__bases__/__class__` yeniden ataması ve
sınıfın adını değiştirme desteklenmez.
Callable instance için inherited `__call__` ve `len(instance)` için inherited
`__len__` sınıf MRO'sundan çözülür; instance üzerindeki aynı adlı alanlar implicit
protokol çağrısını değiştirmez. Function/staticmethod/classmethod bağlama geçici
BoundMethod ayırmaz. `__len__` sonucu i64/Py_ssize benzeri bootstrap sınırında
integer ve negatif olmama kontrolünden geçer; bool 0/1'e dönüştürülür. Özel
descriptor nesnesi üzerinden `__call__`/`__len__` bağlama henüz yoktur.
Koşul, `while`, `and/or` ve `not` instance truthiness için önce `__bool__`, sonra
`__len__` arar; ikisi de yoksa instance doğrudur. Guest metot çalışırken normal VM
frame'i askıya alınır. `__bool__` yalnız bool döndürebilir; `__len__` aynı integer ve
negatiflik sözleşmesini kullanır. Kısa devrede özgün operand korunur.
Property data-descriptor önceliği ile normal attribute, `getattr` ve `setattr`
yollarında çalışır. Guest exception handler'ları property/getattr hata yollarını
yakalayabilir; property `getter` yardımcısı henüz yoktur.
Bootstrap sınıf doğrulaması `__init__/__new__/__call__/__len__/__bool__/__get__/__set__/__delete__/__set_name__/__module__/__qualname__/__doc__/__name__`
dışındaki `__...__` üyeleri reddeder; özel metadata adları da bu geçici
kısıta dahildir.
`range` başlangıç/bitiş/adım ve `fastmath.add` i64 ile sınırlıdır; genel Tonic
integer aritmetiği keyfî hassasiyetlidir. Repr/traceback metni CPython ile birebir
sözleşme değildir. Array/buffer ve C ABI için henüz public stable sözleşme yoktur.

**Kaynak sınırları:** kaynak başına 1 MiB, mantıksal satır başına 256 token,
64 indentation seviyesi ve kaynak başına 128 compound/conditional token;
16-bit bytecode/register/symbol indeksleri. Bunlar bootstrap kaynak korumalarıdır.
Varsayılan VM call depth 1024 ve register bütçesi 1,048,576'dır. Fuel bytecode
adımlarını sayar; bir native işlem için duvar saati veya bellek sınırı değildir.
Expanded çağrılar için argüman bütçesi 65.535'tir.

**Tekrar yürütme:** `Vm::run` her seferinde yeni module globals açar. Persistent
primitive/container handle'lar korunur; eski çalıştırmadan kalmış fonksiyon
çağrısı `RuntimeError` verir. Module/code sahipliği tasarımı tamamlanana kadar
callable'ları farklı run'lar arasında saklamayın. Henüz callback/reentry yoktur.
Sınıf/metot kapsamı ve GC sözleşmesi: [ADR 0003](docs/adr/0003-classes-shapes.md).
Decorator ve method descriptor sözleşmesi: [ADR 0004](docs/adr/0004-decorators-method-descriptors.md).
Property continuation sözleşmesi: [ADR 0005](docs/adr/0005-property-continuations.md).
Örtük class cell ve `super` sözleşmesi: [ADR 0010](docs/adr/0010-class-cell-super.md).
Constructor/`__new__` sözleşmesi: [ADR 0011](docs/adr/0011-new-constructor.md).
Callable/length protokol sözleşmesi: [ADR 0012](docs/adr/0012-call-len-protocols.md).
Truthiness protokol sözleşmesi: [ADR 0013](docs/adr/0013-truthiness-protocol.md).
Cranelift leaf JIT/deopt sözleşmesi: [ADR 0014](docs/adr/0014-cranelift-leaf-jit.md).
Resumable JIT call sözleşmesi: [ADR 0018](docs/adr/0018-resumable-jit-calls.md).
JIT backedge poll sözleşmesi: [ADR 0019](docs/adr/0019-jit-backedge-poll.md).
Loop OSR sözleşmesi: [ADR 0020](docs/adr/0020-loop-osr.md).
JIT loop binary fallback sözleşmesi: [ADR 0021](docs/adr/0021-jit-loop-binary-fallback.md).
JIT direct global load sözleşmesi: [ADR 0022](docs/adr/0022-jit-direct-global-load.md).
Polymorphic inline cache sözleşmesi: [ADR 0023](docs/adr/0023-two-entry-pic.md).
Class dependency invalidation sözleşmesi: [ADR 0024](docs/adr/0024-class-dependency-invalidation.md).
Generational GC sözleşmesi: [ADR 0025](docs/adr/0025-generational-gc.md).
JIT exact-callee leaf inlining sözleşmesi: [ADR 0026](docs/adr/0026-jit-direct-leaf-call.md).
Default/keyword binding planı: [ADR 0027](docs/adr/0027-jit-direct-call-binding.md).
Plain bound-method fusion sözleşmesi: [ADR 0028](docs/adr/0028-jit-bound-method-fusion.md).
Staticmethod fusion sözleşmesi: [ADR 0029](docs/adr/0029-jit-staticmethod-fusion.md).
Classmethod ve class-level method fusion sözleşmesi: [ADR 0030](docs/adr/0030-jit-classmethod-fusion.md).
Custom descriptor JIT resume sözleşmesi: [ADR 0031](docs/adr/0031-jit-custom-descriptor-resume.md).
Expanded-call JIT segment sözleşmesi: [ADR 0032](docs/adr/0032-jit-expanded-call-segment.md).
Gözlenmeyen boş variadic direct-call sözleşmesi: [ADR 0033](docs/adr/0033-jit-unobserved-variadics.md).
Method dependency cache sözleşmesi: [ADR 0034](docs/adr/0034-jit-method-entry-cache.md).
Positional sequence expansion sözleşmesi: [ADR 0035](docs/adr/0035-jit-positional-sequence-expansion.md).
Mapping expansion sözleşmesi: [ADR 0036](docs/adr/0036-jit-mapping-expansion.md).
Materialized variadic sözleşmesi: [ADR 0037](docs/adr/0037-jit-materialized-variadics.md).
Native float direct leaf sözleşmesi: [ADR 0038](docs/adr/0038-jit-native-float-leaf.md).
Unboxed loop deopt-map sözleşmesi: [ADR 0039](docs/adr/0039-jit-unboxed-loop-deopt-map.md).
Adaptive integer quickening sözleşmesi: [ADR 0015](docs/adr/0015-adaptive-integer-quickening.md).
Monomorphic function-call cache sözleşmesi: [ADR 0016](docs/adr/0016-monomorphic-call-cache.md).
Instance attribute cache sözleşmesi: [ADR 0017](docs/adr/0017-instance-attribute-cache.md).

Kuralların analizi: [ANALYSIS.md](docs/ANALYSIS.md).
Kararlar/riskler: [ADR 0001](docs/adr/0001-bootstrap.md).
Native sahiplik sözleşmesi: [NATIVE_API.md](docs/NATIVE_API.md).
Sonraki aşamalar: [ROADMAP.md](docs/ROADMAP.md).
Orijinal `AGENTS.md` ve `TONIC_INTEROP_RUNTIME.md` değiştirilmemiştir.

## Katkıda bulunma

Proje aktif geliştirme aşamasındayken katkılar kabul edilir. Pull request
açmadan önce [CONTRIBUTING.md](CONTRIBUTING.md) dosyasını okuyun. Sıcak yolu,
nesne temsilini, GC invariant'ını, bytecode sözleşmesini veya JIT varsayımını
değiştiren çalışmalar ölçüm ve gerekiyorsa ADR içermelidir.

Güvenlik bildirimleri için [SECURITY.md](SECURITY.md) politikasını izleyin.

## Lisans

Henüz bir açık kaynak lisansı seçilmemiştir. Telif hakları proje sahibinde
kalır. İlk herkese açık sürümden önce lisans eklenecektir; o zamana kadar depo,
inceleme ve proje sahibinin açıkça kabul ettiği koşullarla katkı için sunulur.
