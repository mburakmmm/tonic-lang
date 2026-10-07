# Annotation destekli kısmi statik JIT planı

## Amaç

Tonic, normal Python annotation'larını kullanan kodu değiştirmeden kabul eder.
Annotation'ı olan ve çözümlenebilen fonksiyonlar, yalnız çalışma zamanı profili
bekleyen tier'ın önüne geçen bir typed baseline JIT adayı olur. Hedef, sayısal
döngüleri, çağrı zincirlerini, sabit şekilli sınıfları ve typed buffer işlerini
unboxed çalıştırmaktır. Kaynak hâlâ Python gibi davranır: standart `int`, `float`,
`list[int]` veya kullanıcı sınıfı annotation'ı runtime doğrulama zorunluluğu
getirmez. Varsayım tutmazsa kod aynı bytecode PC'sinde generic yürütmeye döner.

Bu özellik Cython'ın hız hedefinden esinlenir, ancak ayrı bir C object modeline,
CPython `PyObject*` yerleşimine veya zorunlu ahead-of-time build adımına dayanmaz.
Tonic'in moving GC'si, opaque `Value`/handle ABI'si, Python protokolleri ve
interpreter doğruluk yolu korunur.

## Ürün sözleşmesi: annotation isteğe bağlıdır

Tonic'te annotation kullanmak zorunlu değildir. Aynı dil üç yürütme derinliğini
tek Python sözdizimi içinde sunar:

1. Annotation'sız kod normal adaptive dinamik tier'larda çalışır.
2. Standart Python annotation'ı bulunan ve kanıtlanabilen kod profil sıcaklığını
   beklemeden guarded typed-JIT'e yönlendirilebilir. Annotation bir runtime type
   check değildir; uyuşmayan değer generic Python semantiğine geri döner.
3. Sabit genişlik, layout veya overflow garantisi isteyen kod yalnız açık Tonic
   strict türlerini kullanır. Bu sözleşme standart `int`/`float` annotation'ına
   sessizce yüklenmez.

Bu nedenle hedef yalnız annotated leaf fonksiyonlarını hızlandırmak değildir.
Annotation sınırından başlayan kısmi statik analiz local/closure akışına, çağrı
grafiğine, sınıf shape/field bilgisine, homogeneous container ve buffer'lara
yayılır. Kanıtın bittiği her noktada materialized `Value`, version guard ve exact
bytecode-PC fallback korunur. Bounded specialization/code budget kod patlamasını;
effect, alias ve escape bilgisi de yanlış unboxing veya scalar replacement'ı
önler. Kullanıcı `inspect/explain` yüzeyinden bir fonksiyonun neden typed-JIT'e
girdiğini, generic kaldığını veya deopt ettiğini görebilmelidir.

## İki davranış kipi

### Advisory Python kipi

Standart Python annotation'ları optimizasyon ipucudur. Örneğin:

```python
def dot(left: list[float], right: list[float]) -> float:
    total: float = 0.0
    for index in range(len(left)):
        total += left[index] * right[index]
    return total
```

JIT iki exact Tonic listesi, float-compatible storage, eş uzunluk ve değişmeyen
container/version koşullarını guard eder. Bir caller farklı değer gönderirse
`TypeError` eklemez; generic Python semantiğine deopt eder. `__annotations__`
mutasyonu da observable sonucu değiştirmese bile gelecekteki specialization
seçimini invalidate eder.

### Tonic strict değer kipi

C-benzeri representation ve overflow davranışı yalnız açık Tonic türleriyle
seçilir. Planlanan yüzey `tonic.types.i8..i64`, `u8..u64`, `f32/f64`, typed
buffer ve packed struct türleridir. Bu türler için range, layout, mutability,
checked/wrapping policy ve FFI ABI belgelenir. Standart Python `int` hiçbir zaman
sessizce sabit genişlikli integer'a dönüştürülmez. Strict kip için decorator veya
module policy normal Python grammar'ı içinde kalır; yeni sözdizimi gerekmez.

## Derleyici hattı

```text
evaluated annotation values
        |
        v
canonical TypePlan + dependency versions
        |
        v
verified bytecode + typed data-flow overlay
        |
        v
Cranelift typed baseline / optimized entry
        |
        +-- guards succeed --> unboxed machine path
        |
        +-- guard/helper/overflow --> exact-PC deopt --> generic VM
```

`TypePlan`, runtime annotation nesnesinden bağımsız, Tonic-owned ve sürümlü bir
temsildir. Başlangıç lattice'i exact `bool/int/float/str/bytes`, `None`,
union/optional/literal, Tonic class identity/shape, fixed veya homogeneous tuple,
homogeneous list/dict/set, callable signature ve buffer dtype/shape içerir.
`Any`, çözülmeyen forward reference, arbitrary metaclass/subscription sonucu veya
kullanıcı tarafından değiştirilebilir bilinmeyen annotation optimize edilmez.
Reddetme sessiz bir yanlış derleme yerine sorgulanabilir neden kodu üretir.

Typed analiz verified bytecode'un üstünde ayrı bir overlay'dir. Register giriş ve
çıkış türleri, branch birleşimleri, loop phi'ları, locals, return ve direct-call
sonuçları hesaplanır. Bytecode formatı ve generic interpreter bu metadata olmadan
da eksiksiz çalışır. Böylece type inference hatası veya desteklenmeyen opcode
runtime semantiğini değiştirmez.

## Guard, invalidation ve deoptimization

Her native giriş en az şunları bağlar:

- exact function/code identity ve execution id;
- canonical annotation-plan hash'i ve annotation dictionary version'ı;
- argüman tag/type/class/shape ve gereken container storage/version bilgisi;
- kullanılan global/module/class/MRO dependency version'ları;
- strict kipte overflow/layout policy kimliği.

Guard failure bütün interpreter-visible register'ları, bytecode PC'sini ve canlı
managed referansları mevcut deopt stack-map formatına yazar. Python `int`
makine-word aralığını aşarsa bigint sonucu generic yolda yeniden hesaplanır veya
semantik olarak güvenliyse helper tarafından materialize edilir. Allocation,
call ve loop backedge'leri safepoint'tir. Unboxed değerler stack map'te türüyle
yer alır; managed değer gibi taranmaz.

## Tier seçimi ve cache

Annotation tek başına “her şeyi import sırasında derle” anlamına gelmez.
Desteklenen planı olan fonksiyon first call'da typed baseline'a girebilir; küçük
veya kârsız fonksiyonlar ölçümlü cost model ile adaptive tier'da kalabilir.
`@tonic.compile` veya module policy, deploy/startup senaryoları için import-time
warmup ister. Hata halinde import başarısız olmaz; generic giriş korunur ve
sebep diagnostics/statistics yüzeyinde görünür.

Disk cache anahtarı source/bytecode hash, canonical `TypePlan`, dependency
versions, Tonic/bytecode/Cranelift sürümü, target triple ve gerekli CPU
özelliklerini içerir. Yükleme verifier'dan geçer. Rust struct layout'u, native
heap adresi, handle-table slot adresi veya doğrulanmamış makine kodu kalıcı format
sayılmaz.

## Teslim sırası

1. TypePlan ve annotation version/invalidation modeli.
2. Parametre/dönüş guard'lı `int` ve `float` typed leaf baseline.
3. Typed local data-flow, branches, loops, arbitrary-PC deopt ve safepoints.
4. Direct annotated call graph, recursion ve bounded inlining.
5. Homogeneous containers, buffer/dtype ve alias/escape materialization.
6. Annotated class fields, fixed shape planı ve dinamik mutation fallback'i.
7. Strict Tonic scalar/layout türleri ve native ABI entegrasyonu.
8. Sürümlü AOT cache, diagnostics, explain/inspect araçları.
9. Differential, fuzz, sanitizer ve platform matrisi.
10. Yalnız bütün roadmap/coverage kapıları kapandıktan sonra CPython/Cython ile
    nihai benchmark ve bottleneck profili.

## Uygulama durumu

TypePlan v2 temeli tamamlandı. Runtime'da değerlendirilen annotation değerleri
exact `None`/`NoneType`/`bool`/`int`/`float`/`str`, plain container, homogeneous
`list`/`dict`/`set`, fixed tuple ve kullanıcı class identity/version planlarına
dönüşür. PEP 604 `A | B` ve `T | None` union nesneleri düzleştirilir, eşdeğer
üyeler elenir ve plan üyeleri sıralanarak yazım sırasından bağımsız canonical
`Union` planı oluşturulur. User-class üyeleri normal class version dependency'si
taşır. Planlar isim sırasına göre canonical hale getirilip açık schema sürümü ve
sabit FNV-1a kodlamasıyla hash'lenir. Desteklenmeyen değer, generic origin, arity,
type parameter, recursive alias ve recursion limit ayrı reason code üretir.

Fonksiyonun `__annotations__` dict'i için structural iterator epoch'undan ayrı
bir content-mutation epoch vardır. Var olan anahtarın değerini değiştirmek de bu
epoch'u artırır; function içindeki plan cache yalnız epoch eşleşiyorsa kullanılır.
TypePlan temeli tek başına typed native entry seçmez. Union narrowing/branch
refinement, `typing.Literal`/`Callable`, variadic tuple, bytes ve buffer/dtype
sonraki genişleme dilimleridir. PEP 604 runtime nesnesi; `types.UnionType`,
GenericAlias üyeleri, sıra-bağımsız equality/hash, `__args__/__origin__`,
`isinstance`/`issubclass`, GC tracing ve metaclass operator önceliğiyle birlikte
uygulanmıştır. Karar [ADR 0115](adr/0115-pep604-union-type-plan.md) içindedir.
Temel kararın ayrıntısı [ADR 0104](adr/0104-type-plan-v1.md) içindedir.

İlk typed giriş dilimi de tamamlandı. Bütün parametreleri ve dönüşü exact
`int`/`float` planına çözümlenen, mevcut Cranelift subset'ine uygun fonksiyonlar
sıcaklık profili ve küçük-leaf kârlılık eşiğini beklemeden ilk çağrıda derlenir.
Giriş seçimi exact function handle, code id, execution id, annotation dict
identity/content epoch, canonical plan hash ve gerçek argüman türlerini guard
eder. Dönüş annotation'ı da native dönüş kabul edilmeden kontrol edilir; uyuşmazlık
`TypeError` üretmez, `RETURN` bytecode PC'sine deopt edip interpreter sonucunu
korur.

`function.__annotations__` dict veya `None` ile değiştirilebilir ve silinebilir.
Content mutasyonu, replacement/delete ve annotation planındaki class version
dependency değişimi cache'i yeniler; canlı compiled giriş eski planı görürse o
çağrıyı generic çalıştırır ve sonraki uygun çağrıda yeniden derler. Yanlış tipte
tek çağrı compiled girişi bozmaz. Eager annotation semantiğinde sonradan global
alias ismini rebind etmek daha önce değerlendirilmiş annotation nesnesini
değiştirmediği için invalidation sebebi değildir. Ayrıntı
[ADR 0105](adr/0105-annotation-jit-entry-guards.md) içindedir.

Bu dilim henüz genel typed SSA overlay, unboxed integer register/çağrı ABI'si,
typed container, decorator/import-time warmup veya disk cache değildir. Float
işlemleri mevcut F64 data-flow/stack-map yolunu kullanır; integer işlem guard'ları
taşmada mevcut BigInt deopt yoluna döner.

Verified-bytecode typed overlay'in exact-small-int dilimi de uygulanmıştır.
Parametre, immediate integer sabiti, `Move`, integer unary/binary sonucu, branch
merge ve loop fixed-point bilgisi her bytecode PC'si için hesaplanır. Cranelift
normal girişte ve side-exit/OSR sonrası arbitrary-PC girişte o PC'de canlı olduğu
ispatlanan integer register'larını tag-guard eder. Bu tek giriş doğrulamasından
sonra aritmetik ve karşılaştırma noktalarındaki yinelenen tag guard'ları üretilmez;
overflow, sıfıra bölme veya bozulmuş resume state yine tam PC'de deopt eder.
Elision sayısı JIT metadata'sı ve VM istatistiklerinde yayımlanır. Karar ayrıntısı
[ADR 0106](adr/0106-typed-int-dataflow-overlay.md) içindedir.

Scalar overlay artık ortak `Unknown/Int/Float/Bool` lattice'i kullanır. Exact
`bool` annotation'lı fonksiyonlar da first-call typed girişe adaydır; parametre,
bool sabiti, `Move`, `not`, scalar karşılaştırma ve branch sonuçları taşınır.
Arbitrary-PC giriş exact `bool` değerini guard eder. `bool` aritmetiği Python'daki
`bool <: int` davranışını koruyarak `False=0`, `True=1` olarak lower edilir ve
sonucu tagged `int` olur. Bu genişletmenin kararı
[ADR 0107](adr/0107-typed-scalar-bool-lattice.md) içindedir.

Immediate `None` fact'i de typed imzaya katılmıştır. Kaynak annotation'ındaki
`None` ve exact `NoneType` aynı singleton fact'ine çözülür; normal girişte ve
arbitrary-PC resume'da opaque value word doğrudan `VALUE_NONE` ile karşılaştırılır.
`Const None` ve `Move` fact'i taşır, bütün erişilebilir dönüşler kanıtlandığında
host return guard kaldırılır. Yanlış argüman annotation'ı zorunlu type check'e
dönüştürmeden generic çalışır. Karar
[ADR 0114](adr/0114-typed-none-fact.md) içindedir.

Canonical `int | None` ve `bool | None` planları da immediate tag-set typed
imzalarına lower edilir. Native giriş hem değer tag'ini hem `None` singleton'ını
kabul eder; başka bir tür annotation'ı zorunlu kontrole çevirmeden generic yola
gider. Control-flow birleşimi `int`/`None` ile `bool`/`None` yollarını kaybetmeden
optional fact'e yükseltir ve bütün erişilebilir dönüşlerin union üyesi olduğu
kanıtlanırsa host return guard kaldırılır. Optional fact aritmetik için exact
`int`/`bool` sayılmaz. `x is None`, `x is not None` ve operandları ters yazılmış
biçimler branch'in true/false kenarlarında optional fact'i singleton `None` ile
exact scalar'a ayırır. Derleyicinin ürettiği saf `Move` zinciri kaynağa kadar
izlenir; kopyadan sonra yeniden yazılmış bir kaynak stale alias olarak daraltılmaz.
Genel union üye/test narrowing'i açık kalır. Immediate union kararı
[ADR 0116](adr/0116-optional-immediate-union-jit.md), identity refinement ayrıntısı
[ADR 0117](adr/0117-optional-none-branch-refinement.md) içindedir.

None-dışı ilk union genişlemesi `int | bool` imzasıdır. Native giriş exact int
ve exact bool tag'lerini kabul eder; bool üye Cranelift'te Python'ın `bool <: int`
davranışına uygun olarak `False=0`, `True=1` decode edilir. Bu nedenle iki üye
aynı integer arithmetic lowering'ini kullanabilir. Branch join'inde ayrı `int`
ve `bool` fact'leri canonical `IntOrBool` fact'ine yükselir; result planı her iki
üyeyi de kabul eder. Üye olmayan değer yine generic Python yolunda çalışır.
Karar [ADR 0118](adr/0118-int-bool-union-jit.md) içindedir.

Overlay henüz raw `i64` register/çağrı ABI'si değildir: integer ve boolean
değerler precise root buffer'da tagged `Value` olarak kalır ve işlem sınırında
decode/encode edilir. F64 lowering mevcut ayrı stack-slot/stack-map mekanizmasını
kullanır.

Dönüş propagation'ı bütün erişilebilir `RETURN` noktalarını signature result
planıyla karşılaştırır. Her yol kanıtlandığında bu gerçek public JIT metadata'sına
yazılır ve VM, native dönüşte aynı type guard'ını ikinci kez çalıştırmaz. Tek bir
unknown/uyuşmayan yol kanıtı düşürür; host return guard ve exact-PC advisory deopt
korunur. Bu karar [ADR 0108](adr/0108-typed-return-proof.md) içindedir. Annotated
call-result propagation'ın ilk exact-global dilimi aşağıda tanımlıdır; recursive/
method özetleri ve kullanıcıya açık explain/rejection metadata'sı açık kalır.

İlk call-result dilimi exact global Tonic function leaf'lerini kapsar. Caller ilk
çağrıda derlenirken callee'nin scalar parametre/result planı çözülür; public JIT
proof kapısı callee bytecode'undaki bütün erişilebilir dönüşlerin bu result ile
uyuştuğunu ayrıca kanıtlar. Ancak bundan sonra `CALL` sonucu caller lattice'ine
aktarılır ve leaf doğrudan inline edilir. Caller guard'ı callee function/code/
execution/annotation object/version/hash bağımlılığını taşır; annotation mutasyonu
caller'ı invalid eder, global rebinding ise generated exact-callee guard'ında
deopt eder. Yalan annotation veya kanıtlanamayan gövde özet üretmez. Ayrıntı
[ADR 0109](adr/0109-guarded-annotated-call-result.md) içindedir.

Bu exact-global leaf yolu artık exact `bool`/`None` ile `int | None`,
`bool | None` ve `int | bool` sonuçlarını da caller overlay'ine aktarır. Callee
yalnız atomik deopt halinde yeniden yürütülebilen side-effect-free direct-inline
subset'indeyse özet kabul edilir. Optional sonuç caller'ın `is None` kenarında
daralabilir; int-bool sonuç aynı çağrıda integer arithmetic'e girebilir. Callee
annotation planı değişirse dependency guard caller entry'yi invalid eder ve yeni
plan body proof vermiyorsa union kenarı yeniden kurulmaz. Karar
[ADR 0119](adr/0119-guarded-immediate-union-call-results.md) içindedir.

Exact global self-recursion için sonuç özeti leaf inlining gerektirmez. Recursive
`CALL` normal VM side-exit yolunda çalışır; child döndüğünde caller JIT tam
successor PC'den yeniden girer ve annotated immediate scalar sonucu orada guard
eder. Guard geçen sonuç native local/return akışına katılır. Yanlış return
annotation'ı veya global rebinding, denetlenmemiş bir varsayım oluşturmaz: re-entry
guard'ı exact PC'de generic yürütmeye döner. Bu ilk recursive kenar yalnız
self-recursion içindir; karşılıklı recursion/SCC compilation unit'i açık iştir.
Karar [ADR 0120](adr/0120-guarded-self-recursive-call-results.md) içindedir.

İkinci kenar `Math.leaf(...)` biçiminde, `Math` exact global class olduğunda ve
attribute plain class-level function ya da `staticmethod` olarak çözüldüğünde
çalışır. Mevcut allocation-free `ATTR`+`CALL` fusion kullanılır fakat method
profili beklenmez: callee annotation planı ve bytecode return proof caller'ın ilk
derlemesinde result fact'i kurar. Generated code güncel owner'ı tekrar yükler;
static binding kind ve exact function identity'yi `ATTR` PC'sinde guard eder.
Callee annotation mutasyonu caller entry'sini invalid eder; class attribute
rebinding atomik deopt ile normal descriptor semantiğini tekrarlar. Yalan return
annotation'ı kenar üretmez. Karar
[ADR 0111](adr/0111-annotated-class-function-edge.md) içindedir.

Exact global instance method ve exact global class `classmethod` kenarları da
aynı first-call yola katılır. Public typed signature receiver slotunu
`Dynamic` olarak işaretleyebilir: değer materialized guest `Value` olarak kalır,
scalar guard/elision üretmez ve typed dönüş olarak kabul edilmez. `self`/`cls`
annotation'ı gerekmeksizin kalan parametreler ile dönüş exact scalar planından
gelir; return proof receiver'ı `Unknown` fact olarak taşır. Generated method
helper instance/class binding türünü ve exact function kimliğini korur. Instance
attribute shadowing, class rebinding ve callee annotation mutasyonu sırasıyla
exact-PC deopt veya caller invalidation üretir. Karar
[ADR 0112](adr/0112-opaque-receiver-call-summary.md) içindedir.

Exact kullanıcı-sınıfı annotation'lı caller parametresi de instance-method
kenarına kaynak olabilir. Native entry exact runtime class identity ve annotation
planındaki class type/version değerini host tarafında guard eder; Cranelift bu
register'ı scalar sanmadan materialized `Value` olarak taşır. `ATTR` owner'ı
yalnız doğrulanabilir saf `Move` zincirinden parametreye bağlanır. Generated
method helper binding türünü ve exact function'ı tekrar guard eder. Yanlış sınıf
ilk çağrı derlemesini kullanmaz, instance shadow exact `ATTR` PC'sine deopt eder,
class mutation annotation dependency üzerinden caller'ı invalid eder. Karar
[ADR 0113](adr/0113-annotated-class-parameter-edge.md) içindedir.

Annotated field/shape propagation, recursive/SCC özetleri, ayrı compilation
unit ve graph-wide code-size bütçesi hâlâ açıktır.

JIT reddi sessiz bir `Unsupported` biti değildir. Runtime her code object için
son kalıcı ret kararını function/code kimliği, `unprofitable`/`code-budget`/
`unsupported-bytecode`/`unstable-guards` kategorisi, varsa bytecode PC/opcode ve
deterministic neden ile saklar. Embedder bu kayıtlara public accessor ile ulaşır;
CLI `--stats` her kaydı makinece ayrıştırılabilir tek satır halinde yazar. Karar
[ADR 0110](adr/0110-jit-rejection-diagnostics.md) içindedir.

## Kabul ölçütleri

- Advisory kipte annotation eklemek veya kaldırmak program çıktısını ve exception
  türünü değiştirmez; yalnız tier/statistics değişebilir.
- Yanlış argüman, annotation mutation/replacement, class rebinding, container
  widening ve integer overflow exact-PC deopt ile interpreter sonucunu verir.
- Hot typed numeric loop, iteration başına boxing/allocation yapmaz.
- Typed call zinciri geçici argument tuple/dict ve bound-method ayırmaz.
- Moving ve her-allocation GC altında native stack map tamdır; stale handle veya
  kayıp root yoktur.
- Compile failure, code-budget aşımı ve cache reddi deterministic tanı/sayaçla
  generic yürütmeye döner.
- Interpreter, adaptive tier, profile-JIT ve annotation-JIT aynı differential
  corpus'u debug/release ve desteklenen iki mimaride geçirir.
- Performans iddiası ancak compile latency, warm throughput, allocation, peak
  RSS, code size, deopt oranı ve profiler çıktısı CPython/Cython oracle'larıyla
  aynı girdide raporlandıktan sonra yapılır.
