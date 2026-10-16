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
`@tonic.compile` fonksiyon identity'sini koruyan açık bir first-call compile
isteğidir. Hotness ve küçük-fonksiyon profitability bekleyişini atlar; mevcut
annotation TypePlan varsa typed guard'ları aynen kullanır, yoksa desteklenen
generic/profiled JIT subset'ini dener. Yanlış annotation girdisi veya unsupported
bytecode program semantiğini değiştirmez: generic giriş korunur. Global native
code budget yine zorunludur. `jit_explicit_candidates`,
`jit_explicit_compiled` ve `jit_explicit_rejections` sayaçları sonucu ayırır;
karar [ADR 0140](adr/0140-explicit-first-call-jit-decorator.md) içindedir.
Modül sonunda çağrılan `tonic.warmup()` o modülde o ana kadar tanımlanmış exact
Tonic fonksiyonlarını deterministic symbol sırasıyla seçer;
`tonic.warmup(f, g)` seçimi daraltır. Tam destekli annotation guard'ı bulunan
fonksiyonlar import sırasında, gerçek argüman uydurmadan typed baseline'a
derlenir. Dinamik fonksiyonlar identity-preserving explicit marker alıp ilk
gerçek çağrıya ertelenir. Her warmup isteği varsayılan 16 MiB ve execution
geneli 64 MiB native-code bütçesinin ikisine birden uyar. Unsupported/backend/
budget reddi import semantiğini bozmaz; generic yol ve deterministic tanı
korunur. Ayrı warmup sayaçları kararları görünür kılar. Karar
[ADR 0141](adr/0141-module-import-jit-warmup.md) içindedir.

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

TypePlan v8 temeli tamamlandı. Runtime'da değerlendirilen annotation değerleri
exact `None`/`NoneType`/`bool`/`int`/`float`/`str`/`bytes`, plain container, homogeneous
`list`/`dict`/`set`, fixed/variadic tuple, `typing.Literal`, `typing.Callable` ve
kullanıcı class identity/version planlarına; native `fastmath.Buffer` ise exact
`f64` dtype, unconstrained rank ve unconstrained mutability taşıyan buffer planına
dönüşür. PEP 604 `A | B` ve `T | None` union nesneleri düzleştirilir, eşdeğer
üyeler elenir ve plan üyeleri sıralanarak yazım sırasından bağımsız canonical
`Union` planı oluşturulur. User-class üyeleri normal class version dependency'si
taşır. Planlar isim sırasına göre canonical hale getirilip açık schema sürümü ve
sabit FNV-1a kodlamasıyla hash'lenir. Desteklenmeyen değer, generic origin, arity,
type parameter, recursive alias ve recursion limit ayrı reason code üretir.

Fonksiyonun `__annotations__` dict'i için structural iterator epoch'undan ayrı
bir content-mutation epoch vardır. Var olan anahtarın değerini değiştirmek de bu
epoch'u artırır; function içindeki plan cache yalnız epoch eşleşiyorsa kullanılır.
`typing.Literal` iç içe kullanımları düzleştirir, aynı tipte tekrarları eler ve
`1` ile `True` gibi Python eşitliği olan farklı literal tiplerini ayrı tutar.
`None`/bool/int birleşimleri güvenli scalar kategoriye indirilerek typed JIT'e
erken aday olabilir; çalışma zamanı guard'ı kaldığı için annotation zorlayıcı
tip kontrolü değildir. Str/Ellipsis veya desteklenmeyen değer taşıyan Literal
planı generic yürütmede kalır. Karar [ADR 0123](adr/0123-literal-type-plan.md)
içindedir.

Canonical `int | bool | None` planı da tek-word immediate typed-JIT imzasına
çevrilir. Giriş ve arbitrary-PC guard'ı üç üyeyi doğrular; `value is None` veya
ters `is not` dalı non-None kenarında `int | bool` fact'i üretir. Böylece Python'ın
`bool <: int` sayısal davranışı korunarak toplama gibi integer lowering'leri tag
guard tekrarına gerek kalmadan çalışır. Yanlış annotation girdisi normal generic
semantiğe döner; karar [ADR 0143](adr/0143-three-way-immediate-union-jit.md)
içindedir.

Exact builtin `isinstance(value, bool)` ve `isinstance(value, int)` çağrıları da
immediate union branch'lerini daraltabilir. Çağrının kendisi generic VM sınırında
Python semantiğiyle çalışır; successor PC'deki native resume girişi, materialize
edilmiş callee ile type-object logical handle'larını derleme anındaki exact
değerlerle karşılaştırmadan sonucu typed fact olarak kullanmaz. `isinstance`
veya type global'i yeniden bağlanırsa hesaplanmış bool sonuç korunur, JIT aynı
successor PC'de deopt eder ve branch generic yürür. `int` testi Python'ın
`bool <: int` ilişkisini korur. Karar
[ADR 0144](adr/0144-guarded-isinstance-union-narrowing.md) içindedir.

Heap-backed union altyapısının ilk dilimi `float | None` için tamamlandı. Normal
ve arbitrary-PC girişler logical value'yu `None` veya exact Tonic float olarak
guard eder. `is None` kontrolünün non-None CFG kenarı boxed root'u F64 stack
slotuna unbox eder; sonraki toplama/çıkarma/çarpma ve dönüş mevcut deopt map'leriyle
native kalır. Float fact'inin boxed union birleşiminde kaybolacağı control-flow
şekilleri materialization eklenene kadar bu optimizasyondan güvenle çıkarılır.
Exact builtin `isinstance(value, float)` de aynı union'ı `Float`/`None`
kenarlarına ayırır. Resume girişi callee ve type-object logical handle'larını
guard ettikten sonra true edge mevcut F64 materialization yolunu kullanır;
`isinstance` veya `float` yeniden bağlanırsa successor PC'de generic yürütmeye
döner.
Annotated direct callee'nin `float | None` sonucu da generic CALL side-exit'inden
sonra successor PC'de exact union guard'ından geçer. `None` logical value olarak
kalır; float dalı branch edge'de F64 slota alınır. Callee annotation'ı yanlışsa
ve gerçek sonuç union dışındaysa guard aynı successor PC'de deopt eder ve caller
normal Python değeriyle generic devam eder.
Karar [ADR 0145](adr/0145-optional-float-union-jit.md) içindedir.

Karma numeric union'ın ilk dilimi `int | float` için tamamlandı. Giriş ve
arbitrary-PC resume guard'ı ya immediate small-int tag'i ya exact Tonic float
handle'ı kabul eder. Exact builtin `isinstance(value, float)` true edge'i F64,
false edge'i i64; `isinstance(value, int)` ise tersini üretir. Her dal kendi
native arithmetic ve dönüş yolunu kullanır. Guarded direct-call sonucu da aynı
union fact'ini caller'a taşıyabilir; union dışı gerçek sonuç successor PC'de
generic yola döner. Float ve int bilgisinin tek register'da tekrar birleştiği
akışlar açık materialization eklenene kadar mevcut güvenlik reddini korur.
Karar [ADR 0146](adr/0146-int-float-union-jit.md) içindedir.

Optional karma numeric union'ın ilk tam branch zinciri `int | float | None`
için tamamlandı. Giriş ve arbitrary-PC resume guard'ı üç representation'ı da
ayırt eder. `is None` non-None kenarında fact'i `int | float`a daraltır; exact
builtin `isinstance(..., float/int)` bunu i64 ve F64 native dallarına böler.
Guarded direct-call sonucu aynı lattice fact'iyle caller'a taşınır. Yanlış
annotation girdisi veya sonucu generic Python değerini korur; `isinstance` ya da
type global'i yeniden bağlanırsa successor PC'de deopt edilir. Float ve integer
akışının ayrılmadan kullanıldığı noktalar güvenli materialization eklenene kadar
native kabul edilmez. Karar
[ADR 0147](adr/0147-optional-int-float-union-jit.md) içindedir.

`bool | float` karma representation imzası da typed scalar lattice'e eklendi.
Giriş/resume guard'ı exact bool immediate veya exact Tonic float kabul eder;
exact builtin `isinstance(..., float)` true dalını F64, false dalını Python 0/1
integer semantiğindeki bool olarak daraltır. Ters `isinstance(..., bool)` testi
de bool/F64 kenarlarını üretir. Guarded direct-call sonucu caller'a aynı fact ile
taşınır. Exact int, string ve yeniden bağlanmış builtin durumları annotation'ı
zorlayıcı kontrole çevirmeden generic Python yoluna döner. Karar
[ADR 0148](adr/0148-bool-float-union-jit.md) içindedir.

Optional biçim `bool | float | None` de aynı guarded lattice zincirine katıldı.
None identity testi non-None kenarını `bool | float`a indirir; exact builtin
`isinstance(..., float/bool)` bundan sonra bool 0/1 ve F64 native state'lerini
seçer. Giriş, arbitrary-PC resume ve guarded direct-call sonucu üç üyeyi exact
olarak doğrular. Exact int/string annotation uyuşmazlığı ile builtin rebinding
generic Python semantiğine exact PC'de döner. Karar
[ADR 0149](adr/0149-optional-bool-float-union-jit.md) içindedir.

Üç numeric üyeyi birleştiren `int | bool | float` imzası da tamamlandı. Exact
float testi true/false kenarlarını `Float` ve `IntOrBool`; exact bool testi
`Bool` ve `IntOrFloat`; exact int testi ise Python'ın `bool <: int` kuralıyla
`IntOrBool` ve `Float` olarak üretir. Giriş, arbitrary-PC resume ve guarded
direct-call sonucu üç representation'ı exact guard eder. String gibi union dışı
değerler ve builtin rebinding generic Python yürütmesine döner. Karar
[ADR 0150](adr/0150-three-way-numeric-union-jit.md) içindedir.

Temel scalar birleşim ailesinin üst sınırı `int | bool | float | None` ile
tamamlandı. Exact giriş/resume guard'ı dört representation'ı kabul eder; None
identity non-None kenarında `int | bool | float` üretir ve mevcut exact
float/bool/int testleri bunu native alt lattice'lere böler. Guarded direct-call
sonucu aynı fact'i caller'a taşır. Union dışı değer ve builtin rebinding exact
PC'de generic Python semantiğine döner. Karar
[ADR 0151](adr/0151-optional-full-numeric-union-jit.md) içindedir.

Function-local annotation yolunun ilk güvenli sınırı da uygulanmıştır.
`name: scalar = ordinary_call()` biçiminde derleyici annotation ifadesini runtime'da
değerlendirmeden bytecode v34 advisory call-result metadata'sına dönüştürür.
Desteklenen syntactic plan `int`, `bool`, `float`, `None`/`NoneType` ve bunların
canonical PEP 604 birleşimleridir. `CALL` yine normal VM side-exit'inde çalışır;
caller JIT successor PC'den dönerken gerçek materialized sonucu guard eder ve
ancak eşleşirse local `Move` ile devamındaki typed data-flow'a fact taşır. Exact
float sonuç burada F64 slota unbox edilir. Hint yanlışsa veya isim kullanıcı
tarafından başka bir değere bağlanmışsa annotation zorunlu kontrole dönüşmez;
native resume reddedilir ve bytecode semantiği devam eder. Call dışı producer,
closure/cell ve genel definite-assignment/SSA inference sonraki dilimlerdir.
Karar [ADR 0152](adr/0152-local-call-result-annotations.md) içindedir.

İkinci local dilim bytecode v35 ile aynı güvenlik sınırını ordinary attribute ve
item sonuçlarına taşır. Metadata artık `AdvisoryResult`, JIT girdisi
`GuardedResult` adını kullanır; verifier yalnız `CALL/ATTR/ITEM` producer PC'lerini
kabul eder. Runtime bu sitelerde competing direct lowering'i kaldırır, JIT
generic side-exit'i zorlar ve typed fact'i yalnız successor-PC exact guard'dan
sonra kullanır. Buffer item hint'i de native indexed-load sayılmaz; guard'lı
generic sınır olarak ölçülür. Karar
[ADR 0153](adr/0153-local-generic-producer-annotations.md) içindedir.

Üçüncü local dilim bytecode v36 ile `LOAD_GLOBAL` producer'ını ekler. Annotated
global load sitesi native global helper yolunda kesintisiz ilerlemek yerine
generic side-exit yapar; VM güncel binding'i yükledikten sonra arbitrary-PC JIT
resume successor'da exact scalar guard uygular. Rebinding ile hint uyuşmazsa
interpreter aynı değeri ve Python kontrol akışını sürdürür. Karar
[ADR 0154](adr/0154-local-global-result-annotations.md) içindedir.

Dördüncü local dilim bytecode v37 ile `CALL_EXPANDED` producer'ını ekler. Guarded
site'ın direct expanded-call profili kaldırılır; native yol `BEGIN_ARGS` PC'sinde
VM'e çıkar ve aynı argument-stack derinliği işaretiyle bütün `*args/**kwargs`
hazırlığını, binder'ı ve çağrıyı mevcut generic semantikle tamamlar. Callee
döndüğünde successor PC exact result guard'ı typed local akışı yeniden açar.
Karar [ADR 0155](adr/0155-local-expanded-call-result-annotations.md) içindedir.

Beşinci local dilim bytecode v38 ile `LOAD_CELL` producer'ını ekler. JIT cell
handle/layout bilgisini ABI'ye taşımaz; `LOAD_CELL/STORE_CELL` exact PC'de VM'e
side-exit eder. VM mevcut precise cell root, unbound hata ve write-barrier
semantiğini uygular. Annotated load sonucu successor-PC exact guard'dan sonra
typed local akışa girer; escaping closure'ın `nonlocal` mutation'ı uyuşmazlık
üretirse generic Python değeri korunur. Karar
[ADR 0156](adr/0156-local-closure-cell-result-annotations.md) içindedir.

Yol haritasındaki Cython-benzeri tamamlanma kapısı bu mekanizmayı scalar leaf
örneklerinin ötesine taşır: local/closure inference, graph-wide typed calls,
annotated field/shape, homogeneous container ve alias/escape analizi ile geniş
native bölgeler kurulacaktır. Standart Python annotation'ları advisory kalır;
sabit genişlik ve layout garantileri yalnız açık Tonic strict türleriyle gelir.
Son kabul, tüm dil coverage kapıları kapandıktan sonra aynı kaynakların CPython,
Cython ve Tonic üzerindeki compile latency, throughput, allocation, peak memory
ve code-size ölçümlerini içerir.

TypePlan temeli tek başına her plan için typed native entry seçmez.
`tuple[T, ...]`, Python `Ellipsis` singleton'ı üzerinden ayrı
`VariadicTuple(T)` planına dönüşür ve
fixed tuple'dan farklı canonical hash üretir. `typing.Callable[[A, B], R]`, boş
parametre listesi ve `typing.Callable[..., R]` ayrı canonical imza planlarıdır.
Bu bilgi guarded direct-call graph ve callee result analizinde kullanılacaktır;
Callable planı tek başına opaque callable nesnesini unboxed scalar yapmaz. Karar
[ADR 0124](adr/0124-callable-type-plan.md) içindedir. Mevcut buffer planı
[ADR 0125](adr/0125-f64-buffer-type-plan.md) içindedir. Runtime henüz yalnız
`f64` depolama uyguladığı için diğer ABI dtype sabitleri desteklenmiş planlar
sayılmaz. Advisory parametrik yüzey `Buffer[float]`,
`Buffer[float, rank]` ve `Buffer[float, rank, writable]` biçimlerini çözer;
`...` rank veya mutability kısıtını kaldırır. Plain `Buffer`, `Buffer[float]` ve
`Buffer[float, ..., ...]` aynı canonical planı üretir. Desteklenmeyen dtype,
geçersiz rank/mutability ve arity ayrı rejection code taşır. Karar
[ADR 0127](adr/0127-parameterized-f64-buffer-annotations.md) içindedir.
Immutable bytes literal/runtime/constructor/container davranışı ile exact bytes
ve `Literal[bytes]` canonical planı [ADR 0142](adr/0142-immutable-bytes-type-plan.md)
içindedir. Bytes heap-backed managed değer olarak kalır; planın varlığı tek başına
unboxed scalar JIT ABI'si oluşturmaz. Ek gerçek storage dtype'ları, strict
`tonic.types`, giriş ötesindeki loop guard'ları ve heap-backed/user-class union
narrowing sonraki genişleme dilimleridir.
Concrete buffer planı annotation-JIT parametre kapısına da bağlıdır. Exact native
buffer object kind ve `f64` storage doğrulanır; planda mevcutsa rank ve mutability
da kontrol edilir. Buffer tagged managed root olarak kaldığı için scalar ABI'ye
yanlış biçimde unbox edilmez. Mevcut ilk dilim `fastmath.sum` çağrısında güvenli
generic side-exit üzerinden native zero-copy döngüyü çalıştırıp successor PC'de
JIT'e döner. Annotation'a uymayan list gibi bir değer normal generic semantiği
çalıştırır; karar [ADR 0126](adr/0126-buffer-annotation-entry-guard.md)
içindedir. Exact `Buffer[float, 1, False]` parametresinden yapılan `buffer[index]`
okumaları için ilk doğrudan Cranelift yolu da tamamlandı. Owner tagged precise
root olarak kalır; runtime girişte yalnız çağrı ömürlü data/length view üretir ve
rank-1 C-contiguous shape/stride koşulunu guard eder. JIT küçük tam sayı indeksi
çözer, negatif indeksi Python gibi normalize eder, bounds miss'te tam `ITEM`
PC'sine deopt eder ve yüklenen `f64` değerini numeric loop boyunca kutulamadan
taşır. Karar [ADR 0128](adr/0128-native-f64-buffer-index-load.md) içindedir.
Canonical sıfır başlangıçlı, birer artan `while index < count` döngüsü ayrıca
verified induction analiziyle tanınır. `count <= len(buffer)` guard'ı native
girişe kaldırılır; normal PC 0 girişinde ilgili `ITEM` tekrar bounds kontrolü
yapmaz. Side-exit sonrası arbitrary-PC resume aynı siteyi checked çalıştırır.
Offset index, farklı başlangıç/artış, iç control flow veya loop içinde buffer/
bound rebinding kanıtı reddeder; exception region ve suspended function da bu
ilk kanonik analiz kapsamının dışındadır. Karar
[ADR 0129](adr/0129-buffer-loop-bounds-hoisting.md) içindedir. Genelleştirilmiş
sabit induction dilimi negatif olmayan başlangıç ve pozitif sabit adımı da kabul
eder; `bound <= len(buffer)` aynı güvenlik kanıtını korur, integer taşması
exact-PC deopt eder. Karar [ADR 0130](adr/0130-strided-buffer-range-proof.md)
içindedir. Tek sabit toplama/çıkarma kullanan affine indeks de alt sınırı
derleme zamanında, üst sınırı offset-aware giriş guard'ında kanıtlar. Pozitif
offset için `bound <= len-offset`, negatif offset için saturating
`bound <= len-offset` uygulanır; indeks arithmetic taşması exact-PC deopt eder.
Karar [ADR 0131](adr/0131-affine-buffer-index-proof.md) içindedir. Dinamik range
ve shape kanıtından önce generic buffer fallback'i rank-exact tuple indeksleme,
her eksende Python negatif-index normalizasyonu ve writable `f64` item assignment
kazanmıştır. Read-only atama reddedilir; karar
[ADR 0132](adr/0132-multidimensional-buffer-fallback.md) içindedir. Exact
`Buffer[float, 2, False]` yolu ayrıca iki öğeli tuple indeksini başarılı native
yolda materialize etmeden row-major `row * columns + column` adresine indirir.
İki eksen de Python negatif indeks ve bounds guard'ı taşır; başarısız guard,
generic yolun tuple register'ını kurabilmesi için `TUPLE` PC'sine deopt eder.
Owner precise root olarak kalır, shape metadata'sı yalnız çağrı ömürlü native
stack slotundadır ve yüklenen `f64` dönüşe kadar unboxed taşınır. Karar
[ADR 0133](adr/0133-native-rank2-f64-buffer-load.md) içindedir. Dinamik range/
shape kanıtının ilk dilimi de rank-2 kanonik iç eksen döngüsünü kapsar. Verified
`index=0; while index<count; index+=1` induction'ı tuple'ın sütun bileşenini
besliyorsa `count <= columns` guard'ı native girişe kaldırılır. Normal girişte
sütun bounds kontrolü atlanır; row ekseni checked kalır ve arbitrary-PC resume
iki ekseni de yeniden denetler. Fazla count girişte deopt edip generic
`IndexError` yolunu korur; karar
[ADR 0134](adr/0134-rank2-column-range-proof.md) içindedir. Çok eksenli/nested
range kanıtı da rank-2 kanonik matris taramasına genişletilmiştir. Analiz bütün
uygun geri kenarları dener; başlangıç atamasının loop header'ını ve induction
güncellemesinin seçilen backedge'i CFG üzerinde domine etmesini zorunlu tutar.
Bu sayede dış row ve iç column eksenleri ayrı ayrı `shape0/shape1` guard'larına
bağlanır; normal girişte iki bounds kontrolü de kalkar. Koşullu initialization
ve yanlış nested backedge proof üretmez. Karar
[ADR 0135](adr/0135-nested-rank2-range-proof.md) içindedir. Bu descriptor ve
kanıt artık exact read-only C-contiguous rank-N buffer'a genellenmiştir. Native
giriş data/length yanında shape pointer ve rank taşır; tuple bileşenlerini bütün
eksenlerde guard edip row-major fold ile tek flat offset üretir. Kanonik N-level
nested loop'ta her induction bağımsız `bound <= shape[axis]` giriş guard'ına
dönüşür ve normal girişte bütün kanıtlı eksen kontrolleri kalkar. Karar
[ADR 0136](adr/0136-native-rankn-f64-buffer-load.md) içindedir. Dış stride
lowering'den önce exact writable C-contiguous rank-N store yolu da eklenmiştir.
Buffer'ın ayrı non-moving mutation epoch'u generic ve native her store'da
ilerler. Writable C export owner handle'ı açıkken buffer native girişte fail
closed davranır; release epoch'u konservatif artırır. Native `SET_ITEM` bütün
rank/index/version guard'larını store'dan önce tamamlar ve bounds deopt'unda
canlı unboxed float register'larını generic resume için yeniden kutular. Karar
[ADR 0137](adr/0137-native-writable-f64-buffer-store.md) içindedir. Dış stride
lowering'den önce runtime buffer modeli logical origin/length ile signed byte
stride'ları taşır; reachable backing aralığını constructor'da doğrular ve generic
rank-N read/write, logical sum ile C descriptor export'unda negative/zero/padded
layout'ları korur. C-contiguous istek gerçek olmayan layout'ta fail-closed olur.
Karar [ADR 0138](adr/0138-owned-arbitrary-stride-buffer-runtime.md) içindedir.
Cranelift descriptor'ı stride pointer'ını da çağrı ömrü boyunca taşır. Rank-N
tuple yolu adresi eksen başına `index * signed_byte_stride` toplamıyla; logical
flat yol ise indeksi C-order koordinatlarına ayırıp aynı toplamla üretir. Bounds
kanıtları shape üzerinde kaldığından negative/zero/padded stride yeni bir
per-item guard veya helper gerektirmez; writable store mutation epoch ve alias
kapısını aynen korur. Karar
[ADR 0139](adr/0139-native-arbitrary-stride-buffer-addressing.md) içindedir.
Genel container alias/escape materialization açık iştir.
PEP 604 runtime nesnesi; `types.UnionType`,
GenericAlias üyeleri, sıra-bağımsız equality/hash, `__args__/__origin__`,
`isinstance`/`issubclass`, GC tracing ve metaclass operator önceliğiyle birlikte
uygulanmıştır. Karar [ADR 0115](adr/0115-pep604-union-type-plan.md) içindedir.
Variadic tuple kararı [ADR 0122](adr/0122-ellipsis-variadic-tuple-plan.md)
içindedir.
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
guard'ı exact PC'de generic yürütmeye döner. Karar
[ADR 0120](adr/0120-guarded-self-recursive-call-results.md) içindedir.

Aynı guard-backed özet mutual-recursive/SCC kenarlarına ve direct leaf subset'ine
girmeyen method gövdelerine genişletilmiştir. Generic `ATTR` ve `CALL` ayrı exact-PC
side-exit'larda çalışır; dönüşten sonraki typed continuation yalnız callee'nin
gerçek sonucu guard'ı geçerse yürür. Callee annotation planı caller dependency'si
olur; mutation iki yönlü recursive caller'ları invalid eder. Bu, native recursive
çağrı ABI'si veya SCC-wide codegen değildir; o compilation-unit optimizasyonu açık
iş olarak kalır. Karar
[ADR 0121](adr/0121-guarded-scc-and-method-results.md) içindedir.

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
