# ADR 0062: Exception chaining ve managed traceback

## Durum

Kabul edildi ve uygulandı. Traceback nesnesinin frame/code introspection yüzeyi
ileride genişletilebilir; exception state ve unwind kökleri bu genişlemeye uygun
opak managed nesne sınırında tutulur.

## Karar

Bytecode sürümü 12, `RAISE` instruction'ının operand kiplerine explicit cause
biçimini ekler. `b=0` normal explicit raise, `b=1` bare reraise, `b=2` ise `a`
exception ve `c` cause register'ını taşır. Verifier kip, reserved operand ve her
iki register sınırını codegen/çalıştırma öncesinde denetler. Derleyici exception
ifadesini cause ifadesinden önce değerlendirir.

Managed exception nesnesi kendi kullanıcı attribute storage'ına ek olarak
`cause`, `context`, `suppress_context` ve `traceback` handle'ını taşır. Bütün
kenarlar precise GC tracing ve owner-aware write barrier üzerinden güncellenir.
`raise X from Y` cause'u BaseException olarak normalize eder ve implicit context
gösterimini bastırır; `from None` aynı suppression'ı cause oluşturmadan yapar.
Aktif handler sırasında oluşan yeni hata, dış exception'ı bir kez implicit
context olarak alır. Kullanıcı exception alt sınıfları aynı managed temsili,
normal descriptor/method lookup'u ve attribute mutation yolunu kullanır.

Dispatcher unwind sırasında kaynak span'li function girişlerini exception'a
bağlı opak `Traceback` nesnesine kaydeder. Bu nesne exception tarafından güçlü
biçimde köklenir, hareket eden GC ile taşınabilir ve yakalanan exception üzerinden
`__traceback__` olarak erişilir. Senkron context manager hata çıkışında aynı handle'ı
`__exit__(type, value, traceback)` çağrısına aktarır. Ayrıntılı Python frame/code
introspection bu ilk yüzeyin parçası değildir; nesne opak tutulduğu için iç layout
public ABI olmaz.

## Doğrulama

Parser/AST/HIR/lowering testi `raise ... from ...` biçimini ve operand kipini;
core ile public JIT verifier testleri geçerli/geçersiz register biçimlerini sınar.
Runtime ve CPython differential corpus'u explicit cause, implicit context,
`from None`, invalid cause, custom exception initializer/attribute/args,
`__traceback__` ve context-manager traceback argümanını kapsar. Aynı corpus
interpreter/JIT ile normal ve her-allocation stress-GC modlarında çalıştırılır.
