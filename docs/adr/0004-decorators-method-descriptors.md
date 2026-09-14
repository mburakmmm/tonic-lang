# ADR 0004 — Decorator uygulaması ve method descriptor wrapper'ları

Durum: uygulanmış bootstrap. 1 Eylül 2026. Public stable ABI değildir.

## Karar

Function ve class decorator ifadeleri kaynak sırasıyla, function default'larından
ve class base ifadelerinden önce değerlendirilir. Oluşturulan function/class değeri
decorator'lara ters sırayla verilir. Her uygulama normal `Call` bytecode'u ve genel
çağrı binder'ını kullanır; yalnızca decorator için ayrı bir çağrı ABI'si yoktur.
Son decorator sonucu tanımlanan isme bağlanır ve herhangi bir Tonic değeri olabilir.

`staticmethod` ve `classmethod` builtin'leri bir managed wrapper ayırır. Wrapper
içindeki callable logical `Value` olarak tutulur ve precise GC trace tarafından
ziyaret edilir. `staticmethod` class veya instance erişiminde işlevi değiştirmeden
döndürür. `classmethod` erişilen gerçek class'ı receiver yapan mevcut BoundMethod
nesnesini üretir; inherited erişimde tanımlayan base yerine derived class bağlanır.
Implicit receiver yine guest tuple/dict oluşturmaz.

Static wrapper doğrudan çağrılabilir; class wrapper doğrudan çağrılamaz.
İkisinde `__func__` okunabilir. Wrapper'lar kimlik tabanlı eşitlik/hash kullanır.
Classmethod hedefi callable değilse erişim yine bound method verir, çağrı TypeError
olur. Bound-method dict hash malzemesi alt değerleri güvenli biçimde hash eder;
non-callable hedef host panic üretemez. Wrapper nesneleri ile class/function/cell
döngüleri full-heap collector tarafından toplanır.

## Sınırlar ve performans

Bu tasarım genel descriptor protokolü değildir. `property`, custom `__get__`,
`__set__`, `__delete__`, `__set_name__`, data-descriptor önceliği, metaclass
attribute lookup, decorator metadata yardımcıları ve `super` yoktur. Annotation
ve type-parameter syntax'ı da ayrı kabul kapısıdır.

Şimdiki generic classmethod erişimi her seferinde BoundMethod ayırır. Staticmethod
erişimi wrapper'ı açar ve loop içinde guest allocation eklemez. Class/type/shape
version guard'lı cache eklenene kadar descriptor sonuçları kalıcı cache'de tutulmaz;
class rebinding generic lookup'ta hemen görünür. Ölçümler ve sınırlamalar
[Stage 4 raporunda](../STAGE4_BENCHMARKS.md) kayıtlıdır.

JIT daha sonra decorator'ı tanım adına göre özel saymamalıdır. Method erişimi için
specialization, descriptor tür kimliği ile class/MRO version guard'larını ve doğru
generic fallback'i kullanmalıdır. Wrapper'larda raw managed adres tutulmaz.
