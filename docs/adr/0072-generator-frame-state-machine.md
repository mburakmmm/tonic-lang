# ADR 0072: Generator frame durum makinesi

## Durum

Kısmen uygulandı. Bu karar senkron generator çekirdeğini, bytecode v16 `YIELD` /
`YIELD_FROM` opcode'larını, askıya alınmış kesin GC köklerini ve tam senkron
delegasyon protokolünü kapsar. Coroutine/async ve finalization açık kalır.

## Karar

Bir `CodeObject`, lexical HIR taramasından gelen açık `generator` bayrağı taşır.
Parser Tonic-owned AST'ye `Yield` ve `YieldFrom` düğümleri üretir; HIR yalnız
yield'in bulunduğu lexical function/lambda scope'unu generator yapar. Module ve
class body içinde yield tanı hatasıdır. Verifier `YIELD` operandlarını, generator
bayrağını ve expanded-argument stack dengesini yürütmeden önce denetler. Module
entry ve class-body code nesneleri generator olamaz.

Generator fonksiyonu normal binder ile argümanlarını ve closure cell'lerini bağlar,
ancak bytecode gövdesine girmez. Hazırlanmış register/cell dizileri
`Object::Generator` içindeki `GeneratorFrame`'e taşınır. Frame dört durumdan
birindedir: `Created`, `Running`, `Suspended`, `Completed`. Resume VM register ve
cell stack'lerine owned state'i geri taşır. `YIELD` instruction pointer'ı,
register'ları, cell'leri, exception stack'ini ve gönderilen değerin yazılacağı
register'ı tekrar nesneye alır. Running generator'ın yeniden çalıştırılması
`ValueError`; yeni generator'a `None` dışında send `TypeError` üretir.

Askıya alınmış register, cell, exception state, class ve tamamlanma değeri precise
GC kenarlarıdır. State'in nesneye her dönüşü owner üzerinden write barrier uygular.
Bu yüzden nursery promotion, compaction ve stress collection generator frame'inde
native adres veya conservative stack taraması gerektirmez. Completed frame owned
vektör kapasitelerini bırakır; yalnız gözlenebilir return değeri korunur.

`iter` ve `__iter__` generator'ı kendisi olarak döndürür. `next`, `__next__` ve
`send` ortak resume yolunu kullanır. Tek-argüman `throw` managed exception'ı aktif
generator noktasına enjekte eder. `close`, `GeneratorExit` enjekte eder; generator
bunu yakalayıp yeniden yield ederse `RuntimeError` üretir. Generator içinde
yakalanmayan açık `StopIteration`, normal return'den ayırt edilir ve PEP 479
uyarınca `RuntimeError("generator raised StopIteration")` olur.

VM continuation'ları generator kullanan for/`NEXT`, list/tuple/dict constructor,
unpack ve `*args` genişletme state'ini taşır. Böylece tüketici normal Tonic veya
custom iterator gibi generator üzerinde de askıya alınabilir. Senkron builtin
iterator genişletmesi PC'yi geri sarmaz; yalnız gerçekten askıya alınan deferred
`*args` çağrısı `CALL_EXPANDED` PC'sine döner.

`yield from` doğrulanmış bir `YIELD_FROM`/`YIELD` döngüsüne lower edilir.
`YIELD_FROM` aynı register'daki resume değerini delegenin `send` yoluna verir;
normal `None` resume `__next__` kullanır. Askıdaki dış frame aktif delegate logical
handle'ını taşır. `throw` ve `close` dış frame'i önce yeniden kurar; enjekte edilen
managed exception opcode sınırında delegenin protokolüne aktarılır. Delege
exception'ı dış frame stack'te dururken yayıldığı için dış `except` ve `finally`
bölgeleri olağan VM unwind yolunda çalışır. Delege tamamlandığında
`StopIteration.value` hedef register'a yazılır ve ifadenin sonucu olur.

`StopIteration.value` exception nesnesinde ayrı, yazılabilir ve precise-traced bir
alan olarak tutulur. Generator `return` değeri ilk tükenme exception'ının `args`
ve `value` alanlarına taşınır; sonraki tükenmeler `None` üretir. Modern tek
exception argümanlı `throw` ile legacy `throw(type, value, traceback)` desteklenir.
Legacy tuple value positional args olarak açılır, custom exception `__init__`
normal Tonic frame'inde çalışabilir ve traceback girdisi doğrulanıp köklenir.

Generator code'u Cranelift adaylığından ve direct-call specialization'dan hariç
tutulur. JIT'te çalışan çağıran kod generic sınırdan interpreter generator frame'ine
geçebilir.

## Açık kapsam

- Ulaşılamayan askıdaki generator'ların logical finalization/close politikası,
  genel finalizer tasarımıyla birlikte belirlenecektir.
- `async def`, `await`, async generator ve coroutine state machine ayrı bir
  genişletme olarak eklenecektir.
- Generator code'unun JIT edilmesi ancak deopt metadata ve suspended-root stack
  map tasarımı hazır olduğunda değerlendirilecektir.

## Doğrulama

Compiler testleri lexical generator işaretlemesini, module/class reddini ve owned
AST/bytecode üretimini kapsar. Verifier testi `YIELD` için generator metadata ve
operand sınırını doğrular. Runtime testleri tembel çağrı, `iter`/`next`, bütün
builtin tüketiciler, `send`/modern ve legacy `throw`/`close`, return-değerli ve tam
forwarding yapan `yield from`, PEP 479, closure/cell/exception/delegate state'i ve
moving stress GC'yi interpreter ile JIT çağıran modlarda sınar. Python differential
corpus'u senkron protokol ve hata türlerini CPython 3.14.6 ile karşılaştırır.
