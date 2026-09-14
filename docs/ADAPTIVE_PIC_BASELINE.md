# Adaptive PIC ara baseline

5 Eylül 2026, macOS ARM64 release profili. Her satır üç warmup sonrasındaki 15
fresh-VM çalıştırmasının medyanıdır; parse/compile/verify süre dışındadır.

| İş yükü | Önceki adaptive | Generic (son binary) | PIC adaptive | Generic'e göre |
|---|---:|---:|---:|---:|
| İki Tonic function arasında 100.000 çağrı | 32.942 ms | 32.223 ms | 29.972 ms | %7,0 hızlı |
| İki instance shape arasında 100.000 attribute load | 31.814 ms | 30.572 ms | 26.420 ms | %13,6 hızlı |
| Her iterasyonda ilgisiz class mutation + 100.000 attribute load | 18.980 ms | 18.900 ms | 13.062 ms | %30,9 hızlı |

İki testte de site önce sekizden fazla kez tek hedef görür, ardından iki hedef
arasında dönüşür. Son koşuda her site bir monomorphic miss, bir PIC terfisi ve
sonraki iterasyonlarda sıfır ek miss kaydetti. PIC tabloları yalnız terfi eden
siteler için ayrılır ve toplam bytecode instruction sayısıyla sınırlıdır.
Üçüncü satır eski global class epoch'unun her `Noise.y` yazımında `Target.x`
cache'ini gereksiz düşürmesini ölçer. Dependency version sonrasında site bir kez
quicken olur ve sıfır miss kaydeder; class mutation yolu heap taramaz veya geçici
target vektörü ayırmaz.

Yeniden üretim:

```sh
cargo bench -p tonic-runtime --bench adaptive_pic --locked --offline
```
