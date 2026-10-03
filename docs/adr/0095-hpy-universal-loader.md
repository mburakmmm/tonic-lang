# ADR 0095 — HPy Universal library doğrulaması ve pinleme

## Durum

Kabul edildi ve H1a'da uygulandı.

## Bağlam

HPy `0.9.0` Universal modülü, dosya adındaki `hpy0` ABI etiketi yanında modül
adından türetilen dört C sembolü yayınlar. Loader yanlış modül adını, eksik
sembolü veya desteklenmeyen ABI'yi çalıştırma bağlamı kurulmadan reddetmelidir.
Extension içindeki method, type ve payload function pointer'ları import
çağrısından daha uzun yaşayabilir. H1'de bunların tamamının güvenli biçimde
geçersiz kılındığını kanıtlayan bir unload protokolü yoktur.

## Karar

`tonic-hpy`, macOS ve Linux'ta yalnız `<module>.hpy0.so` dosyalarını açar. Modül
adı ASCII C identifier olmalıdır. Loader sırasıyla şu sembolleri çözer:

- `get_required_hpy_major_version_<module>`;
- `get_required_hpy_minor_version_<module>`;
- `HPyInitGlobalContext_<module>`;
- `HPyInit_<module>`.

Extension major ABI'si host ile tam eşleşmeli, minor ABI'si host minor
sürümünden büyük olmamalıdır. Başarılı doğrulamadan sonra library handle'ı
`ManuallyDrop` içinde tutulur ve nesnenin düşürülmesi `dlclose` çağırmaz. Böylece
extension pointer'ları süreç boyunca geçerli kalır. Windows H1a'da açık bir
`UnsupportedPlatform` hatasıyla fail-closed kalır.

Native library yüklemek güvenli Rust'ın doğrulayamayacağı constructor ve C
signature sözleşmelerini çalıştırır. Bu nedenle public yükleme ve module-def
çağrıları `unsafe` kalır; crate genelinde unsafe yasaktır, yalnız küçük loader
modülü ve ABI fixture testleri gerekçeli unsafe bloklarına izin verir.

## Sonuçlar

Loader libpython'e bağlanmaz ve Tonic'in `Value`/heap düzenini HPy'ye sızdırmaz.
H1a yalnız doğrulama ve lifetime temelidir: `HPyContext` kurulana, module def
materialize edilene ve capability manifesti güncellenene kadar hiçbir HPy
fonksiyonu kullanılabilir ilan edilmez. Gelecekte unload eklenecekse bütün canlı
function/type/payload sahiplikleri için kanıtlanmış bir invalidation protokolü
gerekecektir.

Entegrasyon testleri derleme sırasında küçük C library'leri üretir ve başarılı
yükleme, hatalı ad/ABI, eksik sembol, null module definition ve drop sırasında
destructor çalışmaması durumlarını doğrular.
