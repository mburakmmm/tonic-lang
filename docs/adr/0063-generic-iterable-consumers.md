# ADR 0063: Genel iterable tüketicileri

## Durum

Kabul edildi ve uygulandı. Bu karar `list`/`tuple` constructor'larını, exact
unpack'i ve çağrıdaki `*args` genişletmesini kapsar. `dict` iterable-pair
constructor'ı ile generator/coroutine frame'leri ayrı roadmap işidir.

## Karar

Builtin iterator'lar mevcut allocation-free `heap.iterator`/`heap.next` hızlı
yolunda kalır. Bu yol bir user instance için uygun olmadığında VM `__iter__` ve
ardından `__next__` special method'larını normal guest frame'leriyle çağırır.
Çağrı askıya alınırsa `ReturnAction` içinde iterator, toplanan değerler ve hedef
işlem saklanır; bu state moving GC tarafından kesin biçimde izlenir.

Yalnız `__next__` çağrısından dışarı kaçan `StopIteration` tüketimi bitirir.
Iterator içinde yakalanan `StopIteration` ile diğer guest exception'ları normal
dispatcher akışını kullanır. Exact unpack bütün değerleri doğrulamadan hedef
register'lara yazmaz ve CPython'ın `expected`/`got` tanı ayrıntısını üretir.

Genişletilmiş çağrı scratch alanı continuation boyunca VM root'u olarak kalır.
Tek deferred `*args` yolu özel iterasyonu tamamladıktan sonra `CallExpanded`
instruction'ını yeniden çalıştırır; çoklu yıldız yolu aynı scratch'e sırayla ekler.
Argument bütçesi her eklemede uygulanır, dolayısıyla genel iterable hızlı yolun
kaynak sınırlarını aşamaz.

## Doğrulama

Language testi her-allocation stress GC altında self-iterator ve ayrı iterator
döndüren iterable'ı; list/tuple, başarılı/eksik/fazla unpack, tek/çoklu yıldız,
geçersiz iterator ve non-StopIteration hata yayılımını kapsar. Aynı program Python
3.14.6 oracle'ına karşı interpreter/JIT ve normal/stress-GC kombinasyonlarında
çalıştırılır.
