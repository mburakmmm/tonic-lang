use std::{
    ffi::c_void,
    hint::black_box,
    io, mem,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use tonic_compiler::compile;
use tonic_runtime::{
    c_api::{negotiate_api, CAP_FOREIGN_OBJECT_V1},
    CollectionStats, Stats, TonicContext, TonicForeignVTable, TonicHandle, TonicStatus, Vm,
    FOREIGN_OWNED, TONIC_ABI_VERSION,
};

const WARMUP: usize = 3;
const SAMPLES: usize = 15;
const OBJECTS: usize = 100_000;
static DESTROYS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C-unwind" fn destroy(payload: *mut c_void) {
    DESTROYS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: each successful wrapper owns one Box allocation.
    drop(unsafe { Box::from_raw(payload.cast::<u64>()) });
}

static VTABLE: TonicForeignVTable = TonicForeignVTable {
    struct_size: mem::size_of::<TonicForeignVTable>() as u32,
    abi_version: TONIC_ABI_VERSION,
    adapter_id: 1,
    flags: FOREIGN_OWNED,
    trace: None,
    destroy: Some(destroy),
};

unsafe extern "C-unwind" fn make_foreign(
    context: *mut TonicContext,
    _: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_FOREIGN_OBJECT_V1).unwrap();
    let payload = Box::into_raw(Box::new(black_box(42u64))).cast();
    // SAFETY: payload, static vtable, and output live for the call.
    let status = unsafe { (api.foreign_create)(context, payload, &VTABLE, output) };
    if status != TonicStatus::OK {
        // SAFETY: failed creation leaves ownership with this callback.
        drop(unsafe { Box::from_raw(payload.cast::<u64>()) });
    }
    status
}

#[derive(Clone, Copy)]
struct Sample {
    create: f64,
    collect: f64,
    stats: Stats,
    collection: CollectionStats,
    destroys: usize,
}

fn measure(source: &str, foreign: bool) -> Sample {
    let program = compile(source, "<foreign-lifecycle-bench>").unwrap();
    let mut samples = Vec::new();
    for sample in 0..WARMUP + SAMPLES {
        DESTROYS.store(0, Ordering::Relaxed);
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        if foreign {
            vm.register_c_native(
                "foreign",
                "make",
                0,
                make_foreign,
                TONIC_ABI_VERSION,
                CAP_FOREIGN_OBJECT_V1,
            )
            .unwrap();
        }
        let started = Instant::now();
        vm.run(black_box(&program), &mut io::sink()).unwrap();
        let create = started.elapsed().as_secs_f64();
        let stats = vm.stats;
        let started = Instant::now();
        let collection = vm.collect_garbage().unwrap();
        let collect = started.elapsed().as_secs_f64();
        let destroys = DESTROYS.load(Ordering::Relaxed);
        if sample >= WARMUP {
            samples.push(Sample {
                create,
                collect,
                stats,
                collection,
                destroys,
            });
        }
    }
    samples.sort_by(|left, right| {
        (left.create + left.collect).total_cmp(&(right.create + right.collect))
    });
    samples[SAMPLES / 2]
}

fn main() {
    let list = measure(
        &format!("i=0\nwhile i<{OBJECTS}:\n    [i]\n    i+=1"),
        false,
    );
    let foreign = measure(
        &format!("import foreign\ni=0\nwhile i<{OBJECTS}:\n    foreign.make()\n    i+=1"),
        true,
    );
    println!(
        "kind,objects,create_ms,collect_ms,total_ms,guest_allocations,native_calls,foreign_wrappers,reclaimed,destroys"
    );
    for (kind, sample) in [("managed_list", list), ("owned_foreign", foreign)] {
        println!(
            "{kind},{OBJECTS},{:.3},{:.3},{:.3},{},{},{},{},{}",
            sample.create * 1e3,
            sample.collect * 1e3,
            (sample.create + sample.collect) * 1e3,
            sample.stats.heap_allocations,
            sample.stats.native_calls,
            sample.stats.foreign_wrapper_creations,
            sample.collection.reclaimed,
            sample.destroys,
        );
    }
}
