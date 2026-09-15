# Rust-native ilk dilim

Bu API Rust kaynağı birlikte derlenen modüller içindir. Stable C/binary ABI,
shared library loader veya CPython adapter değildir.

```rust
use tonic_runtime::{Context, Handle, Vm};
use tonic_core::diagnostic::{Diagnostic, Result};

fn add(ctx: &mut Context<'_>, args: &[Handle]) -> Result<Handle> {
    let a = ctx.to_i64(args[0])?;
    let b = ctx.to_i64(args[1])?;
    let sum = a.checked_add(b)
        .ok_or_else(|| Diagnostic::new("OverflowError", "i64 sum overflow"))?;
    ctx.from_i64(sum)
}

fn register(vm: &mut Vm) -> Result<()> {
    vm.register_native("demo", "add", 2, add)
}
```

Tonic: `import demo; print(demo.add(20, 22))`.
VM arity'yi native koda girmeden doğrular. Fonksiyon pointer'ı ve argument slice
kullanılır; guest tuple/dict oluşturulmaz. Bootstrap boundary'de local handle
ve argument vektörleri host tahsisi yapabilir; bunlar benchmark guest allocation
sayacına dahil değildir.

## Yaşam süresi

- Internal `Value` ve public `Handle` ayrı tiplerdir. Hiçbiri managed native adresi
  değildir. Dış kod Value bitlerini veya heap layout'unu göremez.
- `Vm::context()` bir local scope açar. `Drop`, local handle'ları hem normal hem
  hata yolunda iptal eder. Yerel result VM register'ına scope kapanmadan aktarılır.
- `ctx.persist(local)` yeni bir explicit persistent root oluşturur.
- Sonraki scope `ctx.borrow_persistent(&persistent)` ile local referans alır.
- `ctx.release_persistent(&persistent)` persistent root'u iptal eder. Önceden
  alınmış local kopya kendi scope'u boyunca geçerlidir. Double release hatadır.
- `PersistentHandle` Drop otomatik release yapmaz: runtime'a bağlı ownership
  explicit'tir; serbest bırakılmazsa VM kapanana kadar root tutulur.
- C ABI aynı sahipliği ayrı `TonicPersistentHandle` tipi ve
  `persistent_create/borrow/release` işlemleriyle taşır. Token ordinary local
  değer gibi kullanılamaz ve release için doğru runtime'ın aktif context'i gerekir.
- Her tahsis yeni, global olarak benzersiz token alır. Stale ve cross-runtime
  kullanım release build'de de `HandleError` üretir. Token wrap yasaktır.
- Scope bitimi **fiziksel heap reclamation** demek değildir. Nesne, başka kökü
  kalmadıysa sonraki collection sırasında toplanabilir.
- `as_str` sıfır kopyalı kısa bir Rust borrow'udur; bu borrow yaşarken context'in
  mutable allocation metodunu çağırmak borrow checker tarafından engellenir.
  Bu genel buffer/pinning API'si değildir.

Native kod guest exception için `Result<_, Diagnostic>` döndürür; panic kullanmaz.
Rust API trusted in-process kod içindir. C sınırı olmadığı için C panic trampoline
varmış gibi bir garanti verilmez. Native koddaki panic host bug'ıdır.

## C function-table ABI v1

Public C sözleşmesi [`include/tonic.h`](../include/tonic.h) başlığındadır. Uzantı
yalnız `tonic_get_api` bootstrap sembolünü çağırır; kalan işlemler immutable
`TonicApi` tablosundaki function pointer'lardan geçer. Tablo `struct_size`,
`abi_version` ve capability bitleriyle başlar. Sürüm eşleşmezse, uzantının istediği
prefix tablodan büyükse veya zorunlu capability yoksa kayıt başlamadan kesin hata
üretilir. `Vm::initialize_c_extension` pazarlık edilen aynı immutable table ve
geçici local scope ile init callback'ini çalıştırır. `Vm::register_c_native` exact
arity ile C callback'i modüle ekler.

ABI yalnız fixed-width sayılar, opaque `TonicContext*`, logical 64-bit
`TonicHandle` ve function pointer taşır. Rust enum/String/Vec/reference, internal
`Value` bitleri ve heap adresleri dışarı çıkmaz. Yabancı girdilerde exception kind
ve capability integer wrapper'dır; bilinmeyen değer Rust enum invalid-discriminant
UB'si yaratmadan reddedilir.

`value_kind` opak handle'ı
`NONE/BOOL/INT/FLOAT/STR/FOREIGN/LIST/TUPLE/DICT/OTHER` sınıflarından birine ayırır;
representation bitlerini açmaz. `bool_from/bool_as`, integer, float ve iki-pass
UTF-8 işlemleri primitive adapter'ların türü koruyarak dönüşüm yapmasını sağlar.
`int_from_decimal/int_decimal` I64 dışındaki keyfî hassasiyetli integer'ı decimal
UTF-8 üzerinden taşır.

Her işlem `TonicStatus` döndürür ve sonucu out-parametresine yazar. Hata ayrıntısı
context'te tutulur; `exception_kind/message` önce `NULL,0` ile gereken UTF-8 byte
uzunluğunu sorgular, sonra çağıranın buffer'ına kopyalar. Başarılı yeni normal API
işlemi eski exception state'ini temizler. `raise_exception` guest-visible exception
oluşturur. Callback `OK` döndürdüğünde result handle aynı call scope'unda doğrulanır;
scope dışından kalan veya başka runtime'a ait handle `HandleError` olur.

Bütün tablo girişleri `catch_unwind` sınırındadır. Rust'ta yazılan test extension
callback'i `C-unwind` ile çağrılır ve dış panic `RuntimeError`a çevrilir; unwind C
sınırını aşmaz, local scope temizlenir ve VM kullanılabilir kalır. Bu trusted
in-process ABI'dır: opaque tipler mimariyi ve yanlışlıkla layout bağımlılığını
korur, kötü niyetli C'nin keyfi pointer erişimini güvenli yapmaz. Dynamic library
discovery/unload henüz uygulanmamıştır.

## Container ve protocol erişimi

`TONIC_CAP_CONTAINER_ACCESS_V1`; boş list/dict oluşturma, barrier'lı append/set,
tuple oluşturma, sequence length/item ve insertion-order dict entry erişimini sunar.
Handle kimliği `is_identical` ile logical olarak sınanır; hiçbir API heap adresi
veya container backing pointer'ı açmaz. Bu işlemler CPython adapter'ının bigint,
list, tuple ve dict değerlerini alias/cycle bilgisi korunarak materialize etmesini
sağlar.

`TONIC_CAP_PROTOCOL_ACCESS_V1`; keyword dict alan `call_kw`, UTF-8 adlı
`get_attr/set_attr` ve Tonic string handle'ı döndüren `repr_value` girişlerini
ekler. Attribute işlemleri property/custom descriptor continuation'larını normal
VM frame'leriyle yürütür. Keyword çağrısı ordinary binder'ın positional-only,
keyword-only, default ve duplicate kurallarını kullanır. Native adapter bu nedenle
ayrı bir dil semantiği uygulamaz.

`foreign_reference_borrow`, foreign trace token'ını yeni bir call-scope local
handle'a çözer. Token global root değildir; yalnız trace edilen foreign wrapper
erişilebilirse managed hedefi yaşatır. CPython proxy cycle protokolü persistent
kökü düşürdükten sonra callback ve attribute işlemlerini bu local borrow üzerinden
sürdürür.

## Typed buffer v1

`Context::from_f64_buffer(values, shape, writable)` veriyi bir kez dış, non-moving
`Box<[f64]>` tahsisine kopyalar. Küçük GC nesnesi dtype/shape/stride ve backing
allocation sahipliğini taşır; collector bu nesneyi sıkıştırsa da data, shape ve
stride pointer'ları değişmez. Safe Rust `f64_buffer` görünümü context'i ödünç alır.

C tablosundaki `buffer_export` `TonicBuffer` descriptor'ı doldurur: byte pointer ve
uzunluk, dtype, item size, rank, shape, byte strides, contiguity/mutability flags ve
ayrı owner handle. Export edilen owner ilgili buffer'ı call scope'unda kökler;
`buffer_release` yalnız `BufferOwner` sınıfındaki handle'ı erken bırakabilir ve
descriptor'ı temizler. Double release ve başka local handle'ı owner gibi kullanma
`HandleError`dır. Read-only buffer için writable export `BufferError` üretir.

`fastmath.array([1.0, ...])` boxed sequence'i bir kez f64 buffer'a dönüştürür.
`fastmath.sum(buffer)` daha sonra `&[f64]` üzerinde copy ve element unboxing olmadan
çalışır. Stats `buffer_copies` ile `buffer_exports` sayaçlarını ayrı gösterir.
Metadata pointer'ı yalnız aktif ödünç boyunca kullanılabilir; bu borrow bırakılmadan
callback/reentry veya başka bir safepoint işlemi yapılmamalıdır.

## Thread, callback ve shutdown

`Vm::new` runtime'ı current OS thread'e attach eder. Yalnız attached thread run,
context, collection, kayıt ve callback yapabilir. Idle runtime
`detach_current_thread` sonrasında başka thread'e taşınabilir ve orada
`attach_current_thread` ile kullanılabilir; aynı anda iki thread veya attached
değilken kullanım `ThreadError`dır. Bu model paralel shared-heap yürütme sözü
vermez; izole VM sahipliğini açık hale getirir.

Bir Tonic callable `PersistentHandle` olarak saklanabilir. `Vm::call_persistent`
son başarılı run'ın owned `Arc<Program>` kopyısını kullanarak idle runtime'a yeni
root/frame scope'uyla girer ve persistent result döndürür. Kaynak
`VerifiedProgram` artık yaşamıyor ve GC heap girişlerini taşımış olsa da closure
cells/captures handle kökünden izlenir. Yeni normal `run` execution kimliğini ve
retained programı değiştirir; eski program callable'ı tanımlı `RuntimeError` ile
reddedilmeye devam eder.

C ABI `runtime_owner_acquire/release` ile VM adresi içermeyen ref-counted opaque
bir owner verir. `persistent_release_deferred` foreign destructor gibi aktif
`TonicContext` bulunmayan bir noktada token'ı doğru runtime kuyruğuna bırakır; VM
native dönüşü, GC ve shutdown sınırlarında kuyruğu boşaltır. `runtime_owner_matches`
cross-runtime callback'i, `runtime_execution_id` ise önceki `Vm::run` programından
kalmış callable proxy'yi reddetmek için kullanılır. VM dead olduktan sonra owner
yeni release/callback kabul etmez.

Aktif native çağrıda `Context::call`, C tablosunda `api->call`, caller
frame/register/cell/argument uzunluklarını checkpoint eder. Callback child frame'i
aynı explicit VM stack'inde hedef derinliğe kadar yürür; başarı veya hatada scratch
state eski uzunluğa kesilir. Nested Tonic→native→Tonic zincirleri ve stress GC
test edilir. Her native sınır host Rust stack'inde bir trampoline kullanır; guest
fonksiyon çağrıları yine explicit VM frame'leridir.

Shutdown `Running → ShuttingDown → Finalizing → Dead` aşamalarını izler.
`begin_shutdown`, `finalize_shutdown`, `complete_shutdown` ayrı; `shutdown` birleşik
yoldur. Finalization registry ve runtime root'larını bırakır, boş root setiyle full
collection ve foreign destructor queue drain yapar, ardından bütün handle'ları
geçersiz kılar. ShuttingDown
başladıktan sonra yeni run/context/callback/attach reddedilir.

## Foreign wrapper, trace ve payload yaşamı

C tablosundaki `foreign_reference_create`, dış payload içinde tutulacak bir Tonic
değeri için özel logical handle üretir. Bu sınıf handle global root değildir ve
ordinary local/persistent/buffer-owner yerine kullanılamaz. `foreign_create`,
size/version/flag doğrulamalı `TonicForeignVTable` ile dış payload'ı managed
`Object::Foreign` wrapper'a bağlar. Başarılı create sahipliği runtime'a geçirir;
başarısız create'te payload ve henüz bağlanmamış reference çağıranda kalır.

Vtable trace callback'i opaque payload ve geçici `TonicTraceVisitor` alır. `visit`,
`foreign_reference_create` sonucu wrapper-owned handle'ları kabul eder; duplicate,
stale, başka runtime'a ait veya yanlış-kind handle kesin hata üretir. Artık
bildirilmeyen owned handle'lar runtime tarafından release edilir.
`TONIC_CAP_CROSS_COLLECTOR_V1` ile eklenen `visit_borrowed`, sahipliği payload'da
kalan foreign-reference handle'ı yalnız o trace için `Value` kenarına çözer;
wrapper bu token'ı release etmez. `promote`, borrowed token'dan yeni persistent
root üretir. `runtime_identity` graph içindeki proxy'lerin doğru runtime'a ait
olduğunu doğrulamayı, `foreign_reference_release_deferred` ise aktif context
olmayan proxy deallocation yolunun token'ı doğru owner kuyruğuna bırakmasını sağlar.
Her normal GC başlamadan callback yeniden çalışır ve wrapper'ın precise trace
listesi güncellenir. Old foreign wrapper'a yeni nursery değeri yazılmışsa owner
remembered set'e kaydedilir; minor collection kenarı kaybetmez.

`TONIC_FOREIGN_OWNED` destroy callback'ini zorunlu kılar; borrowed payload destroy
slotu veremez. Sweep destructor çalıştırmaz: payload ve reference handle'lar pending
queue'ya taşınır. Compaction bittikten sonra queue destructor'ı traced kenarlar
finalization root olarak hâlâ canlıyken tam bir kez çağırır; owned reference'lar
destructor döndükten sonra bırakılır. Panic tutulur, yeniden denenmez ve
`foreign_destructor_panics` sayacına yazılır. VM normal Rust Drop yolu da henüz
toplanmamış owned payload için son cleanup garantisi verir. Vtable kodunu sağlayan
library runtime kapanana kadar yüklü kalmalıdır; unload protokolü yoktur.

`foreign_borrow_payload` wrapper ve beklenen adapter kimliğini doğruladıktan sonra
opaque payload pointer'ını ödünç verir. Pointer yalnız aktif native scope boyunca
geçerlidir; bu scope'ta GC/finalization çalışamaz. İlk tüketici ayrı
`tonic-cpython` crate'indeki `ForeignPyObject` adapter'ıdır. CPython-owned referans
foreign finalization kuyruğunda CPython execution state'i edinilerek `Py_DecRef`
edilir. Core runtime `PyObject`, CPython refcount veya GIL ayrıntısı içermez.

## GC/JIT geçişi

Collector register frames, closure cells, aktif callable'lar, class namespace ve
constructor dönüş continuation'ları, hazırlanmakta olan
expanded call arguments, globals, constants, builtin/native module registry ve
local/persistent handle table'ı kök olarak ziyaret eder. Managed edge'leri izler,
döngüleri toplar ve nesne girişlerini taşır. İç slotun generation'ı yeniden
kullanımda artar; native Handle bit düzeni bu iç mekanizmadan bağımsızdır.

`Vm::collect_garbage()` explicit collection sağlar. Otomatik collection VM
instruction sınırında olur. `call/call_kw` ve attribute descriptor yolları aktif
native context içinden normal VM frame'lerine reentry yapabilir; local handle'lar
bu sırada precise root'tur. Ödünç foreign payload veya buffer pointer'ı reentry
boyunca saklanmamalıdır. Bu gerçek zaman veya bellek bütçesi garantisi değildir;
uzantılar her allocation/safepoint'te GC olabileceğini varsayıp scoped handle
kullanmalıdır.

`Heap::append_list`, `set_item`, `dict_set`, `store_cell`, `add_module_member`,
`namespace_set`, `set_attr` ve foreign trace refresh
owner/value mutation sınırlarıdır. Bu API'ler old owner'a young değer yazıldığında
owner slotunu remembered set'e ekler; minor collection yalnız nursery'yi precise
roots ve bu owner'ların kenarlarından tarar. Her 32. otomatik collection major'dır
ve bütün heap'i tarar. Finalizer ve bounded-pause garantisi henüz yoktur.
Persistent string handle'ının collection sonrası aynı içeriği verdiği ve release
sonrası toplandığı test edilir. Native result/error scope cleanup stress GC ile
doğrulanır. Tonic↔foreign wrapper döngüsü, shutdown cleanup ve exactly-once payload
destruction ayrıca test edilir. Doğrudan `PyTonicProxy` wrapper döngüsü refcount-aware
root demotion ile otomatik toplanır; dış CPython referansı kökü yeniden güçlendirir.
Arbitrary `ForeignPyObject` iç grafiklerinde transitif proxy kenarları public
`Py_tp_traverse` slotuyla bounded biçimde taranır. Traversal hatası, graph limiti
veya başka runtime'a ait proxy görülürse adapter güçlü kökü koruyarak conservative
retention uygular. Ayrıntılı sözleşme
[ADR 0048](adr/0048-cpython-cross-collector-graph-tracing.md) içindedir.

İlk Cranelift katmanı leaf numeric fonksiyonları çalıştırır ve `/` için opak bir
runtime helper kullanır; döngülü kodda integer olmayan `+ += - * // %` işlemleri
de aynı helper ABI'sindeki generic runtime yoluna düşer. Native `Context`
uzantılarını henüz çağırmaz. Register'lar
helper öncesinde explicit `Value` dizisinde materialized olur. Allocation safepoint'i
bu diziyi diğer VM roots ile birleştirir; guard failure ve helper error exact
bytecode PC'siyle döner. Helper trampoline'ı Rust panic'inin FFI sınırını aşmasını
engeller. Ardışık helper allocation'ları arasında yalnız JIT register'ında yaşayan
değer stress GC altında doğrulanır. Gelecekte Tonic/native calls ve unboxed managed
değerler eklendiğinde aynı root/scope sınırı korunmalı; native backedge'ler şimdiden
1024 geçişte bir aynı explicit-root helper'ını poll eder. Safepoint'teki live machine
register'ları kesin stack map veya explicit materialized roots üzerinden görünür
olmalıdır. Bu yapılmadan native extension çağrıları JIT içine alınmaz.
Direct expanded-call sequence/mapping helper sonuçları guest register'ları ezmez;
her dinamik öğe toplam `root_count` içindeki ayrı JIT-private köke yazılır. Runtime
native dönüşte yalnız `register_count` uzunluğundaki guest önekini frame'e kopyalar.
Observed ordinary-call `*args/**kwargs` materialization helper'ları da aynı kesin
kök kuyruğunu kullanır ve allocation öncesinde bütün dilimi safepoint'e verir.

Bound JIT global okumaları helper çağırmaz. VM her execution başında symbol sayısı
kadar raw `Value` aynası kurar ve `STORE_GLOBAL` ile aynı slotu günceller. Native
entry salt-okunur slice pointer/count alır; sınır dışı veya `UNBOUND` yük yalnız
tanı helper'ına gider. Aynadaki heap handle'ları ayrıca GC owner değildir; karşılık
gelen `Vm::globals` slotları kesin root olmaya devam eder.
