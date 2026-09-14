# ADR 0010 — Örtük class cell ve `super`

Durum: uygulanmış bootstrap. 4 Eylül 2026. Public stable ABI değildir.

Sınıf içinde tanımlanan bir function veya lambda `__class__` okuduğunda ya da
sıfır argümanlı `super()` kullandığında scope resolver sınıf gövdesine sentetik bir
local cell ekler. Bytecode v6 doğrulayıcısı class-body code object'inde yalnız
`__class__` adlı tek bir local cell'e izin verir. Class body tamamlanıp namespace
gerçek Class nesnesine dönüştürüldükten sonra VM bu `Cell(Value)` içine logical
class handle'ını yazar. Closure'lar aynı hücreyi yakalar; native adres, Rust
referansı veya CPython layout'u taşımaz.

Gerçek builtin `super` sıfır argümanla çağrıldığında aktif Tonic frame'den ilk
parametreyi ve free-variable tablosundaki `__class__` hücresini okur. Adı yeniden
bağlanmış başka bir callable'a gizli argüman eklenmez. `super(type, receiver)` aynı
vekili açıkça kurar. Receiver, type'ın instance'ı veya alt sınıfı değilse çağrı
`TypeError` üretir. Tek argümanlı unbound `super(type)` henüz desteklenmez.

Super vekili yalnız iki izlenebilir `Value` taşır: aramanın başlayacağı sınıf ve
receiver. Attribute çözümü receiver'ın gerçek sınıfının C3 MRO'sunda başlangıç
sınıfından sonraki ilk tanımı seçer. Function instance'a, classmethod gerçek owner
sınıfına bağlanır; staticmethod doğrudan döner. Property ve custom `__get__`
çağrıları normal VM continuation/call yolunu kullanır. Böylece guest argüman
tuple/dict'i oluşmaz ve GC, vekil ile devam eden çağrıların bütün kenarlarını görür.

`__self__`, `__thisclass__` ve `__self_class__` introspection alanları logical
değerler olarak sunulur. Metaclass, `__new__`, tek argümanlı unbound super ve genel
özel method protokolleri bu kararın kapsamı dışındadır.
