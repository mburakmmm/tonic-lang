# ADR 0077: Comprehension lexical scope ve iterator zamanlaması

## Durum

Uygulandı. Bu karar senkron list/dict comprehensions ve generator expressions
içindir. Set ve async comprehensions ayrı kabul kapısıdır.

## Karar

Her comprehension ayrı bir Tonic code object ve lexical scope olarak derlenir.
Hedef değişkenleri dış scope'a sızmaz; dış fonksiyon değerleri normal free-cell
mekanizmasıyla yakalanır. Comprehension içinde oluşturulan lambda veya iç içe
comprehension hedefi yakalarsa ilgili local, mevcut closure çözümleyicisi
tarafından cell'e yükseltilir.

Outermost iterable dış scope'ta değerlendirilir ve `ITER` işlemi expression
oluşturulurken tam bir kez çalışır. Oluşan iterator gizli `.0` positional
parametresiyle child code'a aktarılır. Bu, generator expression gövdesi tembel
kalırken `__iter__` yan etkisi ve hatasının Python gibi expression oluşturma
anında gözlenmesini sağlar. Sonraki iterable'lar ve filtreler ilgili dış clause
öğesi başına child frame içinde değerlendirilir.

List comprehension boş managed liste oluşturur ve her kabul edilen öğeyi yeni
bytecode v22 `LIST_APPEND(owner, value)` ile ekler. Opcode yalnız exact internal
accumulator için üretilir ve managed heap mutation sınırını kullanarak old→young
write barrier'ını korur; tek elemanlı geçici liste tahsis etmez. Dict
comprehension, key sonra value değerlendirme sırasıyla mevcut `SET_ITEM` ve
suspending hash/equality protokolünü kullanır. Generator expression aynı loop
gövdesinde `YIELD` üretir ve normal generator lifecycle/GC kurallarını paylaşır.

Comprehension code'u `ITER`, `NEXT`, mutation ve kullanıcı protokol çağrıları
içerdiği için mevcut Cranelift destek kümesinde değildir. Public JIT verifier
`LIST_APPEND` operandlarını doğrular; tier seçimi code object'i güvenli biçimde
generic interpreter'da bırakır.

## Reddedilen seçenekler

- Comprehension hedeflerini enclosing scope'a açmak: Python lexical scope ve
  closure davranışını bozar.
- Generator expression'ın outer `iter()` çağrısını ilk `next()`e ertelemek:
  yan etki ve hata zamanını değiştirir.
- Listeyi `result += [item]` ile büyütmek: her öğede gereksiz managed liste
  tahsisi ve generic operator dispatch'i üretir.

## Doğrulama

Compiler testleri Tonic-owned AST, child code metadata, generator flag'i ve
`LIST_APPEND` üretimini denetler. Verifier ve public JIT testleri operand sınırı
ile desteklenmeyen-op fallback'ini kapsar. Runtime testleri iç içe clause ve
filtreleri, unpack hedefini, dış local capture'ını, lambda cell'lerini, class
scope görünürlüğünü, eager outer iterator ile lazy element zamanlamasını ve
generator tüketimini interpreter/JIT-caller modlarında her allocation'da moving
stress GC altında çalıştırır. Aynı gözlenebilir program CPython differential
korpusunda karşılaştırılır.
