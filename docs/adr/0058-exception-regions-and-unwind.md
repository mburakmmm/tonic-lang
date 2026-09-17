# ADR 0058: Exception region'ları ve VM unwind

## Durum

Kabul edildi ve uygulandı. `finally` daha sonra
[ADR 0060](0060-finally-unwind.md) ile, `with` ise
[ADR 0061](0061-context-manager-unwind.md) ile, exception chaining ve managed
traceback ise [ADR 0062](0062-exception-chaining-and-traceback.md) ile tamamlandı.

## Karar

Bytecode sürümü 10, instruction aralıklarını handler girişlerine bağlayan açık
`ExceptionRegion` tablolarını taşır. Kayıtlar başlangıç/bitiş, handler hedefi ve
managed exception'ın yazılacağı register'ı içerir. Verifier register ve hedef
sınırlarını, boş/çapraz aralıkları ve handler girişindeki argument-builder
durumunu doğrular. Ham Rust enum layout'u wire formatına girmez.

VM hata alan instruction'ın en iç region'ını seçer; gerekirse frame, register,
cell, argument scratch ve bekleyen class state'ini kesin tabanlara kadar açar.
Handler dispatch'i typed, tuple ve bare eşleşmeyi normal class/MRO semantiğiyle
yapar. Exception context dispatcher tarafından örtük değiştirilmez:
`PUSH_EXCEPTION` yalnız eşleşen handler'a girerken bağlamı yığına alır,
`CLEAR_EXCEPTION` çıkışta kaldırır. Bu ayrım nested handler içindeki yeni bir
hata sırasında dış context'in sızmasını ya da kaybolmasını önler.

Derleyici `except as` bağını normal tamamlama, handler hatası, `return`, `break`
ve `continue` çıkışlarında temizler. Handler body için sentetik cleanup region'ı
eski context'i kaldırıp yeni hatayı tekrar fırlatır. Bare `raise` yığının üst
context'ini kullanır. JIT'in runtime-helper hatası aynı VM dispatcher'ına girer;
handler opcode'ları içeren function şimdilik güvenli interpreter fallback'inde
kalır.

## Doğrulama

AST/lowering testi region ve opcode üretimini, verifier bozuk metadata/operand
reddini sınar. Runtime testleri typed/tuple/bare handler, `else`, frame'ler arası
unwind, nested context restoration, binding cleanup, bütün yapısal çıkışlar ve
JIT callee hatasının caller tarafından yakalanmasını kapsar. Aynı davranış
CPython differential corpus'unda interpreter/JIT ve normal/stress-GC modlarında
karşılaştırılır.
