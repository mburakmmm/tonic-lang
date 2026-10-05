# ADR 0105 — Annotation planlı first-call JIT ve giriş guard'ları

## Durum

Kabul edildi; exact `int`/`float` leaf fonksiyonlar için ilk typed giriş dilimi
uygulandı.

## Bağlam

TypePlan v1 annotation değerlerini canonical metadata'ya dönüştürüyordu, ancak
tier seçimini etkilemiyordu. Mevcut profile-JIT küçük fonksiyonları bilinçli olarak
adaptive interpreter'da tutuyor ve float specialization için gözlenmiş çağrıları
bekliyordu. Annotation'ın Cython benzeri hız yoluna anlamlı katkı vermesi için
desteklenen bir planın ilk çağrıda güvenli derleme adayı olması gerekir. Python
annotation'ı bir runtime type sözleşmesi olmadığı için her başarısız varsayım aynı
programı generic bytecode yolunda yürütmelidir.

## Karar

Bütün bound parametreleri ile dönüş annotation'ı exact `int` veya exact `float`
olan ve Cranelift'in mevcut verified-bytecode subset'ine giren fonksiyonlar ilk
çağrıda typed baseline adayıdır. Bu aday normal hotness, direct-call profiling ve
küçük-leaf kârlılık bekleyişini atlar; code budget, bytecode verifier ve desteklenen
opcode kapıları aynen kalır.

Compiled entry aşağıdaki runtime-owned guard snapshot'ını taşır:

- logical function handle;
- code id ve execution id;
- annotation dict logical handle ile content epoch'u;
- canonical TypePlan hash'i;
- parametre scalar planları ve dönüş scalar planı.

Girişten önce function/plan ve gerçek argümanlar doğrulanır. Yanlış argüman yalnız
o çağrıyı interpreter'a yollar; sağlam compiled entry korunur. Annotation content
mutasyonu, dict replacement/delete veya class dependency version değişimi planı
stale yapar; entry invalid edilir ve uygun sonraki çağrı yeniden derleyebilir.
Native dönüş annotation'la uyuşmazsa `RETURN` PC'sinde deopt edilir. Böylece
annotation hiçbir yeni `TypeError` eklemez.

`function.__annotations__` Python'a uygun biçimde dict veya `None` kabul eder;
silme ve `None` sonraki okumada boş dict verir. Başka değerler `TypeError` üretir.
Tonic 0.x eager annotation semantiğinde global bir alias adını function
definition'ından sonra rebind etmek eski evaluated annotation nesnesini değiştirmez;
bu nedenle stale plan sayılmaz. Yeni definition güncel alias değerinden yeni plan
üretir.

## Sonuçlar

Exact float parametreler mevcut F64 data-flow, safepoint stack-map ve boxing-on-
return yolunu kullanır. Exact int işlemleri mevcut immediate tag guard'larıyla
çalışır ve overflow/BigInt sınırında exact-PC deopt eder. Bu karar henüz genel
typed SSA overlay, raw unboxed integer call ABI, container specialization,
`@tonic.compile` warmup politikası veya AOT cache sağlamaz. Bu kapılar roadmap'te
açık kalır.
