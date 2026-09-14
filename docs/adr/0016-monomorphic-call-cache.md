# ADR 0016 — Monomorphic Tonic function-call cache

## Durum

Kabul edildi; basit exact-arity Tonic fonksiyon çağrıları için uygulanmıştır.

## Karar

Bir `Call` sitesi aynı Tonic function `Value` kimliğini sekiz başarılı çağrıda
görürse monomorphic cache'e geçer. Cache exact logical handle ve code ID tutar.
Fast path yalnız şu koşullarda kullanılır:

- positional argüman sayısı signature ile aynıdır;
- keyword, default, keyword-only, `*args` veya `**kwargs` yoktur;
- fonksiyon closure/cell taşımaz ve class body değildir;
- cached callee `Value` mevcut callee ile aynıdır.

Fast path generic callable/descriptor ayrıştırmasını ve binder'ı atlayıp register
frame'ini doğrudan kurar. Frame limiti, register bütçesi, return destination,
callable root'u ve JIT giriş politikası normal frame ile aynıdır.

Callee guard failure cache'i hemen generic duruma döndürür; mevcut çağrı normal
`invoke` ve binder üzerinden yürür. Ardından yeni callee sekiz kararlı gözlemle
cache olabilir. Böylece global veya local function rebinding semantiği korunur.

Cache'teki handle GC root'u değildir. Canlı callee çağrı register'ında zaten root
olur. Eski nesne toplanırsa slot generation değişir; generation wrap yasak olduğu
için yeni nesne stale cached kimlikle eşleşemez. Function nesnesinin code ve
capture/default metadata'sı oluşturulduktan sonra mutasyona açık değildir.

## Ölçüm

Release A/B benchmarkında 100.000 kısa leaf add çağrısı generic 15.588 ms'den
adaptive 13.692 ms'ye indi; süre %12,2 azaldı. Mul/floor/mod çağrı işinde %7,7,
float add çağrı işinde %10,2 azalma görüldü. Polymorphic cache, bound method ve
attribute-call specialization bu ölçümden çıkarılmaz; ayrı guard/version tasarımı
gerektirir.
