# Tonic HPy Universal ve aHPy stratejisi

Durum: proje kapsamına kabul edildi; uygulama bekliyor.

Bu belge, Tonic için yapılan dış mimari değerlendirmenin ve aHPy'nin mevcut
uygulama sözleşmesinin projeye nasıl alındığını tanımlar. Hedef, HPy Universal
uzantılarını CPython çalıştırmadan Tonic üzerinde yükleyip çalıştırabilen bir host
oluşturmaktır. aHPy'nin ürettiği Universal uzantılar bu hostun ana gerçek-dünya
üretim hattıdır.

Bu hedef Tonic'in nesne modelini değiştirmez. Tonic `Value`, handle table,
moving generational GC, shapes ve JIT tasarımına sahip olmaya devam eder. HPy
uyumluluğunun bütün maliyeti uzantı sınırında ödenir.

## GPT 5.6 Sol değerlendirmesinin proje kararı

Kullanıcının sağladığı GPT 5.6 Sol değerlendirmesi bir tasarım girdisi olarak
incelendi; doğrulanmış ürün durumu veya bağımsız benchmark kanıtı olarak kabul
edilmedi. Değerlendirmenin sonuçları şu kararlara dönüştürüldü:

| Değerlendirme | Proje kararı |
|---|---|
| Tonic-native handle uzantıları doğrudan çalışabilir | Kabul; mevcut opaque C ABI bu yolun temelidir |
| HPy Universal alternatif runtime için doğal portable katmandır | Kabul; yeni ana native ekosistem milestone'u |
| aHPy Cython ekosistemini HPy Universal'a taşıyabilir | Kabul; pinned ortak corpus ve pilot CI ile kanıtlanacak |
| HPy handle doğrudan Tonic handle olabilir | Koşullu; binary/lifetime sözleşmesi ve benchmark kanıtı gerekir |
| NumPy/SciPy/Pandas/PyTorch native olabilir | Uzun vadeli olasılık; paket başına port ve test olmadan destek iddiası yok |
| CPython `PyObject *` facade ile legacy wheel yüklenebilir | Ayrı, pahalı compatibility araştırması; HPy milestone'una dahil değil |
| Sınır maliyeti sıfıra yaklaşır | Hedef olarak kabul edilmedi; scalar ve kernel maliyeti ayrı ölçülecek |

## Değerlendirmeden kabul edilen kararlar

- Tonic-native handle ABI en düşük maliyetli yerel uzantı yolu olarak kalır.
- HPy Universal ayrı ve taşınabilir native uzantı yolu olur.
- aHPy, desteklenen Cython kaynağını `Python.h` kullanmayan HPy Universal C'ye
  dönüştürerek Tonic native ekosistemini büyütür.
- CPython Limited API/abi3 ve klasik `PyObject *` ABI aynı problem değildir;
  bunlar HPy tamamlandıktan sonra ayrı compatibility katmanları olarak
  değerlendirilebilir.
- Mevcut `tonic-cpython` adapter'ı unsupported paketler için açık maliyetli kaçış
  yolu olarak kalır; HPy host libpython'e bağlanmaz.
- Küçük native çağrılarda boundary maliyeti ölçülür. Büyük sayısal kernel'larda
  birkaç handle çözümünün düşük kalması beklenebilir, fakat bu benchmark sonucu
  alınmadan performans iddiası değildir.

## Düzeltilen veya sınırlandırılan iddialar

HPy ve Tonic handle'ları kavramsal olarak benzerdir, fakat binary temsilleri
kendiliğinden aynı kabul edilemez. HPy Universal ABI'nin `HPy`, `HPyField`,
`HPyGlobal`, builder, tracker ve `HPyContext` düzeni desteklenen HPy sürümünün
tam sözleşmesine uymalıdır. Doğrudan tek tablo kullanımı ancak bit düzeni, null
değer, lifetime ve runtime identity kuralları testlerle eşleşirse seçilir.

aHPy rastgele Cython veya NumPy C-API kaynağını bugün değişikliksiz desteklediğini
iddia etmez. Yerel aHPy sözleşmesi `PyObject *`, `Python.h`, `cpython.*`, legacy
HPy ve CPython-only üçüncü taraf API'leri Universal modda fail-closed tanılarla
reddeder. Generators/coroutines, typed memoryview tüketimi ve fused types gibi
alanlar da güncel HPy yüzeyine ve aHPy destek matrisine bağlıdır.

Bu nedenle NumPy, Pandas, SciPy veya PyTorch desteği yol haritası sonucu olarak
ilan edilmez. Her paket exact revision, kaynak değişikliği, binary sembol denetimi,
semantik test, Debug/Trace koşusu ve performans kaydıyla ayrı kabul edilir.

## Uyumluluk katmanları

| Katman | Amaç | Durum |
|---|---|---|
| Tonic-native ABI | `TonicHandle` ve `TonicApi` kullanan yerel uzantılar | İlk ABI dilimi var; shared-library loader açık |
| HPy Universal | `.hpy0` uzantısını Tonic HPy host üzerinde çalıştırmak | Yeni ana kapsam |
| aHPy Universal | Desteklenen `.pyx` kaynağını HPy Universal'a üretip Tonic'te çalıştırmak | Ortak doğrulama kapsamı |
| Limited API / abi3 | CPython'ın opak fonksiyon yüzeyini ayrı facade ile taşımak | HPy sonrasına ertelendi |
| Full CPython ABI | Layout/macro bağımlı legacy binary uyumluluğu | Ana yol değil; gerekirse compatibility modu |
| Embedded CPython | Dönüşüm/proxy üzerinden paket çalıştırmak | İzole bridge dilimi var |

## Hedef mimari

```text
Python source --------------------------> Tonic compiler / VM / JIT

Cython source -> aHPy -> *.hpy0 --------+
handwritten HPy Universal -> *.hpy0 ----+--> Tonic HPy loader
                                              |
                                              v
                                       versioned HPyContext
                                              |
                                      Tonic handle adapter
                                              |
                         +--------------------+-------------------+
                         |                    |                   |
                       objects           exceptions            buffers
                         |                    |                   |
                         +------------ Tonic runtime ------------+
                                           moving GC
```

`tonic-hpy` izole bir crate olmalıdır. HPy C isimleri ve layout'ları bu crate
dışına sızmamalı; `tonic-runtime` yalnız kendi handle, context, foreign object,
buffer ve module/type registration sözleşmelerini sunmalıdır.

## Temel eşleme

| HPy yüzeyi | Tonic karşılığı | Kabul şartı |
|---|---|---|
| Local `HPy` | scoped Tonic local handle | close/use-after-close ve runtime identity testleri |
| `HPy_Dup` / `HPy_Close` | handle çoğaltma / release | Debug mode leak ve stale-handle denetimi |
| `HPyGlobal` | interpreter-owned persistent slot | per-runtime isolation ve shutdown cleanup |
| `HPyField` | GC tarafından izlenen managed field | moving GC ve old→young barrier testi |
| builders/trackers | bounded native construction scopes | failure-path cleanup ve double-close testi |
| numbers/Unicode | immediate veya heap Tonic değeri | overflow, Unicode ve exception eşdeğerliği |
| list/tuple/dict | Tonic container API | alias, cycle, order ve mutation semantics |
| attr/item/call | Tonic protocol ve call binder | descriptor, keyword ve error-state testleri |
| `HPyType_Spec` / module defs | Tonic native type/module descriptors | slot validation, GC trace ve unload ownership |
| HPy buffer | `TonicBuffer` ve owner token | zero-copy lifetime, mutability ve pinning |

## Uygulama aşamaları

### H0 — Sürüm ve ABI envanteri

- HPy sürümünü exact tag/commit ile sabitle.
- Universal header, ABI tag, context layout, init symbolü ve platform suffix
  sözleşmesini makinece doğrulanabilir envantere al.
- Desteklenen ve fail-closed kalan HPy fonksiyonlarını bir capability manifestiyle
  yayınla.
- HPy isimlerini Tonic native ABI'sine eklemek yerine adapter crate'inde tut.

### H1 — Loader ve minimal context

- macOS/Linux shared-library yükleme; ardından Windows.
- `.hpy0` dosyasının platform, ABI, init symbolü ve module-name doğrulaması.
- Modül sabiti ve tek scalar fonksiyon için gereken local handle, dup/close,
  integer conversion, exception state ve module init yüzeyi.
- Unload mümkün değilse açıkça pinle; canlı function/type/payload varken library
  kapatma.

H0, `tonic-hpy` crate'indeki typed envanter ve
`abi/hpy-0.9.0-universal.toml` ile tamamlanmıştır. Envanter HPy `0.9.0`
tag'ini, `hpy0` ABI/context slot düzenini, PyPI source hash'ini ve aHPy
`880d46d7d348df759ef062711ab3b4876bd648b8` revision'ını sabitler. H0'da bütün
çalıştırma capability'leri `unavailable` kalır; bilinmeyen capability ve yanlış
module/suffix sözleşmeleri fail-closed reddedilir. Bu kayıt loader veya context
uygulandığı anlamına gelmez.

İlk uçtan uca kanıt:

```text
aHPy veya handwritten fib extension
    -> fib.hpy0
    -> Tonic import loader
    -> fib(40)
    -> interpreter oracle ile aynı sonuç
```

### H2 — Core object API

- bool/int/bigint/float/Unicode;
- list/tuple/dict ve builders;
- attribute/item/contains/length;
- positional ve keyword calls;
- error indicator, raise/fetch/match ve failure cleanup;
- normal, stress-GC ve invalid-handle testleri.

### H3 — Global, field ve moving GC

- `HPyGlobal` per-runtime persistent state;
- `HPyField` load/store, precise trace ve write barrier;
- tracker ve module-state teardown;
- gerçek compaction sırasında extension field'ından erişilen nesnenin kimlik ve
  içerik doğruluğu;
- field/global cycle ve finalizer sırası.

Bu aşamanın ana mimari kabul testi, extension içindeki `HPyField` bir Tonic
nesnesini tutarken heap'in gerçekten compact edilmesi ve sonraki native çağrının
aynı logical nesneyi okumasıdır.

### H4 — Pure types ve protokoller

- `HPyType_Spec`, methods, members, get/set ve desteklenen slotlar;
- native payload allocation/zeroing/destruction;
- inheritance, descriptor binding ve type version invalidation;
- extension type GC traversal, cycles ve shutdown;
- unsupported slotlarda deterministic import/registration tanısı.

### H5 — Buffer ve execution state

- buffer producer/consumer sözleşmesini HPy sürümünün public yüzeyiyle eşle;
- owner/pin/no-move süresi ve release kuralları;
- leave/enter execution state, thread attach ve callback sınırları;
- Tonic'in global lock kullanmadığı modelde HPy çağrı güvenliği.

HPy sürümü gerekli portable API'yi vermiyorsa Universal yol CPython fonksiyonuna
kaçmaz; capability açık kalır ve import/compile katmanında tanı üretilir.

### H6 — Debug, Trace ve sağlamlaştırma

- context decoration ile Debug ve Trace modları;
- leak, use-after-close, double-close, builder/tracker misuse;
- fault injection, malformed module/type specs ve symbol audit;
- ASan/UBSan ve platform CI;
- version/capability geriye uyumluluk testleri.

### H7 — aHPy uyumluluk kapısı

Her aHPy testi exact aHPy ve HPy revision'ı kaydeder. Sıra:

1. constant-only module;
2. scalar function ve iterative Fibonacci;
3. list/dict/call/exception;
4. pure `cdef class` ve native scalar fields;
5. `HPyField` ile moving-GC testi;
6. inheritance ve supported slots;
7. buffer producer;
8. cypack portu;
9. murmurhash scalar adapter;
10. frozenlist supported subset.

aHPy'nin intentionally blocked NumPy/typed-memoryview pilotu Tonic'te yanlış
pozitife çevrilmez. aHPy compiler'ın reddettiği kaynak Tonic host desteği olarak
sayılmaz.

## Ortak doğrulama matrisi

Her desteklenen özellik için en az şu yollar gerekir:

- handwritten HPy Universal extension;
- aynı semantiği üreten aHPy extension;
- Tonic host normal ve stress-GC;
- HPy Debug ve Trace context;
- aynı `.hpy0` için desteklenen başka bir HPy hostta oracle;
- malformed/unsupported binary ve source için fail-closed tanı;
- shutdown sonunda sıfır local/persistent/field/global handle ve native payload.

CI ilk etapta Linux x86-64 ve macOS arm64 ile başlar; destek iddiası yapılmadan
önce Linux arm64, macOS x86-64 ve Windows x64 eklenir. GCC/Clang/MSVC kapsamı
platforma göre kaydedilir.

## Performans bütçesi

Ölçümler şu yolları ayrı raporlar:

1. doğrudan Tonic-native C ABI;
2. handwritten HPy Universal;
3. aHPy-generated HPy Universal;
4. mevcut CPython bridge;
5. eş semantiğe sahip CPython + HPy Universal oracle.

Cold import, module init, tek scalar call, toplu scalar call, attr/item, type
method, exception, builder, field/global, buffer ve büyük native kernel ayrı
ölçülür. Handle resolve sayısı, local handle peak, dup/close, context call sayısı,
allocation/copy bytes, GC pause, native code size ve unload/shutdown kaynakları
raporlanır. İki handle tablosu kullanılırsa ek indirection ölçülmeden korunmaz.

## Kapsam dışı ilk hedefler

- Mevcut `numpy.cpython-*.so` veya başka full-ABI wheel'i değişikliksiz yüklemek;
- CPython private struct/layout, `Py_TYPE` veya refcount macro emülasyonu;
- aHPy'nin reddettiği arbitrary Cython/NumPy C-API kaynağını sessizce kabul etmek;
- Tonic-specific HPy fork'u ile Universal taşınabilirliği erken bozmak;
- HPy context veya local handle'ı çağrı ömrü dışında saklamak;
- benchmark olmadan “sıfır maliyet” ya da paket uyumluluğu iddia etmek.

## Kaynaklar ve doğrulanan temel sözleşmeler

- [HPy overview](https://docs.hpyproject.org/en/stable/overview.html): alternatif
  runtime Universal ABI fonksiyonlarını sunar ve `.hpy0` library loader ekler.
- [HPy API reference](https://docs.hpyproject.org/en/stable/api-reference/index.html):
  Universal modda core API `HPyContext` function table üzerinden çağrılır.
- [HPy quickstart](https://github.com/hpyproject/hpy/blob/master/docs/quickstart.rst):
  Universal binary adlandırması ve `hpy0` yükleme örneği.
- [HPy debug mode](https://docs.hpyproject.org/en/stable/debug-mode.html): context
  decoration ile runtime'da leak ve invalid-handle denetimi.
- [HPy 0.9.0 PyPI kaydı](https://pypi.org/project/hpy/0.9.0/): sabitlenen
  source distribution ve SHA-256 kaydı.
- aHPy değerlendirmesi: `mburakmmm/aHPy` revision
  `880d46d7d348df759ef062711ab3b4876bd648b8`; proje sözleşmesi,
  `support-matrix.md` ve pinned pilot matrisi incelendi.
