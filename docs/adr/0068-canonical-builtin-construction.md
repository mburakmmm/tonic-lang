# ADR 0068: Canonical builtin kurucuları ve numeric conversion

## Durum

Uygulandı. Bu karar `object.__init__`, int/bool/float/str/list/tuple/dict/range
`__new__`, list/dict `__init__`, native builtin alt sınıflarında custom `__new__`
ve `int`/`float` constructor conversion protokollerini kapsar. Kalan yürütülebilir
`__index__` tüketicileri ADR 0069'dadır; `__hash__` ayrı açık kapsamdır.

## Karar

Canonical kurucular runtime type namespace'lerinde gerçek builtin descriptor
değerleridir. `object.__new__` native backing gerektiren class'ları doğrudan
ayıramaz; ilgili builtin `__new__` class uyumluluğunu doğrular ve exact builtin
değeri ya da class kimliği ile instance slotlarını koruyan native wrapper üretir.
Bool ve range final kaldığı için yalnız exact class kabul edilir.

List ve dict `__new__` aşamasında boş backing ayırır. Canonical `__init__` daha
sonra aynı backing'i temizleyip doldurur; custom `__init__` boş backing'i görür.
Doğrudan yeniden başlatma, list self-source temizleme ve dict self-source koruma
davranışı Python ile aynı sırayı izler. Guest iterable tüketimi normal VM
frame'lerinde askıya alınabilir. List continuation owner ve dönüş değerini;
dict continuation sonuç backing'ini, keyword değerlerini ve dış instance'ı
precise root olarak taşır. Mutasyon yalnız heap API'sinden geçerek write barrier,
heap accounting ve dict version invalidation kurallarını korur.

`int(value)` önce `__int__`, sonra `__index__`; `float(value)` önce `__float__`,
sonra `__index__` arar. Guest metotlar normal VM frame'inde çalışır ve dönüşte
protokole göre exact int/float sonucu doğrulanır. Python 3.14'ün geçiş davranışıyla
uyumlu olarak int/index protokolünden dönen bool `0` veya `1` olarak normalize
edilir. Custom native subclass `__new__` sırasında conversion askıya alınırsa
pending class ve native-finish state continuation içinde köklenir; sonuç backing'e
dönüştürüldükten sonra wrapper tamamlanır.

Bu dinamik kurucu ve protokol yolları JIT tarafından özel native semantik olarak
lower edilmez. Desteklenen caller JIT kodu çağrı sınırında mevcut VM helper/frame
yolunu kullanır; desteklenmeyen code object doğrulanmış exact-PC interpreter
fallback'inde kalır.

## Doğrulama

Runtime testleri direct descriptor çağrılarını, custom int/float/str/tuple/list/
dict `__new__`, canonical list/dict reinitialization, self-source davranışı,
bool/range class doğrulaması ve her-allocation GC altında suspending iterable
yollarını kapsar. Ayrı numeric test `__int__`, `__float__`, `__index__` fallback,
native override, custom int subclass completion ve yanlış dönüş tiplerini sınar.
Python 3.14.6 differential corpus'u aynı gözlenebilir çıktı ve exception türlerini
interpreter/JIT ile normal/stress-GC matrisinde karşılaştırır.
