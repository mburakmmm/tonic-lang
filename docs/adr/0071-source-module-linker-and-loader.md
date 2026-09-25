# ADR 0071: Kaynak modül bağlayıcısı ve yükleyicisi

## Durum

Uygulandı. Bu karar `.tonic`/`.py` kaynak modüllerini, absolute dotted paketleri,
`from ... import ...`, circular import, başarısız import rollback'i, modül global
sürümlerini ve import edilen fonksiyonların Cranelift yürütmesini kapsar.

## Karar

Bytecode v14 her programda doğrulanmış bir `ModuleInfo` tablosu taşır. Her kayıt
modül adını, kaynak dosyasını, module-entry code kimliğini, bitişik code aralığını
ve kendisine ait global symbol slotlarını belirtir. Verifier code aralıklarının
tüm programı tam ve çakışmasız örttüğünü, module entry'lerin parametresiz olduğunu,
modül adları ile entry'lerin benzersizliğini ve global slot sahipliğinin modüller
arasında paylaşılmadığını doğrular.

Derleyici her kaynağı önce bağımsız Tonic AST/HIR/bytecode programına çevirir,
sonra programları tek code alanında bağlar. Local, attribute ve keyword adları
canonical symbol kimliklerini paylaşır. Global operandlar ise aynı yazılı ada
sahip olsalar bile modül başına private symbol slotuna taşınır; bu nedenle JIT ve
yorumlayıcı mevcut düz global dizi hızlı yolunu korurken modül ad alanları birbirini
kirletmez. Function-site code kimlikleri ve bütün symbol operandları bağlama
sırasında yeniden konumlandırılır, ardından birleşik program yeniden doğrulanır.

Dosya CLI yükleyicisi importları nested function/class/control-flow gövdelerinde
önden keşfeder. Giriş dosyasının dizininde `<name>.tonic`, `<name>.py`,
`<name>/__init__.tonic` ve `<name>/__init__.py` sırasını kullanır. Dotted import
her paket önekini sırayla yükler. Alias yoksa Python davranışı gibi üst paket,
alias varsa leaf modül bağlanır. `from package import name` önce hazır attribute'u
kullanır; yoksa bağlı `package.name` alt modülünü yükleyip ebeveyne bağlar. Native
modül kayıtları aynı generic import fallback'inde kalır.

Runtime kaynak modüllerini `Uninitialized`, `Initializing` ve `Loaded` durumlarıyla
izler. Nesne ilk gerçek importta tembel oluşturulur; böylece import kullanmayan tek
dosyalı programlara modül/string allocation maliyeti eklenmez. `Initializing`
nesnenin önbellekten dönmesi circular importta kısmi ad alanını görünür kılar.
Başarı yalnız bir kez çalıştırılır. Dışarı yayılan hata import frame'lerini bulur,
kısmi global/member durumunu temizler, parent bağlantısını kaldırır ve sonraki
importın temiz biçimde yeniden çalışmasına izin verir.

Her global store/delete ve module attribute store/delete aynı private global slotu
ile module member'ını birlikte günceller ve checked `u64` sürümünü artırır. Parent'a
alt modül eklemek ve rollback de parent sürümünü artırır. Cranelift module-entry
code'unu derlemez; import edilen normal fonksiyonları olağan hotness kurallarıyla
derler. JIT global load'u materialized `jit_globals` slotunun güncel değerini okur,
bu yüzden `module.value = ...` sonrasında eski değer cache'lenmez.

Import opcode'ları normal VM frame/exception sistemini kullanır. `ImportFrom`
bytecode'u dinamik alt modül yüklemesinde askıya alınabilir; JIT desteklemediği
import yolunu exact PC'de yorumlayıcıya bırakır. Kaynak hata tanısı code aralığından
doğru module filename'e eşlenir ve CLI o dosyayı render eder.

Relative import ve `from x import *` parser tarafından konumlu
`UnsupportedSyntax` olarak reddedilir. Bunlar genel modül runtime'ını taklit eden
sessiz davranışlar olarak eklenmemiştir; kapsamlı syntax conformance maddesinde
açık kalır.

## Doğrulama

Compiler testleri nested import keşfini, dotted prefixleri, `ImportFrom` opcode'unu,
symbol/code relocation'ını ve verifier global sahipliğini kapsar. Runtime testleri
normal/stress GC altında tek seferlik yükleme, circular partial state, izole ve
versioned global, package/child bağlama, `from` alt modül fallback'i, başarısız
import rollback/retry ve module attribute mutation'ını sınar. Ayrı JIT testi import
edilen hot fonksiyonun native derlendiğini ve modül global değişiminden sonra yeni
değeri okuduğunu sayımlarla doğrular. CLI testleri `.tonic`, `.py`, package init,
dotted/from biçimleri ve imported-file hata konumunu gerçek geçici dosyalarla
çalıştırır.

Debug ve release çalışma alanındaki 292 test; `clippy -D warnings`, C başlık smoke
testi ve Python 3.14.6'ya karşı debug/release × interpreter/JIT × normal/stress-GC
sekizli differential matrisi geçmiştir. Her differential koşu 299 stdout ve 147
exception vakası içerir.
