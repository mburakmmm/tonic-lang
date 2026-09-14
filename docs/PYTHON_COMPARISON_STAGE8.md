# Tonic–CPython ara performans karşılaştırması

4 Eylül 2026. **Nihai dil benchmarkı değildir.** Bu ölçüm yalnız her iki motorun
da bugün çalıştırabildiği 13 ortak Python-syntax programını kapsar. Tonic adaptive
tier veya JIT içermez. CPython 3.14.6 kurulumu da JIT'sizdir (`is_available=false`,
`is_enabled=false`). Makine macOS 26.6 ARM64, Rust stable 1.86.0; Tonic release
thin LTO ve tek codegen unit ile derlenmiştir.

Beş bağımsız benchmark süreci çalıştırıldı. Her süreçte motor/iş yükü başına beş
warmup ve 30 warm örnek, üç cold warmup ve 15 cold örnek alındı. Birleştirilmiş
dağılımda her warm hücre 150, her cold hücre 75 örnektir. Bütün programlar aynı
`.py` kaynak dosyasını kullanır ve her timed execution checksum çıktısıyla
doğrulanır.

Warm Tonic ölçümünde kaynak önceden compile/verify edilir; fresh `Vm` timer dışında
kurulur, `Vm::run` ve fresh module state timer içindedir. CPython kaynak kodu bir
kez `compile()` edilir; `exec` fresh globals ile ölçülür. Cold CLI her örnekte yeni
`tonic file.py` veya `python3 file.py` sürecini, parse/compile/run/startup dahil
ölçer. Oran `Tonic / CPython` medianıdır: 1'in üstü Tonic'in yavaş olduğunu gösterir.

## Warm execution

| İş yükü | Tonic median µs | CPython median µs | Tonic p95 µs | CPython p95 µs | Oran |
|---|---:|---:|---:|---:|---:|
| integer_loop | 4.863,271 | 4.898,916 | 5.074,208 | 5.192,417 | **0,99×** |
| fib_calls | 2.410,812 | 635,125 | 2.446,375 | 648,959 | **3,80×** |
| known_calls | 1.145,854 | 667,104 | 1.192,333 | 685,042 | **1,72×** |
| float_loop | 689,042 | 418,896 | 744,333 | 428,542 | **1,64×** |
| list_iteration | 275,104 | 206,125 | 287,833 | 219,625 | **1,33×** |
| closure_calls | 1.076,917 | 579,770 | 1.106,500 | 604,750 | **1,86×** |
| keyword_calls | 1.398,583 | 824,000 | 1.422,375 | 856,166 | **1,70×** |
| dict_lookup | 952,980 | 536,396 | 989,500 | 578,250 | **1,78×** |
| dict_insert | 1.075,896 | 542,251 | 1.105,250 | 584,500 | **1,98×** |
| attribute_load | 848,333 | 537,083 | 893,500 | 570,500 | **1,58×** |
| bound_method_calls | 1.700,792 | 693,208 | 1.792,166 | 754,959 | **2,45×** |
| descriptor_load | 2.121,958 | 819,771 | 2.193,875 | 840,583 | **2,59×** |
| super_calls | 3.425,208 | 1.019,333 | 3.611,083 | 1.060,000 | **3,36×** |

Integer döngüsü ölçüm gürültüsü içinde başa baştır. Fonksiyon/frame maliyeti
`fib_calls` sonucunda görünür; method, descriptor ve `super` yolları generic MRO
arama ve normal frame çağrısı nedeniyle daha pahalıdır. Float sonuçları boxed,
dict/list yolları henüz specialize değildir. Bu bulgular M4 için önceliği known
Tonic call/frame reuse, shape/attribute cache, bound-method call fast path ve
descriptor/super monomorphic cache sırasına verir.

## Cold CLI

| İş yükü | Tonic median ms | CPython median ms | CPython / Tonic |
|---|---:|---:|---:|
| integer_loop | 7,287 | 19,633 | **2,69×** |
| fib_calls | 4,533 | 15,268 | **3,37×** |
| known_calls | 3,237 | 15,432 | **4,77×** |
| float_loop | 2,788 | 15,038 | **5,39×** |
| list_iteration | 2,400 | 14,874 | **6,20×** |
| closure_calls | 3,102 | 15,074 | **4,86×** |
| keyword_calls | 3,495 | 15,372 | **4,40×** |
| dict_lookup | 3,098 | 15,190 | **4,90×** |
| dict_insert | 3,243 | 15,217 | **4,69×** |
| attribute_load | 2,954 | 15,168 | **5,14×** |
| bound_method_calls | 3,843 | 15,276 | **3,97×** |
| descriptor_load | 4,086 | 15,314 | **3,75×** |
| super_calls | 5,378 | 15,705 | **2,92×** |

Tonic bütün cold programlarda daha hızlıdır. Bu sonuç çoğunlukla küçük Rust CLI'nin
başlangıç maliyetini gösterir; warm dil yürütme üstünlüğü diye yorumlanamaz.
Compile medianları iş yüküne göre Tonic'te 12–30 µs, CPython'da 11–39 µs aralığındadır;
iki compiler farklı artifact ve metadata ürettiği için tek hız skoru çıkarılmamıştır.

## Artifacts ve sınırlamalar

- [39 satır aggregate özet](benchmarks/python-comparison-stage8.csv)
- [9.750 ham örnek](benchmarks/python-comparison-stage8-samples.csv)
- [Ortam, binary/source hash ve yöntem manifesti](benchmarks/python-comparison-stage8-process.json)
- [Warm/cold oran grafiği](benchmarks/python-comparison-stage8.png)
- Tekrar üretim araçları: `benches/compare_python.py`,
  `benches/aggregate_comparison.py`, `python_compare` Rust bench'i ve
  `benches/comparison/*.py` ortak kaynakları.

Masaüstü yükü, CPU frekansı ve affinity kontrol edilmedi. Hardware counter,
iş yükü başına RSS, host allocator count/bytes ve enerji ölçülmedi. Karşılaştırma
exception, generator/async, import ağı, stdlib, generational GC, adaptive
interpreter, Cranelift JIT veya interop/bridge performansını kapsamaz. Bu nedenle
sonuçlar Tonic'in genel Python performansı veya tamamlanmış dil hızı değildir;
nihai kapsam [FINAL_BENCHMARK_PLAN.md](FINAL_BENCHMARK_PLAN.md) içindedir.
