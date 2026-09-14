# ADR 0013 — Instance truthiness protokolü

Durum: uygulanmış bootstrap. 4 Eylül 2026. Public stable ABI değildir.

Instance truthiness önce class C3 MRO'sunda `__bool__`, o yoksa `__len__` arar.
İkisinin de olmaması instance'ı doğru yapar. Instance üzerindeki aynı adlı alanlar
implicit protokol aramasını değiştirmez; class rebinding ve deletion bir sonraki
değerlendirmede görülür. Function, `staticmethod` ve `classmethod` mevcut
allocation-free `DescriptorCall` bağlamasını kullanır.

Koşul dalları ve `not`, guest protokolünü normal Tonic frame'inde çalıştırabilir.
`ReturnAction::Truth` protokol türünü ve yapılacak branch/not işlemini taşır. Guest
frame döndüğünde `__bool__` sonucunun exact bool olduğu doğrulanır; `__len__` sonucu
integer, i64 sınırı ve negatiflik kontrollerinden geçer. Synchronous native/builtin
sonuçlar da aynı doğrulama yolunu kullanır.

`and` ve `or` doğru/yanlış değeri değil seçilen özgün operandı döndürür. Bu yüzden
branch continuation'ı protokol sonucunu yalnız karar için kullanır, destination
register'a özgün operandı geri yazar. Operand continuation root traversal'ına
katılır; moving collection sırasında geçerliliği korunur. `not` ise canonical
Tonic bool üretir.

Bu dilim custom descriptor nesnesi üzerinden `__bool__`/`__len__` bağlamayı,
metaclass truthiness'ini ve inline-cache specialization'ını kapsamaz.
