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
exception constructor ve traceback doğrulamasıyla desteklenir. Bu dilim tam
coroutine aşaması değildir; ulaşılamayan generator finalization'ı ve
`async`/`await`/coroutine state machine açık kalır. Ayrıntı
[ADR 0072](adr/0072-generator-frame-state-machine.md) dosyasındadır.

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
