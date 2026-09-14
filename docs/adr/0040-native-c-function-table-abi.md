# ADR 0040 — Native C function-table ABI v1

Durum: uygulanmış. 12 Eylül 2026.

## Karar

Public native ABI tek `tonic_get_api` bootstrap sembolü ve immutable `TonicApi`
function table kullanır. Header sırasıyla 32-bit `struct_size`, 32-bit
`abi_version` ve 64-bit capability maskesidir. ABI v1 exact eşleşir; extension
istediği minimum prefix size ve zorunlu capability bitlerini bildirir. Uyuşmazlık
kısmi kayıt yapmadan reddedilir.

`TonicContext` opaque pointer, `TonicHandle` 64-bit logical ID'dir. Her operasyon
fixed-width `TonicStatus` döndürür ve değerleri out-parametresine yazar. Guest
exception context'te saklanır; kind/message çağıran buffer'ına kopyalanır. Normal
başarılı API işlemi önceki hata durumunu temizler. Local handle'lar callback scope
çıkışında topluca iptal edilir; result scope kapanmadan internal `Value`ya çözülür.
Extension init callback'i pazarlık edilen aynı static table ve ayrı local context
scope'u alır; init başarısızlığı veya panic'i kayıtlı native çağrıdan bağımsız
tanıya çevrilir.

Rust enum discriminantları yabancı girdide kullanılmaz. Exception kind,
capability ve status C ile uyumlu integer temsillerdir; bilinmeyen değerler tanımlı
status ile reddedilir. Her function-table girişi panic'i yakalar. Native callback
trampoline'ı `C-unwind` çağrısını `catch_unwind` ile sarar; Rust test extension
panic'i `RuntimeError` olur ve unwind ABI sınırından çıkmaz.

## Gerekçe ve sonuçlar

Function table alan eklemeyi prefix-size kontrolüyle, isteğe bağlı alt sistemleri
capability sürümüyle ayırır. Internal `Value`, heap/object layout'u ve Cranelift
helper ABI'si public sözleşmeye dönüşmez. External kod argument tuple veya keyword
dict materialize etmeden handle slice/count alır.

ABI trusted native code içindir. Null/count eşleri doğrulanır, fakat keyfi non-null
pointer'ı doğrulamak C proses içi güvenlik modeli içinde mümkün değildir. Dynamic
library loader, unload, persistent callback, buffer ve foreign vtable ayrı sonraki
aşamalardır.

100.000 çağrılı release A/B ölçümünde Rust-native yol 17,770 ms, C ABI 20,145 ms
medyan verdi; sınır maliyeti yaklaşık %13,4'tür. Ayrıntı
[`NATIVE_C_ABI_BASELINE.md`](../NATIVE_C_ABI_BASELINE.md) dosyasındadır.
