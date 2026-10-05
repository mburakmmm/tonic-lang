# ADR 0104 — Canonical TypePlan v1 ve annotation mutation epoch'u

## Durum

Kabul edildi; annotation destekli typed-JIT hattının ilk temeli uygulandı.

## Bağlam

Python annotation ifadeleri definition sırasında normal Tonic değerleri olarak
değerlendirilir ve `__annotations__` dict'inde saklanır. JIT'in AST metnine veya
mutable runtime nesnelerine doğrudan bağımlı olması canonical cache anahtarını,
invalidation'ı ve deopt tanısını belirsizleştirir. Mevcut dict `version` alanı
yalnız structural değişiklikleri izlediği için var olan annotation değerinin
değiştirilmesini yakalamaz.

## Karar

Runtime-owned `TypePlan` v1 exact scalar/plain container, homogeneous container,
fixed tuple ve user-class identity/version düğümlerini içerir. Çözümleme depth ve
alias cycle sınırlarına sahiptir. Desteklenmeyen değerler exception üretmez;
stable `TypePlanRejection` koduyla optimizasyon dışı kalır.

Plan entry'leri annotation adına göre sıralanır ve schema version dâhil açık
discriminant kodlamasıyla sabit FNV-1a hash'i üretir. Raw Rust layout'u, heap
adresi veya pointer hash'e girmez. User class planı compact `TypeId` ve class
version taşır.

Dict'e structural `version` yanında her başarılı set/delete/clear işleminde
artan `mutation_version` eklenir. Function plan cache bu content epoch'unu taşır;
epoch değişmişse annotation değerlerinden yeniden kurulur. Structural iterator
semantiği aynı kalır, dolayısıyla iteration sırasında value update izinli olmaya
devam eder.

## Sonuçlar

Annotation metadata'sı Python semantiğini veya call ABI'sini değiştirmez. Plan
oluşturma ya da reddetme yalnız gelecekteki tier seçimini etkiler. V1 henüz
Union/Optional/Literal/Callable, variadic tuple, bytes, buffer/dtype, global alias
dependency veya typed native entry içermez; bunlar genişleme kapılarıdır.
