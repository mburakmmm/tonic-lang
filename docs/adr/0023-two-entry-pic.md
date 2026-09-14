# ADR 0023 — İki girişli polymorphic inline cache

## Durum

Kabul edildi — 5 Eylül 2026.

## Karar

Adaptive interpreter'daki monomorphic basit Tonic-call ve instance shape-slot
cache'leri, guard miss sonrasında ikinci geçerli hedef gözlenirse iki girişli
polymorphic inline cache'e terfi eder. Call girdisi exact callee handle ve code
ID; attribute girdisi class handle, shape, slot ve dependency version taşır. İki guard da
başarısız olursa aynı opcode generic semantik yolda yürür.

PIC payload'ları her `AdaptiveState` değerini büyütmez. Yalnız terfi eden siteler
VM-owned yan tablolara indekslenir. Her yan tablo programdaki toplam bytecode
instruction sayısıyla sınırlıdır; instruction başına cache genişliği iki girdidir.
Run sınırında state ve tablolar temizlenir. Handle'lar weak guard verisidir;
generation doğrulaması slot reuse nedeniyle yanlış eşleşmeyi engeller.

## Ölçüm

100.000 iterasyonluk workload önce tek hedefle monomorphic cache'i ısıtır, ardından
iki function veya iki instance shape arasında dönüşümlü çalışır. Değişiklik öncesi
adaptive medyan call için 32,942 ms, attribute için 31,814 ms idi. PIC sonrasında
medyanlar 29,972 ms ve 26,420 ms oldu. Aynı son binary'nin generic moduna göre
kazanımlar yaklaşık %7 ve %13,6'dır.

PIC üçüncü hedefi genelleştirmez. Daha geniş cache ancak ayrı ölçümle eklenir.
Class guard'ının dependency-version invalidation'ı [ADR 0024](0024-class-dependency-invalidation.md)
ile ayrıca tanımlanır.
