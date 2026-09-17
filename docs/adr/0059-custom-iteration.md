# ADR 0059: User-defined iterator çağrı zinciri

## Durum

Kabul edildi ve `for` için uygulandı. Constructor, unpack ve star-expansion
yollarının aynı genel iterator sürücüsüne geçirilmesi açık iştir.

## Karar

Builtin iterator hızlı yolu korunur. `ITER` bu yol uygulanamadığında class MRO
üzerinden `__iter__` çözer ve normal suspending Tonic call frame'ine girer.
Dönen değer `__next__` özel metodunu sağlamıyorsa `TypeError` oluşur. `NEXT` de
user iterator için `__next__` çağrısını geçici bound-method nesnesi oluşturmadan
başlatır.

Iterator-next frame'i dönüş hedefi ve loop çıkış PC'sini `ReturnAction` içinde
taşır. VM önce `__next__` frame'inin kendi exception region'larını arar. Böylece
iterator kodunun yakaladığı `StopIteration` normal şekilde çalışır. Yalnız bu
frame'den kaçan `StopIteration` frame'i açar ve `for` çıkışına dallanır; diğer
hatalar sıradan exception unwind yolunu izler.

## Doğrulama

Runtime ve CPython differential testleri stateful iterator, helper frame'inden
kaçan `StopIteration`, iterator içinde yakalanıp değer döndüren durum, `for/else`,
normal hata yayılımı, `__iter__`ın iterator olmayan değer döndürmesi ve eksik
`__next__` yollarını kapsar. Matris interpreter/JIT ile normal/stress-GC
modlarında çalıştırılır.
