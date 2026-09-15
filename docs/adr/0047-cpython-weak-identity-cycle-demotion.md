# ADR 0047 — CPython weak identity ve proxy-cycle root demotion

## Durum

Kabul edildi — 14 Eylül 2026; genel `ForeignPyObject` grafik taraması ADR 0048 ile tamamlandı.

## Karar

Bridge, `PyTonicProxy` nesnelerini GIL altında erişilen runtime/execution-scoped
non-owning pointer cache'inde tutar. Cache bir CPython referansı taşımaz; `tp_dealloc`
nesne belleğini bırakmadan önce girdiyi siler. Böylece canlı aynı Tonic logical
değeri tekrar ihraç edildiğinde aynı proxy kimliği kullanılır, ölü proxy cache
tarafından yaşatılmaz. Cache erişimi GIL'e ek olarak Rust mutex ile serialized olur.

`python.proxy(value)` tarafından üretilen Tonic foreign wrapper, hedef için ayrı
non-rooting foreign-reference token'ı taşır. Proxy yalnız bu wrapper'ın CPython
referansıyla yaşıyorsa persistent kök bırakılır; wrapper trace callback'i hedefi
foreign edge olarak bildirir. Böylece

```text
Tonic closure -> Tonic container -> proxy wrapper -> Tonic closure
```

halkası dış root kalkınca Tonic collector tarafından bütünüyle toplanabilir.
`foreign_reference_borrow` callback/attribute yollarına call-scope local handle
sağlar.

Proxy CPython'a yeniden ihraç edilirken persistent handle tekrar oluşturulur.
Trace callback'i GIL altında `Py_REFCNT` ile wrapper dışı CPython referansı bulunduğu
sürece bu güçlü kökü korur. Dış referans bırakıldığında proxy deallocator deferred
release'i doğru runtime owner kuyruğuna yollar. Explicit `close_proxy` aynı yapıda
idempotent acil kırma mekanizması olarak kalır.

## Kanıt

Testler canlı identity cache hit'ini, son decref'te cache temizliğini, explicit close
olmadan proxy/closure/list döngüsünün toplanmasını ve proxy `sys` modülünde dış
referansla tutulurken hedefin yaşamasını doğrular. Dış attribute silindikten sonra
deferred persistent kök ve foreign token sıfıra iner. Parallel bridge test koşusu,
C header ve clippy kapıları da çalıştırılır.

100.000 canlı cached proxy repr geçişi 112,791 ms medyan ve saniyede 886.596 çağrı
ölçmüştür. Positional callback'in keyword'süz fast path'i 100.000 çağrıda iki guest
allocation sınırını korur.

## Sınır

Bu ADR doğrudan `PyTonicProxy` foreign wrapper'ını kapsar. Arbitrary
`ForeignPyObject` payload'ının transitif CPython nesne grafiği, borrowed trace edge,
promotion ve finalizer sırası [ADR 0048](0048-cpython-cross-collector-graph-tracing.md)
ile eklenmiştir. Bounded traversal sınırını aşan veya global interpreter altyapısına
giren graph'lar conservative retention kullanır.
