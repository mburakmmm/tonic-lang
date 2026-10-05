# ADR 0101 — Annotation destekli kısmi statik JIT

## Durum

Kabul edildi; uygulama yol haritasına alındı.

## Bağlam

Tonic bugün Python annotation ifadelerini değerlendirip managed
`__annotations__` metadata'sı olarak saklar. Mevcut Cranelift tier'ı ise türleri
runtime profillerinden öğrenir. Annotation bulunan sayısal veya veri-yoğun kodun
aynı profilleme süresini ödemesi gereksizdir; öte yandan Python annotation'larını
zorunlu runtime type check saymak Python davranışını bozar. Standart Python
`int` ayrıca sabit genişlikli C integer'ı değildir.

## Karar

Çözümlenebilen annotation'lar advisory kipte typed-JIT planını önceden besler.
JIT exact runtime type/shape/version guard'ları üretir ve varsayım tutmadığında
generic interpreter'a exact bytecode PC'sinde deopt eder. Annotation yanlışlığı
tek başına `TypeError` değildir. Function/code identity, annotation dictionary,
global/module/class bağımlılıkları ve container layout varsayımları versioned
guard veya invalidation taşır.

Typed bilgi verified bytecode üzerinde ayrı Tonic-owned data-flow overlay'idir;
AST veya runtime object layout'una gömülmez. Unboxed integer/float/bool değerler,
typed calls, containers, buffers ve sabit şekilli annotated class fields aşamalı
olarak eklenir. Python `int` overflow'u bigint semantiğine deopt/materialization
ile döner.

Sabit genişlik, packed layout ve C-benzeri overflow ancak açık Tonic strict değer
türleriyle seçilir. Strict türler normal Python grammar'ındaki annotation ve
decorator/module policy üzerinden kullanılır; standart annotation'ların anlamını
değiştirmez.

## Sonuçlar

Annotation'lı Python kodu değişmeden daha erken typed native tier'a girebilir.
Dinamik davranış, monkey patching ve annotation mutation generic fallback ile
korunur. Ek maliyet TypePlan çözümleme, dependency/version takibi, code cache ve
deopt metadata'sıdır; desteklenmeyen annotation yalnız optimizasyonu reddeder.
Disk cache raw Rust layout'u veya heap adresi saklayamaz.

Bu karar ADR 0079'un annotation'ların mevcut call ABI'sini değiştirmediği
tespitini korur; yeni typed entry ayrı, guard'lı bir ABI katmanıdır. Ayrıntılı
teslim ve kabul matrisi `docs/ANNOTATION_JIT_PLAN.md` içindedir. Benchmark ve
bottleneck analizi, bütün roadmap ve Python coverage kapıları kapandıktan sonra
CPython/Cython karşılaştırmasıyla yapılır.
