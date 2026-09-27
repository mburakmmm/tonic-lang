# ADR 0073: Coroutine ve await durum makinesi

## Durum

Kısmen uygulandı. Bu karar `async def`, `await`, coroutine nesnesi,
`coroutine_wrapper`, `async for` ve `async with` çekirdeğini kapsar. Async generator
[ADR 0074](0074-async-generator-protocol.md) ile tamamlanmıştır. Temel
event-loop/future/task protokolleri [ADR 0075](0075-asyncio-event-loop-future-task.md)
ile eklenmiştir.

## Karar

`CodeObject`, `generator` bayrağından ayrı bir `coroutine` bayrağı taşır. Bu
karar diliminde ordinary coroutine yalnız `coroutine` bayrağını kullanır; daha
sonra ADR 0074 iki bayrağın birleşimini async generator olarak tanımlar. Parser
Tonic-owned AST'ye async function ve `Await` düğümlerini aktarır. Lexical HIR
async function scope'unu coroutine olarak işaretler. Bytecode v17
`GET_AWAITABLE` ekler. Verifier opcode'u yalnız coroutine code'unda kabul eder ve
module/class entry metadata'sını yürütmeden önce doğrular.

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

Bytecode v18 `GET_AITER`, `GET_ANEXT` ve `END_ASYNC_FOR` ekler. Bu opcode'lar
yalnız coroutine code'unda geçerlidir. `GET_AITER` özel `__aiter__` çağrısının
sonucunda `__anext__` bulunduğunu doğrular. `GET_ANEXT` dönen değeri ortak await
lowering'ine verir. Compiler yalnız next çağrısı ve await döngüsünü kapsayan kesin
bir exception region üretir; handler'daki `END_ASYNC_FOR` yalnız exact veya alt
sınıf `StopAsyncIteration` için else yoluna atlar, diğer exception'ı yeniden
yayar. Target/body bu region'ın dışında kaldığı için kullanıcı kodunun açık
`StopAsyncIteration` hatası yanlışlıkla tüketilmez. Break/continue/else mevcut
loop ve cleanup patching kurallarını kullanır.

Bytecode v19 `ASYNC_CONTEXT_ENTER` ve `ASYNC_CONTEXT_EXIT` ekler; verifier iki
opcode'u yalnız coroutine code'unda ve üç geçerli register operandıyla kabul
eder. Giriş yolu class manager için metaclass'tan, normal instance için class
MRO'sundan `__aexit__` metodunu önce çözer ve callable/receiver çiftini managed
tuple token'ında saklar. Sonra `__aenter__` çağrılır ve sonucu ortak await
lowering'inden geçirilir. Çıkış opcode'u yakalanan metodu `None` üçlüsüyle veya
aktif exception'ın type/value/traceback üçlüsüyle çağırır; sonuç tekrar await
edilir ve exception yolunda truthiness suppression kararını verir.

Compiler senkron context manager'ın nested exception region ve yapısal cleanup
modelini `is_async` niteliğiyle paylaşır. Exit çağrısı ile bütün await döngüsü tek
replacement region'ında kaldığından askıdan sonra yükselen hata da eski aktif
exception'ı doğru biçimde değiştirir. Return/break/continue sırasında async exit
tamamlanmadan kontrol aktarımı yapılmaz. Birden fazla manager iç içe lower edilir;
iç giriş başarısızsa yalnız başarıyla girilmiş dış manager kapanır. Token, aktif
exception ve await delegesi VM register/frame kökleriyle moving GC altında yaşar.
Coroutine code'u ve coroutine direct-call hedefleri JIT dışında olduğundan bu
suspend noktaları native stack-map gerektirmeden interpreter fallback'inde kalır.

## Açık kapsam

- OS I/O selector'ları, gerçek zamanlı timer ve thread-safe scheduling.
- Coroutine frame'lerinin JIT edilmesi ve native suspended-root metadata'sı.

## Doğrulama

Compiler ve verifier testleri owned AST/HIR metadata'yı ve bytecode v17–v19
operand kurallarını kapsar.
Runtime testleri tembel yürütme, nested coroutine, özel awaitable, public
`coroutine_wrapper`, `send`/`throw`/`close`, yanlış `__await__` sonucu ve await
edilemeyen değerleri interpreter ile JIT-caller altında stress GC kullanarak
sınar. Ayrı finalization testi ulaşılamayan askıdaki coroutine'in await ettiği
iterator'ı içten dışa kapattığını ve finalizer sayaçlarını doğrular. Differential
corpus aynı başarı ve hata türlerini CPython 3.14.6 ile karşılaştırır. Async-for
testleri anında ve askıya alınan `__anext__`, loop kontrolü, invalid protocol
sonuçları, exhaustion exception sınırı ve stress-GC köklerini kapsar. Async-with
testleri nested/kısmi giriş, yakalanmış exit metodu, normal ve askıya alınan
enter/exit, suppression/replacement, bare reraise, target hatası, metaclass
manager, bütün yapısal çıkışlar ve geçersiz protokol sonuçlarını kapsar.
