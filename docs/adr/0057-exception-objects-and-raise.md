# ADR 0057: Exception nesneleri ve `RAISE`

## Durum

Kabul edildi ve uygulandı. Handler/unwind tablosu daha sonra
[ADR 0058](0058-exception-regions-and-unwind.md) ile tamamlandı.

## Karar

Tonic exception'ları yalnızca host `Diagnostic` metni değildir. VM precise GC
root setinde canonical `BaseException`, `Exception`, `TypeError`, `ValueError`,
`RuntimeError` ve `StopIteration` class handle'ları tutar. Yerleşik exception
constructor'ları class kimliği ve mesaj taşıyan managed `Object::Exception`
üretir. User-defined exception alt sınıfları normal instance class/MRO yolu ile
`BaseException` üyeliğini korur.

Bytecode sürümü 9, explicit ve bare biçimi ayıran `RAISE` opcode'unu tanımlar.
Verifier register ve reserved operand biçimini denetler; `RAISE` terminal kontrol
akışı olarak kabul edilir. Explicit raise yalnızca `BaseException` instance'ı ya
da subclass class'ı kabul eder. Uncaught hata class adını owned diagnostic kind'e,
mesajı ve korunmuş source span/frame zincirini traceback'e taşır. Dynamic user
exception adları için hem `Diagnostic` hem JIT `RuntimeFailure` hata türünü owned
string olarak taşır.

Cranelift bu opcode'u güvenli biçimde doğrular fakat henüz native handler entry
üretmez. Böyle bir function `Unsupported` olarak işaretlenir ve interpreter'da
çalışır; tekrar tekrar codegen denenmez. Native helper hataları ise ADR 0058'deki
VM dispatcher'ına dönerek interpreted caller handler'ında yakalanabilir.

## Doğrulama

Parser/AST/lowering testi Tonic-owned `Raise` node'u ve opcode'u, verifier geçerli
ve bozuk operandları, runtime testi type/MRO/format/explicit/bare/user-subclass
raise ile span/traceback'i kapsar. Ayrı JIT testi hot function içindeki `RAISE`ın
compile attempt sonrası güvenli fallback verdiğini doğrular. CPython differential
corpus'u 277 stdout ve 102 exception vakasına ulaşmıştır.
