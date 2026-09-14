# Native C ABI v1 ara ölçümü

12 Eylül 2026; Apple Silicon arm64, macOS 26.6, Rust 1.86.0, release LTO.
Bu ölçüm nihai Python karşılaştırması değildir.

Komut:

```sh
cargo bench -p tonic-runtime --bench native_c_abi --locked --offline
```

Her örnek fresh VM'de `bench.double(i)` fonksiyonunu 100.000 kez çağırır. Parse ve
compile süre dışında, VM run süre içindedir. Üç warmup ardından 15 örnek alınır.
Her iki yol exact arity doğrulaması, argument `Value`→local handle kurulumu, integer
decode/encode, result doğrulaması ve scope cleanup yapar. C ABI ayrıca opaque context,
function-table ve status/exception sınırından geçer.

| Sınır | Medyan | Min–maks | Çağrı/s |
|---|---:|---:|---:|
| Rust-native `fn` | 17,770 ms | 17,479–18,901 ms | 5.627.317 |
| C function-table ABI v1 | 20,145 ms | 19,887–20,928 ms | 4.964.103 |

C ABI medyan çağrı süresini bu mikroişte yaklaşık %13,4 artırdı. Bu maliyet
argument tuple/dict guest allocation'ından gelmez; her iki yol da slice/count ve
scoped logical handle kullanır. ABI yolu iki status-checked table çağrısı ve panic
sınırı ekler. Sonuç mimari kabul bütçesi içinde tutuldu; ileride CALL_NATIVE JIT
yolu eklendiğinde aynı benchmark yeniden alınmalıdır.

Ek doğrulama:

```sh
cc -std=c11 -Wall -Wextra -Werror -Iinclude -fsyntax-only tests/c_header_smoke.c
```

Smoke test 64-bit handle ile table header offsetlerini static assert eder ve C
callback/bootstrap imzalarını derler.
