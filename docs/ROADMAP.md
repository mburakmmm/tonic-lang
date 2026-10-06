# Uygulama yol haritası

Orijinal iki mimari belgesi hedefi belirler. Bu dosya çalışılmış dilimi ve gerçek
kalan işleri ayırır; kutular yalnızca doğrulanmış uçtan uca iş için işaretlenir.

Tamamlanma hedefi bu listedeki bütün açık kutuların kapanması ve JIT'in yalnızca
bir demo yolu değil, tier seçimi, çalışma zamanı yardımcıları, GC safepoint'leri,
guard/deopt, hata yayılımı ve ölçüm kapılarıyla kullanılmaya hazır olmasıdır.
Yeni HPy/aHPy native ekosistem kapsamının aşağıdaki kabul kapıları da bu hedefe
dahildir. Annotation destekli kısmi statik tier da Python semantiğini koruyan
guard/deopt yolu ve açık opt-in strict değer türleriyle tamamlanmadan JIT hazır
sayılmaz. Python karşılaştırmalı nihai benchmark ancak bu koşullar sağlandıktan
sonra alınır.

- [x] Workspace, Rust stable, CLI, tanı, benchmark harness.
- [x] Parser adapter → Tonic AST → whole-function local resolution → register bytecode.
- [x] Bytecode verifier ve immutable verified program sınırı.
- [x] 64-bit Value, immediate integer/bool/None, BigInt fallback.
- [x] Fonksiyonlar, açık VM frames, while/for, compare, kısa devre, parallel assignment.
- [x] AGENTS §56 fib(40) ve interop §96 fastmath.add örnekleri.
- [x] Native local/persistent handles, cross-runtime/stale validation, native exception cleanup.
- [x] Temel parser/compiler/VM/CLI testleri, deterministic decoder mutation, CPython oracle.
- [x] Lexical HIR: global/nonlocal/cells/free vars; nested functions ve closure semantiği.
- [x] Call binder: defaults, keywords, positional-only metadata, kw-only, *args/**kwargs.
- [x] Insertion-ordered dict, list/dict item mutation ve augmented item assignment.
- [x] User classes/instances, class scope, constructor ve function-method binding.
- [x] Ortak shape/slot storage, bounded transition metadata ve dictionary fallback.
- [x] C3 MRO/multiple inheritance, private mangling, class identity/version ve generic rebinding.
- [x] Function/class decorators; `staticmethod` ve `classmethod` method descriptor'ları.
- [x] Lambda AST/HIR scope; closure, defaults ve mevcut call binder ile yürütme.
- [x] Property getter/setter, data-descriptor önceliği ve frame continuation.
- [x] List/tuple/string read-only slicing; açık/negatif sınırlar, negatif adım ve Unicode.
- [x] Custom data/non-data descriptor `__get__/__set__`, inherited owner ve tahsissiz çağrı yolu.
- [x] Otomatik descriptor `__set_name__`: tanım sırası, class continuation ve kesin GC roots.
- [x] `del obj.attr`, custom `__delete__`, property deleter ve normal instance/class deletion.
- [x] Örtük `__class__` cell; sıfır/iki argümanlı `super` ve C3 descriptor binding.
- [x] `object.__new__`, custom/inherited `__new__`, static binding ve init continuation.
- [x] Callable instance `__call__` ve `len(instance)`/`__len__`; MRO lookup, dönüş doğrulama.
- [x] Truthiness `__bool__` → `__len__` fallback; branch/not continuation ve operand koruma.
- [x] Metaclass ve class namespace customization.
  - [x] Canlı, salt okunur class `__dict__` mappingproxy; index/len/iteration ve
    GC izleme.
  - [x] `type` bootstrap nesnesi, explicit metaclass seçimi, kalıtım ve en türemiş
    uyumlu metaclass conflict çözümü.
  - [x] Kalıtılan `__prepare__` çağrısı, dict namespace identity/live body erişimi
    ve tamamlanırken class attribute materialization.
  - [x] Standart metaclass `__new__`/`__init__` çağrı zinciri; gerçek namespace
    kimliği, `type.__new__`, `__set_name__` sırası, iç içe oluşturma ve kesin GC roots.
  - [x] String-key dict için üç argümanlı doğrudan `type(name, bases, namespace)`;
    input mapping kopyası, implicit object base ve `__set_name__` devamları.
  - [x] Programatik class dict içinde string olmayan key'lerin insertion order,
    eşitlik tabanlı mappingproxy lookup/iteration ve precise GC ile korunması.
  - [x] Dict dışı özel `__prepare__` namespace mapping'leri; VM `__getitem__`/
    `__setitem__` dispatch'i, global fallback ve `type.__new__` için dict dönüşümü.
- [x] Type nesneleri ve kalan özel numeric/operator/attribute/iteration protokolleri.
  - [x] Instance `__getitem__`/`__setitem__`/`__delitem__`; special-method MRO
    lookup, static/class binding, suspending frame ve mutation-return discard.
  - [x] Class subscription `__class_getitem__`; metaclass `__getitem__`
    önceliği, plain/explicit-classmethod ve inherited binding, generic-class
    override ile managed generic-alias fallback'i, precise roots ve JIT-caller.
  - [x] `del list[index]` ve `del dict[key]`; negatif index, insertion-order
    korunumu, iterator version invalidation ve doğru IndexError/KeyError.
  - [x] Canonical builtin type nesneleri: None/int/bool/float/str/list/tuple/dict/
    range/function class-of-value, `bool <: int`, tuple classinfo, canonical
    callable `range` kimliği ve basic int/float/bool/str/list/tuple/dict
    constructor yolları.
  - [x] Mevcut builtin-iterable yüzeyi için constructor varyantları: `int` base,
    `dict` mapping/iterable-pair/keyword ve list/tuple iterable materialization.
  - [x] Instance ve metaclass `__getattr__` fallback'i; descriptor/normal lookup
    sonrası suspending call, iki argümanlı `getattr` ve stress-GC roots.
  - [x] `for` için user-defined `__iter__`/`__next__`; suspending çağrı zinciri,
    yalnız iterator sınırından kaçan `StopIteration` tüketimi ve hata yayılımı.
  - [x] `list`/`tuple` constructor'ları, exact unpack ve `*args` için genel user
    iterable tüketimi; suspending `__iter__`/`__next__`, precise GC roots,
    protokol-sınırı `StopIteration` ve CPython uyumlu unpack tanıları.
  - [x] `dict` constructor'ı için genel iterable-pair tüketimi; dış/pair
    iterator'larında suspending çağrılar, CPython sıra/index tanıları, keyword
    override ve moving-GC continuation roots.
  - [x] Instance ve metaclass `__getattribute__`/`__setattr__`/`__delattr__`;
    canonical `object`/`type` delegasyonu, data/non-data descriptor önceliği,
    suspending `AttributeError`→`__getattr__`, `getattr` default/`hasattr`,
    `delattr`, precise roots ve attribute/direct-method cache güvenliği.
  - [x] Builtin type alt sınıflarının native storage kurucuları ve kalan
    numeric/comparison protokolleri.
    - [x] Varsayılan kurucu yolunda `int`/`float`/`str`/`list`/`tuple`/`dict`
      alt sınıfları için class identity ve instance slotlarını koruyan, exact
      builtin backing'e sahip precise-GC uyumlu native instance'lar; iterable
      kurucu continuation root'ları, mutation/hash/slice/iteration ve JIT
      guard-deopt güvenliği.
    - [x] `+`, `+=`, `-`, `*`, `/`, `//`, `%`, rich comparison, unary
      `+`/`-` ve `abs` için suspending direct/reflected protokoller; strict
      subclass önceliği, `NotImplemented`, `__iadd__` fallback'i, `!=` için
      `__eq__` truth terslemesi ve metaclass dispatch'i.
    - [x] `**`, `|`, `^`, `&`, `<<`, `>>` ve `~`; direct/reflected/in-place
      protokoller, bool/int/BigInt semantiği, negatif shift ve sıfırın negatif
      kuvveti tanıları, büyük sonuç kaynak sınırı, bytecode v13 doğrulaması ve
      Cranelift desteklenmeyen-op interpreter fallback'i.
    - [x] `@` ve `@=`; suspending `__matmul__`/`__rmatmul__`/`__imatmul__`, strict
      subclass önceliği, `NotImplemented` ve in-place fallback, metaclass
      dispatch, bytecode v32 doğrulaması ve Cranelift generic-tier fallback'i.
    - [x] `divmod`; native int/float floor-division ve modulo çiftinin managed
      tuple sonucu, suspending `__divmod__`/`__rdivmod__`, strict-subclass
      reflected önceliği, `NotImplemented`, metaclass dispatch, precise roots ve
      JIT-caller generic fallback'i.
    - [x] `round`; keyword ve direct int/float `__round__` descriptor yüzeyi,
      suspending kullanıcı/metaclass hook'u ve `__index__`, bigint decimal
      yuvarlama, exact IEEE-754 ratio üzerinden ties-to-even, signed zero,
      NaN/infinity sınırları, precise roots ve JIT-caller generic fallback'i.
    - [x] İki/üç argümanlı `pow`; positional/keyword binder, suspending
      `__pow__`/`__rpow__`, Python 3.14 ternary strict-subclass reflected
      önceliği, `NotImplemented`, metaclass dispatch, BigInt modüler üs ve
      negatif üs için modüler ters, negatif modül işareti, precise roots ve
      JIT-caller generic fallback'i.
    - [x] `repr`/`ascii`/`format`; positional-only arity, ortak suspending
      `__repr__`/`__format__` continuation'ı, Unicode ASCII escaping, metaclass
      dispatch, dönüş/spec tipi doğrulaması ve f-string ile tek protokol yolu.
    - [x] `sum(iterable, /, start=0)`; geçici koleksiyon üretmeyen streaming
      iterator tüketimi, suspending `__iter__`/`__next__` ve `__add__`/`__radd__`,
      `NotImplemented`/strict-subclass sırası, BigInt sınırı ve Python 3.14
      Neumaier compensated exact-float sonucu, precise roots ve JIT-caller
      generic fallback'i.
    - [x] `any(iterable, /)` ve `all(iterable, /)`; materialization yapmadan
      kısa devreli streaming iterator tüketimi, suspending `__iter__`/`__next__`,
      `__bool__`/`__len__`/`__index__` zinciri, kesin `StopIteration` sınırı,
      precise continuation root'ları ve JIT-caller generic fallback'i.
    - [x] `min`/`max`; iterable ve variadic biçimler, keyword-only `key`/`default`,
      streaming tüketim, ilk eşit öğeyi koruyan strict karşılaştırma, suspending
      iterator/key/comparison/truth zinciri, precise roots ve JIT fallback'i.
    - [x] İki argümanlı `iter(callable, sentinel)`; managed ve kalıcı-exhaustion
      durumlu callable iterator, sıfır argümanlı guest callable, sentinel-sol
      rich equality/truth protokolü, bounded VM reentry, precise roots ve bütün
      iterator tüketicilerinde interpreter/JIT-caller uyumu.
    - [x] `__iter__` yokluğunda ardışık integer `__getitem__` sequence fallback'i;
      managed ve kalıcı-exhaustion durumlu iterator, instance/metaclass binding,
      class rebinding, bounded VM reentry ve tüketici continuation root'ları.
    - [x] Canonical `object.__init__` ile int/bool/float/str/list/tuple/dict/range
      `__new__` descriptor'ları; list/dict `__init__`, custom native subclass
      `__new__`, yeniden başlatma semantiği ve precise continuation roots.
    - [x] `int`/`float` constructor'larında suspending `__int__`/`__float__` ve
      `__index__` fallback'i; dönüş tipi doğrulaması, bool normalizasyonu ve
      custom native `__new__` tamamlaması.
    - [x] `range`, list/tuple/string/range scalar index, list set/delete, slice
      bileşenleri, `int(..., base=...)` ve `__len__` sonucu için suspending
      `__index__`; bool/native-int normalizasyonu ve precise continuation roots.
    - [x] `hash`/`object.__hash__`, suspending `__hash__`, `__eq__` tanımlayan
      sınıflarda implicit unhashable kuralı; collision-aware dict lookup/mutation/
      constructor/merge ve list/tuple/dict/slice içi suspending equality ile
      lexicographic sequence comparison, precise roots ve JIT exact-PC deopt.
    - [x] Bytecode v21 `is`/`is not` ve `in`/`not in`; Cranelift'te tahsissiz
      logical-Value kimlik karşılaştırması, native container hızlı yolları,
      suspending `__contains__`/iterator/equality zinciri, metaclass dispatch'i,
      precise continuation roots ve üyelik için güvenli interpreter fallback'i.
- [x] General module resolver/loader, Python kaynak modülleri, circular imports, versioned globals.
  - [x] Bytecode v14 module table, disjoint private global slotları, code/symbol
    relocation ve verifier sahiplik/aralık kontrolleri.
  - [x] `.tonic`/`.py` dosya çözümleme, package `__init__`, dotted import,
    `from ... import ...`, parent-child bağlama ve doğru imported-file tanısı.
  - [x] Tembel module object, tek seferlik/circular yükleme, başarısız import
    rollback/retry ve checked module version güncellemeleri.
  - [x] Import edilen hot fonksiyonlarda Cranelift yürütme ve module attribute
    mutation sonrasında güncel global slot okuması.
- [x] Exception objects/handlers/traceback state, try/raise/finally/with.
  - [x] Canonical BaseException/Exception/TypeError/ValueError/RuntimeError/
    StopIteration type nesneleri, managed exception instance'ı, user subclass,
    bytecode v9 `RAISE`, uncaught traceback ve JIT-safe interpreter fallback.
  - [x] Bytecode v10 exception region'ları, nested frame unwind, dynamic tuple/
    bare handler matching, active exception stack, bare reraise, `except as`
    cleanup ve `try/except/else`; JIT helper hatasının interpreter handler'ına
    aktarılması.
  - [x] `try/finally`; normal/hata/return/break/continue çıkışlarında exactly-once
    çalışma, pending exception context, nested finalizer ve override semantiği.
  - [x] Bytecode v11 senkron context manager (`with`); capture edilmiş `__exit__`,
    nested unwind, suppression truthiness, metaclass manager ve bütün yapısal
    çıkışlar.
  - [x] Bytecode v12 `raise ... from ...`; explicit/implicit cause-context ve
    suppression state'i, GC-traced managed traceback nesnesi, `__traceback__`
    erişimi ve context-manager traceback aktarımı.
- [x] Generators, yield/from, coroutine/async, suspended frame roots.
  - [x] Bytecode v15 `YIELD`, generator function çağrısında tembel frame oluşturma,
    `next`/`iter`/`send`/tek-argüman `throw`/`close`, dönüş değerli temel
    `yield from`, PEP 479 sınırı ve suspended register/cell/exception GC kökleri.
  - [x] For/list/tuple/dict/unpack/`*args` tüketicilerinde generator devamları;
    generator code'unun JIT'ten güvenli biçimde ayrılması ve JIT çağıran koddan
    yorumlayıcı resume.
  - [x] Bytecode v16 `YIELD_FROM`; generator ve özel iterator delegelerine
    `send`/`throw`/`close` forwarding, dış handler/finally unwind'ı, tam legacy
    `throw(type, value, traceback)` imzası ve GC-traced `StopIteration.value`.
  - [x] Ulaşılamayan askıdaki generator'lar için collector-dışı logical close,
    finalization roots, delege kapatma, bastırılan hata sayaçları, safepoint başına
    sekiz öğelik bounded drain ve shutdown kapanışı.
  - [x] Bytecode v17 `async def`/`await`; tembel coroutine nesnesi, exact Tonic
    coroutine ve özel `__await__` delegasyonu, ayrı GC-traced
    `coroutine_wrapper`, `send`/`throw`/`close`, askıdaki frame/delege kökleri,
    logical finalization ve JIT-caller interpreter fallback'i.
  - [x] Bytecode v18 `async for`; `__aiter__`/`__anext__`, await edilen next
    sonucu, `StopAsyncIteration` exhaustion sınırı, break/continue/else,
    precise continuation roots ve JIT-caller interpreter fallback'i.
  - [x] Bytecode v19 `async with`; girişte yakalanan `__aexit__`, await edilen
    `__aenter__`/`__aexit__`, suppression ve exception replacement, çoklu manager,
    bütün yapısal çıkışlar, precise suspended roots ve JIT güvenli fallback.
  - [x] Bytecode v20 async generator; ayrı `ASYNC_YIELD`, tembel
    `async_generator`, `__aiter__`/`__anext__`, tek kullanımlık GC-traced
    `asend`/`athrow`/`aclose` awaitable'ları, iç `await` forwarding,
    `StopAsyncIteration`/PEP 479 sınırı, logical finalization ve JIT-caller
    interpreter fallback'i.
  - [x] Tonic-owned tek thread event loop; GC-traced Future/Task/await iterator,
    FIFO scheduling, deterministik timer, done callback, exception propagation,
    cancellation ve temel native `asyncio` modülü (`run`, `create_task`,
    `current_task`, `get_running_loop`, `sleep`, `Future`). Gerçek zamanlı I/O
    selector ve thread-safe scheduling standart kütüphane kapsamında kalır.
- [x] Kapsamlı syntax conformance korpusu: comprehensions, match, f-strings, annotations vb.
  - [x] Bytecode v22 senkron list/dict comprehensions ve generator expressions;
    gizli lexical scope, dış scope'ta eager outer `iter`, iç içe `for`/`if`,
    closure cell'leri, geçici guest-list üretmeyen `LIST_APPEND`, precise
    suspended roots ve JIT
    generic-tier fallback'i.
  - [x] Set ve async comprehensions.
    - [x] Async list/dict comprehensions ve async generator expressions; async
      clause ile expression/filter/later-iterable `await`, eager outer
      `iter`/`aiter`, coroutine/async-generator hidden scope, mixed sync/async
      clause'lar, precise suspended roots ve interpreter/JIT-caller stress-GC.
    - [x] Bytecode v23 native set storage, set literal ve sync/async set
      comprehension; ortak hash/collision tablosu, suspending `__hash__`/`__eq__`,
      membership/equality/iteration, precise GC ve generic-tier JIT fallback'i.
  - [x] Bytecode v29 structural pattern matching (`match`/`case`); value,
    singleton, wildcard/capture, `as`, OR ve guard; fixed/starred nested sequence,
    mapping/`**rest`, dynamic duplicate-key; keyword/positional class,
    `__match_args__`, builtin self-pattern, descriptor-aware missing attribute,
    precise continuation roots ve Cranelift generic-tier fallback'i.
  - [x] Bytecode v30 f-string/format-spec lowering; Tonic-owned joined/formatted
    AST, source-order evaluation, nested dynamic spec, `!s`/`!r`/`!a`, Unicode
    string width/precision, integer/float format mini-language, suspending
    `__str__`/`__repr__`/`__format__`, precise continuation roots ve Cranelift
    generic-tier fallback'i.
  - [x] Değişken, parametre ve dönüş annotations/type-parameter yüzeyi.
    - [x] Positional-only, positional, variadic, keyword-only ve mapping
      parametreleri ile dönüş annotation ifadeleri; Tonic-owned AST/HIR,
      definition-scope evaluation, doğrulanmış function-site metadata,
      GC-traced `function.__annotations__` ve interpreter/JIT-caller stress-GC
      testleri.
    - [x] Değişken annotation'ları; Tonic-owned annotated-assignment AST/HIR,
      module/class `__annotations__`, function-local binding semantiği, karmaşık
      hedef değerlendirme sırası ve interpreter/JIT-caller stress-GC testleri.
    - [x] Python 3.12 type parameter ve `type` alias yüzeyi; TypeVar,
      TypeVarTuple, ParamSpec ve bound metadata'sı, lexical cell/shadowing,
      `__type_params__`, managed alias/generic-alias nesneleri, builtin/class
      subscription, generic base çözümleme, verifier ve JIT fallback sınırı.
  - [x] Python 3.13 type-parameter default sözdizimi; TypeVar/TypeVarTuple/
    ParamSpec `__default__`, `typing.NoDefault`, starred tuple metadata'sı,
    default-order doğrulaması, generic-class argument tamamlama, bytecode v33
    verifier ve interpreter/JIT-caller stress-GC uyumu.
  - [x] PEP 649/749 dil kararı: Tonic 0.x annotation/bound/type-parameter-default
    ifadelerini eager değerlendirir; `__annotate__`, `annotationlib` formatları
    ve fake-globals thunk'ları çekirdek runtime sözleşmesine alınmaz. Bu bilinçli
    Python 3.14 uyumluluk farkı ADR 0094 ile sabitlenmiştir.
- [x] Generic A/B baseline ve adaptive integer `+`, `+=`, `-`, `*` specialization; sekiz gözlem ve guard-failure de-specialization.
- [x] Exact-callee guard'lı monomorphic basit Tonic function call cache; rebinding miss ve generic binder fallback.
- [x] Monomorphic instance-slot attribute cache; class + shape + slot + dependency-version guard'ı ve descriptor-safe fallback.
- [x] Ölçümlü iki girişli polymorphic call/attribute cache; bounded side table, exact guards ve üçüncü hedefte generic fallback.
- [x] Granüler class/MRO dependency invalidation: weak descendant listeleri, ilgili version guard'ı ve GC pruning; bound globals varsayımsız direct slice okur.
- [x] Precise full-heap tracing GC, cycle toplama, compaction; slot/generation doğrulaması.
- [x] Stress GC, generation exhaustion, cycle/movement, native roots ve mutation graph testleri.
- [x] Nursery/old-generation ayrımı, write barrier ve remembered set; minor/major collection testleri.
- [x] Genel finalizer semantiği ve finalization roots; bounded pause tasarımı.
  - [x] Suspended generator finalization queue, precise roots ve safepoint başına
    bounded logical close; fiziksel reclamation sonraki collection'a ayrılır.
  - [x] Kullanıcı `__del__`, resurrection, unraisable hook ve genel nesne
    finalization sırası.
    - [x] Erişilemez user instance/exception nesneleri için collector-dışı
      `__del__` kuyruğu, precise finalizer roots, bounded safepoint drain,
      resurrection ve exactly-once çağrı; finalizer hataları ölçülerek
      unraisable biçimde yalıtılır.
    - [x] `sys.unraisablehook`/`sys.__unraisablehook__`, precise rooted
      `UnraisableHookArgs`, hook hata yalıtımı ve bütün GC girişlerinde suspended
      generator → user object → foreign payload finalization sırası. Aynı kategori
      içinde dil düzeyinde sıra garantisi verilmez.
- [x] Sabitlenmiş Cranelift 0.119 backend; immediate integer leaf numeric-loop bytecode'u, `--jit`, guard ve register-materialized deopt.
- [x] Leaf JIT differential korpusu; guard/fallback, entry hotness ve ölçümlü küçük-fonksiyon kârlılık eşiği, bounded de-specialization, compile-time/code-size sayaçları.
- [x] Opak runtime helper ABI; allocation üreten true division, kesin hata türü/PC dönüşü ve panic'in FFI sınırını aşmasını engelleyen trampoline.
- [x] Allocation helper safepoint'i: materialized JIT register root dizisi, diğer VM roots ve stress-GC altında hareket eden ara değer testi.
- [x] Resumable Tonic çağrıları: `CALL` yan çıkışı, explicit VM frame, arbitrary-PC native resume, recursive fib ve dinamik global rebinding.
- [x] Call sınırında materialized frame roots; recursive normal/stress GC ve hata konumu testleri.
- [x] Native backedge safepoint'i: inline 1024 sayacı, yavaş poll helper'ı ve materialized managed-root testi.
- [x] Loop OSR: 64 interpreted backedge hotness eşiği, exact loop-PC native entry ve cold-loop tier testi.
- [x] Döngü binary fallback: exact-int hızlı dalı, `+ += - * // %` için generic runtime yavaş dalı, materialized roots ve stress-GC float testi.
- [x] Bound global direct load: VM-owned raw value mirror, `STORE_GLOBAL` eşzamanlaması, rebinding doğruluğu ve `UNBOUND` exact-error helper fallback'i.
- [x] Profile-backed exact-callee/arity tamsayı leaf-call inlining; atomik guard deopt, logical-call sayacı, stress-GC ve A/B benchmark.
- [x] Exact function leaf-call için tahsissiz positional-only/keyword-only/default binding planı; generic binder eşdeğerliği ve A/B benchmark.
- [x] Plain bound-instance method lookup + leaf-call fusion; receiver binding, exact method guard, instance/class rebinding ve tahsis A/B testi.
- [x] Instance üzerinden staticmethod lookup + leaf-call fusion; no-receiver binding-kind guard, rebinding testi ve A/B benchmark.
- [x] Instance/class üzerinden classmethod ve class-level function/staticmethod lookup + leaf-call fusion; dinamik subclass receiver, binding-kind guard ve A/B benchmark.
- [x] Custom descriptor `ATTR` side-exit/resume + returned exact leaf-call inlining; mutation deopt, stress GC ve ölçümlü kârlılık kapısı.
- [x] `BEGIN_ARGS`→`CALL_EXPANDED` generic segment side-exit/resume; nested builder depth, stress GC ve A/B benchmark.
- [x] Bytecode tarafından gözlenmeyen boş `*args/**kwargs` için materialization-free direct leaf; kullanılan variadic parametrede generic fallback.
- [x] Native giriş kapsamlı lazy method dependency cache; exact owner/function guard, rebinding/owner deopt ve helper amortization benchmarkı.
- [x] Düz `*list/*tuple` + named/default expanded direct leaf; kararlı uzunluk profili, compile-time slot binding, güncel item lookup, `BEGIN_ARGS` deopt ve A/B benchmark.
- [x] Düz exact-dict `**mapping` expanded direct leaf; kararlı string-key profili, current value lookup, kesin argument roots, `BEGIN_ARGS` deopt ve A/B benchmark.
- [x] Ordinary direct leaf'te gözlenen materialized `*args/**kwargs`; compile-time extra binding, allocation safepoint'i, kesin roots, stress GC ve A/B benchmark.
- [x] Native float öncesi ölçüm: function-loop boxed helper ve üç-op direct leaf; allocation/helper/deopt sayaçlarıyla kabul bütçesi.
- [x] Geniş JIT: exact-float profilli direct leaf'te unbox-once, Cranelift F64 SSA, box-on-return, atomik guard deopt ve IEEE/stress-GC testleri.
- [x] Unboxed machine değerleri için PC-indexli deopt stack map, arbitrary-PC OSR initialization ve poll-deopt'ta tam interpreter-register rekonstrüksiyonu.
- [x] Public JIT API'sinde bağımsız `CodeObject` yapısal doğrulaması; bozuk
  register/constant/jump/profile girdisi codegen öncesinde tanımlı hataya döner.
- [x] Yürütme başına 64 MiB varsayılan native code bütçesi; taşma/limit halinde
  derlenmiş giriş bırakılır, sayaçlanır ve interpreter güvenle devam eder.
- [x] Parser ve public JIT `CodeObject` girişi için libFuzzer hedefleri; her
  hedefte ilk 10.000 coverage-guided mutation koşusu crash'siz tamamlandı.
- [x] Linux x86-64 ve macOS AArch64 debug/release JIT platform matrisi; iki
  mimaride parser ve public JIT girişi için 10.000'er gerçek AddressSanitizer
  libFuzzer koşusu. CI run 35772128579 ile doğrulandı.
- [ ] Annotation destekli kısmi statik derleme ve doğrudan typed-JIT tier'ı.
  Standart Python annotation'ları dil semantiğini değiştirmeden optimizasyon
  varsayımıdır; annotation yazmayan kod tamamen dinamik kalır. Desteklenen bir
  annotation planı bulunan kod profil sıcaklığını beklemeden guarded typed-JIT
  yoluna aday olur; yanlış tipte çağrı generic Python yoluna deopt eder.
  - [x] TypePlan v1 temeli: exact `None/bool/int/float/str`, plain container,
    nested homogeneous list/dict/set, fixed tuple, class identity/version,
    canonical hash, deterministic rejection code ve annotation-dict content
    mutation epoch'uyla lazy cache yenileme.
  - [ ] Çözümlenmiş annotation değerinden canonical `TypePlan`: exact builtin,
    union/optional/literal, fixed/variadic tuple, homogeneous list/dict/set,
    callable, class/shape ve buffer/dtype; unsupported/dynamic annotation için
    deterministic “optimize edilmedi” nedeni. V1 temeli tamamlandı; union,
    optional/literal/callable, variadic tuple, bytes ve buffer/dtype genişlemesi
    açık.
  - [x] Function identity + code/execution + annotation-dict content/version
    guard'ı; `__annotations__` mutation/replacement/delete ve class dependency
    version değişiminde cache invalidation veya atomik generic fallback. Eager
    annotation kararına göre sonradan global isim rebinding'i mevcut function'ın
    değerlendirilmiş annotation nesnesini değiştirmez; yeni definition yeni plan
    kurar.
  - [ ] Verified bytecode üzerinde typed data-flow/SSA overlay; parametre,
    local, branch merge, loop phi, dönüş ve çağrı sonucu propagation'ı. Dinamik
    bytecode ve object model tek doğruluk kaynağı olarak kalır.
    - [x] Exact-small-int forward overlay: typed parametre, immediate sabit,
      `Move`, unary/binary integer sonuçları, branch merge ve loop fixed-point;
      arbitrary-PC giriş guard'ı ve ispatlanan aritmetik/karşılaştırma
      noktalarında redundant tag-guard elimination.
    - [ ] Birleşik `int/float/bool` lattice, dönüş/call-result propagation,
      direct annotated callee özeti ve diagnostics/explain metadata'sı.
      - [x] Ortak `Unknown/Int/Float/Bool` forward lattice; exact-bool parametre,
        sabit, `Move`, `not`, karşılaştırma ve branch propagation'ı; `bool`
        annotation'lı first-call JIT, arbitrary-PC exact-bool guard'ı ve Python
        `bool <: int` sayısal lowering'i.
      - [x] Immediate `None` fact'i: `None` ve exact `NoneType` parametre/dönüş
        annotation'ı, normal ve arbitrary-PC tek-word giriş guard'ı, `Const None`/
        `Move` propagation'ı ve kanıtlı dönüşte host guard elimination.
      - [x] Bütün erişilebilir `RETURN` noktalarında result planı kanıtı,
        public JIT metadata/VM sayacı ve kanıtlı native dönüşte yinelenen host
        type-guard elimination; kanıtlanamayan dönüşte exact-PC advisory deopt.
      - [x] Exact global function için guarded annotated leaf özeti ve call-result
        propagation: callee return bytecode proof'u, function/code/execution/
        annotation dependency invalidation'ı, first-call direct inline ve caller
        return proof zinciri; yalan callee annotation'ında özet reddi.
      - [ ] Recursive/method call-result özetleri.
        - [x] Exact global class owner üzerinden plain function/`staticmethod`
          leaf: profil beklemeden `ATTR`+`CALL` fusion, callee annotation/return
          proof dependency'si, binding-kind/exact-function guard'ı ve class
          rebinding'de exact-ATTR deopt.
        - [x] Exact global instance owner ve exact global class `classmethod`
          leaf: annotation gerektirmeyen opaque `self/cls` parametresi, normal
          method/class binding guard'ı, instance shadow/class mutation deopt'u.
        - [x] Exact kullanıcı-sınıfı annotation'lı caller parametresinden method
          edge'i: exact class identity/version giriş guard'ı, saf `Move` zinciri
          owner çözümleme, opaque receiver, exact binding/function guard'ı,
          yanlış sınıfta advisory fallback, instance shadow'da exact-ATTR deopt
          ve class mutation'da caller invalidation.
        - [ ] Recursive/SCC özetleri ve annotated field/shape propagation.
      - [x] Kullanıcıya açık deterministic rejection metadata'sı: function/code,
        kategori, bytecode PC/opcode ve sabit neden; public runtime accessor ve
        CLI `--stats` explain satırları.
  - [ ] Annotation bulunan uygun fonksiyon için profil beklemeden first-call
    typed baseline compile; açık `@tonic.compile`/modül politikasıyla import-time
    warmup. Derleme hatası programı bozmaz ve generic tier'a kayıtlı nedenle döner.
    - [x] Tam `int`/`float` parametre ve dönüş planlı uygun leaf için first-call
      Cranelift seçimi; exact function/code/execution/plan/argument guard'ı,
      return guard'lı exact-PC deopt, annotation invalidation sayaçları ve
      normal/stress-GC testleri.
    - [x] `None`/exact `NoneType` imzaları için first-call Cranelift seçimi;
      yanlış argümanda advisory generic fallback ve kanıtlı `None` dönüşü.
    - [ ] `@tonic.compile`, modül warmup politikası ve import-time code-budget
      planlaması.
    - [x] Compile-rejection tanısı: unsupported bytecode, profitability, code
      budget ve guard-instability için deterministic kayıt ve CLI açıklaması.
  - [ ] Unboxed `i64`/`f64`/`bool` register ve çağrı ABI'si; Python `int` için
    overflow'da bigint deopt'u, IEEE float sınırları, exact exception PC'si,
    safepoint stack-map ve interpreter state rekonstrüksiyonu.
  - [ ] Typed container/buffer yolu: bounds/shape/dtype/mutability guards,
    allocation-free numeric loop, write barrier ve alias/escape halinde doğru
    materialization. NumPy C ABI veya raw object layout varsayımı yapılmaz.
  - [ ] Typed direct-call graph: annotated callee/return planı, recursion,
    monomorphic method/class/shape guard'ları, inline bütçesi ve ayrı compilation
    unit/code-size sınırı.
    - [x] Exact global int/float leaf için ilk guarded kenar: profil beklemeden
      callee identity ve annotation-plan dependency guard'ı, kanıtlı result
      propagation ve mutation/rebinding fallback'i.
    - [x] Exact global class owner'lı plain function/`staticmethod` için ilk
      annotated class edge: allocation-free method load, static binding guard'ı,
      callee plan invalidation'ı ve class mutation fallback'i.
    - [x] Exact global instance method ve global class `classmethod` edge'i:
      opaque receiver parametresi, instance/class binding guard'ı ve
      shadow/rebinding fallback'i.
    - [x] Exact kullanıcı-sınıfı annotation'lı caller parametresi üzerinden
      guarded instance-method edge'i; class identity/version, method binding ve
      function identity guard'ları ile mutation/shadow fallback'i.
    - [ ] Recursion/SCC, annotated field/shape kenarları ve graph-wide inline/
      code-size bütçesi.
  - [ ] Kısmi statik sınıf yolu: annotated fields için shape slot planı,
    constructor definite-assignment analizi, descriptor/metaclass mutation
    guard'ı ve dinamik attribute fallback'i.
  - [ ] Kısmi statik program analizi: annotation sınırlarından local/return/
    closure tür çıkarımı, union narrowing, effect/alias/escape bilgisi, bounded
    monomorphization ve ayrı compilation unit'ler. Bilinmeyen veya megamorphic
    akışlar doğruluk kaynağı olan dinamik bytecode'a döner.
  - [ ] Annotation-JIT yönlendirme ve gözlemlenebilirlik: cost model, compile
    queue/code budget, specialization cache sınırı, `inspect/explain` çıktısı ve
    her karar için derlendi/reddedildi/deopt nedeni.
  - [ ] Python-compatible advisory kip ile açık strict kip ayrımı. Advisory kip
    annotation'ı runtime type check'e dönüştürmez; strict kip yalnız Tonic'e ait
    `i8..i64`, `u8..u64`, `f32/f64`, packed struct ve checked/wrapping politika
    türlerinde tanımlı hata/overflow ve FFI layout sözleşmesi uygular.
  - [ ] Ahead-of-time cache için canonical type-plan hash'i, bytecode/runtime/
    target/CPU feature sürümü, doğrulanmış yükleme ve stale-cache reddi; cache
    içinde raw Rust layout'u veya native heap adresi yoktur.
  - [ ] Interpreter/adaptive/profile-JIT/annotation-JIT eşdeğerlik korpusu;
    doğru/yanlış tip, overflow, mutation, exceptions, moving/stress GC, deopt,
    code budget ve compile-failure testleri. Son kabul Cython/CPython karşılaştırmalı
    micro/macro ölçüm, compile latency, code size, allocation ve bottleneck profili.
  Ayrıntılı tasarım ve teslim sırası:
  [ANNOTATION_JIT_PLAN.md](ANNOTATION_JIT_PLAN.md), karar:
  [ADR 0101](adr/0101-annotation-guided-partial-static-jit.md).
- [x] Native C function-table ABI/version/capability, exception status ve panic guard.
- [x] Buffer descriptor, dtype/shape/stride, owner, mutability; fastmath.sum zero-copy örneği.
- [x] Thread attach, persistent callback, reentry, shutdown/finalization.
- [x] Foreign vtable/trace/lifecycle, deferred exactly-once payload destruction.
- [x] CPython bridge: PyTonicProxy, ForeignPyObject, conversions, interpreter execution state.
  - [x] İzole `tonic-cpython` crate, int/float/UTF-8 primitive conversion,
    `ForeignPyObject`, GIL/execution-state guard ve deferred `Py_DecRef`.
  - [x] GC-owned callable `PyTonicProxy`, stable runtime-owner/execution guard,
    callback forwarding, delayed doğru-runtime release ve Python traceback metni.
  - [x] İsimle module/function çözümleyen unary `call1`; `None/bool/i64/float/UTF-8`
    ve arbitrary `ForeignPyObject` sonuç/girdi dönüşümü; positional proxy forwarding.
  - [x] Bigint/container/keyword conversion; alias/cycle-aware materialization ve
    gerçek CPython heap type üzerinde proxy call/attribute/set/repr protokolleri.
- [x] Bridge cycles/finalizers ve identity.
  - [x] Persistent proxy anchor, doğru-runtime deferred release, idempotent explicit
    `close_proxy`, closed/inactive diagnostic ve cycle kırma testi.
  - [x] GIL-serialized non-owning weak proxy identity cache; doğrudan proxy wrapper
    için trace edge/root demotion, dış CPython referansı promotion'ı ve otomatik cycle testi.
  - [x] Arbitrary `ForeignPyObject` nesne grafiklerinde proxy kenarlarını bulan bounded
    iki-collector cycle detection, conservative fallback ve finalizer sırası.
- [ ] Tonic HPy Universal host ve aHPy uyumluluk hattı.
  - [x] Exact HPy sürüm/ABI/context envanteri, fail-closed capability manifesti ve
    izole `tonic-hpy` crate sınırı.
  - [x] Platform/ABI/init-symbol doğrulamalı `.hpy0` shared-library loader; libpython
    bağımlılığı olmadan constant module ve scalar Fibonacci.
    - [x] macOS/Linux dosya adı, ABI ve dört init sembolü doğrulaması; başarılı
      library mapping'ini unload protokolü gelene kadar süreç ömrüne pinleme.
    - [x] Minimal context/module materialization ile constant module ve scalar
      Fibonacci; Windows loader.
      - [x] Extension durumunu VM ömrüne bağlayan stateful native callback ve
        çağrı-scope module handle erişimi.
  - [x] Local `HPy`, `HPy_Dup`/`HPy_Close`, sayı/Unicode, module init ve exception
    state; stale/cross-runtime/failure cleanup testleri.
  - [x] List/tuple/dict builders, attr/item/call yüzeyi ve keyword binder eşlemesi.
    - [x] List/tuple/dict constructor ve exact-check yüzeyi; fixed-size list/tuple
      builder build/cancel, eksik/leaked/stale builder denetimi.
    - [x] Attr/item/call yüzeyi, dict mutation ve keyword binder eşlemesi.
  - [x] H2 tamamlayıcı scalar/exception kapısı: bool/bigint/float dönüşümleri,
    HPy 0.9 public raise/match/no-memory yüzeyi ve normal/stress-GC/fault/error
    testleri. Bu HPy sürümünde public fetch/restore API'si yoktur.
  - [x] Per-runtime `HPyGlobal`; precise traced `HPyField`, write barrier ve gerçek
    moving-GC altında field/global cycle testi.
  - [x] `HPyTracker` ownership, `Close`/`ForgetAll`, stale/leak/fault yolları ve
    deterministic call teardown.
  - [ ] Non-zero module C state: pinned HPy 0.9 Universal public yüzeyinde state
    accessor olmadığı için private ABI uydurmadan upstream sürüm/contract kararı.
    Native payload field yerleşimi H4 type yüzeyiyle birlikte tamamlanacak.
  - [ ] Pure `HPyType_Spec`, native payload, methods/slots/inheritance, trace,
    finalizer ve shutdown sözleşmeleri.
  - [ ] Public HPy buffer ve execution-state yüzeyi; owner/pin/thread/callback
    lifetime testleri, unavailable API için versioned tanı.
  - [ ] Normal/Trace/Debug context, leak/use-after-close/fault injection, symbol
    audit, sanitizer ve Linux/macOS/Windows CI.
  - [ ] Handwritten ve aHPy-generated ortak corpus; constant/scalar/container/
    exception/type/`HPyField` sırası ve exact aHPy+HPy revision kaydı.
  - [ ] aHPy cypack, murmurhash scalar adapter ve frozenlist supported-subset
    pilotları; NumPy/typed-memoryview blocked durumunu yanlış destek saymama.
  - [ ] Tonic-native ABI, handwritten HPy, aHPy HPy, CPython bridge ve CPython HPy
    oracle için aynı semantik A/B benchmarkı.
- [ ] REPL, bytecode caching/version validation, standard library kapsamı.
- [ ] Coverage-guided fuzzing ve gerekiyorsa unsafe/JIT için Miri/sanitizers.
- [x] Ara benchmark: 14 interpreter iş yükü, GC açık/kapalı, compile zamanı, ham örnekler, process RSS.
- [x] Class aşaması ara benchmark: 8 ek instance/attribute/method iş yükü ve önceki baseline.
- [x] Slice aşaması ara baseline: 1.000 list slice, GC açık/kapalı ve ham örnekler.
- [x] Descriptor ara baseline: 10.000 `__get__/__set__`, tahsis ve GC sayaçları.
- [x] Ara CPython karşılaştırması: 13 ortak workload, beş süreç, warm/compile/cold ayrımı.
- [ ] Tamamlanma sonrası nihai benchmark: tier ve backend matrisi, host allocation, macro workloads, tekrar üretilebilir ortam.

Sıradaki çekirdek işler HPy tracker/module-state ile H4 type/native-payload yüzeyi,
annotation destekli typed-JIT, REPL/bytecode cache/stdlib ve coverage-guided
güvenlik testleridir.
JIT'in desteklenen tier'ı
x86-64/AArch64 debug-release, normal/stress
GC differential ve iki mimaride AddressSanitizer fuzz kapılarını geçmiştir;
desteklenmeyen bytecode generic interpreter fallback'inde kalır.
HPy/aHPy kararının ayrıntıları
[HPY_AHPY_STRATEGY.md](HPY_AHPY_STRATEGY.md) ve
[ADR 0049](adr/0049-hpy-universal-host.md) içindedir. M5'in
generational kabul koşulu nursery, remembered set, write barrier, minor/major
zamanlama, stress doğruluğu ve önce/sonra benchmarkıyla kapanmıştır.
Foreign object vtable/lifecycle, ana CPython bridge sözleşmeleri ve bounded
cross-collector identity/cycle politikası tamamlanmıştır.
Nihai benchmark için kabul matrisi: [FINAL_BENCHMARK_PLAN.md](FINAL_BENCHMARK_PLAN.md).
