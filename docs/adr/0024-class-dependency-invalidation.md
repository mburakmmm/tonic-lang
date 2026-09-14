# ADR 0024 — Class/MRO dependency invalidation

## Durum

Kabul edildi — 5 Eylül 2026.

## Karar

Instance-slot inline cache guard'ı global class epoch yerine instance'ın exact
class sürümünü kullanır. Bir class attribute eklendiğinde, değiştirildiğinde veya
silindiğinde owner class ile MRO lookup sonucu etkilenebilecek bütün mevcut alt
sınıfların sürümü artırılır.

Class oluşturulurken her ancestor kendi `dependents` listesine yeni descendant'ın
logical handle'ını ekler. Bu bağlantı weak metadata'dır ve object graph tracing'e
dahil edilmez. Full GC ölü veya generation'ı değişmiş descendant girdilerini
listeden temizler. Mutation common path'i descendant yoksa heap taramaz ve target
vektörü allocation'ı yapmaz. Sürüm tükenmesi bütün değişikliklerden önce kontrol
edilir; kısmi invalidation oluşmaz.

Bir instance slot adı MRO'da halihazırda herhangi bir class attribute ile
çakışıyorsa cache kurulmaz. Sıradan görünen bu değer kendi class'ı sonradan
`__get__`/`__set__` alınca descriptor'a dönüşebilir; bu transitive dependency'yi
hot guard'a eklemek yerine söz konusu daha seyrek şekli generic bırakmak hem basit
hem güvenlidir. MRO'da bulunmayan bir ad sonradan eklendiğinde owner/descendant
version invalidation mevcut cache'i düşürür.

Bound JIT globals compile-time kimlik varsayımı taşımadığı ve her entry'de güncel
raw slice'ı okuduğu için ayrıca global version guard istemez.

## Ölçüm

`Target` instance alanı 100.000 kez okunurken her iterasyonda ilgisiz `Noise`
class'ı değiştirildi. Eski global epoch ile adaptive medyan 18,980 ms idi. Granüler
dependency sürümüyle aynı workload 13,062 ms oldu; son binary'nin generic 18,900
ms sonucuna göre %30,9 kazanç sağlandı. İlgili base mutation testi descendant
sürümünü değiştirir, tam bir cache miss görür ve sonrasında descriptor semantiğine
geçer.
