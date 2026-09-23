# ADR 0067: Power, bitwise ve in-place operator protokolleri

## Durum

Uygulandı. Bu karar `**`, `|`, `^`, `&`, `<<`, `>>`, `~` ve arithmetic/bitwise
in-place operator ailesini kapsar. Üç argümanlı `pow` builtin'i ve complex sayı
sonuçları bu aşamanın kapsamı dışındadır.

## Karar

Bytecode v13 her operator için bağımsız, sabit wire opcode kullanır. In-place
opcode önce `__i*__` adayını çağırır; `NotImplemented` sonucunda aynı direct ve
reflected zincire geçer. Strict sağ-alt-sınıf reflected önceliği, aynı tipte
reflected çağrıyı yinelememe ve guest çağrılarında precise continuation root'ları
ADR 0066 ile aynı sözleşmeyi izler. `~` suspending `__invert__` lookup'undan sonra
builtin tam sayı yoluna düşer.

Builtin uygulama arbitrary-precision integer power, bitwise ve shift işlemlerini
destekler. Bool-bool bitwise sonucu bool, bool-int karışımı int'tir. Negatif shift
`ValueError`, sıfırın negatif kuvveti `ZeroDivisionError`, bitwise float operandı
`TypeError` üretir. Tonic henüz complex sayı taşımadığı için negatif tabanın
kesirli kuvveti açık `TypeError` verir. Tahmini sonucu 2^26 bitten büyük shift ve
power işlemleri host belleğini tüketmeden `MemoryError` ile kesilir; `0`, `1` ve
`-1` tabanları bu sınırı sabit sonuçla güvenle aşabilir.

Cranelift bu yeni opcode'ları henüz native lower etmez. JIT derleme denemesi
desteklenmeyen opcode'u gördüğünde code object'i interpreter'da bırakır. Böylece
operator protokolü veya hata davranışı atlanmaz; ilerideki native lowering aynı
bytecode ve guard sözleşmesini koruyacaktır.

## Doğrulama

Parser testi bütün sözdizimini, verifier testi operand şekillerini, runtime testi
BigInt/bool/float sınırlarını, negatif shift ve power hatalarını, kaynak limitini
ve JIT fallback'ini kapsar. Class testi direct/reflected/strict-subclass sırasını,
`NotImplemented` zincirini, `__invert__` ve on bir yeni in-place metodu suspending
guest frame'leriyle doğrular. Differential corpus aynı observable sonuç ve
exception türlerini Python 3.14.6 ile karşılaştırır.
