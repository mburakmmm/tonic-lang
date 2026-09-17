# ADR 0051: Metaclass seçimi ve bootstrap `type`

## Durum

Kabul edildi ve uygulandı.

## Karar

Tonic runtime artık `type` sınıfını bootstrap eder; `object.__class__`,
`type.__class__` ve normal sınıfların `__class__` ilişkileri mantıksal class
handle'larıyla temsil edilir. Class bytecode çağrı penceresi yalnız bir adet
`metaclass` keyword'ünü kabul eder. Değer base ifadelerinden sonra ve class
body'den önce değerlendirilir.

Runtime explicit aday ile base sınıfların metaclass'larını karşılaştırır. Daha
türemiş aday seçilir; birbirinin alt sınıfı olmayan iki aday `TypeError` ile
reddedilir. Explicit aday `type` alt sınıfı olmalıdır. Metaclass handle'ı class
nesnesinin precise GC edge'idir ve hareket sonrası geçerliliğini korur.

## Prepare aşaması

Kalıtılan custom `__prepare__`, normal Tonic çağrı continuation'ı üzerinden class
adı ve bildirilmiş base tuple'ıyla çağrılır. Dönen dict class body boyunca aynı
identity ile kullanılır; body tamamlanınca string-key girdileri class attribute
tablosuna materialize edilir. Dict dışı özel mapping henüz açık `TypeError` verir.

Custom metaclass `__new__`/`__init__` zinciri daha sonra ADR 0053 ile uygulanmıştır.
Dict dışı `__prepare__` mapping desteği hâlâ açık roadmap kapısıdır.
