# ADR 0012 — Callable instance ve length protokolleri

Durum: uygulanmış bootstrap. 4 Eylül 2026. Public stable ABI değildir.

Instance çağrısı `__call__`, `len(instance)` ise `__len__` adını instance alanlarına
bakmadan class C3 MRO'sunda arar. Böylece `obj.__call__ = value` ve
`obj.__len__ = value` explicit attribute okumalarını değiştirir, implicit `obj()` ve
`len(obj)` davranışını değiştirmez. Sonradan class rebinding bir sonraki çağrıda
görülür; henüz kalıcı inline cache yoktur.

Function, `staticmethod` ve `classmethod` descriptor'ları `DescriptorCall` içindeki
callable ve isteğe bağlı receiver ile bağlanır. Protokol dispatch'i geçici
`BoundMethod`, guest argument tuple veya keyword dict ayırmaz. Callable değerlerden
oluşan yönlendirme zinciri host recursion kullanmadan çözülür ve 100 halkada
`RecursionError` verir.

Guest `__len__` gövdesi sıradan Tonic frame'inde çalışır. Frame askıdayken owner,
callable ve argümanlar normal register/frame root kümesindedir. Dönüşte
`ReturnAction::Length` değerin integer olmasını, i64 tabanlı bootstrap length
sınırına sığmasını ve negatif olmamasını denetler. Bool sonuçlar Python'ın gözlenebilir
`len` davranışı için 0 veya 1 integer değerine çevrilir. Frame açmayan native/builtin
sonuç aynı doğrulamadan hemen geçirilir.

Property veya kullanıcı tanımlı descriptor nesnesiyle sunulan özel metotların
`__get__` continuation'ı bu dilimde uygulanmamıştır. Metaclass `__call__`, diğer
numeric/iteration protokolleri ve specialization ayrı aşamalardır. Truthiness
fallback'i [ADR 0013](0013-truthiness-protocol.md) ile eklenmiştir.
