# Uygulama yol haritası

Orijinal iki mimari belgesi hedefi belirler. Bu dosya çalışılmış dilimi ve gerçek
kalan işleri ayırır; kutular yalnızca doğrulanmış uçtan uca iş için işaretlenir.

Tamamlanma hedefi bu listedeki bütün açık kutuların kapanması ve JIT'in yalnızca
bir demo yolu değil, tier seçimi, çalışma zamanı yardımcıları, GC safepoint'leri,
guard/deopt, hata yayılımı ve ölçüm kapılarıyla kullanılmaya hazır olmasıdır.
Python karşılaştırmalı nihai benchmark ancak bu koşullar sağlandıktan sonra alınır.

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
- [ ] Metaclass ve class namespace customization.
- [ ] Type nesneleri ve kalan özel numeric/operator/attribute/iteration protokolleri.
- [ ] General module resolver/loader, Python kaynak modülleri, circular imports, versioned globals.
- [ ] Exception objects/handlers/traceback state, try/raise/finally/with.
- [ ] Generators, yield/from, coroutine/async, suspended frame roots.
- [ ] Kapsamlı syntax conformance korpusu: comprehensions, match, f-strings, annotations vb.
- [x] Generic A/B baseline ve adaptive integer `+`, `+=`, `-`, `*` specialization; sekiz gözlem ve guard-failure de-specialization.
- [x] Exact-callee guard'lı monomorphic basit Tonic function call cache; rebinding miss ve generic binder fallback.
- [x] Monomorphic instance-slot attribute cache; class + shape + slot + dependency-version guard'ı ve descriptor-safe fallback.
- [x] Ölçümlü iki girişli polymorphic call/attribute cache; bounded side table, exact guards ve üçüncü hedefte generic fallback.
- [x] Granüler class/MRO dependency invalidation: weak descendant listeleri, ilgili version guard'ı ve GC pruning; bound globals varsayımsız direct slice okur.
- [x] Precise full-heap tracing GC, cycle toplama, compaction; slot/generation doğrulaması.
- [x] Stress GC, generation exhaustion, cycle/movement, native roots ve mutation graph testleri.
- [x] Nursery/old-generation ayrımı, write barrier ve remembered set; minor/major collection testleri.
- [ ] Finalizer semantiği ve finalization roots; bounded pause tasarımı.
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
- [ ] Bridge cycles/finalizers ve identity.
  - [x] Persistent proxy anchor, doğru-runtime deferred release, idempotent explicit
    `close_proxy`, closed/inactive diagnostic ve cycle kırma testi.
  - [x] GIL-serialized non-owning weak proxy identity cache; doğrudan proxy wrapper
    için trace edge/root demotion, dış CPython referansı promotion'ı ve otomatik cycle testi.
  - [ ] Arbitrary `ForeignPyObject` nesne grafiklerinde proxy kenarlarını bulan genel
    iki-collector cycle detection ve finalizer sırası.
- [ ] REPL, bytecode caching/version validation, standard library kapsamı.
- [ ] Coverage-guided fuzzing ve gerekiyorsa unsafe/JIT için Miri/sanitizers.
- [x] Ara benchmark: 14 interpreter iş yükü, GC açık/kapalı, compile zamanı, ham örnekler, process RSS.
- [x] Class aşaması ara benchmark: 8 ek instance/attribute/method iş yükü ve önceki baseline.
- [x] Slice aşaması ara baseline: 1.000 list slice, GC açık/kapalı ve ham örnekler.
- [x] Descriptor ara baseline: 10.000 `__get__/__set__`, tahsis ve GC sayaçları.
- [x] Ara CPython karşılaştırması: 13 ortak workload, beş süreç, warm/compile/cold ayrımı.
- [ ] Tamamlanma sonrası nihai benchmark: tier ve backend matrisi, host allocation, macro workloads, tekrar üretilebilir ortam.

Sıradaki çekirdek iş: arbitrary `ForeignPyObject` grafikleri için genel
iki-collector cycle/finalizer politikasını kurmak; ardından kalan özel
protokoller/metaclass kapsamını genişletmek. M5'in
generational kabul koşulu nursery, remembered set, write barrier, minor/major
zamanlama, stress doğruluğu ve önce/sonra benchmarkıyla kapanmıştır.
Foreign object vtable/lifecycle ve ana CPython bridge sözleşmeleri tamamlanmıştır;
advanced bridge identity/cycle optimizasyonu ayrı açık maddedir.
Nihai benchmark için kabul matrisi: [FINAL_BENCHMARK_PLAN.md](FINAL_BENCHMARK_PLAN.md).
