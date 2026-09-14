# ADR 0003 — Sınıf scope'u, C3 MRO ve shape tabanlı instance

Durum: uygulanmış bootstrap. 31 Ağustos 2026. Public stable ABI değildir.

## Problem ve alternatifler

Fonksiyon/closure altyapısı artık nesne semantiğine genişletilebilir. Her
instance'a attribute hash tablosu koymak hedef belleği ve specialization yolunu
zorlaştırır. Sınıf gövdesini sıradan fonksiyon local'ları gibi çalıştırmak ise
Python class namespace / method closure ayrımını yanlış uygular.

## Compiler kararı

Tonic-owned AST ayrı Class ve attribute assignment target taşır. Adapter class
private name mangling yapar; ham function/class label'ı mangled binding'den ayrı
tutulur. Class scope bir namespace'tir, method'ların enclosing lexical scope'u
değildir. Outer function cells class gövdesinden metotlara iletilebilir.

Class local adı henüz namespace'te yoksa global aranır; outer cell'e sırf aynı
isimli diye gidilmez. Serbest class-body adı önce namespace, sonra outer cell'de
aranır. Açık global/nonlocal bildirimleri bu yoldan ayrılır. Class içindeki
global bildirimi metotların outer closure aramasını kesmez. Class içinde return
(enclosing function olsa bile) ve dış döngüye break/continue reddedilir.

Bytecode v3: class-body flag, Class/LoadName/StoreName/ClassDeref/SetAttr.
Verifier namespace opcode'larını class body ile sınırlar; parametre/cell/window/
symbol sınırlarını doğrular. Class body normal callable olarak yürütülemez.
Explicit bytecode numaraları korunur; Rust enum layout'u serialize edilmez.

Class body mevcut frame VM'sinde çalışır. Dönüş continuation'ı namespace'i C3
doğrulamasından sonra Class'a dönüştürür. Adın dış scope'a bağlanması daha sonra
olur. Sınıf gövdesindeki yan etkiler invalid/duplicate base veya MRO hatasından
önce gerçekleşebilir; Python oracle bunları kontrol eder. Metaclass seçimi ve
__mro_entries__ henüz bu pipeline'a eklenmemiştir.

## Nesne ve attribute modeli

Class metadata: tekrar kullanılmayan TypeId, mutation version, isim, namespace
üyeleri, bases ve self hariç C3 ancestors. Soğuk/büyük Class metadata ayrı Box
içindedir; bütün Object enum'unu büyütüp her scalar'a bu maliyeti yüklemekten
kaçınılır. Box adresi dışarı verilmez; class Value yine generation'lı logical
handle'dır. ID/version tükenmesi wrap yerine hata verir.

Instance class handle + Attributes taşır. Normal Attributes bir ShapeId ve
Value slot vektörüdür. Shared transition tablosu `(parent shape, field name)`
ile prefix paylaşır; generic lookup parent zincirini izler. Şimdilik inline cache
yoktur. Instance'ların per-object hash tablosu yalnızca dictionary fallback'te
vardır. Fallback semantiği bozmaz; alanlar ve değerler korunur.

Bootstrap metadata bütçeleri: 64 shape field, runtime başına 4096 transition,
transition adı başına 1024 byte. Bunlar dilin attribute sınırı değildir; daha
dinamik instance dictionary moduna geçer. Shape metadata managed edge taşımaz,
VM ömrünce saklanır ve bütçeyle sınırlanır. Program SymbolId'leri saklanmaz;
farklı run'ın symbol tablosu eski shape adlarını değiştiremez.

Instance'daki kendi attribute önce aranır; sonra class/MRO. Tonic function
class'tan instance üzerinden okunursa BoundMethod oluşturulur. Class üzerinden
function okunursa unbound function kalır; instance'a doğrudan atanan function
kendiliğinden bind edilmez. Bound method receiver + function tutar, eşitliği
ve hash'i bu iki kimliğe dayanır. Dict key trace'leri bu logical slotların
yeniden kullanılmasını key yaşarken önler.

Implicit self çağrı descriptor'ında taşınır; arg tuple/keyword dict oluşturmaz.
Constructor varsa Tonic __init__'i çağırır ve None dönüşünü doğrular. Dönüş
continuation'ı None yerine oluşturulan instance'ı caller register'ına yazar.
Guest recursion yine VM frame stack'indedir. Kaydedilmiş bound method'lar
receiver'ı ve function'ı GC boyunca yaşatır.

## GC ve invalidation sözleşmesi

Class/namespace attrs, bases ve MRO; instance class/slot/dictionary values;
bound method receiver/function managed edge'lerdir. Class namespace ve
initializer instance'ı aktif frame continuation roots'udur. Mutationlar
namespace_set/set_attr sınırından geçer. Collector bu metotların ortasında
çalışmaz; sonraki instruction safepoint'inde bütün yeni edge'ler görünürdür.

Class attribute mutation version artırır. Şu an cache olmadığı için generic MRO
arama hemen yeni binding'i görür. Gelecekte inherited lookup cache'i yalnızca
derived class version'ını kontrol edemez: ancestor mutation da invalidation'a
dahil olmalıdır. Shapes farklı class'lar arasında paylaşılır; ileride descriptor
semantiği için gerekli type/version guards yalnızca shape guard ile ikame edilemez.

## Kapsam ve sonuçlar

isinstance/issubclass kullanıcı Class, object ve tuple classinfo destekler.
getattr/setattr/hasattr generic attribute yolunu kullanır. object root'u immutable,
onun tam instance'ları attribute eklemeye kapalıdır; user subclass'lar açıktır.
Class __name__/__qualname__/__doc__/__module__/__bases__/__mro__ ve bound-method
__self__/__func__ okunabilir. Repr fiziksel adres içermez ve CPython metniyle
birebir eşleşme garantisi yoktur.

Bu ADR dilimi itibarıyla henüz yoktu: metaclass, decorators,
property/staticmethod/classmethod, custom
descriptor, super/implicit __class__ cell, __new__, özel operator/attribute/
iteration/truthiness protokolleri, __dict__ view, deletion ve layout replacement.
Bu protokolleri class namespace'e eklemek sessizce etkisiz bırakılmaz;
UnsupportedFeature verir. Açık object.__init__ gibi builtin method descriptor'ları
da henüz sunulmaz. Desteklenmeyen syntax konumlu UnsupportedSyntax verir.
Bootstrap doğrulaması whitelist dışındaki bütün `__...__` class üyelerini
reddeder; bu kısıt `__version__` gibi protokol olmayan metadata'yı da kapsar.
Method gövdesindeki çıplak `__class__` adı, yerel binding olsa bile şimdilik
reddedilir. Bu iki koruma tam Python semantiği olarak değerlendirilmemelidir.

Class namespace üyeleri bootstrap Vec araması, shape lookup zincir araması,
metot erişimi ise bound-method allocation kullanır. Bu tamamlanmış adaptive
fast path değildir. Genel yolu ölçen [Stage 3 benchmark](../STAGE3_BENCHMARKS.md)
ileride attr/method cache ve call specialization için baseline'dır. Karşılaştırmalı
ölçüm olmadan shape storage'ın daha hızlı olduğu iddia edilmez.

İleriki adım: descriptor/decorator/super doğruluğu; ölçümlü attr/method caches;
generational barrier ve JIT guard/deopt. Native ABI ve CPython bridge bu değişiklikte
çekirdeğe karıştırılmamıştır. Eski run'ın function/method code ownership kısıtı sürer.

Sonraki [ADR 0004](0004-decorators-method-descriptors.md), function/class decorator
ile staticmethod/classmethod dilimini eklemiştir. Property/custom descriptor,
metaclass ve super hâlâ beklemektedir.
