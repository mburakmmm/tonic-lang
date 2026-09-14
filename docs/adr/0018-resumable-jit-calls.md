# ADR 0018 — Resumable JIT ve explicit VM çağrı frame'leri

## Durum

Kabul edildi; recursive Tonic çağrıları için ilk güvenli yol uygulanmıştır.

## Karar

JIT, guest recursion'ı Rust veya native makine recursion'ına dönüştürmez. `CALL`
opcode'unda exact PC içeren kontrollü bir side exit üretir. VM register dizisini
geri alır, mevcut generic/adaptive binder ile explicit child frame'i kurar ve
child tamamlandığında caller'ı `CALL` sonrasındaki PC'den aynı native code object
içinde yeniden başlatır.

Native entry ABI bu nedenle bir `start_pc` alır. Cranelift giriş bloğundaki switch,
verified instruction sınırlarını resume entry olarak eşler. Her instruction bloğu
aynı materialized register dizisini kullanır. Return, guard deopt, helper error ve
call side exit birbirinden ayrı status bitleri ve exact bytecode PC taşır.

`LOAD_GLOBAL` allocation yapmayan runtime helper'dır ve güncel globals dizisini
her erişimde okur. Bu aşamada global değeri native koda sabitleyen bir cache yoktur;
dolayısıyla rebinding için invalidation gerekmeksizin semantik korunur. Eksik isim
`NameError` türü ve kaynak konumuyla döner. Allocation yapmadığı için global helper
GC safepoint'i değildir. `/` helper'ı allocation yapabildiğinden safepoint olmaya
devam eder.

## Gerekçe

Bu model Tonic'in explicit frame ve kesin root mimarisini korur, host stack taşması
ve native recursion'a bağlı GC görünmezliği oluşturmaz. Call binder, keyword/default
ve callable protokollerinin tek doğruluk kaynağı olarak VM'de kalır. İlk aşama
side exit ve resume maliyeti öder; daha sonra exact-callee guard'lı hızlı call yolu
aynı continuation sözleşmesi üzerinde eklenebilir.

## Ölçüm ve sınırlar

`fib(20)` ölçümünde ilk sürüm her `LOAD_GLOBAL` helper'ında tüm recursive roots'u
taradığı için adaptive interpreter'dan yavaştı: 4,018 ms / 2,267 ms. Allocation
yapmayan global helper safepoint olmaktan çıkarılınca son koşuda JIT 1,974 ms,
adaptive interpreter 2,245 ms oldu; 1,14× kazanç. Çalışma 21.876 side exit ve aynı sayıda
resume yapar. Bu doğrudan native call değildir ve çağrı maliyeti hâlâ yüksektir.

Global rebinding, recursive frame dönüşü, stress GC, exact hata PC'si ve normal
interpreter/JIT differential matrisi test edilir. Native backedge polling, OSR,
unboxed stack map ve dependency-guarded global cache açık işlerdir.
