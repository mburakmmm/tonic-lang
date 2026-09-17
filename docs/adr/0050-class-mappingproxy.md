# ADR 0050: Canlı class mappingproxy

## Durum

Kabul edildi ve uygulandı.

## Karar

`C.__dict__`, sınıf attribute tablosunu kopyalayan normal bir dict yerine sınıfın
mantıksal handle'ını tutan salt okunur bir mappingproxy döndürür. Proxy sınıfı GC
boyunca canlı tutar; indeksleme, `len`, temsil ve anahtar iterasyonu her erişimde
güncel sınıf namespace'ini okur.

İteratör başladığında namespace boyutunu kaydeder ve iterasyon sürerken boyut
değişirse `RuntimeError` üretir. Değer değiştirmek proxy görünümünü canlı biçimde
günceller. Item assignment mevcut mutation sınırından reddedilir; değişiklikler
yalnız class attribute API'sinden geçerek write barrier ve version invalidation
kurallarını korur.

## Sınır

Bu dilim explicit metaclass, `__prepare__` veya özel namespace mapping'i
uygulamaz. Bunlar roadmap'teki bir sonraki class customization aşamasıdır.
