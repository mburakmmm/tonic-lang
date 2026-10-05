# ADR 0106 — Verified bytecode exact-int data-flow overlay'i

## Durum

Kabul edildi; annotation-JIT için exact-small-int overlay ve guard elimination
uygulandı.

## Bağlam

İlk annotation-JIT dilimi TypePlan ile ilk çağrıda Cranelift seçiyor, fakat her
integer işlem yine operand tag'larını yeniden doğruluyordu. Parametre guard'ı
başarılı olduktan ve yalnız checked native integer işlemleri yürütüldükten sonra
bu tekrarlar gereksizdir. Buna karşılık side exit, OSR veya interpreter resume
girişi herhangi bir bytecode PC'sinden başlayabildiği için yalnız function-entry
guard'ına güvenmek güvenli değildir.

## Karar

Cranelift derlemesinden önce verified bytecode üzerinde forward fixed-point analiz
çalışır. Her PC için exact-small-int olduğu ispatlanan virtual register'lar tutulur.
Başlangıç durumu typed signature'ın `int` parametrelerinden gelir; immediate int
sabitleri, `Move`, integer unary ve checked binary işlemler bilgiyi taşır. Branch
birleşiminde yalnız bütün gelen kollarda exact-int kalan değer korunur. Loop
backedge'leri aynı meet işlemiyle kararlı duruma ulaşır. Global, call, attribute ve
generic helper sonuçları bilinmeyen kabul edilir.

Generated entry, seçilen `start_pc` için canlı exact-int register'larının tagged
immediate olduğunu doğrular. Bu arbitrary-PC guard başarıyla geçtikten sonra
analizin ispatladığı arithmetic/comparison sitelerinde per-op operand tag guard'ı
üretilmez. Overflow, division-by-zero ve immediate aralık sınırları yerinde kalır;
başarısızlık ilgili bytecode PC'sine deopt eder. Public typed-signature girişi arity
ve float-profile tutarlılığını codegen'den önce doğrular.

## Sonuçlar

Metadata ile VM istatistiği kaldırılan tag-guard sayısını yayınlar. Analiz generic
bytecode veya interpreter davranışını değiştirmez ve bilinmeyen değerde mevcut
guard/helper yolunu korur. Bu aşama integer'ı raw machine register'da sürekli
unboxed tutmaz; root buffer hâlâ tagged `Value` taşır. Float/bool birleşik lattice,
return/call-result propagation ve unboxed integer call ABI sonraki kararlardır.
