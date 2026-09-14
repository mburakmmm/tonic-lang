# Cranelift leaf JIT ara baseline

Bu ölçüm 5 Eylül 2026 tarihinde macOS ARM64 release profiliyle alınmıştır. Her
satır 3 warmup sonrasındaki 15 bağımsız VM çalıştırmasının medyanıdır. Parse,
compile ve verify zaman dışındadır; Cranelift compile süresi JIT çalışma süresine
dahildir. Bu bir ara mühendislik baseline'ıdır; Tonic tamamlanınca alınacak Python
karşılaştırmalı nihai benchmark değildir.

| İş yükü | Interpreter | JIT | Oran | JIT compile | JIT giriş/deopt |
|---|---:|---:|---:|---:|---:|
| `sum_to_1000000` | 89.335 ms | 3.752 ms | 23.81× | 280.2 µs | 1 / 0 |
| `leaf_add_calls_100000` | 13.069 ms | 13.019 ms | 1.00× | 0 | 0 / 0 |
| `leaf_mul_floor_mod_100000` | 18.892 ms | 15.415 ms | 1.23× | 205.2 µs | 99,993 / 0 |
| `leaf_runtime_div_100000` | 23.087 ms | 23.829 ms | 0.97× | 0 | 0 / 0 |
| `runtime_div_loop_100000` | 9.830 ms | 1.803 ms | 5.45× | 239.1 µs | 1 / 0 |
| `recursive_fib_20` | 2.428 ms | 1.909 ms | 1.27× | 304.6 µs | 43,760 / 0 |
| `unstable_float_add_100000` | 15.019 ms | 14.929 ms | 1.01× | 0 | 0 / 0 |
| `float_add_loop_100000` | 10.713 ms | 2.741 ms | 3.91× | 256.0 µs | 1 / 0 |
| `instance_attr_load_100000` | 9.354 ms | 9.623 ms | 0.97× | 0 | 0 / 0 |

İlk ölçümde her native çağrı yeni bir host `Vec<u64>` ayırıyordu. Aynı oturumda
alınan ilk medyanlar add için 15.936 ms, mul/floor/mod için 18.691 ms ve kararsız
float için 18.289 ms idi. VM-owned tekrar kullanılan register scratch alanı
eklendiğinde aynı oturumdaki ara ölçüm sırasıyla 13.345 ms, 16.348 ms ve 16.225 ms
oldu. Guest allocation sayaçları değişmedi; iyileşme bridge üzerindeki host
allocation'ı kaldırdı. Yukarıdaki tablo daha sonra eklenen hotness politikasıyla
yeniden alınmıştır.

Aynı binary içinde bütün adaptive state kapatılarak alınan generic/adaptive
medyanları sırasıyla şöyledir: numeric loop 91.685/89.335 ms, leaf add
14.406/13.069 ms, mul/floor/mod 20.224/18.892 ms, helper leaf
25.044/23.087 ms, runtime division loop 9.695/9.830 ms, recursive fib
2.641/2.258 ms, leaf float add 15.814/15.019 ms, internal float loop
10.589/10.713 ms ve instance attribute 13.518/9.354 ms. Call ağırlıklı vakalarda exact-callee cache,
attribute vakasında shape-slot cache baskındır.

Altı instruction'lık düz add fonksiyonunu native çağırmak adaptive interpreter'dan
yavaş ölçüldüğü için varsayılan JIT alt sınırı yedi instruction'dır. Böyle küçük
fonksiyonlar compile edilmez ve tekrarlı frame'ler unsupported cache'i görerek JIT
register materialization yoluna girmez. Runtime helper başına eşik dört instruction
artar. Üç `/` içeren kısa leaf ilk denemede adaptive tier'dan %14 yavaş ölçüldüğü
için varsayılan olarak compile edilmez. Daha büyük mul/floor/mod fonksiyonu native
kaldığı için bu koşuda %16 civarı throughput kazancı korunur.

Uzun tek numeric loop'ta kazanç dispatch'in büyük ölçüde kalkmasından gelir.
Çok kısa leaf çağrıda register kopyalama ve native boundary maliyeti kazancı
sınırlar. Backedge içermeyen fonksiyonun ilk yedi girişi interpreter'da kalır;
sekizinci giriş compile eşiğidir. Float fonksiyon bundan sonra exact-int guard'ını
sekiz kez kaybedip de-specialize edilir; 99.985 sonraki çağrı generic fallback'tir.
Bu bounded politika
tekrarlı pahalı deopt'u önler, ancak type-feedback tabanlı float specialization
yerine geçmez.

True division JIT yolu her `/` için opak runtime helper ve allocation safepoint'i
kullanır. İlk sürüm helper kullanmayan fonksiyonlarda bile her girişte yeni root
vektörü kurduğu için mul/floor/mod vakasını 31.628 ms'ye geriletti. Runtime-call
metadata guard'ı ve VM-owned yeniden kullanılan root tamponu sonrasında aynı vaka
15.815 ms oldu. Helper içeren 100.000 iterasyonluk OSR döngüsü division ve poll
toplamında 100.034 helper, 97 helper-triggered collection ve 100.001 guest
allocation ile 5,45× hızlandı.
Bu sonuç helper ABI'nin döngü dispatch'ini kaldırabildiğini gösterir; allocation ve
helper maliyetinin ortadan kalktığı anlamına gelmez. Döngüde exact-int guard'ını
kaçıran diğer binary işlemler de artık aynı helper yolunu kullanır. Internal float
add döngüsü 100.034 helper ve 97 helper-triggered collection ile 3,91× hızlandı;
her sonuç hâlâ boxed ve allocation yapar.

Recursive `fib(20)`, `CALL` noktasında VM side exit ve child dönüşünde arbitrary-PC
native resume kullanır. İlk uygulama allocation yapmayan her `LOAD_GLOBAL`
helper'ını GC safepoint sayıp recursive root ağacını taradığı için 4.018 ms ile
adaptive interpreter'ın 2.267 ms sonucundan yavaştı. Global helper yalnız dinamik
binding okuması olarak ayrıldıktan sonra bound globals VM-owned raw value aynasından
doğrudan yüklenir. Son koşuda JIT 1.909 ms, adaptive tier 2.428 ms oldu. Bu vaka
21.876 side exit/resume yapar ve artık bound-global helper çağrısı yapmaz; doğrudan
exact-callee native call gelecekteki ayrı optimizasyondur.

Native backedge'ler 1024 geçişte bir runtime poll yapar. Poll-only aşamasında
`sum_to_1000000` 976 poll ile 4.092 ms sürmüş, poll öncesi koşu 3.913 ms olmuştur.
Sonraki OSR aşaması ilk 63 backedge'i adaptive interpreter'da çalıştırır; son
ölçümde 835 interpreter instruction, 976 native poll ve 3.752 ms toplam süreyle
adaptive interpreter'a karşı 23,81× hız korunur. Binary generic yavaş dallarının
eklenmesi code size'ı 1.196'dan 1.284 byte'a çıkardı; integer loop throughput'u
ölçüm gürültüsü içinde kaldı.

Yeniden üretim:

```sh
cargo bench -p tonic-runtime --bench jit --locked --offline
```

Bu baseline sonrasında profile-backed exact-callee tamsayı leaf inlining
eklenmiştir. Yeni yolun side-exit kapalı/açık A/B sonucu ve güncel sınırları
[JIT_DIRECT_CALL_BASELINE.md](JIT_DIRECT_CALL_BASELINE.md) dosyasındadır.
Native float direct leaf için önce/sonra allocation, deopt ve throughput sonucu
[JIT_FLOAT_BASELINE.md](JIT_FLOAT_BASELINE.md) dosyasındadır.
