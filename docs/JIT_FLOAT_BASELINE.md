# Native float JIT baseline ve sonuç

Bu ölçüm 11 Eylül 2026'da macOS ARM64, Rust stable 1.86.0 ve release profiliyle
alındı. Source bir kez parse/compile/verify edilir; her mod 3 warmup ve 15 ayrı VM
örneği kullanır. Bu ara karar benchmarkıdır, nihai Python karşılaştırması değildir.

## Mevcut yollar

| İş yükü | Generic interpreter | Adaptive interpreter | JIT | Heap allocation | Helper | Deopt |
|---|---:|---:|---:|---:|---:|---:|
| Function içi `value += step`, 100.000 | 11,328 ms | 11,692 ms | 3,115 ms | 100.003 | 100.034 | 0 |
| Caller içi üç-op float leaf, 100.000 | 24,516 ms | 23,208 ms | 24,435 ms | 300.004 | 0 | 8 |

İlk vaka JIT loop dispatch'ini kaldırdığı için adaptive tier'dan 3,75× hızlıdır,
fakat her `+=` generic binary helper'a gider, boxed `Float` ayırır ve periyodik GC
poll dahil 100.034 helper çağrısı yapar.

İkinci vaka şu target'ı çağırır:

```python
def fused(a, b):
    x = a + b
    x = x * b
    return x - b
```

Exact-callee integer direct leaf planı float operand guard'ını sekiz kez kaybeder.
Site bounded biçimde de-specialize olur; 300.004 float allocation ve 2.800.007
interpreter instruction kalır. JIT medyanı adaptive tier'dan %5,3 yavaştır. Bu
sonuç generic helper'ı yeniden adlandırmanın veya yalnız daha erken JIT etmenin
yeterli olmayacağını gösterir.

## Uygulama kararı için hedef

İlk native float dilimi direct, side-effect-free leaf için type feedback kullanmalı:

1. Exact callee ve exact-float argument shape en az sekiz kararlı çağrıyla
   profillenmeli.
2. Generated caller her boxed float argümanı çağrı başında bir kez opaque helper ile
   unbox etmeli; miss, guest etkisi oluşmadan özgün `CALL` PC'sine deopt etmeli.
3. `+`, `-` ve `*` ara sonuçları Cranelift `f64` SSA değerleri olarak kalmalı.
4. Yalnız guest-visible dönüş `Float` olarak allocate edilip precise hidden root'a
   yazılmalı. Üç işlemli workload böylece çağrı başına üç allocation yerine bir
   allocation hedeflemeli.
5. Allocation helper exact roots ile safepoint yapmalı; NaN, infinity, signed zero
   ve IEEE-754 sonuç bitleri generic Tonic semantiğiyle differential test edilmelidir.

Bu ilk dilim caller çağrı sınırında sonucu box eder. Float accumulator'ı loop boyunca
tamamen unboxed tutmak; poll, side exit ve guard failure'da interpreter register
durumunu yeniden kurmak; live managed değerler için machine stack map üretmek bir
sonraki daha genel deopt/stack-map işidir.

Kabul kapısı: aynı A/B koşusunda direct-float mod sıfır guard deopt ile adaptive ve
mevcut generic fallback'ten hızlı olmalı, heap allocation sayısını 300.004'ten
yaklaşık 100.004'e indirmeli ve debug/release × normal/stress GC differential
matrisini geçmelidir. Kazanç çıkmazsa karmaşıklık korunmamalıdır.

## Uygulama sonrası sonuç

Exact-callee ve exact-float argüman profili sekiz gözlemden sonra direct planı
seçer. Generated caller her argümanı opaque runtime ABI ile bir kez unbox eder;
`+`, `-`, `*` ve `+=` ara değerleri Cranelift F64 SSA'da kalır. Yalnız dönüş değeri
`BoxFloat` safepoint'inde hidden precise root'a ayrılır. Type miss, target içinde
guest etkisi oluşmadan özgün `CALL` PC'sine döner. Float sabitleri moving heap
adresi olarak gömülmez; VM'in köklü logical handle'ı doğrulanmış `CONST` PC'siyle
derleyiciye verilir.

11 Eylül 2026 tarihli aynı makine ve 3 warmup + 15 VM medyanı:

| Mod | Önce | Sonra | Hızlanma | Heap allocation | Helper | Deopt | Direct call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Generic interpreter | 24,516 ms | 23,582 ms | — | 300.004 | 0 | 0 | 0 |
| Adaptive interpreter | 23,208 ms | 22,523 ms | — | 300.004 | 0 | 0 | 0 |
| Direct float JIT | 24,435 ms | 2,988 ms | 8,18× | 100.130 | 299.908 | 0 | 99.937 |

Son JIT yolu güncel ölçümde generic interpreter'dan 7,89×, adaptive
interpreter'dan 7,54× hızlıdır. 126 ek allocation, ilk 63 interpreted çağrının üç
ara sonuç üretmesidir; kalan 99.937 direct çağrı semantik olarak gerekli tek dönüş
kutusunu ayırır. İki unbox ve bir box helper'ı çağrı başına üç ABI geçişi üretir;
bu maliyet mevcut sonuçta baskın olmamış, kabul kapısı açık farkla geçilmiştir.

Stress-GC integration testleri current float result'ın hareket sonrasında
korunduğunu, sonradan gelen int argümanın atomik deopt ettiğini ve `inf`, `nan`,
`-0.0` çıktılarının interpreter ile aynı olduğunu doğrular. Genel bir döngü
accumulator'ını safepointler boyunca F64 tutan ikinci dilim aşağıda kaydedilmiştir.

## Loop-carried F64 ve deopt map sonucu

Bir sonraki dilim `accumulate(value, step, n)` gibi profilli float parametreli
numeric loop'ları kapsar. CFG dataflow her erişilebilir bytecode PC'sinde float
olduğu kanıtlanan sanal register'ları çıkarır. Bu register'ların değerleri native
stack slotlarında tutulur; managed Tonic handle'ları explicit precise root
tamponunda kalmaya devam ettiği için F64 slotlar GC root değildir.

Derlenmiş fonksiyon her PC için `DeoptMap { register_count,
unboxed_float_registers }` yayınlar. Arbitrary-PC OSR girişi seçilen PC'de canlı
olan F64 register'larını boxed VM tamponundan unbox eder. Backedge poll runtime'ı
deopt isterse generated kod map'teki bütün F64 slotları `BoxFloat` ile özgün guest
register'larına yazar ve exact loop hedef PC'sini döndürür. Unit test baştan giriş,
PC=2 resume, entry type miss ve 1.024'üncü backedge poll-deopt durumlarını ayrı ayrı
doğrular.

| `float_add_loop_100000` | Boxed JIT | Unboxed loop JIT | Değişim |
|---|---:|---:|---:|
| Medyan | 3,115 ms | 1,969 ms | 1,58× hızlı |
| Heap allocation | 100.003 | 67 | %99,93 azalma |
| Runtime helper | 100.034 | 199.975 | 2× kontrol-helper maliyeti |
| JIT code | 1.260 byte | 2.220 byte | +960 byte |
| Deopt | 0 | 0 | aynı |

Yeni yol güncel generic interpreter 10,912 ms'den 5,54×, adaptive interpreter
10,766 ms'den 5,47× hızlıdır. Integer loop counter ve karşılaştırma generic helper'a
alındığı için ABI çağrı sayısı artmıştır; allocation ve GC maliyetinin kalkması net
kazanç üretmiştir. Bu helper'ların yeniden exact-int fast path'e alınması ayrı,
benchmark-güdümlü bir optimization'dır.

```text
float_add_loop_100000,interpreter-generic,10912.167,10515.375,11366.209,1300023,100003,0.000,0,0,0,0,0,0,0,0,0,0,0,0,0
float_add_loop_100000,interpreter-adaptive,10765.750,10587.583,12134.250,1300023,100003,0.000,0,0,0,0,0,0,0,0,0,0,0,0,0
float_add_loop_100000,jit,1969.375,1939.834,2145.417,837,67,449.041,2220,1,1,0,0,0,199975,0,0,0,0,0,0
```

Uygulama sonrası ilgili satırlar:

```text
jit_caller_float_chain_100000,interpreter-generic,23582.042,23500.167,23743.791,2800023,300004,0.000,0,0,0,0,0,0,0,0,0,0,0,0,0
jit_caller_float_chain_100000,interpreter-adaptive,22522.667,22358.750,23118.459,2800023,300004,0.000,0,0,0,0,0,0,0,0,0,0,0,0,0
jit_caller_float_chain_100000,jit,2988.250,2939.916,3067.875,1782,100130,366.916,1956,1,1,0,0,0,299908,97,0,0,1,0,99937
```

Yeniden üretim:

```sh
cargo bench -p tonic-runtime --bench jit --locked --offline
```

İlgili ham satırlar:

```text
float_add_loop_100000,interpreter-generic,11327.875,11009.666,11886.625,1300023,100003,0.000,0,0,0,0,0,0,0,0,0,0,0,0,0
float_add_loop_100000,interpreter-adaptive,11691.875,11094.417,13781.334,1300023,100003,0.000,0,0,0,0,0,0,0,0,0,0,0,0,0
float_add_loop_100000,jit,3115.000,3004.042,3456.667,837,100003,311.916,1260,1,1,0,0,0,100034,97,0,0,0,0,0
jit_caller_float_chain_100000,interpreter-generic,24515.875,23973.208,25119.083,2800023,300004,0.000,0,0,0,0,0,0,0,0,0,0,0,0,0
jit_caller_float_chain_100000,interpreter-adaptive,23207.875,22714.625,23622.875,2800023,300004,0.000,0,0,0,0,0,0,0,0,0,0,0,0,0
jit_caller_float_chain_100000,jit,24434.583,23769.583,24819.000,2800007,300004,202.917,480,8,0,8,1,1,0,0,0,0,0,0,0
```
