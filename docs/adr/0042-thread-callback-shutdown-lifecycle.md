# ADR 0042 — Thread attachment, callback reentry ve staged shutdown

Durum: uygulanmış. 12 Eylül 2026.

## Karar

Her VM aynı anda en fazla bir OS thread'e attached olur. Yaratma current thread'e
implicit attach eder. Idle VM explicit detach edildikten sonra başka thread'e
taşınıp attach edilebilir. Yanlış veya unattached thread'deki mutating runtime
işlemleri `ThreadError` üretir; shared-heap paralel yürütme bu aşamanın parçası
değildir.

Son başarılı run'ın owned `Arc<Program>` kopyası VM'de tutulur. Callable ve
argument persistent handle'larıyla `call_persistent`, run sonrasında ayrı frame/root
scope'u kurar. Yeni run execution kimliğini değiştirir ve eski callable guard'ı
korunur. Aktif native call içindeki `Context::call` ile C `TonicApi.call`, mevcut
frame/register/cell/expanded-argument uzunluklarını checkpoint edip child target'ı
o derinliğe kadar çalıştırır. Scratch state başarı ve exception yolunda kesilir.

Shutdown dört aşamalıdır: Running, ShuttingDown, Finalizing, Dead. Finalization
handle tablosunu topluca geçersiz kılar, VM registry/root state'ini bırakır ve boş
root setiyle major collection çalıştırır. Dead runtime yeni attach, context, run ve
callback kabul etmez.

## Sonuçlar

Persistent closure kaynak verified program bırakıldıktan ve moving GC sonrasında
çalışır. Callback exception'ından sonra ikinci callback başarılıdır. VM detach
sonrası gerçek bir `std::thread` üzerinde çalıştırılıp ana thread'e geri taşınır.
Nested Tonic→C native→Tonic→C native→Tonic zinciri her-allocation stress GC altında
42 sonucunu ve kesin root cleanup'ını korur.

10.000 idle host reentry çağrısı 1,106 ms medyan, yaklaşık 110,6 ns/call ölçülmüştür.
Yöntem [`CALLBACK_BASELINE.md`](../CALLBACK_BASELINE.md) dosyasındadır.

## Sınırlar

Shared runtime concurrency ve C embedding için uzun ömürlü opaque runtime owner
ayrı aşamalardır. Foreign payload kuyruğu ADR 0043'te tamamlanmıştır. Active native
trampoline başına bir host stack frame vardır; guest recursion explicit VM
frame'leri ve mevcut frame limitiyle yürür.
