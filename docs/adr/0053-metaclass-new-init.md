# ADR 0053: Metaclass `__new__` ve `__init__` çağrı zinciri

## Durum

Kabul edildi ve uygulandı.

## Karar

Class body tamamlandıktan sonra runtime, custom metaclass hook'u varsa namespace'i
bir kez gerçek dict nesnesine materialize eder. Aynı name, bildirilmiş bases tuple'ı
ve mapping kimliği `Meta.__new__` ile `Meta.__init__` boyunca korunur. Hook olmayan
normal sınıflar bu materialization maliyetini ödemez.

`type.__new__`, yerleşik type sınıfının opaque builtin'idir. Bu ilk dilimde yalnız
aktif bir metaclass `__new__` çağrısına ait exact pending-class kaydını tamamlar.
Ayrı type çağrı yolu string-key dict için üç argümanlı
`type(name, bases, namespace)` kurucusunu destekler; input dict'i değiştirmeden
kopyalar, boş bases tuple'ını implicit `object` base'e dönüştürür ve aynı class
tamamlama/`__set_name__` yolunu kullanır. Pending kayıt logical
Value handle'larından oluşur, VM root taramasına katılır ve iç içe class oluşturma
için stack düzeninde tutulur. Hata, yeni module run'ı ve native reentry çıkışları
bekleyen kayıtları temizler.

Tamamlama sırası şöyledir:

1. `Meta.__prepare__`
2. class body
3. `Meta.__new__`
4. `type.__new__` içinde class/MRO kurulumu ve `__class__` cell doldurma
5. descriptor `__set_name__` çağrıları
6. `Meta.__new__` dönüşünün kalanı
7. dönen nesne seçilen metaclass'ın instance'ıysa `Meta.__init__`

Her guest hook normal VM frame ve ReturnAction continuation'ını kullanır. Bu sayede
traceback, recursion sınırı, stress GC ve JIT fallback davranışı ayrı bir çağrı
mekanizması olmadan korunur. `__init__` yalnız `None` döndürebilir. `__new__` başka
bir değer döndürürse metaclass init çağrılmaz.

## Sınırlar

`type.__new__` aktif kaydın exact name/bases/mapping nesnelerini bekler. Aynı hook
içinde yeniden oluşturulmuş eşdeğer mapping veya birden çok class üretimi bu tier'da
fail-closed `UnsupportedFeature` verir. Dict dışı `__prepare__` sonucu henüz
desteklenmez. Programatik class dict'teki
string olmayan key'ler attribute lookup tablosuna karışmadan ayrı traced storage ve
ortak insertion-order metadata'sında korunur; mappingproxy bunları dict key eşitliğiyle
okur ve iterate eder.

## Doğrulama

Katman testleri prepare/body/new/set-name/init sırasını, namespace mutation'ını,
init-only metaclass'ı, class dışı dönüşü, iç içe class oluşturmayı ve her-allocation
stress GC'yi kapsar. CPython differential corpus interpreter/JIT ve normal/stress GC
matrisinde 272 çıktı ile 80 exception vakasını geçirir.
