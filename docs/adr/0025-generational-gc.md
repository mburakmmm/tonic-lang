# ADR 0025 — Nursery ve remembered set

## Durum

Kabul edildi — 9 Eylül 2026.

## Karar

Tonic heap'i logical handle/slot tablosunu koruyarak iki yaş sınıfı kullanır.
Yeni nesneler nursery'de başlar. Minor collection yalnız nursery nesnelerini
precise roots ve remembered old owner'ların managed kenarlarından izler. Bir minor
collection'dan sağ çıkan young nesne old alana terfi eder. Her 32. otomatik
collection major'dır ve bütün heap graph'ını izler. `Vm::collect_garbage()` açıkça
çağrıldığında da major collection çalışır.

Old→young kenar doğruluğu owner-aware heap mutation API'lerinde sağlanır. List
append/index set, dict key/value set, closure cell, module member, class namespace,
instance ve class attribute yazımları ortak `write_barrier` sınırından geçer.
Persistent cache'ler raw nesne adresi tutmaz. Weak class-dependent metadata'sı
managed ownership kenarı olmadığı için trace veya barrier kapsamına girmez; major
collection sonrasında stale logical handle girdileri budanır.

Minor ve major collection aynı compaction/sweep mekanizmasını paylaşır. Minor
sweep old nesneleri mark durumundan bağımsız korur, erişilemeyen young nesneleri
toplar ve survivor'ları terfi ettirir. Major sweep erişilemeyen young ve old
nesnelerin tamamını toplar. Slot generation artışı ile stale handle doğrulaması
iki yolda da aynıdır. Collection yalnız VM instruction, JIT helper ve backedge
safepoint'lerinde çalışır; mutation ortasında veya native `Context` scope'u içinde
çalışmaz.

## Gerekçe

Tonic iş yüklerinde geçici nesneler baskındır. Her safepoint'te bütün old graph'ı
taramak allocation-heavy loop'ların maliyetini artırıyordu. Tek-minor survivor
terfisi basit ve ölçülebilir bir başlangıç politikasıdır; yaş sayacı veya birden
fazla survivor space gerektirmez. Sabit 32 minor/major oranı uzun yaşayan,
artık erişilemeyen old nesnelerin sınırsız kalmasını engeller ve VM instance'ları
arasında korunarak çok sayıda kısa `run` çağrısında da major collection üretir.

## Doğruluk kapıları

- Minor reclamation, survivor promotion ve sonraki major old-space reclamation
  ayrı unit testte doğrulanır.
- List append/set, cell, dict key/value ve module mutation'ları old→young
  kenarlarla sınanır.
- Class namespace ile instance/class attribute yazımları aynı invariant testine
  dahildir.
- Test-only heap denetimi, managed young child taşıyan her old owner'ın remembered
  set'te bulunduğunu mutation sonrasında doğrular.
- Otomatik minor/major sayaçları ve periyodu VM integration testinde doğrulanır.
- 269 çıktı ve 75 exception vakalık differential corpus debug/release,
  interpreter/JIT ve normal/stress-GC matrisinin sekiz kombinasyonunda geçer.

## Sınırlar

Bu karar bounded-pause veya gerçek zaman garantisi vermez. Tek collection'da
survivor'ı terfi ettirmek kısa ömürlü fakat bir safepoint yaşayan nesneleri old
alana taşıyabilir. VM şu anda initialized register'ları kesin root kabul eder;
CFG-live root bitmap'i eklenene kadar bazı ölü temporaries bir sonraki major'a dek
tutulabilir. Finalizer/finalization queue ve concurrent collection yoktur.

Ölçüm ve eski full-heap tabanıyla karşılaştırma
[GENERATIONAL_GC_BASELINE.md](../GENERATIONAL_GC_BASELINE.md) dosyasındadır.
