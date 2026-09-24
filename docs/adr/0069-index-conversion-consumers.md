# ADR 0069: Index conversion tüketicileri

## Durum

Uygulandı. Bu karar mevcut yürütülebilir dil yüzeyinde `range`, list/tuple/string/
range scalar erişimi, list set/delete, read-only slice bileşenleri, explicit
`int(..., base=...)` ve `__len__` sonuçlarının `__index__` dönüşümünü kapsar.
Henüz desteklenmeyen string/sequence repetition ve range slice bu kararın
kapsamında değildir.

## Karar

Bütün tüketiciler `IndexConversion` continuation'ını kullanır. Exact int ve bool
allocation yapmadan normalize edilir; native int subclass backing'i exact int
değerine açılır. Diğer nesnelerde `__index__` instance attribute'una bakmadan
class MRO üzerinden descriptor kurallarıyla çözülür ve normal VM frame'inde
çalışır. Sonuç int/native-int/bool değilse `TypeError` üretilir.

Continuation enum'u tüketiciye özgü state taşır: range'in kalan argümanları,
slice owner/bileşen/sıradaki konumu, item owner ve mutation değeri, explicit-base
string ile pending native subclass finish'i, length ve truth jump action'ı.
State tracer bütün guest değerlerini precise root olarak bildirir; guest metodu
içinde her allocation sonrası collection yapılsa bile raw pointer veya host-stack
varsayımı yoktur.

Protocol sınırı container dispatch'inden sonra uygulanır. Custom `__getitem__`,
`__setitem__` ve `__delitem__` anahtarı aynen görür; dict ve mappingproxy anahtarı
kimlik/hash semantiği için dönüştürülmez. Yalnız exact/native list, tuple, string
ve range builtin fallback'i index conversion ister. Slice bileşenleri soldan
sağa dönüştürülür ve `None` açık uçları korunur.

`__len__` exact int yerine indexable değer döndürürse aynı continuation kullanılır;
negatif sonuç yine `ValueError`, makine length sınırını aşan sonuç `OverflowError`
verir. Explicit int base arbitrary-precision sonucu 0 veya 2..36 dışında olduğunda
host dönüşüm taşması yerine `ValueError` üretir.

JIT bu dinamik protokolü native olarak lower etmez. Desteklenmeyen item/range
bytecode'u doğrulanmış interpreter fallback'inde kalır; JIT caller içinden girilen
çağrı da normal VM continuation ve exact-PC dönüş sözleşmesini kullanır.

## Doğrulama

Stress-GC runtime testi suspending index metotlarıyla range, get/set/delete,
negative index, Unicode string, tuple, slice, int base, len ve truth tüketicilerini
kapsar. Aynı test dict anahtarının dönüştürülmediğini ve custom `__getitem__`
önceliğini doğrular. Yanlış sonuç tipleri, negatif length, bool dönüş ve çok büyük
int base hata yolları Python 3.14.6 differential corpus'unda interpreter/JIT ve
normal/stress-GC koşullarıyla karşılaştırılır.
