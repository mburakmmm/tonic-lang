# ADR 0075: Tonic event loop, Future ve Task protokolleri

## Durum

Uygulandı. Bu karar tek thread'li temel scheduling katmanını kapsar. İşletim
sistemi I/O selector'ları, thread-safe scheduling, gerçek zamanlı timer ve daha
geniş `asyncio` standart kütüphanesi ayrı kapsamdır.

## Karar

Tonic, CPython `asyncio` nesnelerini veya event loop'unu gömmeden kendi native
`asyncio` modülünü sağlar. İlk sürüm `run`, `create_task`, `current_task`,
`get_running_loop`, `sleep` ve `Future` girişlerini sunar. Scheduler VM'ye aittir;
coroutine'ler ADR 0073'teki interpreter state machine'iyle sürülür ve JIT çağıran
koddan güvenli biçimde interpreter'a döner.

`Future`, `Task`, Future await iterator'ı ve `EventLoop` ayrı managed nesnelerdir.
Hepsi opaque `Value` handle'larıyla temsil edilir ve class, coroutine, sonuç,
exception, waiter, callback, beklenen future ve timer sonucu kenarlarını precise
GC trace'e verir. Scheduler'ın ready kuyruğu, timer listesi, task listesi ve aktif
task'ı da VM root setine dahildir. Kalıcı cache veya guest-visible API ham Rust
adresi tutmaz.

Future ve Task ortak dört durum taşır: Pending, Finished, Failed ve Cancelled.
`__await__` ayrı bir iterator üretir. Pending iterator kaynak future'ı scheduler'a
yield eder; terminal durumda `StopIteration.value` ile sonucu döndürür veya
saklanan exception'ı yeniden yükseltir. Task bir Future alt sınıfıdır ve aynı
`done`, `cancelled`, `result`, `exception` ve `add_done_callback` yüzeyini kullanır.
Yalnız plain Future `set_result` ve `set_exception` ile dışarıdan tamamlanabilir.

Ready kuyruğu FIFO'dur. Bir task kaynak Future/Task'a askı verdiğinde kaynak
nesne task'ı waiter olarak tutar. Tamamlama callback'leri ve waiter'ları kuyruğa
alır; exception ve cancellation bir sonraki resume'da coroutine'e enjekte edilir.
Coroutine hatayı yakalayabilir. Yakalanmayan `CancelledError`, Task'ı Cancelled;
diğer exception'lar Failed durumuna geçirir. `cancel()` terminal task/future için
`False`, yeni bir iptal isteği için `True` döndürür.

`sleep` ilk aşamada süreyi sanal scheduler tick sayısına çevirir. Sıfır ve negatif
gecikme en az bir scheduling turu bırakır; pozitif integer/float değerler gerçek
duvar saati bekletmeden deterministik tick timer'ı oluşturur. Bu seçim testleri
tekrar üretilebilir kılar ve OS timer bağımlılığı eklemez. Gerçek zamanlı I/O loop
ileride aynı Future completion sınırını kullanabilir.

`asyncio.run` iç içe çalıştırmayı reddeder, exact Tonic coroutine'den ana Task
oluşturur ve ana task terminal olana kadar ready/timer kuyruklarını sürer. Hazır iş
ve timer kalmadan ana task pending ise deadlock `RuntimeError` olur. Callback
exception'ları loop'u veya tamamlanmış Future'ı bozmaz. Event loop state her
`run` sonunda VM'den ayrılır.

## Reddedilen seçenekler

- CPython `asyncio` loop'unu bridge üzerinden kullanmak: fast native object modelini
  CPython lifetime ve thread kurallarına bağlar.
- Her `await` için native Rust future/stack oluşturmak: suspended VM register'ları,
  moving GC ve traceback/deoptimization state'iyle uyuşmaz.
- İlk sürümde gerçek zamanlı selector eklemek: Future/Task protokolü doğrulanmadan
  platform I/O karmaşıklığı getirir.

## Doğrulama

Runtime language testleri FIFO task ilerlemesini, `sleep(0)`, Future completion,
done callback, task sonucu, exception propagation, cancellation, terminal state
sorguları, `InvalidStateError`, iç içe loop reddi ve `current_task`/
`get_running_loop` davranışını kapsar. Aynı testler interpreter ve JIT-caller
modlarında her allocation'da moving stress GC ile çalışır. Differential corpus,
iki eşzamanlı task'ın `sleep(0)` scheduling sırasını CPython `asyncio` karşısında
denetler.

