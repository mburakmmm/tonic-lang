# Dil tamamlandıktan sonra nihai benchmark

Durum: bekliyor. Stage 2 ölçümleri yalnızca çalışan dilimin regresyon verisidir.
Kullanıcının istediği ayrıntılı nihai benchmark bu kabul kapıları geçilmeden
tamamlandı diye sunulmayacaktır.

## Çalıştırma önkoşulları

ROADMAP'teki dil/runtime kapsamı tamamlanmalı; bilinen uyumsuzluklar ve hedef
Python sürümü açıkça tanımlanmalıdır. Özellikle class/shape/descriptor, exception,
generators/async, imports, generational GC, adaptive interpreter, gerçek Cranelift,
guard/deopt ve interop aşamalarının testleri gerekir. Backend kapalıysa bir
emülasyon sonucu JIT ölçümü diye yazılmaz. Unsupported workload atlanırsa nedeni
ve karşılaştırmanın kapsamı raporda görünmelidir.

## Karşılaştırma matrisi

| Yol | Ayrı ölçülecekler |
|---|---|
| Cold CLI | process start + file/import + parse/compile/verify + ilk execution |
| Tier 0 | compile edilmiş generic register interpreter |
| Tier 1 | adaptive interpreter, warmup/hotness eğrisi ve guard miss |
| Tier 2 | Cranelift compile süresi, ilk native call, warm throughput |
| Deopt | guard failure, interpreter state reconstruction, tekrar specialization |
| GC | normal nursery, farklı heap budgetları, stress doğruluğu, allocation-heavy ve retained-live graph |
| Native | doğrudan Rust baseline, Tonic function, Context/Handle sınırı, typed zero-copy buffer |
| HPy | handwritten ve aHPy-generated Universal; import/init, scalar/bulk call, field/global/type/buffer ve Debug/Trace overhead |
| Bridge | primitive convert, proxy/cache, callback, büyük buffer, ownership/cleanup maliyeti |
| Python referansı | eş semantiğe sahip CPython ve kuruluysa PyPy; sürüm ve seçenekler sabitlenir |

## İş yükleri

Mikro: integer/float, locals, positional/keyword/variadic calls, closure,
method/attribute, shape mutation, instance creation, list iteration, dict lookup
ve insertion, string işlemleri, raise/catch, allocation ve cycle collection.
Stabil monomorphic site yanında değişen tür/shape/global binding de olmalıdır.

Makro: metin sayımı, veri dönüştürme, graph traversal, sayısal array kernel,
recursive/iterative hesaplar, async task zinciri ve module import ağı.
Her iş yükü için doğrulanmış output/checksum gerekir. Native/JIT hızını yalnızca
sayısal tight loop ile genel Python hızı diye genellememek gerekir.

## Metrikler ve yöntem

- Wall/user/system zaman, anlamı açıklanmış operation/s ve VM dispatch sayısı.
- Cold startup ile warm execution ayrı; compile/teardown dahil-hariç açık yazılır.
- Guest object allocation ve bütün host allocator count/bytes ayrı sayaçlar.
- Yaşayan heap payload, reserved capacity, iş yükü başına peak RSS ayrı.
- GC toplam pause, p50/p95/p99/max pause, reclaimed bytes/objects, live set,
  minor/major sayısı ve allocation throughput.
- JIT compile latency, generated code bytes, hotness eşiği, cache hit/miss,
  guard/deopt sayısı ve yeniden derleme sayısı.
- Interop copy bytes, materialized arguments, handles/scopes, buffer pin duration,
  callback maliyeti ve shutdown sonunda canlı kaynak sayısı. Tonic-native ABI,
  handwritten HPy Universal, aHPy-generated Universal ve CPython bridge ayrı
  konfigürasyonlar olarak raporlanır.

Her konfigürasyonda en az 5 bağımsız process tekrarı ve 30 ölçüm örneği;
warmup ayrıca kaydedilir. İş yükü/konfigürasyon sırası dengelenir. Median,
min/max, p95 ve bootstrap confidence interval ham veriden üretilir; kısa
serilerden güvenilir p99 iddiası çıkarılmaz. Karşılaştırma oranları workload
başına verilir, farklı birimlerden tek ortalama hız skoru üretilmez.

Kaynak revision/hash, Cargo.lock, rustc/Cranelift/Python sürümleri, release
flagleri, CPU/OS/RAM, güç modu, thread count ve arka plan yükü kaydedilir.
Donanım/perf-counter erişimi yoksa eksik açıkça yazılır. Profiler sonuçları
optimizasyon kararını gerekçelendirir; tahmin benchmark yerine geçmez.

## Teslimatlar

Tek komutla tekrar çalıştırılabilir harness; raw CSV/JSON örnekleri; semantic
checksums; ortam manifesti; profiler kayıtları; karşılaştırmalı tablolar ve
standalone grafikler. Hızlanmalar kadar regresyonlar, bellek maliyeti ve ölçüm
sınırları da raporlanır. Mevcut `benches/capture.py` ve interpreter harness'i
bu nihai setin başlangıcıdır; henüz bütün matrisi çalıştırmaz.
