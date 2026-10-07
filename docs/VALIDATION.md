# Yerel doğrulama

7 Ekim 2026, macOS ARM64, Rust stable 1.86.0, Python 3.14.6.

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

Test dağılımı: CLI 10, compiler/parser 29, core verifier 20, Cranelift JIT 31, runtime unit 26,
direct bytecode VM 4, buffer 3, C ABI integration 16, foreign wrapper 6, lifecycle/callback 6,
call binder/cache 12, closure 8, dict 8, GC integration 9, class integration 55,
native handles 5, language/runtime 143, CPython bridge 15, HPy inventory/host unit 6,
HPy loader 6 ve HPy Universal entegrasyonu 8; toplam 426 test.

Annotation typed-JIT testleri exact `int` ile mevcut unboxed F64 data-flow yolunu
ilk çağrıda, varsayılan hotness/kârlılık eşiklerini beklemeden derler. Exact
function/code/execution, argüman ve dönüş guard'ları; yanlış tipte çağrının generic
sonucu; `__annotations__` content mutasyonu, dict replacement/delete; geçersiz
replacement `TypeError`'ı; class dependency version yenilemesi ve yanlış dönüşün
`RETURN` PC'sine advisory deopt'u kapsanır. Invalidation dizisi allocation-stress
GC altında da aynı çıktıyı ve sayaçları verir. Ayrı vakalar eager global alias
rebinding kararını ve user-class dependency değişiminin canlı native entry'yi
invalid etmesini doğrular.

Typed integer overlay testi verified loop bytecode'unda parametre/sabit/local,
branch merge ve loop fixed-point sonucunun redundant tag guard'larını gerçekten
azalttığını metadata sayacıyla doğrular. Doğru exact-int giriş native sonucu
verir; bool ile normal giriş ve eksik/corrupt register'larla arbitrary-PC resume
aynı giriş PC'sinde deopt eder. Public JIT API uyumsuz typed-signature arity'sini
Cranelift codegen öncesinde `InvalidBytecode` olarak reddeder.

Typed scalar bool testi ortak `Unknown/Int/Float/Bool` lattice'inde exact-bool
parametre, branch, `bool + int` ve karşılaştırma sonucunu izler. Native yol
`False=0`/`True=1` sayısal dönüşümünü Python ile aynı yapar; exact-bool olmayan
argüman first-call tier'ını atlayıp advisory annotation semantiğiyle generic
sonucu üretir. Ayrı public-JIT testi normal girişte yanlış bool tag'ını reddeder
ve metadata'daki bool guard-elision sayacını doğrular.

Typed `None` testleri `None` ve `type(None)` annotation'larını aynı immediate
fact'e çözer. Public JIT testi `Const None`/parameter propagation'ı, kanıtlı
dönüşü, doğru singleton girişini ve integer ile PC 0 deopt'unu doğrular. Runtime
testi iki imzanın first-call compilation'ını, yanlış argümanın advisory generic
sonucunu, annotation guard miss sayacını ve iki return-guard elimination'ını
allocation-stress GC altında kapsar.

Optional immediate-union testleri canonical `int | None` ve `bool | None`
planlarını iki-tag giriş guard'ına dönüştürür. Public JIT testi doğru union
üyelerini native çalıştırır, üye olmayan tag'i PC 0'da deopt eder, control-flow
birleşimindeki `int`/`None` dönüşlerini tek optional fact altında kanıtlar.
Runtime testi üç fonksiyonu ilk çağrıda derler; `None`, exact scalar ve yanlış
string argümanıyla advisory fallback davranışını allocation-stress GC altında
ve return-guard elimination sayaçlarıyla doğrular.

Optional branch-refinement testleri `x is None`, `None is x` ve `is not`
biçimlerinin true/false CFG kenarlarını ayırır. Non-None dalında `int` aritmetiği
typed guard-elision üretirken None dalı singleton fact'ini korur; iki dönüş de
`int | None` olarak kanıtlanır. Ayrı safety vakası, optional değerin önce başka
register'a kopyalanıp kaynak register'ın yeniden yazıldığı durumda stale alias'ın
daraltılmadığını; `None + 1` işleminin native yanlış sonuç yerine normal
`TypeError` verdiğini allocation-stress GC altında doğrular.

`int | bool` union testleri iki immediate tag'in aynı first-call girişinde kabul
edildiğini, `False=0`/`True=1` decode'uyla native toplamanın Python sonucunu
verdiğini ve int/bool CFG birleşiminin union return proof ürettiğini doğrular.
Runtime vakası int, iki bool ve union identity sonuçlarını allocation-stress GC
altında çalıştırır; float argüman annotation guard'ını kaçırıp generic `2.5`
sonucunu verir.

PEP 604 union testi `int | str`, tekrar eleme, `T | None`, sıra-bağımsız equality/
hash, `__args__`/`__origin__`, `types.UnionType`, `isinstance`/`issubclass`, hatalı
operand reddi ve custom metaclass `__or__` önceliğini interpreter/JIT-caller ×
her-allocation GC matrisinde doğrular. Builtin gözlenebilir çıktı aynı yerel Python
koşusuyla eşleşir. TypePlan unit testi `int | None`, sırası ters `str | int` ve
`list[int] | None` planlarını canonical schema v2 `Union` olarak, eşdeğer yazım
sıralarını da aynı hash ile doğrular.

Typed return proof aynı annotation-JIT vakalarında bütün erişilebilir `RETURN`
değerlerini result planına karşı doğrular ve kanıtlı int/float/bool dönüşlerde
host-side tekrar guard'ının atlandığını sayaçla gösterir. Yalan `-> int`
annotation'lı string dönüş vakası kanıt üretmez; mevcut return guard `RETURN`
PC'sinde deopt ederek advisory semantiği korur.

Guarded call-result testleri exact global annotated int leaf'i profil beklemeden
caller'ın first-call compilation unit'ine alır, sonucu caller lattice'inde
ilerletir ve direct-call/typed-result sayaçlarını doğrular. Callee annotation
mutation'ı caller plan dependency'sini invalid ederken sonuç değişmez. Ayrı yalan
`-> int`/`return True` vakası public callee-return proof kapısında özeti reddeder;
caller generic kalır ve Python `True + 1 == 2` davranışı korunur.

Immediate-union call-result testleri side-effect-free identity leaf'lerinden
`int | None` ve `int | bool` özetlerini caller'a taşır. İlk caller optional sonucu
`is None` ile ayırıp non-None dalında native toplar; ikincisi bool/int sonucu
doğrudan Python 0/1 numeric semantiğiyle toplar. Public JIT testi union result
fact'inin inlined CALL sonrasında caller aritmetiğine aktarıldığını doğrular.
Runtime testi iki direct site/dört direct yürütmeyi, stress-GC çıktılarını ve
callee return annotation mutasyonundan sonra caller invalidation + özetsiz
recompile davranışını sayaçlarla kapsar.

Annotated class-edge testleri exact global class üzerinden plain function ve
`staticmethod` çağrılarını profil beklemeden fused `ATTR`+`CALL` planına alır.
Callee annotation mutation caller entry'sini invalid eder; aynı class owner'da
attribute rebinding exact `ATTR` PC'sine deopt ederek yeni callable'ı generic
descriptor semantiğiyle çalıştırır. Allocation-stress GC vakası method cache
root'larını doğrular. Ayrı `-> int`/`return True` class function vakası body-proof
kapısında reddedilir ve typed result/direct-method sayaçları sıfır kalır.

Opaque-receiver testleri public JIT `Dynamic` parametresinin entry scalar guard'ı
ve return proof üretmediğini, buna karşılık kullanılmayan `self/cls` slotuyla
exact-int dönüşün kanıtlanabildiğini doğrular; `Dynamic` result public compile
API'sinde deterministik olarak reddedilir. Runtime vakaları exact global instance
methodunda shadowing sonrası ATTR deopt'unu ve global class `classmethod` çağrısında
annotation mutation invalidation'ını allocation-stress GC altında kapsar.

Annotated class-parameter testi exact kullanıcı sınıfı annotation'lı caller
parametresini saf `Move` zincirinden instance-method edge'ine bağlar. Doğru sınıf
ilk çağrıda typed result/direct-method yoluna girer; yanlış sınıf advisory giriş
guard'ından generic çalışır, instance method shadowing'i exact-ATTR deopt üretir
ve class method rebinding'i annotation planını invalid ederek güncel generic
sonucu korur. Test her-allocation GC altında bütün dört sonucu ve ilgili sayaçları
doğrular.

JIT rejection diagnostics CLI testi annotation first-call adayındaki destek dışı
`Pow` opkodunu generic semantiği bozmadan çalıştırır ve `--stats` çıktısında exact
function, `unsupported-bytecode` kategori kodu, bytecode PC, `Pow` opcode'u ve
deterministic nedeni doğrular. Code-budget runtime testi aynı public accessor'ın
`code-budget` kaydını ve limit açıklamasını doğrular.

Differential corpus: 332 stdout vakası ve 261 exception türü vakası. Seed 42.
Sequence iterator fallback protokol diliminden sonra debug/release × interpreter/JIT ×
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
tüketicilerini, generator kodunun JIT dışı kalmasını, ulaşılamayan askıdaki
generator'ların collector-dışı logical close'unu, delege kapanma sırasını,
unraisable hata yalıtımını ve shutdown finalization'ını doğrular. User
`__del__` testleri erişilemez nesne kuyruğunu, resurrection'ı, exactly-once
çağrıyı ve finalizer hatasının programdan yalıtılmasını interpreter/JIT-caller
ile doğrular. Ek test user-rebound `sys.unraisablehook` için exception type/value,
traceback, `err_msg` ve kaynak callable alanlarını; hook'un kendi hatasının
yalıtılmasını ve generator → user object kategori sırasını doğrular.
Coroutine corpus'u tembel `async def` çağrısını, nested exact-coroutine `await`
delegasyonunu, özel `__await__` iterator'larını, `coroutine_wrapper` protokolünü,
`send`/`throw`/`close`, geçersiz awaitable hata türlerini ve askıdaki coroutine'in
await edilen iterator ile içten dışa logical close'unu kapsar. Aynı programlar
interpreter/JIT-caller ve normal/stress GC yollarında çalıştırılır.
Async iteration corpus'u senkron ve coroutine `__anext__`, gerçekten askıya alan
özel awaitable, `StopAsyncIteration` sınırı, break/continue/else ile hatalı
`__aiter__`/`__anext__` sonuçlarını kapsar. Body içindeki açık
`StopAsyncIteration` exhaustion olarak tüketilmez.
Async context-manager corpus'u `__aenter__`/`__aexit__` await sözleşmesini,
nested manager çıkış sırasını, exception suppression'ı ve geçersiz/non-awaitable
protokol sonuçlarını CPython ile karşılaştırır. Rust stress testi bunlara ek olarak
gerçekten askıya alınan enter/exit, return/break/continue cleanup'ı, kısmi giriş
başarısızlığı, yakalanmış eski exit metodu, target atama hatası, bare reraise,
özel truthiness ve metaclass manager davranışını interpreter/JIT-caller altında
her allocation'da GC ile doğrular.
Async-generator corpus'u tembel oluşturmayı, `__aiter__`/`__anext__`, tek
kullanımlık `asend`/`athrow`/`aclose`, custom exception initializer'ı, iç
`await` askılarını, async-for tüketimini, awaitable `throw`/`close` forwarding'ini,
eşzamanlı sürücü reddini, `StopAsyncIteration` ve ignored `GeneratorExit`
sınırlarını kapsar. Ayrı finalization testi ulaşılamayan suspended async
generator'ın `finally` bloğunu collector dışında çalıştırır. Başarı ve syntax/
runtime hata vakaları CPython oracle'ıyla; tüm runtime yolları interpreter/JIT
caller ve normal/her-allocation GC modlarında doğrulanır.
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
İki argümanlı `iter(callable, sentinel)` corpus'u sıfır argümanlı guest callable'ı,
sentinel-sol rich equality ve truth zincirini, kalıcı exhaustion'ı, callable kaynaklı
`StopIteration` normalizasyonunu, equality hatasında iterator'ın kullanılabilir
kalmasını ve bütün iterator tüketicilerini normal/stress GC ile interpreter/JIT
caller yollarında doğrular. 5.000 öğelik tarama bounded reentry'nin Rust stack'ini
öğe sayısıyla büyütmediğini sınar.
`__iter__` yokluğundaki `__getitem__` fallback corpus'u instance/static/class/
metaclass binding'i, class rebinding'i, ardışık integer indeksleri, hata sonrası
aynı indeksi yeniden denemeyi, kalıcı exhaustion'ı ve bütün streaming tüketicileri
aynı sekizli matris altında doğrular. Tüketici continuation root'ları kısmi dict,
koleksiyon, toplam ve argument state'ini her-allocation GC altında korur.
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
HPy testleri vendored resmi 0.9 header'larından gerçek `.hpy0` C modülü derler;
binary audit modülün `libpython` bağlamadığını doğrular. Loader filename, dört init
sembolü, exact ABI ve process-lifetime mapping'i sınar. Host testi 263-slot context
düzenini; scoped local handle, stale/cross-runtime, exception ve builder cleanup'ı
denetler. H2 fixture `NOARGS`/`O` yanında `VARARGS`/`KEYWORDS`, custom attribute ve
item protokolleri, list/dict mutation, contains/length/repr ile tuple/dict,
vectorcall ve method-call yollarını interpreter ve JIT caller modlarında çalıştırır.
Yanlış keyword arity, item türü, non-callable ve unsupported signature yolları
deterministic guest/import hatası üretir ve aktif handle sayısı sıfıra döner.
H2 scalar fixture bool, signed/unsigned 32/64-bit, bigint, size/ssize, mask,
pointer, integer-to-double ve float yollarını interpreter/JIT caller ile
default/her-allocation GC matrisinde çalıştırır. Overflow ve yanlış scalar türü,
`HPyErr_SetObject`, built-in hierarchy/nested-tuple `ExceptionMatches` ve
`HPyErr_NoMemory` failure yolları aynı matris içinde sıfır leaked handle ile
doğrulanır. HPy 0.9 public yüzeyinde exception fetch/restore bulunmaz.
H3 fixture module definition'daki `HPyGlobal` listesini gerçek header ile yükler;
aynı Universal module iki VM'ye kaydedildiğinde global değerler birbirini görmez.
Global self-cycle store boyunca root kalır, clear sonrasında toplanır. `HPyField`
testi owner→value kenarını her-allocation moving GC altında load eder, clear eder
ve yalnız field/value→owner tarafından tutulan cycle'ın major collection'da
toplandığını doğrular. Heap unit testleri external field old→young write barrier'ı
ile owner öldüğünde field metadata sweep'ini doğrudan denetler.
H3 tracker fixture `Close` ile sahip olunan local handle'ların kapandığını,
`ForgetAll` sonrasında sahipliğin çağıranda kaldığını ve stale token, kapanmış
handle, negatif kapasite ile açık tracker dönüşlerinin interpreter/JIT ×
default/her-allocation GC matrisinde deterministic hata verdiğini doğrular.
Annotation TypePlan unit testi exact scalar/plain container, nested list/dict,
fixed tuple, set class'ı, kullanıcı class identity/version ve unsupported-value
reason code'unu doğrular. Var olan `__annotations__` anahtarının değeri
değiştirildiğinde structural dict epoch'u korunurken content epoch'u ve canonical
plan hash'i değişir; lazy function cache yeni planla yenilenir.
Leaf JIT aynı differential corpus'ta debug/release ve default/stress GC ile
çalıştırılır. Exact integer guard failure, immediate taşma, floor sıfıra bölme
deopt'u, unsupported opcode fallback'i, native dönüş, compile süresi ve code-size
sayaçları ayrıca test edilir. True division runtime helper'ı float allocation,
exact error PC/türü ve explicit materialized register roots kullanır. Ardışık iki
helper arasında yalnız JIT register'ında yaşayan float, helper-triggered stress
collection'dan sağ çıkar. Bu boxed-register ABI'sinin allocation safepoint testidir;
unboxed machine deopt map kapsamı ayrı JIT testlerinde doğrulanır. Suspended generator
ve kullanıcı `__del__` logical finalization'ı; resurrection, exactly-once çağrı,
`sys.unraisablehook` ve generator → user object → foreign payload sırası ile
uygulanmıştır. CPython bridge'in iki-collector
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

Kimlik ve üyelik karşılaştırmaları aşamasında toplam 329 Rust testi ile 311
stdout ve 164 exception differential vakasına ulaşıldı. Parser/compiler/verifier
testleri bytecode v21 `IS`, `IS_NOT`, `CONTAINS` ve `NOT_CONTAINS` operandlarını;
runtime testleri native container hızlı yollarını, suspending `__contains__`,
truthiness, iterator ve equality continuation'larını, metaclass dispatch'ini ve
moving stress-GC köklerini doğrular. Cranelift `is`/`is not` işlemlerini doğrudan
logical `Value` karşılaştırmasıyla üretir; genel üyelik açık desteklenmeyen-op
sınırından interpreter'a düşer. Debug/release × interpreter/JIT × normal/stress-GC
differential matrisinin sekiz koşusu Python 3.14.6 ile eşleşmiştir.

Senkron comprehension aşamasında toplam 334 Rust testi ile 312 stdout ve 166
exception differential vakasına ulaşıldı. List/dict comprehension ve generator
expression testleri gizli lexical scope'u, dış scope'ta eager outer `iter()`
zamanlamasını, iç içe clause/filtreleri, unpack target'ı, closure cell'lerini,
class-scope görünürlüğünü ve suspended generator köklerini kapsar. Bytecode v22
`LIST_APPEND` verifier sınırını ve geçici guest-list üretmeyen write-barrier'lı
append yolunu;
public JIT testi ise comprehension code'unun açık generic-runtime fallback'ini
doğrular. Debug/release × interpreter/JIT × normal/stress-GC differential
matrisinin sekiz koşusu Python 3.14.6 ile eşleşmiştir.

User finalizer testi erişilemeyen `__del__` sahibi nesnelerin collector dışında
bounded kuyrukla çalıştırılmasını, resurrection'ı, exactly-once çağrıyı ve
unraisable hataların yalıtılmasını interpreter/JIT-caller altında doğrular. Hook
testi `UnraisableHookArgs` kenarlarının moving-GC güvenliğini, kullanıcı hook'una
dispatch'i ve hook hatasının sayaçlanarak bastırılmasını kapsar; ayrı sıra testi
generator finalizer'ın user object'ten önce çalıştığını doğrular.

Fonksiyon annotation dilimi positional-only/positional, `*args`, keyword-only,
`**kwargs` ve dönüş annotation ifadelerini Tonic-owned AST'ten doğrulanmış
function-site metadata'sına taşır. Runtime `return` anahtarlı insertion-ordered
`function.__annotations__` sözlüğünü oluşturur; annotation'sız fonksiyonlar boş
sözlüğü yalnız ilk erişimde ayırır. Verifier symbol/register sınırlarını ve tekil
anahtarları denetler. Runtime testi yalnız annotation sözlüğünden erişilebilen
bir sınıfı interpreter/JIT-caller × allocation-stress GC altında canlı tutar;
Python differential vakası gözlenebilir sözlük sırasını ve çağrı davranışını
karşılaştırır.

Değişken annotation dilimi simple/non-simple hedef ayrımını Tonic-owned AST'te
korur. Compiler testleri module/class prologue sözlüğünü, function-local
annotation suppression'ını ve global/nonlocal hata sınırını doğrular. Runtime
testi eager module/class kayıt sırasını, annotation-only missing binding'i,
çalışmayan control-flow suite'inin boş metadata etkisini, attribute/subscript
target yan etkilerini ve yalnız class annotation dict'inden erişilen bir sınıfın
interpreter/JIT-caller × allocation-stress GC altında yaşamasını kapsar. Python
3.14 differential vakaları ortak class sözlüğü, local binding ve karmaşık target
davranışını karşılaştırır; Tonic'in eager annotation değerlendirmesi ADR 0084'te
açıkça belgelenmiş dil tercihidir.

Type parameter/type-alias dilimi bytecode v31 `TYPE_PARAM` ve `TYPE_ALIAS`
operandlarını, code/function-site name-slot-register tutarlılığını ve duplicate
parametre reddini verifier/compiler katmanında sınar. Runtime testi TypeVar,
TypeVarTuple, ParamSpec, bound, function/class/alias introspection'ı, generic
annotation ve gövde görünürlüğünü, nested lexical cell'i, ordinary binding
shadowing'ini, generic base çözümlemeyi, builtin/class/type-alias subscription ve
class-alias construction'ı interpreter/JIT-caller × allocation-stress GC altında
kapsar. Cranelift testi construction opkodlarının explicit generic fallback'te
kaldığını doğrular. Python 3.14 differential vakaları aynı gözlemlenebilir çıktı
ve shadowing hata sınıflarını karşılaştırır.

Python 3.13 type-parameter default dilimi bytecode v33
`TYPE_PARAM_DEFAULT` operandlarını ve unpack modunu verifier'da sınar. Parser
adaptörü upstream bootstrap grammar'ını kaynak byte offsetlerini değiştirmeden
maskeler; TypeVar, TypeVarTuple ve ParamSpec default ifadeleri aynı Tonic-owned
AST/HIR yoluna girer. Runtime `typing.NoDefault` tekil kimliğini,
`__default__`, starred tuple `__origin__`/`__args__`/`__unpacked__` metadata'sını
ve generic sınıfın eksik trailing argümanlarını doğrular. Interpreter/JIT-caller
× allocation-stress GC testi default graph'larının precise trace edildiğini;
Python 3.14 differential vakası gözlenebilir metadata ve alias argümanlarını
karşılaştırır. Tonic'in eager değerlendirme tercihi ADR 0093'te belgelenmiştir.

PEP 649/749 incelemesi sonucunda Tonic 0.x için eager annotation sözleşmesi ADR
0094 ile kabul edilmiştir. Mevcut function/module/class/type-parameter testleri
definition-time evaluation sırasını ve kesin GC edge'lerini doğrular. Python
3.14'ün `__annotate__`, `annotationlib` formatları, ForwardRef ve fake-globals
mekanizması desteklenmiş gibi raporlanmaz; bu yüzey bilinçli uyumluluk farkıdır.

Matrix multiplication dilimi bytecode v32 `MAT_MUL`/`INPLACE_MAT_MUL`
operandlarını iki verifier'da sınar. Compiler testi Tonic-owned AST ve iki opcode
lowering'ini; runtime testi direct/reflected/in-place yolları, strict subclass
önceliği, `NotImplemented` fallback'i, metaclass dispatch'i ve askıya alınan
method frame'lerinin precise roots'unu JIT-caller × allocation-stress GC altında
doğrular. Cranelift testi opkodun açık generic-runtime fallback'inde kaldığını;
Python 3.14 differential vakaları gözlenebilir sonuç ve TypeError sınırını
karşılaştırır.

Class-subscription testi metaclass `__getitem__` önceliğini, plain ve explicit
classmethod `__class_getitem__` binding'ini, inherited hook'ta dinamik subclass
receiver'ını, generic class override'ını ve hook yokken managed `GenericAlias`
fallback'ini interpreter/JIT-caller × allocation-stress GC altında doğrular.
Python 3.14 differential vakası aynı gözlenebilir çağrı sırasını; hata vakası
callable olmayan hook'un TypeError sınırını karşılaştırır.

`divmod` testi native int/float sonuçlarını, suspending `__divmod__`, strict
subclass `__rdivmod__` önceliği, `NotImplemented` fallback'i ve metaclass
dispatch'ini interpreter/JIT-caller × allocation-stress GC altında doğrular.
Python 3.14 differential vakaları aynı gözlenebilir sonuçları; sıfıra bölme,
desteklenmeyen operand ve yanlış arity hata sınıflarını karşılaştırır.

`round` testi global keyword binder'ını, doğrudan int/float `__round__`
descriptor'larını, suspending kullanıcı/metaclass hook'unu ve `__index__`
dönüşümünü interpreter/JIT-caller × allocation-stress GC altında doğrular.
Exact ratio motoru decimal ties-to-even, bigint negatif basamak, signed zero,
NaN/infinity ve overflow sınırlarını kapsar. Kalıcı Python 3.14 vakalarına ek
olarak seed'li 5.000 finite-f64/basamak ve 1.000 bigint/basamak birleşimi
CPython'ın ürettiği sonuçlarla sıfır değer farkı vermiştir.

`pow` testi iki/üç argümanlı ve positional/keyword çağrıları, explicit `None`,
native int/float kuvvet, pozitif/negatif modulus, bool, BigInt, negatif üs ve
modüler tersi kapsar. Suspended `__pow__`/`__rpow__`, Python 3.14 strict-subclass
ternary reflected önceliği, `NotImplemented`, metaclass ve native int-subclass
fallback'i interpreter/JIT-caller × allocation-stress GC altında çalıştırılır.
Kalıcı differential vakaları arity/binder, sıfır modulus, terslenemeyen taban ve
integer olmayan ternary operand hata sınıflarını da CPython ile karşılaştırır.
Seed 20261001 ile 1.000 nonnegative ve 500 terslenebilir negative-exponent
BigInt/modulus birleşimi debug/release × interpreter/JIT/JIT+`gc_every=1`
yollarında Python 3.14.6 ile toplam 9.000 sonuç karşılaştırmasında sıfır değer
farkı vermiştir.

Global `repr`/`ascii`/`format` testi user instance ve metaclass hook'larını,
askıya alınan `__repr__`/`__format__` frame'lerini, nested Unicode ASCII
escaping'i, native integer/float/string format-spec'lerini ve JIT caller dönüşünü
interpreter/JIT × allocation-stress GC altında doğrular. F-string `!s`/`!r`/`!a`
ve format-spec opkodları aynı alt yordamı kullanır; metaclass override'ları da bu
yüzeyde karşılaştırılır. Differential hata vakaları positional-only arity,
keyword reddi, string olmayan spec ve hook dönüşlerini Python 3.14 ile eşler.

Global `sum` testi list/tuple/range, generator ve genel custom iterator'ları
öğeleri materialize etmeden tüketir; explicit/keyword `start`, boş iterable,
BigInt taşması, list birleştirme ve iterator-before-start-check değerlendirme
sırasını kapsar. Suspended `__iter__`/`__next__` ile `__add__`/`__radd__`, strict
subclass önceliği, `NotImplemented`, iterator sınırındaki `StopIteration` tüketimi
ve toplama içinden kaçan `StopIteration` interpreter/JIT-caller × allocation-
stress GC altında doğrulanır. Seed 42 ile üretilen 160 yüksek dinamik aralıklı
int/float dizisi Python 3.14'ün Neumaier compensated sonucuyla sıfır fark vermiştir.
Kalıcı interpreter ve Python-karşılaştırma benchmark'ları 1.000 öğelik integer ve
float dizilerini 100 kez toplar; integer hızlı yolunda öğe başına guest allocation
yoktur.

Async comprehension dilimi eager list/dict sonuçlarını hidden coroutine'de,
async generator expression'ı ise hidden async-generator frame'inde yürütür.
Compiler testi async clause ile yalnız `await` içeren comprehension ayrımını,
`GET_AITER`/`GET_ANEXT` ve `ASYNC_YIELD` üretimini doğrular. Runtime ve Python
differential vakaları mixed sync/async clause'ları, eager outer `__aiter__`
zamanlamasını, element `await`ini, modül düzeyinde async generator expression'ı
ve tüketimini interpreter/JIT-caller × allocation-stress GC altında kapsar.

Native set dilimi bytecode v23 `SET`/`SET_ADD` operand doğrulamasını, duplicate
eleme ve collision'lı kullanıcı `__hash__`/`__eq__` çağrılarını, hash-aware
üyelik/eşitlik, iteration, boş set gösterimi ile sync/async set comprehension'ı
kapsar. Runtime testi interpreter/JIT-caller × allocation-stress GC altında
precise eleman köklerini; differential çıktı/hata vakaları Python 3.14.6 ile
gözlenebilir sonuçları ve unhashable set elemanı hatasını karşılaştırır.

Structural matching dilimi bytecode v29 `MATCH_SEQUENCE`, `MATCH_MAPPING`,
`MATCH_KEY`, `MATCH_CLASS`, `MATCH_ATTR`, `MATCH_ARGS`, `MATCH_CLASS_ITEM` ve
`MATCH_UNIQUE` sınırlarını doğrular. Compiler/runtime testleri subject'in tek
değerlendirilmesini, staged capture'ları, guard fallthrough'unu, OR birleşimini,
fixed/starred ve nested sequence'leri, mapping `**rest` ile collision-aware
dinamik duplicate key denetimini, positional/keyword class desenlerini,
`__match_args__`, builtin self-pattern'ı ve descriptor'ın tek okunmasını kapsar.
Eksik attribute case başarısızlığına dönüşürken diğer protokol hataları korunur;
Cranelift bu doğrulanmış opkodlarda açık generic-tier fallback uygular.

F-string dilimi bytecode v30 `CONVERT` ve `FORMAT_VALUE` operandlarını iki
verifier'da doğrular. Compiler/runtime testleri owned AST'yi, kaynak sırasını,
nested dynamic spec'i, `!s`/`!r`/`!a`, Unicode string precision ve kullanıcı
`__str__`/`__repr__`/`__format__` continuation'larını interpreter/JIT-caller ×
allocation-stress GC altında kapsar. Kalıcı diferansiyel vaka sonuçları ve hata
türlerini Python 3.14.6 ile karşılaştırır; ek deterministik format grid'i 216
string/int/float birleşimini, seeded numeric tarama ise 2.684 sayı/spec
birleşimini sıfır gözlenebilir farkla doğrulamıştır. Cranelift format opkodlarında
açık generic-tier fallback uygular.

`.github/workflows/ci.yml` Linux/macOS için aynı kontrolleri tanımlar; remote
sonuçlar her push sonrasında ilgili GitHub Actions koşusundan ayrıca doğrulanır.
C ABI header/smoke ve guarded callback testleri vardır,
ancak sanitizer sonucu varmış gibi raporlanmaz; JIT differential yalnız yukarıdaki belgelenmiş kapsamı
kanıtlar.
