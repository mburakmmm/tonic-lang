# ADR 0060: `finally` unwind ve yapısal çıkışlar

## Durum

Kabul edildi ve uygulandı. Context manager (`with`) lowering'i daha sonra bu
mekanizmanın üzerinde [ADR 0061](0061-context-manager-unwind.md) ile tamamlandı.

## Karar

Tonic `finally`yi host stack unwinding veya Rust `Drop` yan etkisiyle modellemez.
Derleyici aktif dil cleanup'larını lexical bir yığında tutar. `return`, `break`
ve `continue`, hedefe gitmeden önce içten dışa exception-binding cleanup ve
finalizer bytecode'unu üretir. `return` değeri finalizer çalışmadan önce register'a
alınır; finalizer içindeki yeni `return` veya kontrol aktarımı önceki sonucu
Python semantiğine uygun biçimde bastırır.

Normal fallthrough final body'yi doğrudan çalıştırır. Hata yolu dış bir
`ExceptionRegion` üzerinden exception'ı register'a alır, aktif context'e iter,
final body'yi çalıştırır ve özgün exception'ı yeniden yükseltir. Finalizer yeni
bir exception yükseltirse sentetik cleanup region özgün context'i kaldırır ve
yeni hatayı korur.

Yapısal çıkış için inline edilen finalizer kodu korunan try aralığının içinde
kalabilir. Bu kodda oluşan hatanın aynı finalizer'ı ikinci kez çalıştırmaması için
derleyici daha dar bypass region'ı üretir; VM en iç region'ı seçerek hatayı try
aralığının dışındaki landing'e taşır. Enclosing finalizer'lar sırayla çalışmaya
devam eder. Bu düzen `finally`yi exactly-once tutarken mevcut register VM ve JIT
fallback sınırını değiştirmez.

## Doğrulama

Compiler testi `finally` AST/lowering ve region üretimini doğrular. Runtime ve
CPython differential testleri normal tamamlama, handled/unhandled exception,
`return`, `break`, `continue`, nested pending exception, bare reraise, finalizer
return'üyle sonuç bastırma ve finalizer hatasıyla eski exception'ı değiştirme
yollarını kapsar. Aynı corpus interpreter/JIT ile normal/stress-GC modlarında
çalıştırılır.
