# Typed f64 buffer ara ölçümü

12 Eylül 2026; Apple Silicon arm64, macOS 26.6, Rust 1.86.0, release LTO.
Bu sonuç tamamlanma sonrası nihai benchmark değildir.

Komut:

```sh
cargo bench -p tonic-runtime --bench buffer --locked --offline
```

Program 1.024 küçük integer değerini kurar ve 10.000 kez `fastmath.sum(values)`
çağırır; toplam 10,24 milyon eleman okunur. Parse/compile süre dışında, VM run
süre içindedir. GC iki varyantta da ölçüm boyunca kapalıdır. Üç warmup ve 15 örnek
alınır. Boxed liste generic fallback'te her `Value`yu sayı olarak çözer; f64 buffer
bir kez oluşturulur ve her sum'da aynı contiguous allocation doğrudan taranır.

| Saklama | Medyan | Min–maks | Eleman/s | Export | Copy | Guest allocation |
|---|---:|---:|---:|---:|---:|---:|
| `List<Value>` | 46,920 ms | 45,537–50,372 ms | 218.242.656 | 0 | 0 | 12.049 |
| non-moving `f64` buffer | 9,401 ms | 8,753–11,183 ms | 1.089.294.027 | 10.000 | 1 | 12.050 |

Typed buffer bu taramada yaklaşık 4,99× throughput verdi. Tek fazla guest
allocation buffer owner nesnesidir; backing data, shape ve strides bunun dışındaki
sabit tahsislerdir. 10.000 sum boyunca yalnız ilk list→buffer dönüşümünde bir copy
sayılmış, exportların hiçbirinde kopya yapılmamıştır. Her sum sonucu mevcut dil
semantiği gereği boxed float ayırdığı için iki varyantta da yaklaşık aynı 10.000
sonuç allocation'ı bulunur.
