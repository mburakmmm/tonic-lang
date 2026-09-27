# ADR 0076: Kimlik ve üyelik karşılaştırmaları

## Durum

Uygulandı.

## Karar

Python karşılaştırma gramerindeki `is`, `is not`, `in` ve `not in` işlemleri
Tonic AST'sinde ve bytecode v21'de ayrı işlemler olarak korunur. `IS` ve
`IS_NOT`, guest-visible native adres yerine 64 bit opaque `Value` kimliğini
karşılaştırır. Immediate değerler kendi canonical temsilleriyle, heap nesneleri
ise taşınmadan etkilenmeyen mantıksal handle'larıyla karşılaştırılır. Cranelift
bu iki opcode'u tahsis, runtime helper veya guard olmadan üretir.

`CONTAINS` ve `NOT_CONTAINS` Python protokol sırasını uygular. Sağ operandın
tipinde veya metaclass'ında `__contains__` varsa sonuç tam truthiness protokolüne
girer. Aksi halde exact string için substring hızlı yolu, diğer nesneler için
iterator fallback'i kullanılır. Iterator öğeleri tam, askıya alınabilir eşitlik
protokolüyle sırayla karşılaştırılır; yalnız iterator sınırından gelen
`StopIteration` üyeliğin bulunamadığını bildirir. `not in`, ayrı bir arama yapmak
yerine nihai truth değerini tersler.

Kullanıcı `__contains__`, `__iter__`, `__next__`, `__eq__` ve `__bool__`
çağrılarının her biri yeni VM frame'i açabilir. Needle, iterator ve bekleyen
protokol state'i `ReturnAction`/`EqualityAction` içinde tutulur ve moving GC'ye
precise root olarak bildirilir. Genel üyelik, kullanıcı kodu ve iterator state'i
gerektirdiği için şimdilik Cranelift destek kümesine alınmaz; hot fonksiyon
güvenli biçimde interpreter tier'ında kalır.

## Reddedilen seçenekler

- Kimlik için heap adresi karşılaştırmak: moving GC ve opaque ABI sözleşmesini
  bozar.
- Üyeliği yalnız builtin container işlemi yapmak: `__contains__`, iterator
  fallback'i ve kullanıcı eşitlik semantiğini kaybeder.
- Üyeliği tek bir senkron runtime helper'ında JIT'e bağlamak: askıya alınan guest
  frame'lerini ve exact-PC deoptimization state'ini gizler.

## Doğrulama

Parser/compiler testleri dört işlemin Tonic AST ve opcode eşlemesini, verifier
testleri bütün register operandlarını denetler. Runtime testleri native
list/tuple/dict/range/string, kullanıcı `__contains__`, truthiness, generator ve
özel iterator fallback'i, suspending equality, metaclass dispatch'i ve hata
yollarını interpreter/JIT-caller ile her allocation'da stress GC altında
çalıştırır. JIT testleri kimlik işlemlerinin native dönüşünü ve üyeliğin açık
`Unsupported` fallback'ini doğrular. Differential korpus ortak gözlenebilir
semantiği CPython ile karşılaştırır.
