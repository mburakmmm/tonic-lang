# ADR 0070: Hash protokolü ve suspending container karşılaştırması

## Durum

Uygulandı. Bu karar `hash`, `object.__hash__`, builtin immutable değerlerin hash'i,
custom `__hash__`, list/tuple/dict/slice içi eşitlik ve list/tuple lexicographic
rich comparison yollarını kapsar.

## Karar

Hash değeri heap adresinden değil Tonic'in stable logical handle/value kimliğinden
üretilir. Integer, integral float, string, range, tuple, slice, bound method ve
identity-hash kullanan nesneler ortak dil hash yardımcılarını kullanır. Eşit builtin
değerler aynı hash'i verir; algoritmanın kendisi dil garantisi değildir. `-1`
sonucu `-2` olarak normalize edilir, arbitrary-precision custom sonuçlar signed
hash genişliğine indirgenir. List, dict, buffer ve mappingproxy hashlenemez.

`hash(x)` special-method lookup'u instance sözlüğünü atlayıp class/metaclass MRO ve
descriptor kurallarıyla yapar. Guest `__hash__` normal VM frame'inde çalışır;
sonuç int/native-int/bool değilse `TypeError` üretilir. Sınıf gövdesi `__eq__`
tanımlayıp açık `__hash__` tanımlamazsa class completion `__hash__ = None` ekler.
Immutable builtin alt sınıfları native backing hash'ini, sıradan object alt
sınıfları logical identity hash'ini kullanır. `int.__hash__`, `float.__hash__`,
`str.__hash__`, `tuple.__hash__` ve `range.__hash__` doğrudan çağrıldığında da
descriptor sahibine uygun receiver tipi doğrulanır.

Dictionary index'i hash başına candidate vektörü tutar. Aynı hash hiçbir
zaman eşitlik sayılmaz: lookup/set/delete, constructor pair'leri, dict kopyası ve
`**` merge her adayı normal suspending `__eq__` ve truth protokolünden geçirir.
Guest karşılaştırması sırasında boyut değişirse stale index kullanmak yerine
`RuntimeError` üretilir. Entry sırası korunur; value update dictionary version'ını
değiştirmez.

`HashAction`, `EqualityAction`, dictionary operation/merge state'leri ve binary/
truth completion'ları pending owner, key, value, candidate container ve kalan
sequence değerlerini precise GC roots olarak taşır. Tuple/slice hash'i ile
list/tuple/dict/slice equality iç içe değerleri aynı motorla işler. List/tuple
sıralaması önce suspending equality, ilk farklı elemanda ilgili rich comparison
protokolünü yürütür. Ayrı cyclic container'lar bounded nesting sonunda
`RecursionError` verir.

Cranelift exact-int karşılaştırma guard'ı dynamic container geldiğinde exact PC'de
deopt eder. Interpreter aynı opcode'u continuation motoruyla yeniden yürütür;
guest hook veya allocation JIT helper içinde gizlice çalıştırılmaz.

## Doğrulama

Runtime testleri suspending hash/equality/truth gövdelerini, deliberate collision,
cross-type builtin/custom key'i, insert/get/update/delete, dict constructor/copy/
unpack, dictionary value equality, slice key'i, nested equality/order, cyclic
RecursionError ve exact-int JIT deopt'unu stress GC altında kapsar. Python 3.14.6
differential corpus'u 299 çıktı ve 147 exception vakasını debug/release,
interpreter/JIT ve normal/stress-GC matrisinde karşılaştırır.
