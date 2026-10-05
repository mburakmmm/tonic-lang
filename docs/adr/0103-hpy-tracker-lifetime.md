# ADR 0103 — HPyTracker sahipliği ve çağrı ömrü

## Durum

Kabul edildi ve H3b'de uygulandı.

## Bağlam

`HPyTracker`, extension'ın sahip olduğu birden fazla local `HPy` handle'ını hata
yollarında topluca kapatabilmesini sağlar. Tracker token'ını native adres veya
Tonic heap root'u yapmak moving-GC ve stale-handle doğrulamasını zayıflatır.
Tracker'ın bir çağrıdan diğerine taşınması da HPy local-handle ömrüne aykırıdır.

## Karar

Tracker token'ı process-unique opaque bir tamsayıdır; içerik yalnız aktif
`CallState` içindeki tabloda tutulur. `HPyTracker_Add` handle'ı kopyalamaz:
extension'ın mevcut local handle sahipliğini tracker'a bağlar. `Close` tracker'ı
önce tablodan tüketir, ardından kayıtlı handle'ları normal `HPy_Close` yoluyla
kapatır. `ForgetAll` yalnız tracker kaydını tüketir ve handle'ları açık bırakır.

Negatif veya sınırı aşan başlangıç kapasitesi, stale tracker, geçersiz local
handle, çift close/forget ve normal sonuçla birlikte açık tracker guest hatasıdır.
Bir hata zaten ayarlanmışsa call teardown tracker tablosunu düşürerek kaynakları
deterministik biçimde geri alır ve asıl exception'ı gölgelemez.

## Sonuçlar

Tracker hiçbir raw guest adresi taşımaz ve çağrıdan kaçamaz. Kapatılmış tracker'ın
handle'ı tekrar kullanılamaz; `ForgetAll` sonrası extension handle'ı kullanıp
kendisi kapatabilir. Gerçek HPy 0.9 C fixture bu iki başarı yolunu ve stale,
leak, negatif kapasite ile use-after-close hata yollarını interpreter/JIT ve
normal/stress-GC kiplerinde doğrular.

`HPyModuleDef.size` ayrı bir konudur. Pinned HPy 0.9 Universal public context'i
extension'a module-state pointer'ı veren bir API sunmadığından Tonic private bir
erişim ABI'si tanımlamaz.
