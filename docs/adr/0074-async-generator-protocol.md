# ADR 0074: Async generator protokolü

## Durum

Uygulandı. Event-loop, future/task scheduling ve cancellation ayrı açık
kapsamdır.

## Karar

Async generator code'u `CodeObject.generator && CodeObject.coroutine` birleşimiyle
tanımlanır. Bu birleşim yeni bir bağımsız metadata alanı eklemeden ordinary
generator, coroutine ve async generator durumlarını ayırır. Async function içinde
`yield from` ve değerli `return` compile-time `SyntaxError` üretir.

Bytecode v20 `ASYNC_YIELD(destination, value)` opcode'unu ekler. Ordinary `YIELD`,
async generator içindeki `await` delegasyonunun event-loop'a verdiği askı değerini
taşımaya devam eder. `ASYNC_YIELD` ise kullanıcı `yield` ifadesidir ve o anki
`__anext__`/`asend`/`athrow` awaitable'ını verilen değerle tamamlar. Verifier
opcode'u yalnız iki metadata bayrağı da doğru code object'te kabul eder ve iki
register ile reserved operand'ı doğrular.

Çağrı, gövdeyi çalıştırmadan `GeneratorFrame { kind: AsyncGenerator }` üretir.
Nesnenin builtin sınıfı `async_generator`dır ve yalnız async iteration yüzeyini
sunmaktadır:

- `__aiter__` nesnenin kendisini döndürür;
- `__anext__` ve `asend` tek kullanımlık `async_generator_asend` üretir;
- `athrow` ve `aclose` tek kullanımlık `async_generator_athrow` üretir.

İki awaitable sınıfı da kendi await iterator'ıdır ve `__await__`, `__iter__`,
`__next__`, `send`, `throw`, `close` protokollerini taşır. Managed nesne kaynak
generator'ı, operation değerini, exception/traceback'i ve Created/Running/Completed
durumunu precise trace kenarlarıyla tutar. Persistent cache veya JIT metadata'sında
ham nesne adresi saklanmaz.

Generator frame'i aynı zamanda aktif awaitable'ın logical handle'ını tutar. İlk
sürücü bu sahipliği claim eder; iç `await` askıları boyunca korur ve kullanıcı
`yield`i, doğal bitiş veya hata sınırında bırakır. Farklı bir awaitable aynı
generator'ı bu arada sürmeye çalışırsa state bozulmadan `RuntimeError` alır.

Awaitable ilk sürüşte saklanan Send/Throw/Close işlemini uygular; sonraki `send`
değerleri iç await delegesine gider. İç `await` askısı ordinary `YIELD` ile dış
coroutine'e aktarılır ve aynı awaitable Running kalır. `ASYNC_YIELD`, await
ifadesinin `YIELD_FROM` continuation'ını doğrudan tamamlar; public iterator
yüzeyinde aynı sonuç `StopIteration.value` olarak gözlenir. Doğal async-generator
bitişi `StopAsyncIteration` üretir. Kullanıcı kodundan kaçan `StopIteration` veya
`StopAsyncIteration`, async-generator sınırında cause zinciri korunarak
`RuntimeError`a çevrilir.

`athrow` modern instance ve legacy type/value/traceback biçimlerini kullanır;
custom exception `__init__` normal VM frame'i olarak askıya alınabilir. `aclose`
`GeneratorExit` enjekte eder. Aktif iç await delegesi varsa `throw`/`close` önce
ona iletilir. Kapanış sırasında kullanıcı değeri veren async generator
`RuntimeError("async generator ignored GeneratorExit")` üretir. Awaitable
`throw`/`close` da dış coroutine'in aktif await zincirinden gelen exception ve
kapanışı aynı state machine'e aktarır.

Async generator frame'i register, cell, exception stack, resume register ve aktif
delegeyi existing suspended-frame taşıyıcısında tutar. Awaitable nesnesi ve bütün
ReturnAction continuation değerleri GC root taramasına dahildir. Ulaşılamayan
askıdaki async generator mevcut collector-dışı logical-finalization kuyruğuna
girer; collector guest kodu çalıştırmaz ve fiziksel reclamation daha sonraki
collection'a bırakılır.

Async generator code'u Cranelift codegen, OSR ve direct-call inlining kapsamı
dışındadır. JIT'te çalışan çağıran kod nesneyi generic call yolundan oluşturur ve
resume interpreter frame'inde gerçekleşir. Public JIT yapısal doğrulayıcısı
`ASYNC_YIELD` operandlarını tanır, fakat generator/coroutine metadata'sı nedeniyle
codegen'i güvenli biçimde reddeder.

## Reddedilen seçenekler

- Kullanıcı `yield` ile iç `await` askısını aynı opcode'da birleştirmek: awaitable
  tamamlanması ile event-loop token'ını ayırmak için runtime AST bilgisine ihtiyaç
  doğurur.
- Async generator'ı coroutine wrapper'ı olarak göstermek: `StopAsyncIteration`,
  `asend` ve kapanış durumlarını ordinary coroutine tekrar-kullanım kurallarıyla
  karıştırır.
- Async-generator frame'ini native Rust stack'inde tutmak: moving GC rootları,
  finalization ve daha sonraki deoptimization için güvenli değildir.

## Doğrulama

Compiler testleri async-generator metadata'sını, `ASYNC_YIELD` lowering'ini,
değerli `return` ve `yield from` syntax hatalarını doğrular. Bytecode verifier
testleri geçerli/yanlış metadata ve operand biçimlerini kapsar. Runtime testleri
`__aiter__`/`__anext__`, `asend`, legacy/custom-init `athrow`, `aclose`, iç
await askıları, async-for tüketimi, awaitable `throw`/`close` forwarding'i,
yeniden kullanım, `StopAsyncIteration` sınırı, ignored `GeneratorExit`, moving
stress GC, interpreter/JIT caller ve collector-dışı finalization'ı çalıştırır.
Python differential corpus'u başarı çıktısı ile syntax/runtime exception türlerini
CPython 3.14.6 karşısında denetler.
