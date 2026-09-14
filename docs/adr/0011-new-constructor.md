# ADR 0011 — `__new__` constructor protokolü

Durum: uygulanmış bootstrap. 4 Eylül 2026. Public stable ABI değildir.

Her Tonic class çağrısı MRO üzerinde `__new__` arar. Class gövdesinde function
olarak tanımlanan `__new__`, class completion sırasında `StaticMethod` wrapper'ına
çevrilir; instance veya class üzerinden okuma ek bir receiver bağlamaz. Constructor
çağrısı yine de oluşturulacak class'ı ilk protokol argümanı olarak geçirir.

Kök `object` sınıfının `__new__` alanı internal `ObjectNew` builtin'idir. Normal
class çağrısında bu kimlik hızlı yol olarak tanınır ve doğrudan Tonic Instance
ayrılır. Açık `object.__new__(cls)` ve `super().__new__(cls)` aynı allocator'ı
kullanır. Native heap adresi veya CPython type layout'u açığa çıkmaz.

Custom/inherited `__new__` normal Tonic call frame'inde çalışır. Sonuç istenen
class'ın instance'ıysa `__init__` özgün positional ve keyword argümanlarla çağrılır;
aksi halde sonuç değişmeden döner ve `__init__` çalışmaz. `__init__` dönüşünün None
olma kuralı korunur.

Allocator frame'i çalışırken constructor argüman penceresi caller registerlarına
bağlı bırakılmaz. Positional/keyword değerlerin owned bootstrap görünümü
`ReturnAction::New` içinde tutulur. Class, receiver ve bütün argüman değerleri
explicit root traversal'a katılır; moving collection sonrasında logical handle'lar
geçerlidir. Bu kopya yalnız custom `__new__` yolunda ödenir; varsayılan allocator
hızlı yolu ek host argüman container'ı oluşturmaz.

Metaclass `__call__`, native type nesneleri, `__init_subclass__` ve genel type
mutation bu kararın kapsamı dışındadır.
