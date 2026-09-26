# ADR 0073: Coroutine ve await durum makinesi

## Durum

Kısmen uygulandı. Bu karar `async def`, `await`, coroutine nesnesi ve
`coroutine_wrapper` çekirdeğini kapsar. Async generator, `async for`, `async with`
ve event-loop/future/task protokolleri açık kalır.

## Karar

`CodeObject`, `generator` bayrağından ayrı bir `coroutine` bayrağı taşır; iki
bayrak aynı anda doğru olamaz. Parser Tonic-owned AST'ye async function ve
`Await` düğümlerini aktarır. Lexical HIR async function scope'unu coroutine olarak
işaretler ve ilk dilimde async generator'ı açık `UnsupportedSyntax` tanısıyla
reddeder. Bytecode v17 `GET_AWAITABLE` ekler. Verifier opcode'u yalnız coroutine
code'unda kabul eder; module/class entry ve generator/coroutine çakışmasını da
yürütmeden önce reddeder.

`async def` normal call binder ile argüman ve closure cell'lerini bağlar, fakat
gövdeyi çağrı anında yürütmez. Register, cell, instruction pointer ve exception
state mevcut `GeneratorFrame` taşıyıcısına `Coroutine` türüyle alınır. Bu ortak
taşıyıcı depolama ve GC invariants'ını paylaşır; dil seviyesinde coroutine genel
iterable sayılmaz ve generator descriptor'larından ayrı bir builtin class taşır.

`GET_AWAITABLE`, exact Tonic coroutine'i doğrudan kabul eder. Diğer değerlerde
class MRO'sundan `__await__` çağırır ve sonuç normal VM continuation'ında iterator
olarak doğrulanır. Ardından doğrulanmış `YIELD_FROM`/`YIELD` döngüsü delegeyi
sürer. `send`, `throw`, `close`, `StopIteration.value`, exception state ve dış
handler/finally unwind'ı senkron delegasyonun ortak yolunu kullanır.

Public `coroutine.__await__()` kaynak coroutine'i doğrudan döndürmez. Bunun
yerine kendi iterator'ı olan ayrı `Object::CoroutineIterator` oluşturur. Bu
`coroutine_wrapper`, class ve kaynak coroutine logical handle'larını precise trace
kenarları olarak tutar. `__next__`, `send`, `throw` ve `close` wrapper'ı kaynak
coroutine'e açar ve ortak resume yoluna aktarır. Böylece coroutine genel iterable
olmadan Python'ın gözlenebilir await-iterator protokolü korunur.

Askıdaki coroutine frame'i ve delege iterator moving/stress GC altında kesin
köklerdir. Ulaşılamayan `Suspended` coroutine mevcut collector-dışı generator
finalization kuyruğuna alınır. `GeneratorExit` önce aktif await delegesine, sonra
dış coroutine'e yayılır; iç ve dış `finally` sırası normal unwind ile korunur.
Collector guest kodu çalıştırmaz ve fiziksel reclamation logical close'dan ayrı
kalır.

Coroutine code'u Cranelift codegen ve direct-call inlining adaylığından çıkarılır.
JIT çağıran fonksiyon generic call sınırında tembel coroutine nesnesini alabilir;
resume interpreter frame'inde yapılır. Bu ayrım deopt veya GC stack-map desteği
olmayan suspended native frame üretimini engeller.

## Açık kapsam

- Async generator'ın `asend`/`athrow`/`aclose` ve finalization semantiği.
- `async for` için `__aiter__`/`__anext__` ve `StopAsyncIteration`.
- `async with` için `__aenter__`/`__aexit__` unwind zinciri.
- Future/task/event-loop scheduling, cancellation ve thread entegrasyonu.
- Coroutine frame'lerinin JIT edilmesi ve native suspended-root metadata'sı.

## Doğrulama

Compiler ve verifier testleri owned AST/HIR metadata'yı, bytecode v17 operand
kurallarını, coroutine/generator ayrımını ve async-generator reddini kapsar.
Runtime testleri tembel yürütme, nested coroutine, özel awaitable, public
`coroutine_wrapper`, `send`/`throw`/`close`, yanlış `__await__` sonucu ve await
edilemeyen değerleri interpreter ile JIT-caller altında stress GC kullanarak
sınar. Ayrı finalization testi ulaşılamayan askıdaki coroutine'in await ettiği
iterator'ı içten dışa kapattığını ve finalizer sayaçlarını doğrular. Differential
corpus aynı başarı ve hata türlerini CPython 3.14.6 ile karşılaştırır.
