# ADR 0002 — Lexical hücreler, çağrı bağlama ve precise collector

Durum: uygulanmış bootstrap; public stable ABI değildir. ADR 0001'in arena,
scope ve çağrı kararlarını genişletir. 31 Ağustos 2026.

## Problem

Kaçan closure ve mutable defaults çağrı frame'inden daha uzun yaşayabilir.
Kökleri eksiksiz izlenmeyen bu nesneler moving GC için güvenli değildir.
İlk arena bütün nesneleri tuttuğundan uzun allocation döngülerinde bellek büyürdü.
Keyword/variadic çağrılar ise normal positional çağrılara guest tuple/dict
maliyeti yüklemeden eklenmelidir.

## Seçenekler ve kararlar

Değer kopyalayan closure, `nonlocal` ve late binding semantiğini karşılamaz.
Bu yüzden yalnızca yakalanan binding'ler `Cell(Value)` nesnesidir. Ara scope'lar
hücreyi iletir; normal local register'da kalır. Whole-function çözümleme global
bildirimlerini, nonlocal aramasını, parametre çatışmalarını ve unbound davranışını
compile aşamasında ayırır. Comprehension/class scope ve lambda henüz eklenmemiştir.

Defaults tanımın bulunduğu scope'ta bir kez hesaplanır ve fonksiyonda saklanır.
Parametreler positional, keyword-only, vararg ve kwarg register sırasındadır.
Sabit çağrı bir contiguous register penceresini okur; gerçek `*args/**kwargs`
parametreleri varsa dilin gözlenebilir tuple/dict nesneleri oluşturulur.
Expanded çağrıların geçici bilgisi VM scratch vektörlerinde tutulur. Sıradan
çağrılar için zorunlu tuple/dict veya dinamik trait arayüzü eklenmez.

`*` değerlendirme/materialization ve `**` birleştirme sırası test edilir.
Ardışık named keyword grubu birleştirilmeden önce değerlendirilir; duplicate
keyword hatası bu grubun sonraki yan etkilerini atlayamaz. Non-string `**` key
hatası argüman değerlendirmesi sonunda bildirilir; duplicate merge kontrolü
birleştirme sırasında yapılır. Bekleyen geçersiz key/value'lar bile GC köküdür.
Native kayıt API'si positional arity kullanmaya devam eder. `print` sep/end
dışındaki builtin keyword davranışları genişletilmemiştir.

Bytecode v2: cell load/store, function capture/default metadata, signature,
keyword call windows, expanded argument işlemleri ve dict/item mutation.
Verifier yeni operand ve metadata sınırlarını, ayrıca reachable CFG boyunca
argument builder derinliğini doğrular. Join derinlikleri tutarlı olmalı; Return
bekleyen builder bırakamaz. Dosyaya bytecode cache formatı henüz yayımlanmaz.

Refcount ve tüm nesneleri hareketsiz tutan arena yerine stop-the-world precise
mark/compact seçildi. Daha karmaşık nursery/remembered set öncesi gerçek kök ve
hareket sözleşmesini doğrulamak amaçlanır. Bu collector performans hedefinin
son hali değildir; her seferinde tüm heap'i izler.

İç `Value` düşük 3-bit tag kullanır. Heap değeri 32-bit logical slot ve 29-bit
generation taşır; immediate signed integer 61 bit kalır. Slot fiziksel object
indeksine işaret eder. Canlı Object girişleri Vec içinde sıkıştırılır; payload
Vec/String backing allocation'ları ayrıca kopyalanmaz. Boş slot generation'ı
artarak yeniden kullanılır; tükenmiş slot emekliye ayrılır, wrap yoktur.
Native public Handle'ın bağımsız token/slot sözleşmesi değiştirilmemiştir.

## GC kökleri ve mutation sınırları

- VM registerları, closure cell stack'i ve aktif frame callable'ları.
- Globals, bütün loaded constants, builtin registry ve native module registry.
- Hazırlanan expanded args, deferred star, geçersiz keyword dahil key/value'lar.
- Native local ve explicit persistent handle tabloları.

Object trace: list/tuple elemanları, dict key/value'ları, cell değeri,
function captures/defaults, module üyeleri ve iterator kaynağı. Scalar nesneler
managed edge taşımaz. Mark aşaması root/edge validation'ı bitirmeden heap'i
değiştirmez; geçersiz root hata verdiğinde yarım sweep oluşmaz.

`append_list`, `set_item`, `dict_set`, `store_cell`, `add_module_member` mutation
sınırlarıdır. Full collection için write barrier gerekmez. Nursery eklendiğinde
bu sınırlar remembered set/barrier taşımalı; mutable raw alanlara dış erişim
açılmamalıdır. Şu an tek nesil vardır: generation token, GC yaş nesli değildir.

Safepoint instruction öncesindedir; heap allocation ve mutation içinde collector
çağrılmaz. Native Context scope'u bitmeden collection/callback yapılmaz; dış
borrow bunu safe Rust'ta sınırlar. Collector içinde allocation/reentry/finalizer
yoktur. Finalizers, JIT stack maps, generators ve foreign payload kaynakları
mevcut olmadığından bunlara ilişkin doğruluk iddiası yoktur.

## Doğrulama, maliyet ve geçiş riskleri

Kaçan/paylaşılan/recursive closure, mutable default, arg binding, hatalardaki
yan etkiler, dict insertion order ve numeric key equivalence testleri vardır.
GC testleri cycle, slot reuse/stale handle, generation tükenmesi, hareketten
sonra native persistent referans, hata cleanup ve bağımsız graph reachability
modeline karşı 3.200 allocation'ı kapsar. Python corpus'u her allocation sonrası
collection modunda da çalıştırılır. Finalizer/JIT/foreign cycle testi sayılmaz.

Ara ölçümler [STAGE2_BENCHMARKS.md](../STAGE2_BENCHMARKS.md) içindedir.
Yeni doğrulama/bağlama katmanları bazı süreleri artırır. Float sonuçları boxed,
genel list iteration allocation'lı, dict hash malzemesi bootstrap kopyalıdır.
Bu kayıt hızlanma, production hazır olma veya nihai benchmark iddiası taşımaz.

Kısıtlar: bütün initialized registerlar root sayıldığından ölü temporary'ler
overwrite/frame çıkışına kadar yaşayabilir. Heap/table kapasitesi düşmeyebilir.
Allocation-count tetikleyicisi byte bütçesi değildir; uzun native çağrı GC'yi
geciktirir. Finalizer/destructor sırası tanımlanmamıştır. Her `Vm::run` fresh
module bağlamı açar; önceki run'ın callable'ı yine reddedilir. Module ownership,
callback/reentry ve JIT eklendiğinde bu yaşam süresi modeli tekrar ele alınmalıdır.
