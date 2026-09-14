# JIT direct function leaf-call A/B baseline

Bu ölçüm serisi 10–11 Eylül 2026'da macOS ARM64, Rust stable 1.86.0 ve release profiliyle
alındı. Tam dil/Python benchmarkı değildir; yalnız `CALL` specialization kararını
ölçer. İlk ölçüm exact positional çağrıyı, ikinci ölçüm positional-only,
keyword-only ve definition-time default bağlama planını, üçüncü ölçüm plain
bound-instance method lookup/call fusion'ını, dördüncü ölçüm instance üzerinden
staticmethod fusion'ını, beşinci ölçüm inherited classmethod fusion'ını, altıncı
ölçüm custom descriptor sonucu dönen exact leaf çağrısını, yedinci ölçüm generic
expanded-call segment resume'u ile guarded positional sequence yükseltmesini,
sekizinci ölçüm named/default expansion'ı, dokuzuncu ölçüm exact-dict `**mapping`
yükseltmesini, onuncu ölçüm gözlenmeyen boş variadic parametreli direct leaf,
on birinci ve on ikinci ölçümler gözlenen `*args` ve `**kwargs` materialization
çağrılarını izole eder.

## İş yükü

```python
def add(a, b):
    return a + b

def loop(n):
    i = 0
    total = 0
    while i < n:
        total = add(total, 1)
        i += 1
    return total

print(loop(100000))
```

Bağlama planı genişletmesi için aynı iterasyon sayısıyla şu ikinci iş yükü vardır:

```python
def add(a, /, b=1, *, bias=0):
    return a + b + bias

def loop(n):
    i = 0
    total = 0
    while i < n:
        total = add(total, bias=0)
        i += 1
    return total

print(loop(100000))
```

`jit_direct_call` harness'i source'u bir kez parse/compile/verify eder. Her mod 3
warmup ve 15 yeni VM örneğiyle çalışır; tabloda medyan ve gözlenen min/max verilir.
`jit-side-exit` aynı binary ve JIT'i kullanır, yalnız
`Vm::jit_direct_call_inlining=false` ayarıyla yeni yolu kapatır.

## Sonuç

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes |
|---|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 13,368 ms | 13,149 ms | 15,757 ms | 2.000.023 | 0 | 0 |
| JIT side exit | 8,019 ms | 7,797 ms | 8,259 ms | 401.026 | 99.937 / 99.937 | 1.356 |
| JIT direct leaf | 0,760 ms | 0,753 ms | 0,793 ms | 1.278 | 0 / 0 | 1.780 |

Direct mod 99.937 başarılı inline call raporlar. İlk 63 loop iterasyonu OSR profili
için adaptive tier'da çalışır; kalan çağrılar caller native gövdesinde tamamlanır.
Call counter için generated kodun yaptığı tek `u64` increment ölçüme dahildir.
Direct mod side-exit JIT'ten 10,55×, adaptive interpreter'dan 17,58× hızlıdır.
Code size 424 byte artar; bu workload için dispatch/frame/bridge kazancı bu maliyeti
açık biçimde aşar.

### Keyword/default bağlama genişletmesi

Değişiklik öncesinde `jit-direct-call` bu çağrıyı uzmanlaştıramıyor ve
`jit-side-exit` ile aynı 99.937 side exit/resume'u üretiyordu:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 19,662 ms | 18,678 ms | 37,233 ms | 2.200.025 | 0 | 0 / 0 |
| JIT side exit | 12,462 ms | 11,658 ms | 13,534 ms | 101.063 | 99.937 / 99.937 | 0 / 0 |
| Eski direct seçeneği | 12,405 ms | 11,817 ms | 13,032 ms | 101.063 | 99.937 / 99.937 | 0 / 0 |

Önceden doğrulanmış target-slot planı eklendikten sonraki ölçüm:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 14,850 ms | 14,713 ms | 15,306 ms | 2.200.025 | 0 | 0 | 0 / 0 |
| JIT side exit | 10,061 ms | 9,419 ms | 10,559 ms | 101.063 | 99.937 / 99.937 | 1.660 | 0 / 0 |
| JIT binding plan | 1,120 ms | 1,014 ms | 1,163 ms | 1.126 | 0 | 2.284 | 1 / 99.937 |

Binding plan aynı son koşudaki side-exit JIT'ten yaklaşık 8,99× hızlıdır.
Adaptive interpreter'ın aynı düzeni quickening sonrasında doğrudan frame'e
bağlaması da generic binder maliyetini düşürür; iki zaman aralığı farklı koşulardan
geldiği için bu değişim ayrı bir kesin hızlanma iddiası olarak kullanılmaz.

### Plain bound-method fusion

Üçüncü iş yükü `Counter.add(self,a,/,b=1)` metodunu 100.000 kez
`counter.add(total,b=1)` biçiminde çağırır. Değişiklik öncesinde `ATTR` desteklenmediği
için iki JIT seçeneği de native code üretmiyordu:

| Mod | Medyan | Min | Max | Interpreter instruction | Code bytes |
|---|---:|---:|---:|---:|---:|
| Adaptive interpreter | 23,416 ms | 22,853 ms | 23,719 ms | 2.100.032 | 0 |
| JIT side exit seçeneği | 23,860 ms | 23,644 ms | 24,612 ms | 2.100.032 | 0 |
| Eski direct seçeneği | 25,030 ms | 23,998 ms | 27,019 ms | 2.100.032 | 0 |

Fusion sonrasında direct mod 99.937 lookup/call çiftini native yolda tamamlar:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Method site/direct call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 23,027 ms | 22,957 ms | 23,409 ms | 2.100.032 | 0 | 0 | 0 / 0 |
| JIT, fusion kapalı | 23,793 ms | 23,480 ms | 26,415 ms | 2.100.032 | 0 | 0 | 0 / 0 |
| JIT method fusion | 2,024 ms | 2,010 ms | 2,063 ms | 1.350 | 0 | 1.604 | 1 / 99.937 |

Method fusion adaptive kontrole göre yaklaşık 11,38× hızlıdır. `self` identity
testinde 5.000 çağrının sonucu generic yolla aynıdır ve bound-method allocation
sayısı en az 4.800 azalır. Class rebinding ile instance shadowing exact lookup
guard'ını düşürüp özgün `ATTR` PC'sinde generic semantiğe döner.

### Staticmethod fusion

`Math.add(a,/,b=1)` staticmethod'u instance üzerinden 100.000 kez çağrılır.
Değişiklik öncesi direct seçenek `ATTR` nedeniyle code üretmemiştir:

| Mod | Medyan | Min | Max | Interpreter instruction | Code bytes |
|---|---:|---:|---:|---:|---:|
| Adaptive interpreter | 27,471 ms | 25,243 ms | 28,694 ms | 2.100.035 | 0 |
| JIT, fusion kapalı | 28,807 ms | 25,340 ms | 72,799 ms | 2.100.035 | 0 |
| Eski direct seçeneği | 30,207 ms | 29,084 ms | 66,068 ms | 2.100.035 | 0 |

Fusion sonrası aynı son koşudaki A/B sonucu:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Method site/direct call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 20,044 ms | 19,442 ms | 21,870 ms | 2.100.035 | 0 | 0 | 0 / 0 |
| JIT, fusion kapalı | 20,741 ms | 19,641 ms | 23,023 ms | 2.100.035 | 0 | 0 | 0 / 0 |
| JIT staticmethod fusion | 2,379 ms | 2,277 ms | 2,516 ms | 1.353 | 0 | 1.600 | 1 / 99.937 |

Staticmethod fusion son koşudaki adaptive tier'dan yaklaşık 8,43× hızlıdır.
Binding-kind guard testi wrapper aynı underlying function ile plain metoda
çevrildiğinde generic `TypeError` sonucunu korur.

### Classmethod ve class-level erişim

`Base.identity(cls)` classmethod'u `Sub()` instance'ı üzerinden 100.000 kez
çağrılır; doğru implicit receiver her çağrıda dinamik `Sub` class nesnesidir.
Değişiklik öncesinde `ATTR` desteklenmediği için hiçbir JIT modu code üretmemiştir:

| Mod | Medyan | Min | Max | Interpreter instruction | Code bytes |
|---|---:|---:|---:|---:|---:|
| Adaptive interpreter | 29,079 ms | 28,773 ms | 34,311 ms | 2.100.035 | 0 |
| JIT, fusion kapalı | 28,391 ms | 27,895 ms | 31,217 ms | 2.100.035 | 0 |
| Eski direct seçeneği | 31,557 ms | 30,689 ms | 34,054 ms | 2.100.035 | 0 |

Fusion sonrası aynı son koşudaki A/B sonucu:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Method site/direct call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 36,416 ms | 34,869 ms | 39,624 ms | 2.100.035 | 0 | 0 | 0 / 0 |
| JIT, fusion kapalı | 40,805 ms | 37,240 ms | 42,009 ms | 2.100.035 | 0 | 0 | 0 / 0 |
| JIT classmethod fusion | 3,828 ms | 3,630 ms | 4,316 ms | 1.353 | 0 | 1.716 | 1 / 99.937 |

Classmethod fusion aynı koşudaki adaptive tier'dan yaklaşık 9,51× hızlıdır.
Helper exact function ile gerçek receiver'ı ayrı raw değerler olarak döndürür;
kalıtılmış erişim compile-time profildeki base class'a sabitlenmez. Instance ve
class üzerinden inherited receiver, class-level plain function ve aynı function'ın
classmethod wrapper'dan plain metoda çevrilmesi stress/rebinding testlerindedir.
Mutlak süreler farklı koşulardaki host yükünden etkilendiği için karar aynı koşu
içindeki oran ve side-exit/code sayaçlarına dayanır.

### Custom descriptor resume ve returned leaf

`Forward.__get__` global `add` function'ını döndürür; loop 100.000 kez
`math.op(total,b=1)` çağırır. Değişiklik öncesinde `ATTR` yüzünden üç mod da
native code üretmiyor; doğrudan seçenek adaptive ile aynı 2.300.042 instruction'ı
çalıştırıyordu. Son kârlılık kapılı A/B sonucu:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 51,424 ms | 46,146 ms | 54,111 ms | 2.300.042 | 0 | 0 | 0 / 0 |
| JIT, direct call kapalı | 54,859 ms | 49,804 ms | 59,733 ms | 2.300.042 | 0 | 0 | 0 / 0 |
| JIT descriptor resume + leaf | 41,565 ms | 39,202 ms | 44,896 ms | 301.297 | 99.937 / 99.937 | 1.408 | 1 / 99.937 |

JIT yolu adaptive kontrole göre yaklaşık 1,24× hızlıdır; süre %19,2 azalır.
Descriptor getter generic VM frame'inde çalışır ve native caller `CALL` öncesinde
resume edilir. Direct-call kapalı kontrolde code üretilmemesi, iki generic side
exit ödeyecek kârsız biçimin compile kapısından geçmediğini doğrular.

### Expanded-call segmenti

Loop 100.000 kez `add(total,*values)` çağırır. Değişiklik öncesinde argument-builder
opcodları unsupported olduğu için code üretilmemiştir:

| Mod | Medyan | Min | Max | Interpreter instruction | Code bytes |
|---|---:|---:|---:|---:|---:|
| Adaptive interpreter | 28,760 ms | 27,788 ms | 30,074 ms | 2.100.027 | 0 |
| JIT, unsupported | 31,224 ms | 28,564 ms | 33,250 ms | 2.100.027 | 0 |

Resumable segment sonrası aynı koşudaki A/B sonucu:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes |
|---|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 24,762 ms | 23,792 ms | 26,811 ms | 2.100.027 | 0 | 0 |
| JIT expanded segment | 18,747 ms | 18,240 ms | 19,184 ms | 1.000.715 | 99.937 / 99.937 | 1.392 |

Segment resume adaptive kontrole göre yaklaşık 1,17× hızlıdır.
Nested `*`/`**` builder testi yalnız eşleşen dış argument-stack derinliğinde resume
edildiğini ve pending değerlerin stress GC altında köklendiğini doğrular. Expanded
callee binding için eklenen düz positional sequence specialization sonucu:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 21,524 ms | 21,204 ms | 21,950 ms | 2.100.027 | 0 | 0 | 0 / 0 |
| JIT generic segment | 18,747 ms | 18,240 ms | 19,184 ms | 1.000.715 | 99.937 / 99.937 | 1.392 | 0 / 0 |
| JIT guarded expansion | 1,080 ms | 1,030 ms | 1,229 ms | 1.345 | 0 | 1.908 | 1 / 99.937 |

Guarded expansion generic segment JIT'ten 17,37×, adaptive tier'dan 19,94×
hızlıdır. Exact built-in list/tuple uzunluğu guard edilir; öğeler her çağrıda
güncel okunur. Named/default slot binding de aynı yola eklenmiştir:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 29,728 ms | 29,243 ms | 30,853 ms | 2.500.029 | 0 | 0 | 0 / 0 |
| JIT generic segment | 26,342 ms | 25,890 ms | 27,691 ms | 900.752 | 99.937 / 99.937 | 1.752 | 0 / 0 |
| JIT guarded named expansion | 1,236 ms | 1,199 ms | 1,470 ms | 1.319 | 0 | 2.368 | 1 / 99.937 |

Named/default direct yol generic segmentten 21,31×, adaptive tier'dan 24,05×
hızlıdır. Nested builder ve observed variadic target generic yolda kalır.

### Exact-dict `**mapping` expansion

`add(total, **mapping)` çağrısında `mapping={'b':1,'bias':0}` iki string key ile
100.000 kez genişletilir. Kararlı profile sonrasında her key current dict'ten
okunur ve ayrı kesin JIT argument root'una yazılır:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 33,476 ms | 33,133 ms | 34,091 ms | 2.300.033 | 0 | 0 | 0 / 0 |
| JIT generic segment | 30,284 ms | 29,494 ms | 30,905 ms | 700.756 | 99.937 / 99.937 | 1.696 | 0 / 0 |
| JIT guarded mapping | 2,761 ms | 2,699 ms | 2,999 ms | 1.197 | 0 | 2.524 | 1 / 99.937 |

Guarded mapping generic segment JIT'ten 10,97×, adaptive tier'dan 12,13× hızlıdır.
Value mutation deopt olmadan güncel sonucu verir. Exact dict türü, key sayısı veya
key presence guard'ı kaybolursa site `BEGIN_ARGS` PC'sine deopt olur. Boş dict ve
genel mapping protokolü bu aşamada generic yolda kalır.

### Gözlenmeyen boş variadic parametreler

`add(a,b,*rest,**kw): return a+b` 100.000 kez iki positional argümanla çağrılır.
Değişiklik öncesinde direct seçenek hedefi reddedip side-exit yoluyla aynı
401.026 interpreter instruction'ı çalıştırıyordu:

| Mod | Medyan | Min | Max | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 21,343 ms | 20,589 ms | 22,964 ms | 0 | 0 | 0 / 0 |
| JIT side exit | 14,950 ms | 14,493 ms | 15,223 ms | 99.937 / 99.937 | 1.356 | 0 / 0 |
| Eski direct seçeneği | 14,395 ms | 14,142 ms | 14,968 ms | 99.937 / 99.937 | 1.356 | 0 / 0 |

Operand taraması eklendikten sonraki aynı koşu:

| Mod | Medyan | Min | Max | Interpreter instruction | Side exit/resume | Code bytes | Direct site/call |
|---|---:|---:|---:|---:|---:|---:|---:|
| Adaptive interpreter | 16,171 ms | 15,308 ms | 17,433 ms | 2.000.023 | 0 | 0 | 0 / 0 |
| JIT side exit | 9,510 ms | 9,212 ms | 10,113 ms | 401.026 | 99.937 / 99.937 | 1.356 | 0 / 0 |
| JIT direct empty variadic | 0,821 ms | 0,782 ms | 1,019 ms | 1.278 | 0 | 1.780 | 1 / 99.937 |

Direct yol aynı koşudaki side-exit JIT'ten 11,58×, adaptive tier'dan 19,70×
hızlıdır. Bu karar sırasında `rest` veya `kw` register'ını okuyan hedefler generic
kalmaktaydı; aşağıdaki materialization genişletmesi artık bu vakayı kapsar.

### Gözlenen materialized variadic parametreler

`collect(*args): return args` ve `collect(**kw): return kw` workload'ları gerekli
tuple/dict allocation'ını korurken frame, dispatch ve generic binder geçişini
kaldırır:

| Vaka | Adaptive | Generic side-exit JIT | Materialized direct JIT | Generic oranı | Adaptive oranı | Code bytes |
|---|---:|---:|---:|---:|---:|---:|
| `collect(1,2)` | 18,069 ms | 13,374 ms | 3,391 ms | 3,94× | 5,33× | 1.640 |
| `collect(x=1,y=2)` | 35,578 ms | 30,074 ms | 20,119 ms | 1,49× | 1,77× | 1.868 |

İki direct koşu da 99.937 logical call, sıfır side exit/deopt üretir. Tuple tek
allocation helper'ında, kwargs boş dict ve her key için ayrı helper'da oluşturulur;
bütün ara nesneler guest register öneğinden sonraki kesin JIT roots içindedir.

### Invocation-local method dependency cache

İlk method fusion sürümü opaque lookup helper'ını her loop iterasyonunda çağırır.
Lazy entry cache helper'ı ilk gerçek `ATTR` yürütmesine taşır; kalan iterasyonlar
exact owner ve function guard'larıyla ilerler. Son aynı-koşu sonuçları:

| Vaka | Adaptive | Önceki direct | Entry-cache direct | Adaptive oranı | Code bytes |
|---|---:|---:|---:|---:|---:|
| Bound method | 25,090 ms | 2,120 ms | 0,926 ms | 27,11× | 1.812 |
| Staticmethod | 21,526 ms | 2,357 ms | 0,868 ms | 24,81× | 1.804 |
| Classmethod | 25,469 ms | 2,384 ms | 0,853 ms | 29,86× | 1.812 |

Önceki direct değerleri bir önceki karar koşusundandır; host yükü nedeniyle asıl
kanıt helper lookup sayısının 99.937'den bire inmesi ve son koşudaki adaptive
kontrollerdir. Owner loop içinde değişirse cache guard'ı `ATTR` PC'sine deopt eder;
class rebinding sonraki invocation'ın lazy lookup'unda görülür.

## Doğruluk kapıları

- Cranelift unit testi exact-callee başarı yolunu ve farklı callee'de exact caller
  `CALL` PC deopt'unu doğrular.
- Runtime testi 5.000 çağrıyı `gc_interval=1` ile çalıştırır; moving logical handle
  guard'ı ve backedge safepoint'leri birlikte sınanır.
- Ayrı test global rebinding sonrasında yeni callee sonucunu ve float operandlarda
  generic `+` semantiğini doğrular.
- Keyword register offset'i ve exact function default'u ayrı JIT unit testinde;
  5.000 çağrı, stress GC ve sayaçlar runtime integration testinde doğrulanır.
- Plain method lowering testi receiver binding'i ve farklı lookup sonucunda exact
  `ATTR` PC deopt'unu; integration testleri rebinding/shadowing ve tahsis farkını kapsar.
- Staticmethod stress testi no-receiver slot planını, ayrı rebinding testi binding
  türünün exact function identity'den bağımsız guard edildiğini doğrular.
- Classmethod testleri dinamik subclass receiver'ı, class-level function/method
  erişimini, binding-kind rebinding'ini ve her-allocation stress GC'yi doğrular.
- Debug/release toplam 184 test ve debug/release × interpreter/JIT × normal/stress
  GC differential matrisinin sekiz koşusu geçer.

Yeniden üretim:

```sh
cargo bench -p tonic-runtime --bench jit_direct_call --locked --offline
```

Ham çıktı:

```text
case,mode,median_us,min_us,max_us,instructions,jit_compile_us,jit_code_bytes,jit_side_exits,jit_resumes,jit_direct_call_sites,jit_direct_method_sites,jit_direct_calls
positional_exact,interpreter-adaptive,13710.625,13444.792,15230.667,2000023,0.000,0,0,0,0,0,0
positional_exact,jit-side-exit,8771.166,8511.875,9008.000,401026,320.458,1356,99937,99937,0,0,0
positional_exact,jit-direct-call,782.125,768.208,1657.542,1278,332.000,1780,0,0,1,0,99937
keyword_defaults,interpreter-adaptive,15332.167,14846.375,15721.166,2200025,0.000,0,0,0,0,0,0
keyword_defaults,jit-side-exit,10081.250,9859.167,10522.334,101063,429.875,1660,99937,99937,0,0,0
keyword_defaults,jit-direct-call,1117.542,1094.333,1212.500,1126,427.917,2284,0,0,1,0,99937
bound_method,interpreter-adaptive,22841.375,22621.125,23063.000,2100032,0.000,0,0,0,0,0,0
bound_method,jit-side-exit,23447.750,22864.500,23850.292,2100032,0.000,0,0,0,0,0,0
bound_method,jit-direct-call,815.708,804.792,872.791,1350,329.084,1812,0,0,1,1,99937
staticmethod,interpreter-adaptive,19254.625,19099.458,19522.041,2100035,0.000,0,0,0,0,0,0
staticmethod,jit-side-exit,19611.333,19157.458,19942.333,2100035,0.000,0,0,0,0,0,0
staticmethod,jit-direct-call,857.458,822.667,1040.667,1353,354.208,1804,0,0,1,1,99937
classmethod,interpreter-adaptive,22821.750,22636.250,23342.958,2100035,0.000,0,0,0,0,0,0
classmethod,jit-side-exit,23101.250,22780.041,23364.000,2100035,0.000,0,0,0,0,0,0
classmethod,jit-direct-call,815.584,800.875,842.792,1353,324.125,1812,0,0,1,1,99937
custom_descriptor,interpreter-adaptive,27114.917,26564.208,27980.375,2300042,0.000,0,0,0,0,0,0
custom_descriptor,jit-side-exit,28215.167,27669.125,28527.875,2300042,0.000,0,0,0,0,0,0
custom_descriptor,jit-direct-call,20941.916,20630.750,21778.125,301297,484.584,1408,99937,99937,1,0,99937
expanded_call,interpreter-adaptive,21915.542,21329.125,22423.083,2100027,0.000,0,0,0,0,0,0
expanded_call,jit-side-exit,19233.291,18487.542,19635.333,1000715,388.084,1392,99937,99937,0,0,0
expanded_call,jit-direct-call,1075.166,1047.917,1128.417,1345,331.917,1916,0,0,1,0,99937
expanded_named,interpreter-adaptive,28363.375,27909.542,29731.375,2500029,0.000,0,0,0,0,0,0
expanded_named,jit-side-exit,26980.625,26087.250,27857.000,900752,418.333,1752,99937,99937,0,0,0
expanded_named,jit-direct-call,1264.542,1242.959,1442.709,1319,460.917,2408,0,0,1,0,99937
expanded_mapping,interpreter-adaptive,33476.292,33133.292,34091.125,2300033,0.000,0,0,0,0,0,0
expanded_mapping,jit-side-exit,30283.875,29493.750,30905.209,700756,632.959,1696,99937,99937,0,0,0
expanded_mapping,jit-direct-call,2760.625,2698.541,2998.791,1197,470.541,2524,0,0,1,0,99937
unused_variadic,interpreter-adaptive,13800.833,13492.792,14108.959,2000023,0.000,0,0,0,0,0,0
unused_variadic,jit-side-exit,9348.916,8926.250,9694.959,401026,301.000,1356,99937,99937,0,0,0
unused_variadic,jit-direct-call,768.166,754.542,936.708,1278,320.209,1780,0,0,1,0,99937
```

Observed variadic genişletmesinin aynı harness koşusundaki ham satırları:

```text
observed_varargs,interpreter-adaptive,18068.958,17869.500,18251.125,1800023,0.000,0,0,0,0,0,0
observed_varargs,jit-side-exit,13374.125,13102.542,13544.792,201026,269.583,1356,99937,99937,0,0,0
observed_varargs,jit-direct-call,3390.791,3357.417,3476.000,1152,300.792,1640,0,0,1,0,99937
observed_kwargs,interpreter-adaptive,35577.500,35412.625,35763.500,1800023,0.000,0,0,0,0,0,0
observed_kwargs,jit-side-exit,30074.375,29761.333,30269.791,201026,310.500,1356,99937,99937,0,0,0
observed_kwargs,jit-direct-call,20118.958,19935.416,20336.583,1152,324.875,1868,0,0,1,0,99937
```
