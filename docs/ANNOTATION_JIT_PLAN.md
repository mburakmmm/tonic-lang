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
