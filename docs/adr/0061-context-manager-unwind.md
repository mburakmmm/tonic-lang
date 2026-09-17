# ADR 0061: Senkron context manager unwind

## Durum

Kabul edildi ve uygulandı. Async context manager desteği coroutine altyapısıyla
birlikte ayrı kapsamdır.

## Karar

Bytecode sürümü 11, `CONTEXT_ENTER` ve `CONTEXT_EXIT` işlemlerini tanımlar.
`CONTEXT_ENTER`, manager instance'ının class'ından (class nesnesiyse
metaclass'ından) `__exit__` metodunu önce çözer ve callable/receiver çiftini GC
tarafından izlenen iç token'da saklar; ardından `__enter__` çağrısını normal
suspending frame yoluyla yapar. Böylece `__enter__` sırasında class rebinding
olsa bile çıkışta Python'ın capture ettiği özgün `__exit__` çağrılır. Instance
attribute shadow'ları implicit special-method lookup'u değiştirmez.

`CONTEXT_EXIT` normal çıkışta üç `None`, exception çıkışında exact exception
class/value ve [ADR 0062](0062-exception-chaining-and-traceback.md) ile tanımlanan
managed traceback nesnesini geçirir. Sonuç
normal truthiness continuation'ıyla değerlendirilir; doğru sonuç pending
exception'ı bastırır. False sonuç özgün exception'ı yeniden yükseltir. Exit veya
truthiness yeni hata üretirse eski active context temizlenir ve yeni hata korunur.

Çoklu `with` öğesi iç içe context manager olarak lower edilir. Derleyicinin
lexical cleanup yığını `return`, `break` ve `continue` öncesinde exit çağrılarını
içten dışa üretir. `finally`deki dar bypass-region tekniği, bu inline exit
çağrısının hata vermesi halinde aynı manager'ın ikinci kez kapatılmasını önler;
enclosing manager yeni exception ile normal unwind'e devam eder.

## Doğrulama

Parser/HIR/lowering testi Tonic-owned item/target yapısını ve context opcode'larını,
verifier operand sınırlarını kontrol eder. Runtime ve CPython differential testleri
normal/exception çıkışı, suppression, custom truthiness, nested sıra, çoklu item,
target assignment hatası, captured exit rebinding, metaclass manager, cross-frame
bare reraise, yapısal çıkışlar ve exit-error replacement yollarını kapsar. Matris
interpreter/JIT ile normal/stress-GC modlarında çalıştırılır.
