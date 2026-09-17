# ADR 0052 — JIT giriş doğrulaması ve native kod bütçesi

## Durum

Kabul edildi.

## Sorun

`tonic-runtime` JIT'e doğrulanmış program gönderse de `tonic-jit` public API'si
ham `CodeObject` kabul eder. Constant, register, jump veya call-window indeksi
bozuk bir embedder girdisi Cranelift'e ulaşmadan önce Rust dizinlemesinde panic
üretebilirdi. Ayrıca bir yürütme içindeki derlenmiş fonksiyonların toplam native
kod boyutu için üst sınır yoktu.

## Karar

Her public compile yolu caller ve doğrudan inlining target'larında yapısal
doğrulama çalıştırır. Register, constant, jump, call window, fallthrough ve
exact-float profil sınırları codegen öncesinde denetlenir. Hata
`Error::InvalidBytecode` ile PC bilgisi taşır. Public inlineability sorguları da
aynı doğrulamayı kullanıp bozuk girdide `false` döner.

VM bir module execution boyunca kabul edeceği native kodu varsayılan 64 MiB ile
sınırlar. `Vm::jit_max_code_bytes` embedder tarafından düşürülebilir. Limit aşımı
üretilmiş modülü hemen düşürür, code object'i unsupported olarak negative-cache'e
alır ve generic interpreter'da devam eder. `jit_code_budget_rejections` bu yolu
ölçer. Bütçe yeni `Vm::run` başlangıcında sıfırlanır; toplam derleme işi de aynı
bütçeye sayıldığı için kararsız/deopt olmuş kod tekrar tekrar executable memory
ürettiremez.

## Doğrulama

Unit testler constant/register/jump ve exact-float profil bozukluklarının panic
yerine kesin hata olduğunu, public sorguların fail-closed kaldığını doğrular.
Runtime testi sıfır kod bütçesinde derlemeyi reddeder, doğru sonucu interpreter
ile üretir ve sayaçları kontrol eder. `jit_code_object` libFuzzer hedefi metadata
ile bütün instruction sözcüklerini coverage-guided olarak mutasyona uğratır.
