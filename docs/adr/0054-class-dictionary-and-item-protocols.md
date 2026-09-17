# ADR 0054: Tam class dictionary görünümü ve item protokolleri

## Durum

Kabul edildi ve uygulandı.

## Karar

Class attribute hot path'i string isimli kompakt vektör ve type-version guard'ını
korur. Üç argümanlı `type` namespace'indeki string olmayan hashable key'ler ayrı
traced key/value storage'da tutulur. Ortak dictionary-order metadata'sı string ve
diğer key'lerin insertion order'ını korur. Canlı mappingproxy lookup'u non-string
key'lerde dict key eşitliğini kullanır; iteration, length, truth ve repr aynı tam
görünümü okur. Sonradan `setattr` ile ekleme ve `delattr` ile silme sıra metadata'sını
eşzamanlı günceller.

Ordinary instance item işlemleri class MRO'sunda `__getitem__`, `__setitem__` ve
`__delitem__`
arar. Function, staticmethod ve classmethod binding mevcut tahsissiz
`DescriptorCall` yolunu kullanır. Guest method normal VM frame'inde askıya
alınabilir. Mutation hook dönüşleri dil semantiğine uygun biçimde atılır ve opcode
sonucu `None` olur. `del list[index]` negatif indexleri normalize eder; dict silme
insertion order'ı koruyup structural version'ı artırır. Instance üzerine aynı
isimle yazılan attribute special-method
lookup'u gölgelemez.

## GC ve performans

Non-string class key ve value'ları precise class edge'idir; namespace completion
write barrier'ı ikisini de kaydeder. String attribute lookup'a yeni hash lookup veya
managed key allocation eklenmez. Custom item protokolü bulunmayan list/dict/slice
işlemleri mevcut doğrudan Heap yolunda kalır.

## Doğrulama

Katman testleri numeric-equal mappingproxy key lookup'unu, insertion iteration'ını,
canlı class mutation'ını, instance shadowing'i, static/class binding'i ve stress-GC
içinde askıya alınan item methodunu; list/dict silme ve hata türlerini kapsar.
CPython differential corpus 274 çıktı ve 88 exception vakasına genişletilmiştir.
