# ADR 0098 — HPy container ve builder yaşam döngüsü

## Durum

Kabul edildi ve H2a'da uygulandı.

## Bağlam

HPy fixed-size list ve tuple builder'ları oluşturma sırasında henüz guest
container olmayan mutable bir durum taşır. Bu durum Tonic heap'ine erken
konursa eksik elemanlar guest kodu veya GC tarafından gözlemlenebilir. Builder
token'larının local object handle'larıyla karışması da stale kullanımın yanlış
bir nesneye bağlanmasına yol açabilir.

## Karar

Her native çağrı ayrı builder tablosu tutar. Builder token'ları monoton,
process-unique bir namespace kullanır ve list/tuple türünü taşır. Set işlemi
index ile handle'ı doğrular. Build bütün slotların doldurulduğunu kanıtladıktan
sonra tek seferde Tonic list/tuple üretir; cancel durumu nesne üretmeden siler.
Build, cancel veya çağrı sonundan sonra token stale olur. Bitmemiş builder ile
method dönüşü `HandleError`, eksik slotla build `SystemError` üretir.

H2a `HPyList_Check`, `HPyList_New(0)`, `HPyList_Append`, `HPyDict_Check`,
`HPyDict_New`, `HPyTuple_Check`, `HPyTuple_FromArray` ile list/tuple builder
new/set/build/cancel slotlarını yayınlar. Nonzero `HPyList_New`, item mutation
yüzeyi gelene kadar `NotImplementedError` üretir; fixed-size kod builder
kullanır.

## Sonuçlar

Yarım container Tonic heap'ine girmez ve builder lifetime çağrı scope'unu
aşamaz. Builder içindeki Tonic handle'ları native `Context` tarafından köklenir;
build sonrası normal moving-GC kuralları geçerlidir. Sonraki attr/item/call ve
keyword binder alt aşaması [ADR 0099](0099-hpy-object-calls.md) ile tamamlanmıştır.
