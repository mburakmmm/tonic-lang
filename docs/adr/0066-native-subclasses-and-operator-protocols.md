# ADR 0066: Native builtin alt sınıfları ve operator protokolleri

## Durum

Kısmen uygulandı. Bu karar varsayılan kurucu yolundaki `int`, `float`, `str`,
`list`, `tuple` ve `dict` alt sınıflarının native depolamasını; temel binary,
reflected, in-place, rich-comparison, unary ve `abs` protokollerini kapsar.
Canonical builtin `__new__`/`__init__`, conversion/index/hash, power/bitwise ve
container içi suspending element comparison ayrı açık kapsamdır.

## Karar

Builtin alt sınıf instance'ı ordinary instance header'ını korur: class handle ve
shape tabanlı attribute alanları dış nesnededir. `native` alanı exact builtin
backing değerine logical handle taşır. Böylece guest-visible kimlik ve kullanıcı
alanları korunurken mevcut kompakt int/float/string/list/tuple/dict kodu yeniden
kullanılır. Backing handle precise tracer tarafından izlenir; taşınabilir GC raw
pointer görmez. Mutation write barrier'ı backing owner üzerinde, instance alanı
barrier'ı dış owner üzerinde çalışır.

Exact builtin ve immediate fast path'leri wrapper ödemez. Yalnız instance/class
operandı operator protokol lookup'una adaydır. Native alt sınıf özel metot
tanımlamıyorsa backing'e normalize edilerek generic builtin işlemine düşer. Sonuç
Python gibi çoğunlukla exact builtin tipidir; `list +=` dış instance kimliğini
koruyup backing'i yerinde değiştirir. Bool ve range final layout kabul edilerek
alt sınıflanamaz; birden fazla uyumsuz native layout class tamamlanırken reddedilir.

Iterable list/tuple/dict kurucuları guest `__iter__`/`__next__` çağrısında askıya
alınabilir. `NativeSubclassFinish`, class'ı ve özgün argümanları collection/dict
continuation state'inde kesin kök olarak taşır. Backing tamamlandıktan sonra dış
instance oluşturulur ve mevcut `finish_new` zinciri kullanılır. Bu tasarım
constructor için ayrı host stack veya conservative root gerektirmez.

Operator dispatch direct ve reflected adayları önceden sınırlar, strict RHS alt
sınıfının reflected metoduna öncelik verir ve aynı tipte reflected çağrıyı tekrar
etmez. Guest method `NotImplemented` döndürürse sonraki aday denenir; `__iadd__`
sonrasında `__add__`/`__radd__`, `__ne__` sonrasında truth değeri terslenen
`__eq__` zinciri çalışır. Tüm guest çağrıları normal VM frame'lerinde yürür.
`ReturnAction::BinaryProtocol` operandları, kalan callable/receiver/argument
değerlerini; unary action özgün operandı precise root olarak saklar. Bütün adaylar
kaçırırsa mevcut builtin generic semantiği kullanılır.

JIT yalnız kanıtlanmış exact-tag yollarını native tutar. Native alt sınıf veya
user operatorü tag guard'ını kaçırdığında özgün bytecode PC'sinde interpreter
semantiğine döner; protocol çağrısı native fast path tarafından atlanmaz.

## Doğrulama

Runtime testleri altı builtin alt sınıfın type/isinstance kimliğini, instance
alanlarını, aritmetik sonuç tipini, hashing, slicing, iteration, mutation,
custom iterable ve stress-GC köklerini kapsar. Ayrı JIT testi sıcak immediate-int
callee'nin native subclass operandında deopt edip doğru exact-int sonucu verdiğini
kanıtlar. Operator testi direct/reflected sıra, strict subclass önceliği,
`NotImplemented`, in-place fallback, arithmetic/comparison/unary yolları,
metaclass operatorü ve karşılaştırma sonucunun suspending truth dönüşümünü kapsar.
Python 3.14.6 differential corpus'u aynı vakaları interpreter/JIT ve normal/stress
GC koşullarında karşılaştırır.
