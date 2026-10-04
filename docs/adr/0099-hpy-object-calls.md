# ADR 0099 — HPy object protokolleri ve keyword çağrı binder'ı

## Durum

Kabul edildi ve H2b'de uygulandı.

## Bağlam

HPy Universal method'ları `NOARGS`/`O` yanında FASTCALL tabanlı `VARARGS` ve
`KEYWORDS` imzaları kullanır. Tonic'in hızlı native çağrı yolu yalnız exact
arity taşıdığında keyword method'larını guest tuple/dict oluşturmadan çağırmak
mümkün değildi. Attribute, item ve call slotlarının doğrudan heap adresi görmesi
ise moving-GC ile opaque handle sınırını bozardı.

Vendored HPy 0.9 `autogen_ctx.h` ölçümü metadata sonrasında 0..262 aralığında
263 pointer-sized slot bulunduğunu gösterir. Önceki 261-slot envanteri sondaki
`ctx_Call` ve `ctx_CallMethod` alanlarını dışarıda bıraktığı için düzeltilmiştir.

## Karar

VM native kaydı `Exact(n)`, `VarArgs` ve `Keywords` imzalarını taşır. Keyword
callback'i positional handle dilimi ile `(String, Handle)` çiftlerini ayrı alır;
geçici guest tuple veya dict oluşturulmaz. Exact ve VARARGS kayıtları keyword
aldığında çağrı C sınırına ulaşmadan `TypeError` üretir.

HPy host `HPyFunc_NOARGS`, `HPyFunc_O`, `HPyFunc_VARARGS` ve
`HPyFunc_KEYWORDS` tanımlarını import sırasında doğrular. KEYWORDS method'unda
positional değerlerin ardından keyword değerleri aynı contiguous handle dizisine
yazılır; keyword adları ayrı bir tuple handle'ıdır. Desteklenmeyen imza yine
deterministic import hatasıdır.

Attribute, item, membership, length, repr ve call slotları yalnız call-scope
HPy local handle'larını Tonic `Context` operasyonlarına çevirir. Kalıcı cache veya
native nesne adresi tutulmaz. `HPy_Call`/`HPy_CallMethod` keyword adlarını ve
değer sayısını doğrular; `HPy_CallTupleDict` yalnız tuple positional ve string-key
dict kabul eder. Başarısız bütün yollar local `Context` düşerken kökleri temizler.

## Sonuçlar

Gerçek HPy 0.9 C fixture VARARGS/KEYWORDS method binding, ordinary instance
attribute mutation, list/dict item mutation/deletion, contains/length/repr ve
üç call biçimini hem interpreter hem JIT caller modunda çalıştırır. Yanlış
keyword arity, non-callable ve item türü hata yollarında aktif handle sayısı
sıfıra döner. Bu capability yalnız yayımlanan fonksiyon listesi için geçerlidir;
bigint/float, geniş exception API'si, globals/fields/types/buffer ve Debug/Trace
context desteği anlamına gelmez.
