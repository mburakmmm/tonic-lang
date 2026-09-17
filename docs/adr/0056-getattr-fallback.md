# ADR 0056: `__getattr__` fallback protokolü

## Durum

Kabul edildi ve uygulandı.

## Karar

Normal instance/class attribute çözümü, data descriptor ve property yolları
önce çalışır. Yalnızca bu yollar `AttributeError` ile eksik attribute bildirdiğinde
VM instance class'ındaki ya da class nesnesinin metaclass'ındaki `__getattr__`
metodunu çözer. Instance üzerinde aynı adlı bir değer özel protokol lookup'unu
gölgelemez.

Fallback çağrısı geçici bound-method nesnesi üretmez. Descriptor binding bilgisi
`DescriptorCall` ile taşınır, attribute adı yönetilen bir Tonic string'i olarak
oluşturulur ve normal suspending frame çağrısına verilir. Böylece getter iç içe
çağrı, allocation ve GC safepoint'i içerebilir; ad ve receiver kesin frame
root'larında kalır. Staticmethod, classmethod ve metaclass binding aynı yolu
kullanır.

Doğrudan `obj.name` ile iki argümanlı `getattr(obj, name)` aynı fallback'i
kullanır. `getattr(..., default)` ve `hasattr` içindeki `AttributeError` filtreleme
yolu protokol sınırında kalır; guest `try/except` altyapısı ADR 0058'de ayrıca
tamamlanmıştır.

## Doğrulama

Runtime testi normal attribute önceliğini, instance shadow'unun yok sayılmasını,
staticmethod/classmethod/metaclass binding'ini, tahsis yapan getter'ı ve callable
olmayan protokol hatasını kapsar. Aynı program CPython differential corpus'unda
interpreter/JIT ve normal/stress-GC matrisinde çalıştırılır. Corpus 276 stdout ve
97 exception vakasıdır.
