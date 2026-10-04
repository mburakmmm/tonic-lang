# ADR 0097 — HPy H1 minimal context ve local handle sözleşmesi

## Durum

Kabul edildi ve uygulandı.

## Bağlam

HPy Universal library doğrulaması tek başına extension çalıştırmaz. Resmi HPy
`0.9.0` header'ları, metadata alanları ve 263 pointer-sized slot içeren exact bir
`HPyContext` bekler. Extension method'ları ayrıca VM değerlerine çağrı-scope
handle'larla erişmeli, guest exception üretmeli ve module nesnesini doğru `self`
olarak almalıdır. Tonic'in hareketli GC hedefi nedeniyle C tarafına heap adresi
verilemez.

## Karar

`tonic-hpy`, resmi `hpy-0.9.0` include ağacını yalnız ABI oracle ve test derleme
girdisi olarak vendored eder. Kaynak dağıtımının SHA-256 kaydı ve MIT lisansı
vendor dizininde tutulur; HPy'nin CPython runtime uygulaması linklenmez.

Host exact context düzenini boxed, sabit adresli ve çağrılar arasında immutable
tutar. Desteklenen slotlar şunlardır:

- `HPy_Dup` ve `HPy_Close`;
- `HPyLong_FromInt64_t` ve `HPyLong_AsInt64_t`;
- `HPyUnicode_FromString` ve `HPyUnicode_AsUTF8AndSize`;
- `HPyErr_SetString`, `HPyErr_Occurred` ve `HPyErr_Clear`.

Singleton/type/exception context handle'ları negatif, borrowed token'lardır.
Dinamik local handle token'ları süreç içinde monoton ve benzersizdir; her native
çağrı kendi token tablosunu taşır. Böylece kapatılmış, önceki çağrıdan kalmış veya
başka runtime'a ait token host değeri olarak çözülemez. UTF-8 pointer'ları çağrı
bitene kadar call state içinde tutulur. Sonuç ve exception state'in HPy
sözleşmesine aykırı kombinasyonları `SystemError` olur.

H1 module materialization yalnız `HPyFunc_NOARGS` ve `HPyFunc_O` yöntemlerini
kabul eder. Legacy CPython method, module C state, `HPyGlobal`, diğer HPyDef
türleri ve H2 argument imzaları import sırasında reddedilir. Doğrulanmış bütün
yöntemler `register_stateful_module` ile tek adımda görünür yapılır; duplicate
veya mevcut modül çakışması kısmi kayıt bırakmaz. Her modülün C çağrıları H5
execution-state/thread sözleşmesi gelene kadar serialize edilir.

## Sonuçlar

HPy handle hiçbir zaman Tonic `Value`, Rust adresi veya heap slot düzenini açığa
çıkarmaz. Pinlenmiş library method pointer'larını süreç boyunca geçerli tutar;
context ve extension durumu VM callback'lerinin `Arc` sahipliğiyle yaşar.

Gerçek Universal C fixture resmi header'larla derlenir, `libpython` dependency
audit'inden geçer ve interpreter/JIT caller yollarında constant, Fibonacci,
Unicode, dup/close, module self, exception cleanup ve stale/cross-runtime
durumlarını sınar. Unsupported H2 signature yükleme sırasında reddedilir.

Windows `.hpy0.pyd` yükleme kod yolu H1'e dahildir, fakat platform üzerinde
çalışan kanıt H6 CI kapısında kalır. Desteklenmeyen context slotları manifestte
unavailable'dır; güvenilmeyen native library yükleme desteklenmez. H6, yanlış
slot kullanımını versioned tanıya dönüştüren Debug/Trace yüzeyini tamamlayacaktır.
