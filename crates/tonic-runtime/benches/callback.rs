use std::{hint::black_box, io, sync::Mutex, time::Instant};
use tonic_compiler::compile;
use tonic_core::diagnostic::Result;
use tonic_runtime::{Context, Handle, PersistentHandle, Stats, Vm};

const WARMUP: usize = 3;
const SAMPLES: usize = 15;
const CALLS: usize = 10_000;
static CALLBACK: Mutex<Option<PersistentHandle>> = Mutex::new(None);

fn save(context: &mut Context<'_>, arguments: &[Handle]) -> Result<Handle> {
    *CALLBACK.lock().unwrap() = Some(context.persist(arguments[0])?);
    context.none()
}

fn median(mut samples: Vec<(f64, Stats)>) -> (f64, f64, f64, Stats) {
    samples.sort_by(|left, right| left.0.total_cmp(&right.0));
    let (value, stats) = samples[SAMPLES / 2];
    (value, samples[0].0, samples[SAMPLES - 1].0, stats)
}

fn guest_loop() -> (f64, f64, f64, Stats) {
    let source = format!(
        "def add(value):\n    return value+1\ni=0\nwhile i<{CALLS}:\n    add(41)\n    i+=1\n"
    );
    let program = compile(&source, "<callback-guest-bench>").unwrap();
    let mut samples = Vec::new();
    for sample in 0..WARMUP + SAMPLES {
        let mut vm = Vm::new().unwrap();
        let start = Instant::now();
        vm.run(black_box(&program), &mut io::sink()).unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        if sample >= WARMUP {
            samples.push((elapsed, vm.stats));
        }
    }
    median(samples)
}

fn host_callbacks() -> (f64, f64, f64, Stats) {
    let program = compile(
        "import callback\ndef add(value):\n    return value+1\ncallback.save(add)",
        "<callback-host-bench>",
    )
    .unwrap();
    let mut samples = Vec::new();
    for sample in 0..WARMUP + SAMPLES {
        *CALLBACK.lock().unwrap() = None;
        let mut vm = Vm::new().unwrap();
        vm.register_native("callback", "save", 1, save).unwrap();
        vm.run(&program, &mut io::sink()).unwrap();
        let callback = CALLBACK.lock().unwrap().take().unwrap();
        let argument = {
            let mut context = vm.context().unwrap();
            let local = context.from_i64(41).unwrap();
            context.persist(local).unwrap()
        };
        let start = Instant::now();
        for _ in 0..CALLS {
            let result = vm
                .call_persistent(&callback, &[&argument], &mut io::sink())
                .unwrap();
            vm.context().unwrap().release_persistent(&result).unwrap();
        }
        let elapsed = start.elapsed().as_secs_f64();
        {
            let mut context = vm.context().unwrap();
            context.release_persistent(&argument).unwrap();
            context.release_persistent(&callback).unwrap();
        }
        assert_eq!(vm.active_handles(), 0);
        if sample >= WARMUP {
            samples.push((elapsed, vm.stats));
        }
    }
    median(samples)
}

fn main() {
    println!("entry,calls,median_ms,min_ms,max_ms,calls_per_second,callback_calls");
    for (name, measurement) in [
        ("guest_loop", guest_loop()),
        ("host_reentry", host_callbacks()),
    ] {
        let (median, min, max, stats) = measurement;
        println!(
            "{name},{CALLS},{:.3},{:.3},{:.3},{:.0},{}",
            median * 1e3,
            min * 1e3,
            max * 1e3,
            CALLS as f64 / median,
            stats.callback_calls
        );
    }
}
