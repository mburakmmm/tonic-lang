# ADR 0037 — Gözlenen variadic parametrelerin JIT'te materialization'ı

## Durum

Kabul edildi — 11 Eylül 2026.

## Karar

Ordinary `CALL` ile çağrılan exact Tonic function'ın bytecode'u `*args` veya
`**kwargs` parametresini gerçekten okuyorsa direct leaf yolu artık site'ı reddetmez.
Sekiz kararlı exact-callee gözleminden sonra compile-time binder normal positional,
positional-only, keyword-only, default ve extra keyword kurallarını target slotlarına
uygular; variadic slotları açık materialization planlarıyla doldurur.

`VariadicTuple`, target'ın positional parametre sınırından sonraki contiguous caller
register penceresini `BuildTuple` helper'ına verir. `VariadicDict`, named target
slotlarına bağlanmayan her keyword için `(SymbolId, absolute value register)` çifti
taşır; `BuildDict` boş dict'i oluşturur ve `DictSetSymbol` öğeleri çağrı sırasıyla
ekler. Böylece fazla positional değerler keyword-only slotlara yanlışlıkla
bağlanmaz, bilinen keyword'ler kwargs dict'ine sızmaz ve insertion order korunur.

Target variadic register'larını okumuyorsa ADR 0033'teki allocation-free yol devam
eder; boş tuple/dict sırf imzada bulunduğu için oluşturulmaz. Target bunlardan en az
birini okuyorsa mevcut binder davranışıyla her iki tanımlı variadic nesne de
materialize edilir.

## Profil, hata ve GC sözleşmesi

Ordinary call profili expanded-call ile aynı bounded exact function kaydını kullanır,
ancak direct plan yalnız closure/cell/class-body içermeyen mevcut leaf alt kümesinde
üretilir. Callee identity guard'ı kaybolursa özgün `CALL` PC'sine atomik deopt olur.

Her tuple/dict sonucu guest register öneğinin ardındaki ayrı JIT-private precise root'a
yazılır. Allocation helper'ları önce bütün `root_count` dilimiyle safepoint yapar;
dict insertion helper'ları hem kısmen kurulmuş dict'i hem kaynak value register'larını
köklenmiş görür. Allocation/binder helper hatası Rust panic'i olmadan exact caller
PC'sinde guest hatasına döner. Runtime native dönüşte yalnız guest-visible
`register_count` önekini VM frame'ine kopyalar.

## Ölçüm

Her workload 100.000 çağrı, 3 warmup ve 15 ayrı release VM örneği kullanır:

| Vaka | Adaptive | Generic side-exit JIT | Materialized direct JIT | Generic oranı | Adaptive oranı | Code bytes |
|---|---:|---:|---:|---:|---:|---:|
| `collect(1,2) -> args` | 18,069 ms | 13,374 ms | 3,391 ms | 3,94× | 5,33× | 1.640 |
| `collect(x=1,y=2) -> kw` | 35,578 ms | 30,074 ms | 20,119 ms | 1,49× | 1,77× | 1.868 |

Her direct koşu 99.937 native logical call, sıfır side exit ve sıfır deopt üretir.
Kwargs yolu key string ve dict insertion maliyetini semantik gereği ödemeye devam
eder; buna rağmen frame/dispatch/binder geçişini kaldırmak ölçülebilir kazanç sağlar.

Integration testi fixed positional, extra positional, keyword-only, bilinen keyword
ve iki unknown keyword'ü birlikte bağlar. Tuple ve dict target'tan döndürülür;
5.000'er çağrı `gc_interval=1` altında 9.800'den fazla direct call ve 9.800'den
fazla JIT-triggered collection ile doğru sonucu, sıfır side exit/deopt'u korur.

## Sınırlar

Bu karar ordinary `CALL` içindir. `CALL_EXPANDED` üzerinden variadic target
materialization'ı, method receiver'lı observed variadic target, closure/cell target,
genel target bytecode ve escape/scalar replacement ayrı işlerdir. Materialized
tuple/dict semantik gereği heap allocation yapar; optimize edilmiş çağrı bunları
gözlenebilirken kaldırmaz.
