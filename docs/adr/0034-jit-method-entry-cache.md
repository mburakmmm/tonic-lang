# ADR 0034 — Native giriş kapsamlı method dependency cache

## Durum

Kabul edildi — 10 Eylül 2026.

## Karar

Guarded direct-method lookup her loop iterasyonunda runtime helper çağırmaz.
Her method site için JIT root buffer'ın guest register'lardan sonraki gizli
kuyruğunda dört `Value` kelimesi ayrılır. İlk gerçek `ATTR` yürütmesinde helper
güncel owner üzerinden C3/descriptor lookup yapar ve şu değerleri cache'e yazar:

```text
function, implicit receiver, exact owner, initialized
```

Sonraki yürütmeler current owner'ı cached owner ile ve cached function'ı
compile-time exact logical function handle'ıyla karşılaştırır. İki guard da özgün
`ATTR` PC'sine deopt eder. Cache function girişinde uninitialized yapılır; böylece
önceki çağrıdan veri taşımaz ve class/base rebinding sonraki native girişte yeniden
lookup edilir.

Cache girişte eager doldurulmaz. OSR loop hedefi owner register'ını `ATTR` öncesi
hazırlayabilir; ilk `ATTR` noktasında lazy initialization gerçek current owner'ı
görür. Bu ayrıntı yanlış bir pre-loop register değerinin dependency cache'e
alınmasını engeller.

## GC ve invalidation

Cache raw pointer değil logical slot+generation `Value` taşır. Function, receiver
ve owner alanları her helper ve backedge poll'a verilen kesin kök dilimine dahildir.
Native giriş bu alanları önce `UNBOUND`, initialized alanını immediate `False` ile
doldurur; dolayısıyla ilk lookup öncesinde de root scanner başlatılmamış bellek
görmez. VM kök buffer'ını metadata'daki `root_count` kadar ayırır ve dönüşte yalnız
guest-visible `register_count` önekini interpreter frame'ine kopyalar.

Bu açık kök kuyruğu owner'ın geçici guest register'ı sonradan yeniden kullanılsa
bile cached function/receiver/owner'ın collection boyunca canlı kalmasını sağlar.
Side exit/resume yeni native giriş oluşturduğunda cache yeniden initialize edilir.

Owner loop içinde değişirse owner guard hemen deopt eder. Class veya base başka bir
çağrı arasında rebound edilirse lazy helper yeni function'ı döndürür ve exact
function guard deopt eder. Böylece cache invalidation persistent heap adresine ya
da sessiz stale metadata'ya dayanmaz.

## Ölçüm

100.000 çağrılı aynı koşuda:

| Vaka | Adaptive | Entry-cache JIT | Oran | Helper lookup |
|---|---:|---:|---:|---:|
| Bound method | 25,090 ms | 0,926 ms | 27,11× | 1 |
| Staticmethod | 21,526 ms | 0,868 ms | 24,81× | 1 |
| Classmethod | 25,469 ms | 0,853 ms | 29,86× | 1 |

Önceki per-call helper koşusuna göre direct süreler sırasıyla yaklaşık 2,29×,
2,72× ve 2,79× iyileşmiştir. Unit test 2.500 loop iterasyonunda tek helper lookup,
backedge poll kök diliminde cached function/owner ve her native invocation'da
yeniden initialization; integration testi alternating owner deopt'u, mevcut testler
class rebinding ve stress GC davranışını doğrular.

## Sınırlar

Owner-polymorphic loop ilk değişimde generic interpreter'a deopt eder. Ölçüm
gerekçelendirirse iki girişli native owner cache eklenebilir. Inline target hâlâ
yan etkisiz direct-leaf alt kümesidir; native kod içinde observable class mutation
desteklenirse ayrıca version poll/guard gerekir.
