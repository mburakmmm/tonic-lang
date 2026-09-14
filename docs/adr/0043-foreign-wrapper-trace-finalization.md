# ADR 0043 — Foreign wrapper trace ve ertelenmiş payload destruction

Durum: uygulanmış. 12 Eylül 2026.

## Karar

Foreign nesne, Tonic GC'nin yönettiği küçük bir wrapper'dır. Wrapper dış payload
adresini opaque integer olarak, ABI v1 vtable'ının kopyalanmış adapter kimliği,
trace ve destroy function pointer'larıyla taşır. JIT/shape exact-type varsayımları
bu nesnelere uygulanmaz.

Dış payload'ın Tonic değer alanları `foreign_reference_create` ile ayrı logical
handle alır. Bu handle'lar global root değildir. Vtable trace callback'i yalnız
`TonicTraceVisitor` üzerinden o handle'ları bildirir; runtime handle kind/runtime
sahipliğini doğrulayıp karşılık gelen `Value` kenarlarını wrapper'ın precise trace
listesine yazar. Liste her normal GC'den önce yenilenir. Kaldırılan foreign
reference handle'ları otomatik release edilir; old wrapper'ın yeni nursery kenarı
remembered set'e eklenir. Duplicate, stale, local veya cross-runtime referans
collection başlamadan `ForeignError`/`HandleError` üretir.

`TONIC_FOREIGN_OWNED` yalnız destroy callback'iyle geçerlidir; borrowed payload
destroy callback'i alamaz. Sweep erişilemeyen wrapper'dan payload sahipliğini ve
reference handle'larını pending queue'ya taşır. Collector slot/compaction işini
bitirdikten sonra queue handle'ları bırakır ve destructor'ı bir kez çağırır.
Callback başlamadan sahiplik tüketildiği için panic tekrar denemeye yol açmaz;
panic ABI sınırında tutulur ve sayaçlanır. Shutdown boş-root major collection,
queue drain ve ancak sonra toplu handle invalidation sırasını izler.

## Sonuçlar

Tonic list→foreign wrapper→aynı list döngüsü global root varken korunur, root
kalkınca major GC'de iki tarafıyla birlikte toplanır ve payload destructor'ı bir
kez çalışır. Aynı garanti shutdown, tekrar collection, trace panic ve destructor
panic testleriyle kapsanır. Old wrapper'ın bildirilen kenarı nursery nesnesiyle
değiştirildiğinde refresh eski handle'ı bırakır, yenisini remembered set üzerinden
korur. `foreign_wrapper_creations`, `foreign_trace_calls`,
`foreign_destructor_calls` ve `foreign_destructor_panics` sayaçları maliyeti ve
hatalı extension davranışını görünür kılar.

100.000 no-trace owned wrapper oluşturma + toplama 33,675 ms medyandır; eş sayıdaki
managed list baseline'ı 19,670 ms'dir. Yöntem
[`FOREIGN_LIFECYCLE_BASELINE.md`](../FOREIGN_LIFECYCLE_BASELINE.md) içindedir.

## Sınırlar

Vtable v1 yalnız trace ve lifecycle slotlarını taşır; attribute/call/repr adapter
slotları CPython bridge ile birlikte eklenecektir. Foreign reference mutation'ı
bir sonraki safepoint trace'ında görünür olur. Dynamic library unload yoktur;
vtable kodu runtime/payload yaşamı boyunca yüklü kalmalıdır. User-language
`__del__`, resurrection, finalization roots ve bounded pause ayrı roadmap
maddesidir.
