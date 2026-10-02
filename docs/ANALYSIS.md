# Tonic gereksinim analizi

Kaynaklar: `AGENTS.md` (0–57) ve `TONIC_INTEROP_RUNTIME.md` (0–100).
Bu iki belge hedef mimariyi tarif eder; mevcut özellik listesi değildir.
Depo başlangıçta yalnızca bu belgeleri içeriyordu.

## Bağlayıcı mimari kararlar

1. Python sözdizimi ile Python/CPython runtime uyumluluğu farklı işlerdir.
   Parser Rust'ta çalışır, Tonic AST üretir. Runtime parser AST'sine bağımlı değildir.
2. Tonic kendi register VM, Value, heap ve çağrı protokolüne sahiptir.
   CPython layout, referans sayımı, GIL veya C ABI çekirdeğe girmez.
3. İç Value ile dış Handle farklı sözleşmelerdir. Handle doğrulaması runtime
   sahipliği, yaşam süresi ve stale-token kontrolünü kapsamalıdır.
4. GC/JIT/native çağrılar birlikte tasarlanır. Yaşayan referanslar kayıtlı olmalı,
   moving GC için kalıcı native adresler kullanılmamalıdır.
5. Quickening, shape cache ve JIT için her varsayımın guard/fallback yolu gerekir.
   Bunlar doğruluk ve ölçüm temeli kurulmadan uygulanmış sayılmaz.
6. Sözdizimi desteği olmayan yapı sessizce atlanmaz; konumlu tanı üretir.
   Parser'ın bir yapıyı tanıması onun çalıştırılabildiği anlamına gelmez.
7. Interop önce Rust-native handle API, sonra C ABI, buffer, callback ve foreign
   wrapper olarak genişler. CPython proxy/adapter compatibility kaçış yoludur;
   HPy Universal host ile aHPy hattı bağımsız portable-native katmandır.

## Karara bağlanması gereken alanlar

- Python hedef sürümü ve sürümler arası parser conformance matrisi.
- NaN boxing / düşük bit etiketleme, handle kapasitesi, generation taşması.
- Finalizer sırası, bounded-pause hedefi ve daha gelişmiş survivor/aging politikası.
- Shape geçişleri, descriptor/MRO ve version invalidation.
- Baseline Cranelift ABI, deopt register haritası ve kesin stack map.
- ABI v1 function-table'a gelecekte alan ekleme ve capability sürümleme politikası;
  yayımlanmış uzantılar için struct-size öneki korunmalıdır.
- Buffer dtype/shape/stride, owner, mutability ve pinning sözleşmesi.
- CPython proxy kimliği, interpreter sahipliği ve yürütme kilidi.
- Çapraz runtime döngülerinin toplanması, finalizer sırası, shutdown ve callback.
- Native uzantı güven sınırı: opaque handle kötü niyetli native kodu sandbox yapmaz.
- HPy Universal sürüm/context sözleşmesi, `.hpy0` loader, `HPyGlobal`/`HPyField`
  runtime izolasyonu ve aHPy cross-runtime destek matrisi.

## İlk teslimatın sınırı

AGENTS §42 ve §56 gereği ilk doğrulanabilir dilim `fib(40)` programıdır.
Buna M0/M1 altyapısı, M2 fonksiyon/kontrol akışı ve interop §96'daki
`fastmath.add(20, 22)` eklenir. İşlevler doğrudan VM'de çalışır; Python'a
execute/eval gönderilmez. Testlerdeki CPython çağrısı yalnızca referans oracle'dır.

Bu teslimat tam dil değildir. Classes/shapes, closures, generators, async,
exception handlers, descriptors, kapsamlı import/stdlib, quickening, moving GC,
Cranelift, C uzantı yükleyicisi ve CPython bridge sonraki işlerdir.
Eksikleri hazırmış gibi gösteren boş API veya sahte JIT modu eklenmez.

İkinci uygulama diliminde lexical closures/global/nonlocal, tam mevcut fonksiyon
call binder'ı, dict/item mutation ve precise compacting GC eklenmiştir.
Bu, ilk teslimat sınırını genişletti; class/shape, generational GC, JIT ve bridge
kabul kapıları o aşamada açıktı. Güncel durum [ROADMAP.md](ROADMAP.md) dosyasındadır.

Üçüncü dilimde class namespace/scope, constructor ve function-method binding,
C3 MRO, shared shapes/slot storage, dictionary fallback ve class mutation version
eklenmiştir. Yeni nesneler ve class/initializer continuation'ları precise GC
tarafından izlenir. M3 tam kapanmamıştır: descriptor/metaclass/özel protokoller
eksikti; M4 specialization, generational GC, JIT ve interop bridge beklemekteydi.
22 iş yükünün ara ölçümü ve önceki performansla karşılaştırması
[STAGE3_BENCHMARKS.md](STAGE3_BENCHMARKS.md) içindedir.

Dördüncü dilim genel function/class decorator uygulama sırasını ve GC tarafından
izlenen staticmethod/classmethod wrapper'larını eklemiştir. Bu, tam descriptor
protokolü değildir: property, kullanıcı `__get__/__set__`, metaclass ve super
bekler. İki yeni method workload'ı dahil ara kayıt
[STAGE4_BENCHMARKS.md](STAGE4_BENCHMARKS.md) içindedir.

Ardından lambda Tonic-owned expression AST'si ve ayrı HIR child scope'u olarak
eklenmiştir. Runtime yeni callable türü kullanmaz: normal Function/cell/default
temsili, verifier metadata'sı ve call binder paylaşılır. Lambda benchmark sonucu
diye yeni bir JIT/tier iddiası üretilmemiştir.

Beşinci dilim property getter/setter'ı data-descriptor önceliği ve frame dönüş
continuation'larıyla eklemiştir. Altıncı dilim list/tuple/Unicode string read-only
slicing'i Tonic AST, bytecode v4 ve GC-traced Slice değeri üzerinden tamamlamıştır.
Yedinci dilim kullanıcı `__get__/__set__` data/non-data descriptor çözümlemesini
eklemiştir. Callable/receiver ile iki protokol argümanı inline taşındığından erişim
başına BoundMethod, host Vec veya guest argument container tahsisi yoktur. Güncel
ölçüm [STAGE7_BENCHMARKS.md](STAGE7_BENCHMARKS.md) içindedir. Metaclass, M4
specialization, generational GC, JIT ve bridge o aşamada açıktı. Otomatik `__set_name__`
daha sonra class-completion continuation zinciriyle eklenmiştir. Bytecode v5
`DelAttr` ile custom `__delete__`, property deleter ve normal instance/class
attribute deletion da eklenmiştir.
Bytecode v6, class-body code object'inde en fazla bir doğrulanmış sentetik
`__class__` local cell'e izin verir. Sınıf tamamlandığında hücre logical class
handle'ı ile doldurulur; method, iç fonksiyon ve lambda closure'ları aynı hareketli
GC uyumlu hücreyi taşır. `super()` çağrısı aktif Tonic frame'in ilk parametresi ve
bu hücreden bağlam üretir. İki argümanlı biçim de C3 MRO'nun başlangıç sınıfından
sonraki kısmında function, classmethod, staticmethod, property ve custom descriptor
çözümler. Tek argümanlı unbound biçim şimdilik açık bir bootstrap eksikliğidir.

Constructor yolu daha sonra `object.__new__` ve class MRO'dan bulunan custom veya
inherited `__new__` ile genişletilmiştir. Function tanımı class completion sırasında
staticmethod wrapper'ına çevrilir. Custom allocator sonucu istenen class'ın instance'ı
ise özgün positional/keyword argümanlarla `__init__` continuation'ı çalışır; değilse
sonuç doğrudan döner. Bekleyen class ve argümanlar precise GC roots'a dahildir.

İlk genel özel-metot dilimi callable instance ve length protokolüdür. Implicit
`__call__`/`__len__` araması instance sözlüğünden değil class C3 MRO'sundan yapılır.
Function, staticmethod ve classmethod bağlama callable/receiver çifti olarak taşınır;
tekrarlı çağrılar geçici BoundMethod ayırmaz. Guest `__len__` normal VM frame'inde
çalışır ve `ReturnAction::Length` dönüşü integer, i64 sınırı ve negatiflik açısından
doğrular. Callable nesne zinciri 100 yönlendirmede RecursionError ile kesilir.

Truthiness bu altyapıyı `__bool__` ve `__len__` fallback'i için genişletir. Branch
ve unary-not sonucu guest frame dönüşünde `ReturnAction::Truth` ile tamamlanır;
Rust çağrı yığınına recursive guest yürütme eklenmez. `and/or` kısa devresi object
operandını döndürdüğü için continuation özgün değeri explicit root olarak taşır ve
yalnız kontrol kararını protokol sonucundan üretir. `__bool__` exact bool ister;
length fallback ortak signed/i64 doğrulamasını kullanır.

Hash protokolü de normal guest-frame continuation modeline taşınmıştır. Runtime
adresleri hash/identity olarak açılmaz; stable logical Value/handle kimliği,
numeric canonicalization ve implementation-private string/sequence mixing
kullanılır. Sözlük hash kovası yalnız aday kümesini daraltır; gerçek eşitlik
suspending rich comparison ve truth zincirinden gelir. Aynı continuation motoru
list/tuple/dict/slice nested equality ile list/tuple lexicographic ordering'i
çalıştırır, bütün pending container ve elemanları precise root olarak izler.

İlk gerçek JIT dilimi Cranelift 0.119'u ayrı `tonic-jit` crate'inde sabitler.
Doğrulanmış leaf code object'leri immediate integer sabit/move, `+ - * // %`,
unary, karşılaştırma, truthiness, branch ve loop işlemlerinde native koda çevrilir.
`/` opak runtime helper üzerinden generic numeric semantiği ve float allocation'ı
kullanır. ABI, doğrulanmış register sayısında `u64` dizisi alır; başarıda dönüş
değerini, guard failure veya helper error'da exact bytecode PC'sini verir. Böylece interpreter görünür
register durumu tekrar kurulmadan zaten materialized halde kalır. Type, taşma ve
sıfıra bölme guard'ları generic opcode'da semantiği tamamlar. Unsupported bir
opcode bütün fonksiyonu interpreter'a bırakır. Helper öncesinde tüm virtual
register'lar materialize edilir; bunlar diğer VM roots ile birlikte allocation
safepoint'inde zamanlanmış minor/major collector'a verilir. Helper panic'i FFI sınırını aşmaz ve
guest hata türü korunur. `CALL` exact-PC side exit ile explicit VM child frame'i
kurar; dönüşte caller aynı code object içinde sonraki PC'den native yürütmeye devam
eder. Bound `LOAD_GLOBAL`, VM'in raw global değer aynasını doğrudan okur; her
`STORE_GLOBAL` aynı slotu günceller, `UNBOUND` ise helper üzerinden exact hata PC'sine
gider. Recursive çağrı global rebinding sonrasında da doğrudur. Native backedge inline
sayaçla 1024 geçişte bir exact-root poll helper'ına gider.
Loop'lar 64 interpreted backedge sonrasında exact hedef PC'de OSR yapar. Döngülü
kodda `+ += - * // %` exact-int guard'ı başarısız olursa materialized roots ile
generic runtime helper'a gider; küçük düz leaf fonksiyonların ölçülmüş deopt ve
kârlılık politikası değişmez. Profille kararlı exact-callee tamsayı leaf
fonksiyonları side-effect-free native inlining yoluna alınır; callee veya operand
guard miss'i özgün `CALL` PC'sine atomik deopt olur. Exact function profili için
positional-only, keyword-only ve default değerleri ordinary binder kurallarından
türetilen target-slot planıyla tahsissiz bağlanır; exact callee guard'ı default
kimliğini de korur. Static/class instance method, staticmethod ve classmethod
çağrıları allocation-free lookup/call fusion yoluna alınmıştır. Custom descriptor,
native float lowering, dependency invalidation, unboxed
Exact-float numeric loop için PC-indexli deopt map, arbitrary-PC F64 slot
initialization ve poll-deopt'ta tam interpreter-register rekonstrüksiyonu vardır;
managed handle'lar mevcut explicit precise root tamponunda kalır.

Plain instance method çağrısı için profilli `ATTR` ile onu tüketen `CALL`, aradaki
yeniden oynatılabilir `CONST/MOVE` dizisi kanıtlandığında kaynaştırılır. Allocation
üretmeyen lookup helper'ı her girişte instance shadowing ve güncel C3 class lookup
sonucunu okur; generated exact function guard'ı değişimde `ATTR` PC'sine atomik
deopt eder. Receiver caller register'ından target `self` slotuna bağlanır ve hot
yolda `BoundMethod` ayrılmaz. Staticmethod aynı helper seçicisinde no-receiver
binding türüyle guard edilir; aynı function'ın plain metoda rebinding'i yanlışlıkla
fast path'i geçemez. Classmethod için helper exact function yanında gerçek dinamik
`cls` receiver'ını guest register'lardan sonraki kesin JIT root cache'ine yazar;
helper ve backedge poll'lar toplam root dilimini tarar. Class üzerinden
plain function/staticmethod/classmethod erişimi de aynı bağlama planına dahildir.
Custom descriptor getter generic `ATTR` side exit ile normal VM frame'inde çalışır;
dönen exact leaf function caller JIT'e resume edildikten sonra inline edilir.
Bu yol yalnız direct-call tüketicisi varsa derlenir. Expanded çağrılar
`BEGIN_ARGS`→eşleşen dış `CALL_EXPANDED` aralığını tek generic segment olarak
çalıştırıp native caller'a devam eder; nested builder derinliği erken resume'u
engeller.
Ordinary call'da boş kalan ve target bytecode tarafından okunmayan `*args/**kwargs`
register'ları için tuple/dict materialization atlanır; operand taraması kullanılan
variadic parametreleri generic binder'da bırakır. Düz positional `*list/*tuple`
segmenti kararlı function/uzunluk profiliyle güncel öğeleri opaque helper'dan alır
ve direct leaf'i inline eder. Named/default slotlar ile exact-dict `**mapping`,
kararlı string-key profili ve current value lookup'ıyla aynı direct yola bağlanır.
Dinamik expanded öğeler guest source register'ını ezmez; ayrı kesin JIT root
yuvalarında tutulur. Ordinary direct leaf target'ın okuduğu `*args/**kwargs` da
allocation helper'larıyla tuple/dict olarak bu kesin köklere materialize edilir.
Expanded-call ve method target variadic materialization'ı açık kalır.
Method lookup sonucu ilk `ATTR` yürütmesinde invocation-local native cache'e
yazılır. Aynı invocation içindeki çağrılar exact owner/function guard'larıyla
helper'sız ilerler; owner değişimi `ATTR` PC'sine deopt, sonraki invocation ise
class/base rebinding'i gören yeni lazy lookup üretir.

Generational GC diliminde her allocation nursery'de başlar; minor collection
precise roots ile remembered old owner'ların managed kenarlarından yalnız young
nesneleri izler. Survivor'lar bir minor sonunda old alana terfi eder. List, dict,
cell, module, class namespace ve attribute mutation API'leri write barrier'ı tek
bir heap sınırında uygular. Her 32. otomatik collection major'dır; explicit
`collect_garbage()` da full-heap major collection yapar. Üç allocation-heavy
workload'da eski yalnız-full-heap tabanına göre %7,5–18,1 wall-time kazancı
ölçülmüştür. Karar ve ham yöntem [ADR 0025](adr/0025-generational-gc.md) ile
[GENERATIONAL_GC_BASELINE.md](GENERATIONAL_GC_BASELINE.md) içindedir.

Adaptive interpreter katmanı ayrı immutable state tablosunda sekiz gözlemden
sonra immediate integer aritmetiğini, exact-callee basit Tonic çağrılarını ve
instance shape-slot okumalarını specialize eder. Call cache callee kimliğiyle;
attribute cache class, shape, slot ve class/MRO dependency version ile korunur. Her
monomorphic site ikinci kararlı hedefi gördüğünde iki girişli sabit PIC'e terfi
eder; PIC payload'ları yalnız terfi eden siteler için bounded yan tablolardadır.
Guard failure aynı işlemi generic semantik yolunda tamamlar. A/B ölçümleri call
işinde yaklaşık %11, instance attribute okumada yaklaşık %30 süre azalması
gösterdi. Küçük straight-line JIT çağrısının kârsız olduğu da aynı harness ile
ölçüldüğü için yedi instruction altı fonksiyonlar varsayılan adaptive tier'da kalır.
İki hedefli ayrı harness'te call PIC generic moda göre yaklaşık %7, attribute PIC
%13,6 kazanç sağladı; ayrıntı [ADAPTIVE_PIC_BASELINE.md](ADAPTIVE_PIC_BASELINE.md)
dosyasındadır. Class oluşturulurken ancestor'lara weak descendant bağlantısı
kaydedilir; mutation yalnız ilgili class ile alt sınıflarının sürümünü artırır ve
major GC ölü bağlantıları temizler.

Native interop ABI v1 tek bootstrap sembolü ve size/version/capability başlıklı
immutable function table kullanır. Opaque C context ile 64-bit logical handle
internal `Value` ve heap layout'unu dışarı taşımaz. Her işlem status ve out-param
protokolündedir; exception context'te kalır, panic hem API girişinde hem extension
function/init trampoline'ında tutulur. Typed buffer capability'si f64 data, shape
ve byte-stride tahsislerini moving GC owner nesnesinden ayırır. Descriptor ayrı
owner handle, dtype ve mutability flags taşır. `fastmath.sum` buffer taramasında
boxed liste fallback'ine göre 4,99× throughput ölçülmüş, 10.000 export boyunca
yalnız ilk array dönüşümünde bir copy kaydedilmiştir.

Foreign wrapper dilimi dış payload adresini managed object içinde opaque tutar ve
ABI v1 vtable'ından yalnız adapter kimliği, trace ve destroy slotlarını kopyalar.
Payload içindeki Tonic kenarları global root olmayan özel foreign-reference
handle'larıyla bildirilir; her GC öncesi trace refresh bunları exact `Value`
kenarlarına çözer ve generational remembered set'i günceller. Sweep payload'ı
pending queue'ya taşır, compaction bittikten sonra destructor tam bir kez çalışır.
Trace/destructor panic'leri sınırda tutulur; shutdown queue'yu handle invalidation'dan
önce boşaltır. 100.000 owned wrapper oluşturma+toplama managed list baseline'ına
göre yaklaşık %71,2 ek süre göstermiştir; bu fark C crossing, host payload tahsisi
ve destructor çağrısını da içerir. CPython adapter'ının attribute/call ve
execution-state slotları bu genel wrapper'ın sonraki katmanıdır.

CPython ilk dilimi bu katmanı ayrı `tonic-cpython` crate'inde somutlaştırır.
`python3-config --embed` ile bağlanan C API ve `PyGILState` guard'ı yalnız bridge
çağrısında etkinleşir. None/bool/int/BigInt/float/UTF-8 string ile list/tuple/dict
iki yönlü çevrilir; container memo tablosu alias ve list/dict döngülerini korur.
Arbitrary CPython sonuçları adapter-guard'lı `ForeignPyObject` wrapper'a girer ve
foreign finalization kuyruğunda `Py_DecRef` edilir.

`PyTonicProxy`, CPython 3.12+ negative-basicsize stable type API'siyle kurulan gerçek
bir heap type'tır. VM adresi yerine stable runtime owner, execution kimliği ve
persistent handle taşır. `tp_call/tp_getattro/tp_setattro/tp_repr` slotları normal
Tonic binder/descriptor continuation'larına döner; positional ve keyword callback,
property erişimi ve bridge'den geri gelen proxy'nin özgün logical handle'a açılması
test edilir. Eksik attribute sınıfı CPython tarafında `AttributeError` olarak
korunur. GIL altında tutulan non-owning proxy cache canlı identity'yi yeniden
kullanır ve `tp_dealloc` girdiyi kesin siler. Doğrudan Tonic proxy wrapper için
CPython refcount dış sahipliği ayırır: yalnız wrapper referansı kaldığında persistent
kök non-rooting foreign edge'e düşürülür, dış Python referansı oluştuğunda tekrar
güçlendirilir. Arbitrary `ForeignPyObject` içindeki transitif proxy kenarları public
`Py_tp_traverse` ile bounded olarak taranır. Borrowed visitor edge'leri token
sahipliğini proxy'de bırakır; trial-deletion dış-kök testi persistent handle'ı
demote veya promote eder. Type/module/function altyapısı, başka runtime proxy'si,
4.096 düğüm veya 16.384 kenar sınırı conservative retention'a gider. Ayrıntı
[ADR 0048](adr/0048-cpython-cross-collector-graph-tracing.md) içindedir.

## Kaynak modül dilimi

Genel kaynak modül hattı bytecode v14 ile tek doğrulanmış program içinde birden
fazla modül taşır. Linker code identity'lerini ve canonical local/attribute
symbol'lerini paylaşırken global operandları modül başına private slotlara ayırır.
Bu ayrım mevcut düz register/global dizisi ile Cranelift ABI'sini değiştirmeden
Python tarzı ad alanı izolasyonu sağlar. Verifier modül code aralıklarını ve global
sahipliğini trust boundary'de denetler.

CLI giriş dizininden `.tonic`, `.py` ve package `__init__` kaynaklarını çözer;
dotted import önekleri ile `from` alt modül adaylarını graph'a ekler. Runtime
module object'leri yalnız gerçekten import edildiğinde ayırır. Böylece circular
import için `Initializing` nesne kimliği korunurken import kullanmayan tek dosyalı
programların önceki allocation bütçesi değişmez. Başarısız import kısmi global ve
parent bağlantılarını geri alır. Global/member mutation tek sınırdan geçer ve
module version'ı artırır. Import edilen normal fonksiyonlar JIT'e uygundur;
materialized global slot doğrudan güncellendiği için module attribute rebinding
native kodda stale değer üretmez. Ayrıntı
[ADR 0071](adr/0071-source-module-linker-and-loader.md) dosyasındadır.

## Generator ilk dilimi

Bytecode v16 code nesnesine generator niteliğini, açık `YIELD` ve `YIELD_FROM`
opcode'larını ekler.
Generator fonksiyonu çağrıldığında gövde çalıştırılmaz; bağlanmış register ve cell
dizileri heap'teki logical-handle nesnesine taşınır. Resume sırasında aynı frame
VM frame stack'ine geri alınır, `yield` noktasında instruction pointer, register,
cell ve aktif exception state yeniden generator nesnesine yazılır. Bu değerlerin
tamamı precise tracing kenarıdır ve suspend/return yazımları write barrier'dan
geçer; moving ve stress GC native adres varsayımına ihtiyaç duymaz.

`iter`, `next`, generator descriptor'ları, `send`, tek-argüman `throw` ve `close`
normal continuation altyapısını kullanır. For döngüsü, list/tuple/dict oluşturma,
unpack ve `*args` genişletme generator askıya alındığında tüketici state'ini heap
kökü olarak korur. `yield from`, delege generator'ın veya özel iterator'ın
`StopIteration.value` dönüşünü ifadenin sonucuna taşır; `send`, `throw` ve `close`
işlemlerini delegeye iletir ve dış generator'ın handler/finally bölgelerini normal
unwind zincirinde tutar. Açıkça kaçan `StopIteration` generator sınırında
`RuntimeError` olur. Generator bytecode'u şimdilik Cranelift'e verilmez; JIT
çağıran kod exact interpreter continuation ile güvenli biçimde devam eder.

Tek-argümanlı modern `throw` yanında legacy `throw(type, value, traceback)` biçimi
exception constructor ve traceback doğrulamasıyla desteklenir. Major/minor
collector, ulaşılamayan askıdaki generator'ı
silmek yerine logical handle'ıyla finalization kökü olarak kuyruğa alır. VM collector
dışında `GeneratorExit` enjekte eder; delege `yield from` zinciri içten dışa kapanır,
`finally` çalışır ve fiziksel reclamation sonraki collection'a kalır. Normal instruction
sınırında en fazla sekiz finalizer çalıştırılır; idle explicit collection ve shutdown
kuyruğu tamamen boşaltır. Finalizer'dan kaçan guest exception ana yürütmeden yalıtılır
ve sayaçlanır. Kullanıcı `__del__`/resurrection açık kalır. Ayrıntı
[ADR 0072](adr/0072-generator-frame-state-machine.md) dosyasındadır.

## Coroutine ilk dilimi

Bytecode v17, code nesnesindeki generator niteliğinden ayrı bir `coroutine`
niteliği ve doğrulanmış `GET_AWAITABLE` opcode'u ekler. `async def` çağrısı
gövdeyi çalıştırmadan, mevcut suspended-frame altyapısını coroutine türüyle
yeniden kullanarak tembel bir `<coroutine object>` üretir. Exact Tonic coroutine
`await` yolunda doğrudan sürülür; özel awaitable nesnelerde sınıf MRO'sundan
`__await__` çağrılır ve dönen değerin iterator olduğu coroutine devam etmeden
doğrulanır.

Coroutine kendi başına genel iterable değildir. Açık `coroutine.__await__()`
çağrısı, kaynak coroutine'i precise trace kenarıyla tutan ayrı bir
`coroutine_wrapper` üretir. Wrapper kendi iterator'ıdır ve `next`, `send`,
`throw`, `close` işlemlerini alttaki coroutine state machine'ine iletir. Await
delegasyonu gönderilen değerleri, exception'ları, `StopIteration.value` dönüşünü
ve kapanışı iç awaitable'dan dış coroutine'e taşır. Askıdaki register/cell,
exception state ve aktif delege moving/stress GC altında köklenir. Ulaşılamayan
askıdaki coroutine mevcut collector-dışı logical-finalization kuyruğunda kapanır;
await edilen iterator önce, dış `finally` sonra çalışır.

Coroutine code'u ve coroutine hedefli direct call Cranelift kapsamı dışında
kalır. JIT'te çalışan çağıran kod generic çağrı sınırından interpreter resume
yoluna güvenle geçer.

Bytecode v18, yalnız coroutine code'unda doğrulanan `GET_AITER`, `GET_ANEXT` ve
`END_ASYNC_FOR` opcode'larıyla `async for` ekler. `__aiter__` sonucu `__anext__`
protokolüne göre doğrulanır; her next sonucu normal `GET_AWAITABLE` ve coroutine
delegasyon yolundan geçirilir. Yalnız next çağrısı/await bölgesinden kaçan
`StopAsyncIteration` loop exhaustion sayılır. Target ataması veya body içindeki
aynı exception normal biçimde yayılır. Break/continue/else mevcut loop cleanup
altyapısını paylaşır ve askıya alan özel next awaitable'ları moving/stress GC
altında continuation register'larında korunur.

Bytecode v19, yine yalnız coroutine code'unda geçerli `ASYNC_CONTEXT_ENTER` ve
`ASYNC_CONTEXT_EXIT` opcode'larını ekler. Giriş opcode'u manager tipinden
`__aexit__` callable/receiver çiftini girişten önce yakalayıp GC-traced token'da
tutar, ardından `__aenter__` çağrısını başlatır. Compiler hem giriş hem çıkış
sonucunu ortak await state machine'inden geçirir. Senkron `with` lowering'inin
exception-region ve cleanup zinciri async çıkışı da kapsayacak biçimde paylaşılır;
böylece normal çıkış, exception suppression/replacement, return, break, continue,
target ataması hatası ve kısmi çoklu-manager girişi aynı unwind kurallarını izler.
Coroutine code'u JIT adayı olmadığı için async context manager suspend noktaları
interpreter frame'inde kesin köklerle tutulur; JIT çağıran kod generic sınırdan
aynı güvenli fallback'e geçer.

Bytecode v20, async-generator kullanıcı `yield` noktalarını coroutine içindeki
`await` askılarından ayıran `ASYNC_YIELD` opcode'unu ekler. `CodeObject` üzerinde
`generator && coroutine` birleşimi ayrı `async_generator` türünü seçer; normal
çağrı gövdeyi çalıştırmadan suspended frame üretir. `__anext__` ve `asend` bir
`async_generator_asend`, `athrow` ve `aclose` ise bir
`async_generator_athrow` awaitable'ı döndürür. Bu tek kullanımlık nesneler sınıf,
kaynak generator, operation, exception/traceback ve durumlarını precise trace
kenarlarıyla tutar.

Awaitable sürücüsü iç `await` tarafından verilen değerleri dış coroutine'e normal
`YIELD` ile iletir; kullanıcı `ASYNC_YIELD` değeri ise mevcut await'i
`StopIteration.value` üzerinden tamamlar. Doğal bitiş `StopAsyncIteration`, kaçan
`StopIteration` veya `StopAsyncIteration` ise async-generator sınırında
`RuntimeError` olur. `throw`/`close` aktif await delegesine iletilir;
`GeneratorExit` sırasında değer veren generator reddedilir. Askıdaki async
generator aynı collector-dışı logical-finalization kuyruğunu kullanır. Async
generator code'u Cranelift'e verilmez ve JIT çağıran kod generic sınırdan
interpreter'a geçer. Ayrıntılar [ADR 0074](adr/0074-async-generator-protocol.md)
dosyasındadır.

Tonic-owned tek thread event loop, Future/Task await state'i, deterministik
timer, FIFO scheduling ve cancellation ADR 0075'te tanımlanan native `asyncio`
modülüyle sağlanır. Scheduler kökleri moving GC'ye açıkça bildirilir ve coroutine
resume yolu interpreter state machine'ini kullanır.

Bytecode v21, Python karşılaştırma gramerinin kalan kimlik ve üyelik işlemlerini
`IS`, `IS_NOT`, `CONTAINS` ve `NOT_CONTAINS` opcode'larıyla temsil eder. Kimlik
opaque logical `Value` sözcüklerini karşılaştırır; native adres gözlenmez ve
Cranelift bu işlemi tahsissiz üretir. Üyelik önce `__contains__`, sonra exact
native string/container yolları, ardından `__iter__`/`__next__` ve tam eşitlik
protokolü sırasını izler. Kullanıcı çağrılarının tümü askıya alınabilir ve
continuation state'i precise GC köküdür. Genel üyelik opcode'ları Cranelift'te
desteklenmeyen-op sınırından interpreter'a düşer. Ayrıntılar
[ADR 0076](adr/0076-identity-and-membership.md) dosyasındadır.

Bytecode v22, senkron list/dict comprehensions ve generator expressions için
`LIST_APPEND` opcode'unu ve gizli comprehension code object'lerini ekler. İlk
iterable dış lexical scope'ta değerlendirilip hemen iterator'a çevrilir; hedef,
filtreler, sonraki iterable'lar ve sonuç ifadesi ayrı child scope'ta yürür. Bu
hem hedef sızıntısını önler hem de element içindeki lambda'ların iteration
değişkenini gerçek bir cell olarak yakalamasını sağlar. Generator expression aynı
code object'i suspended generator frame'i olarak kullanır; ilk iterator argument
register'ında precise root'tur ve kalan gövde tüketilene kadar tembeldir.

Liste sonucu tek elemanlı geçici guest listeler üretmeden write-barrier'lı
`Heap::append_list` sınırından büyütülür. Dict comprehension mevcut suspending
hash/equality-aware `SET_ITEM` yolunu paylaşır. Comprehension code'u iterator,
mutation ve olası kullanıcı frame'leri içerdiğinden Cranelift destek kümesinin
dışında açıkça generic runtime'a düşer. Set storage bu dilimin parçası değildir. Ayrıntılar
[ADR 0077](adr/0077-comprehension-scopes.md) dosyasındadır.

Async comprehension genişletmesi clause başına sync/async iteration bilgisini
Tonic-owned AST'te taşır. İlk iterable enclosing scope'ta değerlendirilip `ITER`
veya `GET_AITER` ile hemen iterator'a çevrilir. Async clause'lar hidden scope'ta
`GET_ANEXT` + mevcut await state machine'i ve `END_ASYNC_FOR` exception sınırını
kullanır. Eager list/dict comprehension'ın hidden function'ı coroutine olur ve
çağıran expression sonucu otomatik await eder; async generator expression aynı
gövdeyi `ASYNC_YIELD` ile tembel async-generator nesnesi olarak döndürür. Yalnız
element/filter/later iterable içinde `await` bulunan comprehension'lar da aynı
coroutine sınıflandırmasını kullanır. Modül düzeyindeki async generator
expression için `GET_AITER` verifier'da coroutine dışı code object'te güvenlidir;
`GET_ANEXT`, await ve exhaustion işlemleri coroutine sınırında kalır. Ayrıntılar
[ADR 0080](adr/0080-async-comprehensions.md) dosyasındadır.

Bytecode v23 `SET` ve `SET_ADD` ile native set literal/comprehension üretir.
`Object::Set`, dict'in kanıtlanmış insertion-ordered hash malzemesi ve collision
bucket altyapısını paylaşır; değer slotları set için yalnız internal sentinel'dir.
Ekleme ve üyelik aynı suspending `__hash__`/`__eq__` continuation zincirinden
geçtiği için özel nesneler ile hash çakışmaları ikinci bir semantik yol yaratmaz.
Set eşitliği boyut guard'ından sonra karşı kümede hash-aware üyelik denetler;
iterator storage version'ı ile boyut değişimini yakalar. Elemanlar precise trace
edilir ve mutation tek write-barrier sınırından geçer. `SET`/`SET_ADD` Cranelift
destek kümesinin dışında doğrulanıp generic runtime'a düşer. Ayrıntılar
[ADR 0081](adr/0081-native-sets.md) dosyasındadır.

Bytecode v29 structural `match`/`case` lowering'ini Tonic-owned `Pattern`
ağacından doğrulanmış register kontrol akışına taşır. Capture'lar bütün desen
başarılı olana kadar geçici register'larda tutulur; OR kolları aynı binding
kümesini ortak destination'lara birleştirir ve guard yalnız tamamlanmış
binding'lerden sonra çalışır. Sequence guard'ı list/tuple/range ile bunların
native alt sınıf storage'ını materialize eder; string'i sequence saymaz.

Mapping ve class desenleri ikinci bir protokol motoru kurmaz. Mapping anahtarları
dict'in suspending hash/equality continuation'ını, `**rest` mevcut merge/delete
yollarını kullanır. Dinamik duplicate anahtar/ad denetimi aynı collision-aware
tablo üzerinden ve Python'ın kısa devre sırasıyla çalışır. Class guard'ı C3 MRO
instance kontrolünü; attribute çıkarımı descriptor/`__getattribute__`/`__getattr__`
durum makinesini kullanır. Fresh managed sentinel yalnız nihai
`AttributeError`'ı case başarısızlığına çevirir. Positional desenler doğrulanmış
`__match_args__` tuple'ını ve builtin self-pattern'ı destekler. Bütün geçici
container ve continuation değerleri precise GC edge'idir; opkodlar Cranelift
destek kümesi dışında açık generic runtime fallback'inde kalır. Ayrıntılar
[ADR 0082](adr/0082-structural-pattern-matching.md) dosyasındadır.

Bytecode v30 f-string parçalarını Tonic-owned `JoinedString` ve
`FormattedValue` AST düğümlerinden `CONVERT`/`FORMAT_VALUE` sınırlarına indirir.
Expression, conversion ve nested dynamic format-spec kaynak sırasıyla yalnız bir
kez değerlendirilir; sabit ve biçimlenmiş parçalar normal string `ADD` yolu ile
birleşir. `!s`, `!r` ve ASCII-escape üreten `!a`, kullanıcı
`__str__`/`__repr__` metodunu special-method lookup ile çağırır. Format aşaması
aynı biçimde kullanıcı `__format__` metodunu normal Tonic frame'inde askıya
alabilir ve dönüşün string olduğunu continuation tamamlanırken doğrular.

Yerleşik string/int/float format motoru Unicode code-point width/precision,
fill/alignment, sign, alternate form, zero padding, decimal ve radix grouping,
`b/o/d/x/X/c`, `e/E/f/F/g/G/%/n`, significant-digit rounding, normalize edilmiş
exponent ve negatif sıfır `z` semantiğini uygular. Format opkodları iki bytecode
verifier tarafından denetlenir ve Cranelift destek kümesinin dışında açık
generic-runtime fallback'inde kalır. Ayrıntılar
[ADR 0083](adr/0083-f-strings-and-formatting.md) dosyasındadır.

Fonksiyon parametre ve dönüş annotation'ları parser adapter'dan Tonic-owned
AST/HIR'e alınır ve defining scope içinde register değerlerine indirilir.
Function-site metadata yalnız `SymbolId` ve doğrulanmış register index'i taşır;
runtime bunlardan insertion-ordered, managed `__annotations__` dict'i kurar.
Annotation'sız fonksiyonun boş dict'i ilk erişime kadar ayrılmadığı için sıradan
fonksiyon tahsis bütçesi değişmez. Dict function object tarafından trace edilir;
Cranelift call ABI'si ve exact-callee guard'ları annotation depolamasından
bağımsız kalır. Ayrıntılar [ADR 0079](adr/0079-function-annotations.md)
dosyasındadır.

Değişken annotation'ları da parser-owned düğüm taşımadan Tonic AST/HIR'ine
alınır. Module ve class code prologue'u aynı lexical scope'ta basit isim
annotation'ı bulunduğunda managed `__annotations__` dict'i kurar; çalışma
anındaki annotation, normal `SET_ITEM` ve write-barrier yoluyla kaynak isim
anahtarına yazılır. Function-local annotation yalnız lexical binding oluşturur,
ifadeyi değerlendirmez; annotation-only read bu yüzden `UnboundLocalError`
üretir. Attribute/subscript annotation'ları metadata yazmaz, fakat target owner
ve key yan etkilerini Python sırasıyla bir kez çalıştırır. Cranelift yeni ABI ya
da opkod gerektirmeden doğrulanmış generic continuation'a düşer. Ayrıntılar
[ADR 0084](adr/0084-variable-annotations.md) dosyasındadır.

Python 3.12 type parameter ve `type` alias sözdizimi artık parser adapter'dan
Tonic-owned `TypeParam`/`TypeAlias` düğümlerine geçer. Bytecode v31
`TYPE_PARAM`/`TYPE_ALIAS`, doğrulanmış register metadata'sından managed TypeVar,
TypeVarTuple, ParamSpec ve alias nesneleri üretir. Generic function/class
gövdelerindeki aktif isimler frame kurulurken hidden local slotlara yazılır;
nested function veya method bunları kullandığında normal precise cell'e
dönüşür. Ordinary parameter/assignment aynı adı bütün scope'ta Python gibi
gölgeler. Function/class `__type_params__` tuple'ı ile alias `__name__`,
`__type_params__`, `__value__` yüzeyi managed edge'lerdir.

Builtin container, generic class ve generic alias subscription'ı origin/args
taşıyan managed generic-alias üretir; class alias çağrısı origin constructor'a,
generic base ise MRO kurulurken origin class'a gider. Type-parametreli code
direct-call inline planından çıkarılır, yeni construction opkodları Cranelift'in
explicit generic-runtime fallback'inde kalır. Ayrıntılar
[ADR 0085](adr/0085-type-parameters-and-aliases.md) dosyasındadır.

Bytecode v32, Python'ın `@` ve `@=` işlemlerini ayrı `MAT_MUL` ve
`INPLACE_MAT_MUL` opkodlarıyla taşır. Her üç operand verifier tarafından register
sınırında doğrulanır. Runtime mevcut suspending binary-protocol state machine'ini
`__matmul__`, `__rmatmul__` ve `__imatmul__` için paylaşır; strict subclass
reflected önceliği, `NotImplemented`, in-place fallback ve metaclass dispatch
aynı kesin GC kökleriyle korunur. Native bir matrix storage varsayılmadığı için
Cranelift bu opkodları açık generic-runtime fallback sınırında bırakır.

Class subscription, `ITEM` generic runtime yolunda Python'ın protokol sırasını
korur. Önce class nesnesinin metaclass'ındaki `__getitem__` descriptor'ı bağlanır;
bu yoksa class MRO'sundaki `__class_getitem__` plain function olsa bile subscribed
class'a örtük classmethod gibi bağlanır. Ancak iki hook da yoksa builtin, PEP 695
generic class ve type-alias için managed `GenericAlias` hızlı yolu kullanılır.
Kullanıcı hook'u frame askıya alabildiği için receiver/key normal VM register
kökleri olarak moving GC altında korunur; `ITEM` Cranelift'te generic fallback'te
kalır.

`divmod` ayrı bir sentetik bytecode opkodu üretmeden builtin çağrı yolunda
çalışır. Ortak suspending binary-protocol continuation'ı opcode tabanlı işlemler
ile `DivMod` işlemini ayıran kapalı bir kind taşır; böylece `__divmod__` ve
`__rdivmod__` strict-subclass sırası, `NotImplemented` ve metaclass dispatch için
mevcut kesin kök modelini paylaşır. Kullanıcı hook'u bulunmazsa native yol önce
`FloorDiv`, sonra `Mod` semantiğini çalıştırıp iki sonucu managed tuple'a koyar.
Builtin çağrı JIT içinden geldiğinde exact caller PC'sinde generic VM yoluna
çıkar; dönüşte aynı native caller'a güvenle devam eder.

`round` da bytecode yüzeyini büyütmeden versioned builtin çağrı sınırında kalır.
Global binder `number`/`ndigits` positional ve keyword biçimlerini tek kez bağlar;
kullanıcı veya metaclass `__round__` normal VM frame'inde askıya alınabilir.
Native int/float ve bunların alt sınıflarında canonical `__round__` descriptor'ı
aynı motoru kullanır; native `ndigits` dönüşümü mevcut suspending `__index__`
continuation'ına `number` değerini kesin kök olarak ekler.

Float yuvarlama host `round()` fonksiyonuna veya ikili kayan noktalı `x*10^n`
yaklaşımına dayanmaz. IEEE-754 değeri exact BigInt oranına açılır, decimal ölçek
üzerinde quotient/remainder ile ties-to-even seçilir ve mevcut correctly-rounded
ratio→binary64 yordamıyla tekrar f64'e çevrilir. Bu yol `2.675`, subnormal,
signed-zero ve overflow sınırlarını aynı kuralla taşır; bigint negatif basamaklar
da aynı exact ratio yuvarlayıcısını paylaşır. JIT caller builtin çağrıda exact
PC'den generic VM'e çıkar ve dönüşte native yürütmeye devam eder.

Global `pow` iki argümanda normal `**` protokolünü paylaşır; açık `None` üçüncü
argümanı da Python gibi bu yola indirger. Üçüncü argüman verildiğinde ortak
binary continuation her aday için iki positional değer taşıyarak
`__pow__(exp, mod)` ve `__rpow__(base, mod)` çağrılarını askıya alabilir. Python
3.14'ün ternary reflected dispatch'i, strict-subclass önceliği, aynı sınıfta
reflected çağrıyı tekrarlamama, `NotImplemented` fallback'i ve metaclass
dispatch'i korunur. Continuation base, exponent, modulus, kalan descriptor ve
aday argümanlarını ayrı logical `Value` kökleri olarak izler.

Native üç argümanlı yol yalnız int/bool ve native int alt sınıfı backing'lerini
kabul eder. `num-bigint` modüler üs algoritması ara tam kuvveti üretmez; negatif
üs önce modüler ters alır ve terslenemeyen tabanı `ValueError` ile reddeder.
Sonuç işareti modulus'u izler, sıfır modulus reddedilir ve modulus `±1` doğrudan
sıfır döndürür. Yeni bytecode veya JIT ABI'si gerekmez: JIT caller builtin
çağrısında exact PC'den generic VM'e çıkar ve aynı native frame'e döner. Karar
[ADR 0086](adr/0086-pow-builtin-protocol.md) dosyasındadır.

`repr`, `ascii` ve `format` builtin'leri f-string için zaten gereken conversion
motorunu ayrı bir ikinci uygulama oluşturmadan kullanır. `repr` ve `ascii`
`__repr__` descriptor'ını, `format` ise `__format__(spec)` çağrısını normal guest
frame'inde askıya alabilir; dönüşler exact/native string backing üzerinden
doğrulanıp yeni managed string olarak yayımlanır. `ascii`, descriptor sonucuna
veya native fallback temsiline aynı Unicode escape dönüşümünü uygular.

Class değerlerinde bu protokoller instance sınıfı yerine metaclass üzerinden
aranır. Ortak alt yordam f-string `!s`/`!r`/`!a` ve `FORMAT_VALUE` yoluna da
bağlandığı için builtin ile interpolation arasında dispatch farkı kalmaz. Bütün
argümanlar caller veya callee register'larında logical `Value` kökü olarak
kalır; JIT yeni opcode ya da ABI olmadan mevcut generic call side-exit'ini
kullanır. Karar [ADR 0087](adr/0087-repr-ascii-format-builtins.md) dosyasındadır.

İki argümanlı `iter(callable, sentinel)` ayrı managed `callable_iterator`
nesnesidir. Callable, sentinel ve exhaustion biti heap nesnesinde kalır;
`__iter__` kimliği korur, `__next__` ise mevcut native-callback reentry sınırında
yalnız bir callable çağrısını ve sentinel-sol rich equality/truth zincirini
tamamlar. Böylece user frame, JIT caller ve moving GC desteklenirken her öğede
Rust stack büyümez. Callable veya eş sentinel kalıcı exhaustion üretir; equality
hook hatası iterator'ı tüketmez. Karar ve kök invariants
[ADR 0091](adr/0091-callable-sentinel-iterator.md) dosyasındadır.

`__iter__` bulunmayan fakat `__getitem__` sağlayan nesneler managed
`SequenceIterator` üzerinden sıfırdan başlayan ardışık integer indekslerle
tüketilir. Açık `__iter__` (çağrılamayan `None` dahil) fallback'i engeller;
`IndexError`/`StopIteration` kalıcı tükenmeye dönüşür, diğer hatalar indeksi
ilerletmeden yayılır. Instance veya metaclass `__getitem__` her adımda yeniden
çözülür. Bounded guest reentry sırasında iterator kadar tüketicinin pending
continuation durumu da köklenir; böylece kısmi dict/koleksiyon ve generic toplam
moving GC altında korunur. Karar [ADR 0092](adr/0092-sequence-iterator-fallback.md)
dosyasındadır.

## Uygulama sırası ve kabul kapıları

| Aşama | Kabul koşulu |
|---|---|
| M0–M2 / native ilk dilim | fib, scope/loop/call hataları, CLI, verifier, Rust handle testleri, Python differential ve interpreter baseline |
| M3 nesne modeli | sınıf/instance/shape, dict, descriptor/MRO semantiği, trace ziyaretleri |
| M4 specialization | genel yol ile eş sonuç, guard failure, istikrarsız site de-specialization; önce/sonra benchmark |
| M5 GC | explicit roots, cycle, old→young barrier, hareket, stale handle, stress collection |
| M6–M7 JIT | interpreter eşdeğerliği, safepoints, exception ve guard deopt; compile time/code size ölçümleri |
| Geniş sözdizimi | conformance korpusu, comprehension scope, closure, generator/async, match |
| Native C ABI | version/capability, init/exception protokolü, panic sınırı, trusted-code belgesi |
| Buffer/callback/foreign | zero-copy owner ömrü, thread attach, shutdown, tam bir kez destructor |
| CPython bridge | proxy/wrapper, primitive conversions, identity cache, cycle/finalization politikası |
| HPy Universal/aHPy | `.hpy0` loader, context/handle API, field/global moving-GC, pure types, Debug/Trace ve pinned aHPy pilotları |

## Belge kapsam haritası

AGENTS §§0–3 ürün/yürütme; §§4–7 VM/değer/nesne; §§8–9 GC/çağrı;
§§10–16 derleyici/JIT/exception/iteration/builtin/global/symbol;
§§17–20 coroutine/FFI/buffer/concurrency; §§21–27 safety/verification/tests;
§§28–38 ölçüm, tanı, REPL, cache, import ve uyumluluk; §§39–57 aşamalar,
bootstrap, ADR, invariants ve tamamlanma koşulları olarak değerlendirilmiştir.

Interop §§0–16 Value/Handle/scope/API; §§17–31 foreign/buffer/call/exception/
thread/ownership; §§32–48 CPython proxy, identity, cycles, loading, tracing ve
finalization; §§49–63 JIT sınırları, cache/adapter, buffer ve güvenlik;
§§64–79 version/capability/weak-handle/shutdown/debug;
§§80–94 ölçüm ve güvenlik kuralları; §§95–100 teslimat sırası ve örnekleri
olarak değerlendirilmiştir.
