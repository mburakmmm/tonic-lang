# ADR 0064: `dict` için genel iterable çiftleri

## Durum

Kabul edildi ve uygulandı. Bu karar `dict(iterable, **keywords)` yolunu kapsar;
genel mapping-protocol kaynakları ve builtin type alt sınıfları ayrı kapsamdır.

## Karar

`dict` kaynağı akış halinde tüketilir. VM bir dış öğeyi aldıktan sonra o öğeyi
iki elemanlı iterable olarak tamamlamadan sonraki dış öğeye geçmez; böylece guest
yan etkilerinin sırası CPython ile aynı kalır. Exact builtin iterator'lar mevcut
heap hızlı yolunu, user iterator'lar normal `__iter__`/`__next__` guest frame'leri
ve `ReturnAction` continuation'larını kullanır.

Çift dönüşümünde ilk `iter(item)` sonucunun iterator protokolü yeniden normalize
edilir. Bu, self-iterator için `__iter__`ın iki kez; ayrı iterator döndüren bir
iterable için kaynak ve dönen iterator `__iter__`larının sırayla çağrıldığı
CPython davranışını korur. Yalnız dış veya çift `__next__` sınırından kaçan
`StopIteration` tüketilir; `__iter__` hataları ile diğer exception'lar yayılır.

Continuation state'i sonuç dict'ini, bekleyen keyword key/value çiftlerini, dış
iterator'ı, eleman indeksini, çift iterator'ını ve toplanmış çift değerlerini
precise root olarak izler. Her başarılı çift hemen insertion-ordered dict'e
yazılır. Keyword girdileri kaynak tamamlandıktan sonra uygulanır; mevcut key'in
değerini sıra konumunu değiştirmeden override eder.

## Doğrulama

Stress-GC language testi custom dış iterator + custom çift iterator, builtin dış
iterable + custom çift, custom dış iterator + builtin tuple, ayrı iterator
döndüren çift, keyword override, `#0`/`#1` uzunluk tanıları, pair hatası,
geçersiz iterator ve `__iter__` içinden kaçan `StopIteration` yollarını kapsar.
Aynı vaka Python 3.14.6 oracle'ına karşı interpreter/JIT ve normal/stress-GC
modlarında çalıştırılır.
