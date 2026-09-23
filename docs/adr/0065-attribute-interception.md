# ADR 0065: Attribute interception ve canonical delegasyon

## Durum

Kabul edildi ve uygulandı. Bu karar instance ve class nesnelerinin
`__getattribute__`, `__setattr__`, `__delattr__` protokollerini; `getattr`,
`hasattr`, `setattr`, `delattr` builtin'lerini ve `object`/`type` delegasyon
yüzeyini kapsar. Builtin type alt sınıflarının native storage kurucuları ile
kalan numeric/comparison protokolleri ayrı kapsamdır.

## Karar

Normal attribute işlemleri tek bir suspending VM yolunda yürütülür. Instance
hook'ları instance attribute'larından bağımsız olarak type MRO'sundan, class
nesnesi hook'ları metaclass MRO'sundan çözülür. Tonic'in canonical
`object.__getattribute__`/`__setattr__`/`__delattr__` ve
`type.__getattribute__`/`__setattr__`/`__delattr__` builtin descriptor'ları
varsayılan uygulama olarak tanınır; user override'ı sayılmaz. Bu yöntemlerin
doğrudan çağrılması hook'u tekrar çağırmadan varsayılan işlemi yapar ve böylece
hook gövdeleri güvenli biçimde delege edebilir.

Class attribute lookup, metaclass data descriptor'ını class sözlüğünden önce;
metaclass non-data descriptor'ını class sözlüğü kaçırdıktan sonra uygular.
Property ve custom descriptor getter/setter/deleter çağrıları normal guest
frame'lerinde askıya alınabilir. Instance tarafında mevcut data descriptor,
instance slotu ve non-data descriptor sırası korunur. Mutation hook'larının ve
setter/deleter'ların dönüş değeri göz ardı edilerek işlem sonucu `None` yapılır.

`ReturnAction::AttributeGet`, owner'ı ve varsa `getattr` default değerini precise
GC root olarak taşır. Birincil `__getattribute__` veya varsayılan descriptor
yolundan kaçan `AttributeError`, varsa `__getattr__` çağrısını başlatır. Yalnız
bu protokol sınırındaki `AttributeError` tüketilir; diğer exception'lar aynen
yayılır. `__getattr__` da `AttributeError` üretirse `getattr(..., default)`
default'u döndürür, `hasattr` `False` olur, ordinary access hatayı korur.

Özel `__getattribute__` bulunan owner için instance-slot quickening ve JIT
direct-method profili kurulmaz. Sonradan class hook'u ekleme/silme mevcut class
version guard'ını düşürür. Böylece interpreter cache'i veya native method fusion
user hook'unu atlayamaz; desteklenmeyen dinamik yol kesin bytecode PC'sinde
interpreter'da kalır.

## Doğrulama

Stress-GC runtime testleri instance ve metaclass interception, canonical
delegasyon, property/custom metaclass descriptor önceliği, bound varsayılan
method'lar, hook rebinding/deletion, `getattr` default, `hasattr`, `delattr`,
doğrudan çağrıda fallback uygulanmaması ve mutation dönüş değerinin atılmasını
kapsar. JIT testi özel `__getattribute__` altında direct-method site'ı
üretilmediğini doğrular. Python 3.14.6 differential corpus'u aynı yolları ve
yanlış hook/receiver hata sınıflarını debug/release × interpreter/JIT ×
normal/stress-GC matrisinde karşılaştırır.
