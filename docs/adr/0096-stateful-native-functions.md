# ADR 0096 — VM sahipliğinde stateful native fonksiyonlar

## Durum

Kabul edildi ve uygulandı.

## Bağlam

Tonic'in mevcut Rust `NativeFn` ve C `CNativeFn` kayıtları düz function pointer
tutar. Bu model `fastmath` gibi durumsuz fonksiyonlar için yeterlidir. HPy
module method'ları ise pinlenmiş library, doğrulanmış method descriptor ve
runtime'a ait `HPyContext` durumunu çağrılar arasında korumalıdır. Global bir
registry runtime izolasyonunu ve shutdown sahipliğini bozar.

## Karar

Runtime, `Arc<StatefulNativeFn>` alan `register_stateful_native` API'sini sunar.
Callback `Send + Sync + 'static` olmalı ve normal native `Context`/`Handle`
yüzeyini kullanır. VM callback'i çağrıdan önce registry'den clone eder; böylece
VM execution state mutable ödünç alınırken registry referansı canlı tutulmaz.
Extension durumu son kayıt VM shutdown sırasında bırakılana kadar yaşar.

`Context::native_module` kayıtlı modül nesnesini geçerli native-call local
scope'una alır. Bu, HPy module method'larına doğru `self` handle'ını vermeyi
mümkün kılar.

## Sonuçlar

Yeni dinamik dispatch yalnız stateful native extension kolunda ödenir;
Tonic-to-Tonic, builtin, durumsuz Rust ve C native yolları değişmez. Bir VM
taşınabildiği için callback durumu da thread'ler arasında güvenle taşınabilir
olmalıdır. HPy adapter kendi C çağrılarını ayrıca serialize edecek ve local
handle/error state'ini her çağrıda ayrı tutacaktır.
