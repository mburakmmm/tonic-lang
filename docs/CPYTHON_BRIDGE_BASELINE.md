# CPython bridge genişletilmiş ara ölçümü

14 Eylül 2026; Apple Silicon arm64, macOS 26.6, Rust 1.86.0, CPython 3.14.6,
release LTO. Bu sonuç tamamlanma sonrası nihai benchmark değildir.

Komut:

```sh
cargo bench -p tonic-cpython --bench bridge --locked --offline
```

İlk dört yol Tonic `while` döngüsünde `abs(-42)` işlemini 100.000 kez yürütür.
`tonic_builtin` runtime builtin'ini doğrudan çağırır. `cpython_number_abs`, her çağrıda
C ABI'ye geçer, CPython execution state'i edinir, `PyLong` oluşturur,
`PyNumber_Absolute` çağırır, sonucu tekrar Tonic immediate integer'a çevirir ve
execution state'i bırakır. `cpython_named_call` modülü ve callable'ı adla çözer.
`cpython_generic_call1` aynı named call'ı tür-koruyan genel dönüşümle yapar.
`cpython_tonic_callback` CPython callable proxy üzerinden Tonic identity fonksiyonuna
geri döner. Son dört yol sırasıyla genel keyword çağrısını, keyword taşıyan proxy
callback'ini, proxy attribute get'i ve dış CPython referansı canlıyken weak-cache
üzerinden aynı proxy repr'ını ölçer. Kaynak list/dict argüman nesneleri
döngü dışında kurulur; bridge her çağrıda gerekli CPython materialization'ını yapar.
Parse/compile ve üç warmup timer dışındadır; 15 süreç içi VM örneğinin medyanı
raporlanır.

| Yol | Medyan | Min–maks | Çağrı/s | C native call | Guest allocation |
|---|---:|---:|---:|---:|---:|
| Tonic builtin | 10,921 ms | 10,843–12,480 ms | 9.157.055 | 0 | 0 |
| CPython `PyNumber_Absolute` | 25,281 ms | 24,984–25,556 ms | 3.955.553 | 100.000 | 0 |
| İsimle int `builtins.abs` | 65,895 ms | 65,007–67,090 ms | 1.517.564 | 100.000 | 2 |
| Genel dönüşümlü `call1` | 69,653 ms | 68,585–70,846 ms | 1.435.693 | 100.000 | 2 |
| CPython→Tonic positional proxy callback | 47,280 ms | 46,255–48,700 ms | 2.115.063 | 100.001 | 2 |
| Genel CPython keyword çağrısı | 115,243 ms | 114,705–116,744 ms | 867.733 | 100.000 | 100.006 |
| CPython→Tonic keyword proxy callback | 109,487 ms | 108,150–112,395 ms | 913.348 | 100.001 | 200.005 |
| Proxy attribute get | 104,626 ms | 102,721–106,328 ms | 955.784 | 100.001 | 13 |
| Weak-cache proxy repr | 112,791 ms | 110,889–114,945 ms | 886.596 | 100.003 | 200.023 |

Bu mikrobenchmarkta doğrudan CPython sayı sınırı Tonic builtin yolunun yaklaşık 2,32 katı
süre alır. Positional proxy callback için keyword'süz fast path boş dict oluşturmaz;
100.000 çağrı yine yalnız iki başlangıç guest allocation'ı taşır. Keyword yollarındaki
guest allocation sayısı dönüşüm ve binder materialization maliyetini görünür kılar.
Guest allocation sayacının sıfır olması CPython `PyLong` tahsisinin yok
olduğu anlamına gelmez; host/CPython allocation henüz ayrı ölçülmemektedir. Sonuç
uyumluluk maliyetinin yalnız bridge kullanan çağrılarda ödendiğini ve Tonic fast
path'ine CPython lock/object layout maliyeti eklenmediğini doğrular. Weak-cache
satırında dış `sys` modülü proxy'yi canlı tutar; 100.000 ihracın her biri aynı
non-owning cache girdisini ve CPython nesne kimliğini yeniden kullanır.
