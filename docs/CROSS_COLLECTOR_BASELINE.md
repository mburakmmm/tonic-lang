# CPython cross-collector GC baseline

15 Eylül 2026; Apple Silicon arm64, macOS 26.6, Rust 1.86.0, CPython 3.14.6,
release LTO. Benchmark erişilebilir küçük bir
`SimpleNamespace -> PyTonicProxy -> Tonic Box -> ForeignPyObject` grafiği üzerinde
10.000 explicit major collection çalıştırır. Parse, compile ve graph kurulumu süre
dışındadır; üç warmup sonrası 15 VM örneğinin medyanı raporlanır.

Komut:

```sh
cargo bench -p tonic-cpython --bench cross_runtime_gc --locked --offline
```

| Uygulama | Medyan | Min–maks | Collection/s | Collection başına |
|---|---:|---:|---:|---:|
| Trace olmayan önceki baseline | 6,882 ms | 6,748–7,564 ms | 1.453.022 | 0,688 µs |
| İlk transitif graph scanner | 47,976 ms | 47,545–48,344 ms | 208.438 | 4,798 µs |
| Calibrated/cached `sys.getrefcount` + exact proxy-type scanner | 23,355 ms | 23,075–23,591 ms | 428.173 | 2,336 µs |
| Bridge-local `Py_REFCNT` shim + exact proxy-type scanner | 22,763 ms | 21,704–23,446 ms | 439.314 | 2,276 µs |

Exact proxy-type yolu ve doğrudan refcount shim'i sıcak tarama süresini ilk doğru
uygulamaya göre yaklaşık %52,6 azalttı. Son yol trace olmayan baseline'a göre
collection başına yaklaşık 1,59 µs ek maliyet getirir. Maliyet yalnız canlı
`ForeignPyObject` bulunan GC'de ödenir;
ordinary Tonic object graph'ının mark/sweep yoluna CPython çağrısı eklenmez.

`sys.getrefcount` çağrı yolu CPython sürümleri arasında farklı geçici referanslar
gösterebildiği için nihai uygulama onu doğruluk kararından çıkardı. Adapter ile
birlikte derlenen küçük C shim, o derlemenin `Python.h` dosyasındaki `Py_REFCNT`
erişimini kullanır. CPython nesne düzeni Rust'a, Tonic core runtime'a veya public
Tonic ABI'ına sızmaz.

Bu mikrobenchmark CPython heap allocation sayısını ölçmez. Graph limitine veya
traversal sınırına ulaşılan durumlar güçlü kökü koruduğu için güvenli fakat daha
uzun yaşayan nesneler üretebilir.
