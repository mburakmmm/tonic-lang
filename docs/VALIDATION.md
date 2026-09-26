# Yerel doğrulama

26 Eylül 2026, macOS ARM64, Rust stable 1.86.0, Python 3.14.6.

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
- `cargo test --workspace --locked --offline`
- `cargo test --release --workspace --locked --offline`
- `python3 tests/differential/run.py`
- `python3 tests/differential/run.py target/release/tonic`
- `TONIC_GC_EVERY=1 python3 tests/differential/run.py`
- `TONIC_GC_EVERY=1 python3 tests/differential/run.py target/release/tonic`
- `TONIC_JIT=1 python3 tests/differential/run.py`
- `TONIC_JIT=1 TONIC_GC_EVERY=1 python3 tests/differential/run.py`
- `TONIC_JIT=1 python3 tests/differential/run.py target/release/tonic`
- `TONIC_JIT=1 TONIC_GC_EVERY=1 python3 tests/differential/run.py target/release/tonic`

GitHub Actions ayrıca Linux x86-64 ve macOS AArch64 üzerinde debug/release JIT
crate testlerini ve normal/stress-GC differential corpus'unu çalıştırır. Ayrı
iki-mimarili gece derleyicisi işi parser ile public JIT girişini varsayılan
AddressSanitizer altında 10.000'er coverage-guided koşuyla denetler; sanitizer
kapısı yalnız bu işler yeşil olduktan sonra tamamlanmış sayılır.
- `cargo bench -p tonic-runtime --bench interpreter --locked --offline`
- `cargo bench -p tonic-runtime --bench jit --locked --offline`
- `cargo bench -p tonic-runtime --bench jit_direct_call --locked --offline`
- `cargo bench -p tonic-runtime --bench adaptive_pic --locked --offline`
- `cargo bench -p tonic-runtime --bench generational_gc --locked --offline`
- `cargo bench -p tonic-runtime --bench native_c_abi --locked --offline`
- `cargo bench -p tonic-runtime --bench buffer --locked --offline`
- `cargo bench -p tonic-runtime --bench callback --locked --offline`
- `cargo bench -p tonic-runtime --bench foreign_lifecycle --locked --offline`
- `cargo bench -p tonic-cpython --bench bridge --locked --offline`
- `cargo bench -p tonic-cpython --bench cross_runtime_gc --locked --offline`
- `cc -std=c11 -Wall -Wextra -Werror -Iinclude -fsyntax-only tests/c_header_smoke.c`
- `python3 benches/capture.py --output docs/benchmarks/stage2`
- `python3 benches/capture.py --output docs/benchmarks/stage3`
- `python3 benches/capture.py --output docs/benchmarks/stage6`
- `python3 benches/capture.py --output docs/benchmarks/stage7`
- `cargo bench -p tonic-runtime --bench python_compare --locked --offline --no-run`
- `python3 benches/compare_python.py ... --output docs/benchmarks/python-comparison-stage8-runN`
- `python3 benches/aggregate_comparison.py`

Test dağılımı: CLI 9, compiler/parser 16, core verifier 10, Cranelift JIT 16, runtime unit 22,
direct bytecode VM 4, buffer 3, C ABI integration 16, foreign wrapper 6, lifecycle/callback 5,
call binder/cache 12, closure 8, dict 8, GC integration 9, class integration 49,
native handles 5, language/runtime 85, CPython bridge 15 ve HPy manifest 5; toplam 303 test.

Differential corpus: 304 stdout vakası ve 149 exception türü vakası. Seed 42.
Generator ilk diliminden sonra debug/release × interpreter/JIT ×
default/`gc_every=1` matrisinin sekiz koşusu da Python 3.14.6 oracle'ıyla geçmiştir.
Aritmetik sign/overflow/rounding sınırları, fibonacci/factorial, loop, scope,
short-circuit, büyük integer/float karşılaştırması, bigint true division,
subnormal ties-to-even, container alias/cycle, nested closure/cell mutation,
global/nonlocal, defaults/variadic/keyword bağlama, dict numeric keys ve hatalı
çağrılarda değerlendirme sırası kapsanır. Exception vakalarında hata öncesi
stdout da karşılaştırılır. Aynı corpus debug/release ve varsayılan/stress GC
modlarında çalıştırılır. Stress modu her allocation sonrasındaki ilk instruction
sınırında collection yapar; native scope ortasında collection yapmaz.
Class corpus'u constructor/defaults/variadic bound calls, class namespace ile
closure ayrımı, private/qualified names, class docstrings, C3 diamond/invalid
MRO, base mutation, attribute builtin'leri ve class predicates içerir.
Instance ve metaclass `__getattribute__`/`__setattr__`/`__delattr__`, canonical
`object`/`type` delegasyonu, metaclass data/non-data descriptor sırası,
`AttributeError` sonrası `__getattr__`, `getattr` default, `hasattr`, `delattr`,
hook rebinding ve JIT/cache bypass engeli stress GC altında kapsanır.
Kaynak modül testleri `.tonic`/`.py` çözümleme, package init, dotted/from import,
circular partial state, tek seferlik cache, izole/versioned global, başarısız import
rollback/retry, doğru imported-file tanısı ve stress GC köklerini kapsar. Import
edilen hot fonksiyonun Cranelift'e yükseldiği ve `module.attr` değişiminden sonra
güncel global slotu okuduğu sayaçlarla ayrıca doğrulanır.
Generator corpus'u tembel gövde yürütmesini, `yield` resume değerini, `send`,
modern/legacy `throw`, `close`, delege `return` değerli ve tam forwarding yapan
`yield from`, builtin/custom iterable delegasyonu, `StopIteration.value` ve kaçan
`StopIteration` için PEP 479 `RuntimeError` dönüşümünü kapsar.
Rust testleri bunlara ek olarak closure/cell ve aktif exception state'inin
normal/stress GC altında hareketini; list/tuple/dict/for/unpack/`*args`
tüketicilerini ve generator kodunun JIT dışı kalmasını doğrular.
Decorator expression/default/base değerlendirme sırası, ters uygulama sırası,
function/class decorator sonuçları, inherited staticmethod/classmethod binding
descriptor hata yolları ve list/tuple/Unicode string slice semantiği de CPython
oracle'ıyla karşılaştırılır. Ayrıca 7.425 üretilmiş slice kombinasyonu yerel olarak
CPython ile karşılaştırılmıştır.
Custom descriptor corpus'u data/non-data önceliğini, class erişimindeki `None` ve
inherited owner argümanlarını, `getattr/setattr` yollarını ve çağrılamayan hook
hatalarını kapsar. Bin erişim ile tek erişimin guest allocation sayısının aynı
olduğu ayrıca denetlenir; geçici bound-method nesnesi oluşturulmaz.
`__set_name__` tanım sırası, class decorator'dan önce çalışma, inherited descriptor'ın
yeniden çağrılmaması, sonradan rebinding ve callback hata yayılımı da default/stress
GC altında karşılaştırılır. Callback içindeki allocation'lar bekleyen callback,
owner, callable, receiver ve name köklerinin hareket sonrası geçerliliğini sınar.
Attribute deletion corpus'u custom `__delete__`, property decorator/üç argümanlı
constructor deleter'ı, instance shape/dict ve class attribute silmeyi, soldan sağa
çoklu hedef değerlendirmesini ve eksik hook/attribute hatalarını kapsar.
`super` corpus'u örtük `__class__` closure'ını, diamond C3 cooperative çağrıları,
classmethod/property/custom descriptor binding'i, iç fonksiyon ve lambda capture'ını,
iki argümanlı explicit biçimi ve geçersiz frame/receiver hata yollarını kapsar.
Constructor corpus'u `object.__new__`, custom/inherited allocator, otomatik static
binding, instance dışı dönüşte init atlama, init argümanlarının continuation boyunca
GC köklenmesi ve geçersiz allocator hata yollarını kapsar.
Callable/length corpus'u inherited `__call__` ve `__len__`, function/staticmethod/
classmethod binding, instance alanını yok sayan implicit lookup, bool/negatif/non-int
uzunluk sonuçları, bounded callable zinciri ve tekrarlı protokol çağrısının allocation
sayısını kapsar.
Truthiness corpus'u `__bool__` önceliği ve kesin bool dönüşünü, `__len__` fallback'ini,
inherited/static/classmethod bağlamayı, class rebinding/deletion'ı, varsayılan doğru
instance'ı, `if/while/not/and/or` davranışını ve kısa devrede özgün operandın
korunmasını kapsar. Bir ve bin truthiness çağrısının heap allocation sayısı aynıdır.
Lambda positional-only/keyword-only/variadic binding, default değerlendirmesi,
late-bound closure, recursion ve class-scope ayrımı da corpus içindedir.
Property getter/setter, data-descriptor önceliği, setter dönüşünün yok sayılması,
salt-okunur ve getter'sız hata yolları normal/stress GC altında doğrulanır.
Exception corpus'u typed/tuple/bare handler'ları, `else`, frame'ler arası unwind,
JIT helper hatasının caller handler'ına aktarılmasını, nested active-context
restoration'ı, bare reraise'ı, `except as` bağının normal/hata/return/break/continue
çıkışlarında temizlenmesini ve invalid handler tipini kapsar. Custom iterator
corpus'u suspending `__iter__`/`__next__`, iç çağrıdan kaçan `StopIteration`,
iterator içinde yakalanan `StopIteration`, normal hata yayılımı ve geçersiz
iterator dönüşlerini CPython ile karşılaştırır. Aynı suspending protokol
`list`/`tuple` constructor'ları, exact unpack ve tek/çoklu `*args` genişletmesinde
de sınanır; unpack uzunluk tanıları ile `StopIteration` dışındaki hatalar CPython
çıktısıyla eşleşir. `dict` constructor corpus'u hem dış iterable hem çift
iterable'ında suspending protokolü, self/farklı iterator normalizasyonunu,
source-before-keyword sırasını, çift indeksli uzunluk tanısını ve pair hatasının
yayılımını aynı sekizli matris altında doğrular.
`finally` corpus'u normal, handled/unhandled exception, `return`, `break`,
`continue`, nested active exception, return/exception override ve finalizer
içinden yükselen yeni hata yollarında exactly-once çalışma sırasını kapsar.
`with` corpus'u capture edilmiş `__exit__` kimliğini, normal ve exception
argümanlarını, nested ters çıkış sırasını, target-assignment hatasını, suppression
truthiness'ini, metaclass manager'ı, cross-frame bare reraise'ı, `return`/`break`/
`continue` çıkışlarını ve exit hatasının önceki exception'ı değiştirmesini kapsar.
Exception chaining corpus'u explicit `raise from`, örtük `__context__`, `from None`
suppression, custom exception `__init__`/attribute davranışı, managed
`__traceback__` erişimi, invalid cause tanısı ve `__exit__` traceback argümanını
CPython ile karşılaştırır.
Shape tests metadata sınırlarının kullanıcı attribute kaybına yol açmadığını,
GC tests class/instance/bound-method/cell döngülerini ve namespace/init roots'u
kontrol eder. Type ID tükenmesi wrap yapmaz; versiyon ve MRO hareket sonrası korunur.
Bu sayılar Python sürümünün tamamına conformance anlamına gelmez.

Verifier testinde 10.000 deterministic decoder mutasyonu; VM testinde 5.000
mutated program adayı (verify edilenler 30-instruction fuel ile yürütülür).
Bunlar coverage-guided fuzzing veya formel doğrulama değildir.

Coverage-guided katmanda `fuzz/parser` ve `fuzz/jit_code_object` libFuzzer
hedefleri vardır. 15 Eylül 2026 yerel koşusunda her hedef 10.000 mutation'ı
crash/timeout olmadan tamamladı. Parser koşusu 440 corpus girdisi, 2.022 edge ve
3.946 feature; JIT koşusu 60 corpus girdisi, 187 edge ve 193 feature buldu.
macOS 26.6 yerel nightly AddressSanitizer, uygulama `main`inden önce
`AsanInitFromRtl` içindeki recursive malloc kilidinde kaldığı için bu ilk yerel
koşular coverage instrumentation açık, `--sanitizer none` ile yapılmıştı ve ASan
sonucu sayılmadı. 22 Eylül 2026 tarihli GitHub Actions run 35772128579 bu açığı
kapattı: Linux x86-64 ve macOS AArch64 işlerinin ikisi de parser ile public JIT
girişi için 10.000'er gerçek AddressSanitizer koşusunu geçti. Aynı run her iki
mimaride debug/release JIT testlerini ve normal/stress-GC differential corpus'unu
da başarıyla tamamladı.

Handle testleri stale reuse, cross-runtime, local Drop, explicit persistent
release/double release ve borrowed string lifetime kontrol eder. Collector
testleri gerçek heap compaction, cycle collection, generation tükenmesi, release
sonrası reclamation, native exception cleanup, pending expanded args ve closure
roots'u doğrular. Bağımsız erişilebilirlik modeline karşı 200 round/3.200
allocation'lık deterministik graph mutation testi vardır. Nursery testi ilk
minor'da survivor terfisini ve sonraki major'da old-space reclamation'ı doğrular.
List append/set, cell, dict key/value, module, class namespace ve instance/class
attribute mutation'ları old→young remembered-set invariantıyla doğrudan sınanır.
VM testi her 32. otomatik collection'ın major olduğunu ve aradaki collection'ların
minor kaldığını doğrular.
C ABI testleri function-table header düzeni, kesin ABI/struct-size/capability
pazarlığı, bilinmeyen integer capability/status değerlerinin tanımlı davranışı,
başarılı çağrı başında exception temizliği, explicit guest exception yayılımı,
stale local handle reddi ve `C-unwind` trampoline'da panic containment kapsar.
Panic sonrasında aynı VM yeni program çalıştırır; native function ve extension
init kapsamlarının bütün local handle'ları temizlenir. Manuel C11 smoke dosyası public header'ı
`-Werror` ile derler. ABI v1, native kodu güvenilir kabul eder; keyfi geçerli
olmayan non-null pointer'ı sandbox veya memory-safe yapma iddiası yoktur.
Buffer testleri f64 dtype, 2D shape/byte-stride, read-only/writable bayrakları,
ayrı buffer-owner handle ve double-release reddini kapsar. GC sahibi heap girişini
taşıdıktan sonra data/shape/stride tahsis adreslerinin aynı kaldığı doğrulanır.
Writable C export backing storage'u yerinde değiştirir; read-only writable isteği
owner sızdırmadan `BufferError` olur. `fastmath.array`→`fastmath.sum` stress GC
altında bir açık copy ve bir zero-copy export sayar.
Lifecycle testleri VM'nin yaratıldığı thread'e implicit attach olmasını, explicit
detach sonrası başka OS thread'ine taşınıp yeniden attach edilmesini ve attached
runtime'ın yanlış thread'de `ThreadError` vermesini kapsar. Retained module IR ile
persistent closure callback'i kaynak `VerifiedProgram` düşürüldükten ve moving GC
çalıştıktan sonra çağrılır. Callback exception frame/register/argument scratch'ini
temizler ve ikinci çağrı aynı runtime'da başarılı olur. C native→Tonic→C→Tonic
nested reentry her-allocation GC altında precise roots ve iki callback frame'iyle
çalışır. Staged shutdown persistent handle'ları geçersiz kılar, bütün heap'i
toplar ve Dead aşamasında run/context/attach işlemlerini reddeder.
Foreign wrapper testleri vtable size/version/ownership kurallarını, özel
foreign-reference handle'larının global root olmamasını ve Tonic list↔wrapper
döngüsünün root kalkınca toplanmasını kapsar. Trace her collection öncesinde
yenilenir; shutdown ve tekrar major collection payload destructor'ını yalnız bir
kez çalıştırır. Trace panic guest `ForeignError` olur; destructor panic queue'da
tutulur, sayaçlanır ve yeniden denenmez. Owned wrapper benchmarkı 100.000 create,
C crossing ve deferred destroy için ayrı oluşturma/collection sürelerini kaydeder.
CPython bridge testleri gerçek libpython üzerinde None/bool/PyLong/BigInt/PyFloat/
PyUnicode ile list/tuple/dict çift yönlü dönüşümünü; alias/list-dict cycle
materialization'ını; named unary ve positional/keyword genel çağrıları çalıştırır.
Arbitrary `ForeignPyObject`, adapter guard'lı payload borrow ve deferred `Py_DecRef`
yolları korunur. `PyTonicProxy` persistent handle, ref-counted runtime owner ve
execution kimliği taşıyan gerçek CPython heap type'tır; positional/keyword callback,
attribute set/get, property continuation, repr, logical-handle roundtrip ve stress GC
doğrulanır. Proxy foreign wrapper GC ile ölünce release doğru VM kuyruğunda
boşaltılır. Ayrı cycle testi proxy→persistent callable→closure/list→proxy halkasını
idempotent `close_proxy` ile kırar ve kapalı proxy çağrısının kesin diagnostic
üretmesini doğrular. On beş test paralel thread koşusunda `PyEval_SaveThread` başlangıç
bırakması ve çağrı başına `PyGILState` guard'ıyla geçer. Python traceback metni
`PythonError`a çevrilir; Tonic callback hata sınıfı CPython'a aktarılır ve iki tarafın
indicator/state'i temizlenir.
Non-owning proxy cache testi aynı canlı Tonic değerinin aynı CPython proxy adresini
yeniden kullandığını ve son `Py_DecRef` sırasında cache girdisinin silindiğini
doğrular. Otomatik cycle testi yalnız wrapper tarafından tutulan proxy'nin persistent
kökünü non-rooting foreign trace kenarına düşürür ve explicit close olmadan
proxy→closure→list→wrapper halkasını toplar. Ayrı dış-root testi proxy `sys`
modülünde tutulduğu sürece hedefi yaşatır, attribute silindikten sonra deferred kökü
boşaltır. Genel graph testi `SimpleNamespace -> proxy` transitif kenarını public
`Py_tp_traverse` slotuyla bulur; yalnız iki runtime'ın tuttuğu halka explicit close
olmadan toplanır. Aynı proxy için bir dış CPython referansı bulunduğunda borrowed
trace token'ı yeniden persistent köke yükseltilir. Traversal/graph sınırları aşılırsa
veya yabancı runtime proxy'si görülürse güçlü kök conservative biçimde korunur.
Foreign finalizer payload destructor'ı traced Tonic kenarları hâlâ kökken çalışır;
owned trace handle'ları destructor döndükten sonra bırakılır.
Leaf JIT aynı differential corpus'ta debug/release ve default/stress GC ile
çalıştırılır. Exact integer guard failure, immediate taşma, floor sıfıra bölme
deopt'u, unsupported opcode fallback'i, native dönüş, compile süresi ve code-size
sayaçları ayrıca test edilir. True division runtime helper'ı float allocation,
exact error PC/türü ve explicit materialized register roots kullanır. Ardışık iki
helper arasında yalnız JIT register'ında yaşayan float, helper-triggered stress
collection'dan sağ çıkar. Bu boxed-register ABI'sinin allocation safepoint testidir;
unboxed machine deopt map kapsamı ayrı JIT testlerinde doğrulanır. Dil düzeyindeki
kullanıcı finalizer semantiği henüz uygulanmamıştır; CPython bridge'in iki-collector
cycle/finalizer sırası yukarıdaki on beş integration testiyle sınırlanır.
Recursive JIT testi `CALL` side exit'i, explicit child frame, arbitrary-PC native
resume ve direct bound `LOAD_GLOBAL` yolunu birlikte çalıştırır. Global rebinding
sonrasında yeni hedef çağrılır; eksik globalin `NameError` konumu exact helper
PC'sinden korunur. `fib(20)` son ara benchmarkta adaptive 2,428 ms, resumable JIT
1,909 ms ölçmüştür; 21.876 bound-global helper çağrısı sıfıra inmiştir.
Native backedge poll testi 2.500 iterasyonda iki callback görür; yalnız JIT
register'ında duran heap-tag'li raw değer her callback'in kesin root slice'ında
bulunur. Poll error türü exact backedge PC'siyle runtime'a döner. Bir milyonluk
numeric loop 976 periyodik poll ile adaptive tier'dan 24,52× hızlı kalmıştır.
OSR testi sıcak loop'u 64. backedge gözleminde exact hedef PC'den native koda
alır; `sum_to(10)` cold kontrolü compile üretmez. Bir milyon iterasyonda yalnız
835 instruction interpreter'da dispatch edilir ve kalan loop native çalışır.
Float loop testi profilli float parametrelerini arbitrary OSR PC'sinde bir kez
unbox eder ve loop-carried değerleri native stack slotlarında tutar. Her PC'nin
deopt map'i canlı F64 register'larını listeler. Test runtime'ının poll isteği
1.024'üncü backedge'de `value` ile `step` değerlerini yeniden boxed register'lara
yazar ve exact loop PC=2'ye deopt eder. 100.000 iterasyonluk ölçümde boxed JIT
3,115 ms/100.003 allocation iken yeni yol 1,969 ms/67 allocation üretir; güncel
adaptive 10,766 ms'ye göre 5,47× hızlıdır. Integer loop kontrolü şimdilik generic
helper kullandığı için helper sayısı 199.975'tir; sonraki optimization bu ABI
geçişlerini azaltabilir.
Exact-callee leaf inlining testi 100.000 `add` çağrısını caller loop içinde
çalıştırır. A/B harness'inde adaptive 13,368 ms, eski side-exit/resume JIT 8,019 ms,
direct inlining 0,760 ms ölçülmüştür. 99.937 native leaf çağrısı ve logical call
sayacı korunurken side exit/resume 99.937'den sıfıra iner. Callee rebinding ve
float operand guard miss'i özgün `CALL` PC'sinde generic semantiğe döner; exact
logical handle guard'ı moving stress-GC altında da doğrulanır.
Positional-only/keyword-only/default binding testi `add(total,bias=0)` çağrısında
`b=1` değerini exact function nesnesinden alır ve üç target parametre slotunu
önceden doğrulanmış plana göre bağlar. Önceki sürümde 99.937 side exit ile 12,405 ms
olan direct-call modu 1,120 ms ve sıfır side exit'e düşmüştür; aynı ölçümde inlining
kapalı JIT 10,061 ms'dir. Unit test keyword register offset'i ile default raw
değerinin doğru slotlara gittiğini, runtime testi 5.000 çağrıda stress GC, logical
call sayacı ve sıfır deopt'u doğrular. Variadic/expanded ve method çağrıları bu
kanıtın kapsamına dahil değildir.
Plain bound-method fusion testi 100.000 `counter.add(total,b=1)` çağrısını ölçer.
Önceki backend `ATTR` nedeniyle fonksiyonu derlemiyor ve 23,027 ms adaptive
medyan üretiyordu. Fusion sonrasında 2,024 ms, 1.604 byte code, bir method site,
99.937 direct call ve sıfır side exit ölçülmüştür; yaklaşık hızlanma 11,38×'dir.
Lowering unit testi yanlış function lookup sonucunun caller `ATTR` PC'sine deopt
ettiğini doğrular. Runtime testleri implicit receiver kimliğini, class method
rebinding'i, instance shadowing'i, stress GC'yi ve 5.000 çağrıda generic yola göre
en az 4.800 daha az heap allocation'ı kapsar. Staticmethod/classmethod kapsamı
aşağıdaki bağımsız guard ve stress testleriyle genişletilmiştir.
Staticmethod fusion testi instance üzerinden `math.add(total,b=1)` çağrısında
receiver eklenmediğini stress GC altında doğrular. Wrapper daha sonra aynı raw
function ile değiştirildiğinde exact function tek başına eşleşse de binding-kind
guard'ı `ATTR` PC'sine deopt eder ve generic bound-method binder'ın `TypeError`
sonucu korunur. 100.000 çağrılı ölçümde adaptive 20,044 ms, fusion 2,379 ms,
1.600 byte code, 99.937 direct call ve sıfır side exit üretmiştir; yaklaşık
hızlanma 8,43×'dir.
Classmethod fusion testi inherited `Sub` receiver'ını instance ve class erişiminde
dinamik olarak bağlar. Native helper kesin JIT root cache'ine exact function ile
gerçek receiver'ı birlikte yazar; helper ve backedge poll'lar guest register'larla
birlikte bu gizli kuyruğu da tarar. Aynı function classmethod wrapper'dan plain metoda
dönüşürse binding-kind guard özgün `ATTR` PC'sine deopt eder. 100.000 çağrıda
adaptive 36,416 ms, fusion 3,828 ms, 1.716 byte code, 99.937 direct call ve sıfır
side exit ölçülmüştür; aynı koşuda yaklaşık hızlanma 9,51×'dir. Class-level plain
function, staticmethod ve inherited classmethod erişimi ayrıca stress GC altında
iki fusion site ve 9.800'den fazla direct call ile sınanır. Custom descriptor
getter'ın kendisi generic VM semantiğinde kalır.
Custom descriptor returned-call testi `__get__` yürütmesini exact `ATTR` PC'sinde
interpreter'a yan çıkarır, getter tamamlanınca caller native kodunu sürdürür ve
dönen exact leaf'i inline eder. 100.000 çağrıda adaptive 51,424 ms, JIT 41,565 ms,
1.408 byte code, 99.937 side exit/resume ve 99.937 direct call ölçülmüştür; süre
yaklaşık %19,2 azalır. Direct-call profili olmayan generic `ATTR` code object'i
derlenmez. Getter'ın döndürdüğü global function değiştiğinde `CALL` guard deopt'u
yeni sonucu korur; ayrı test her-allocation stress GC altında frame/callable
köklerini doğrular.
Generic expanded-call yolu `BEGIN_ARGS` ile dış `CALL_EXPANDED` arasını tek VM
segmentinde yürütür ve matching argument-stack derinliğinde native caller'a döner.
Nested `*`/`**` builder integration testi erken resume olmadığını ve pending roots'u
sınar. Düz positional sequence vakası aşağıdaki guarded direct yola yükseltilmiştir.
Gözlenmeyen boş variadic testi `add(a,b,*rest,**kw)` hedefinde boş tuple/dict
materialization'ını kaldırır. 100.000 çağrıda side-exit JIT 9,510 ms, direct yol
0,821 ms, 1.780 byte code ve 99.937 direct call üretmiştir; 11,58× hızlanır.
`return rest` negatif kontrolü operand taramasının direct site'ı reddettiğini ve
generic `(2, 3)` sonucunu koruduğunu doğrular.
Method dependency cache lookup'u ilk gerçek `ATTR` yürütmesinde lazy yapar; exact
owner ve function guard'ları native invocation boyunca kullanılır. 100.000 çağrıda
bound/static/classmethod direct süreleri 0,926/0,868/0,853 ms, helper lookup sayısı
site başına birdir. Alternating owner testi değişimde `ATTR` deopt'unu; mevcut
class rebinding testleri yeni native girişte lookup yenilenmesini doğrular.
Positional sequence expansion testi çok öğeli `*list` çağrısını stress GC altında
5.000 kez sıfır side exit ile direct leaf'e taşır. Aynı uzunluktaki item mutation
güncel değeri verir; uzunluk değişimi `BEGIN_ARGS` deopt'undan sonra generic
binder'ın `TypeError` sonucunu korur. 100.000 çağrıda adaptive 21,524 ms, generic
segment JIT 18,747 ms ve guarded direct expansion 1,080 ms ölçülmüştür. Named/default
slot binding varyantı adaptive 29,728 ms, generic JIT 26,342 ms ve direct 1,236 ms'dir.
Exact-dict `**mapping` testi iki string key'i ayrı kesin JIT argument roots'a alır;
aynı key'in value mutation'ı yeni değeri deopt etmeden verir, key kümesi değişimi
ise `BEGIN_ARGS` deopt'undan sonra generic `TypeError` üretir. 100.000 çağrıda
adaptive 33,476 ms, generic segment JIT 30,284 ms ve guarded mapping 2,761 ms'dir;
99.937 direct call sıfır side exit ile tamamlanır. Debug/release testleri ve
interpreter/JIT × normal/stress differential matrisinin sekiz koşusu bu değişiklik
sonrasında yeniden geçirilmiştir.
Observed variadic integration testi fixed/extra positional, keyword-only, bilinen
ve bilinmeyen keyword bağlamasını birlikte çalıştırır. Target'ın döndürdüğü tuple ve
dict 5.000'er direct çağrı ve her-allocation stress GC altında korunur. 100.000
çağrılık A/B'de materialized `*args` direct yolu generic segmentten 3,94×,
adaptive tier'dan 5,33×; materialized `**kwargs` yolu sırasıyla 1,49× ve 1,77×
hızlıdır. Her iki direct koşu 99.937 logical call ve sıfır side exit üretir.
Native float uygulama öncesi baseline'da function içi 100.000 boxed `+=` JIT yolu
100.003 allocation/100.034 helper ile 3,115 ms; üç-op direct float leaf ise exact-int
guard'ını sekiz kez kaybedip 300.004 allocation ve 24,435 ms üretmiştir. Bu ikinci
sonuç adaptive 23,208 ms'den yavaştır ve unbox-once/box-on-return kabul bütçesidir.
Uygulama sonrasında aynı üç-op leaf 99.937 direct çağrıyı sıfır deopt/side exit ile
tamamlar; yalnız dönüşü box ettiği için allocation 100.130'a, medyan süre 2,988 ms'ye
iner. Bu güncel generic interpreter'a göre 7,89×, adaptive interpreter'a göre
7,54× hızlanmadır. Her-allocation GC testi sonuç root'unu; mixed float→int testi
atomik CALL deopt'unu; `inf`, `nan`, `-0.0` testi IEEE gözlenebilirliğini interpreter
eşitliğiyle doğrular.
Attribute interception tamamlandıktan sonra 274 Rust testi ile 290 stdout ve 116
exception differential vakası debug/release olarak geçirildi. Sekiz differential
koşunun tamamı interpreter/JIT × normal/stress-GC matrisinde Python 3.14.6 ile
eşleşti. Özel `__getattribute__` JIT testi direct-method profilinin hook'u
atlamadığını, class version testi ise sonradan hook ekleme/silmenin quickened slot
guard'ını düşürdüğünü doğrular.
Builtin alt sınıf ve operator protokol aşamasında toplam 278 Rust testi ile 293
stdout ve 123 exception differential vakasına ulaşıldı. Native subclass testi
`int`/`float`/`str`/`list`/`tuple`/`dict` backing storage'ını, instance alanlarını,
hash eşdeğerliğini, slice/iteration/mutation yollarını, custom iterable sırasında
continuation root'larını ve immediate-int JIT guard miss'inde exact-PC deopt'u
doğrular. Operator testi direct/reflected sıra, strict subclass önceliği,
`NotImplemented`, bütün in-place fallback'leri, power/bitwise/shift, altı temel
binary grup, rich comparison, `!=` truth terslemesi, unary/`abs`/invert ve
metaclass dispatch'ini her-allocation stress GC altında kapsar. Büyük integer
sonuçları kaynak sınırıyla kontrollü `MemoryError` üretir; bu opcode'lar native
JIT kapsamı dışında kaldığında doğrulanmış exact-PC interpreter fallback'i
kullanır. Debug/release × interpreter/JIT × normal/stress-GC
differential matrisinin sekiz koşusu Python 3.14.6 ile eşleşmiştir.
Canonical builtin constructor aşamasında toplam 280 Rust testi ile 295 stdout ve
130 exception differential vakasına ulaşıldı. `object.__init__`, native builtin
`__new__` descriptor'ları, list/dict reinitialization, custom native subclass
`__new__` ve `int`/`float` conversion protokolleri her-allocation stress GC
altında sınanır. Conversion continuation state'i class ve pending native finish
değerlerini precise root olarak taşır; yanlış sonuç tipleri `TypeError` üretir.
Debug/release × interpreter/JIT × normal/stress-GC matrisinin sekiz koşusu
Python 3.14.6 ile eşleşmiştir.
Index conversion aşamasında toplam 281 Rust testi ile 296 stdout ve 136 exception
differential vakasına ulaşıldı. `range`, scalar/slice sequence erişimi, list
set/delete, explicit int base ve length/truth yolları guest `__index__` metodunu
normal VM frame'inde çalıştırır. Continuation state range argümanlarını, slice
bileşenlerini, container owner/değerlerini, int string argümanını ve truth jump
operandını precise root olarak taşır. Dict anahtarları ve custom `__getitem__`
dönüşüm sınırının dışında kalır. Yanlış sonuç tipi, negatif length ve invalid
arbitrary-precision int base hata yolları CPython oracle'ıyla karşılaştırılır.
Run'lar arasında eski persistent callable yanlış code ID'ye bağlanamaz.

Hash ve container comparison aşamasında toplam 284 Rust testi ile 299 stdout ve
147 exception differential vakasına ulaşıldı. `hash`/`object.__hash__`, custom
suspending `__hash__`, implicit `__hash__ = None`, builtin/native-subclass hash,
tuple/slice/bound-method bileşimi ve logical identity yolları doğrulandı. Dict
kovaları yalnız hash ile eşitlik kararı vermez; custom collision anahtarları get/
set/delete, iterable constructor, dict copy, `**` merge ve dict equality sırasında
normal guest `__eq__`/truth frame'lerinden geçer. List/tuple/dict/slice nested
equality, list/tuple lexicographic order, cyclic comparison sınırı ve exact-int
JIT guard miss sonrası exact-PC continuation yolu kapsanır. Debug/release workspace,
clippy ve interpreter/JIT × normal/stress-GC differential matrisinin sekiz koşusu
Python 3.14.6 ile eşleşmiştir.

`.github/workflows/ci.yml` Linux/macOS için aynı kontrolleri tanımlar; remote
sonuçlar her push sonrasında ilgili GitHub Actions koşusundan ayrıca doğrulanır.
C ABI header/smoke ve guarded callback testleri vardır,
ancak sanitizer sonucu varmış gibi raporlanmaz; JIT differential yalnız yukarıdaki belgelenmiş kapsamı
kanıtlar.
