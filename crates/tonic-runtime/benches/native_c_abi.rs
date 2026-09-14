use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_core::diagnostic::Result;
use tonic_runtime::{
    c_api::{negotiate_api, CAP_CORE},
    Context, Handle, TonicContext, TonicHandle, TonicStatus, Vm, TONIC_ABI_VERSION,
};

const WARMUP: usize = 3;
const SAMPLES: usize = 15;

fn rust_double(context: &mut Context<'_>, arguments: &[Handle]) -> Result<Handle> {
    let value = context.to_i64(arguments[0])?;
    context.from_i64(value * 2)
}

unsafe extern "C-unwind" fn c_double(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    argument_count: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    assert_eq!(argument_count, 1);
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_CORE).unwrap();
    let mut value = 0;
    // SAFETY: the VM supplies a live argument for the declared arity.
    let status = unsafe { (api.int_as_i64)(context, arguments.read(), &mut value) };
    if status != TonicStatus::OK {
        return status;
    }
    // SAFETY: output remains writable for the duration of the native call.
    unsafe { (api.int_from_i64)(context, value * 2, output) }
}

fn median(mut values: Vec<f64>) -> (f64, f64, f64) {
    values.sort_by(f64::total_cmp);
    (values[SAMPLES / 2], values[0], values[SAMPLES - 1])
}

fn measure(kind: &str, source: &str) -> (f64, f64, f64, u64) {
    let program = compile(source, "<native-c-abi-bench>").unwrap();
    let mut samples = Vec::new();
    let mut native_calls = 0;
    for sample in 0..WARMUP + SAMPLES {
        let mut vm = Vm::new().unwrap();
        match kind {
            "rust" => vm
                .register_native("bench", "double", 1, rust_double)
                .unwrap(),
            "c_abi" => vm
                .register_c_native("bench", "double", 1, c_double, TONIC_ABI_VERSION, CAP_CORE)
                .unwrap(),
            _ => unreachable!(),
        }
        let start = Instant::now();
        vm.run(black_box(&program), &mut io::sink()).unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        assert_eq!(vm.active_handles(), 0);
        native_calls = vm.stats.native_calls;
        if sample >= WARMUP {
            samples.push(elapsed);
        }
    }
    let (median, min, max) = median(samples);
    (median, min, max, native_calls)
}

fn main() {
    let source = "import bench\ni=0\nwhile i<100000:\n    bench.double(i)\n    i+=1\n";
    println!("abi,calls,median_ms,min_ms,max_ms,calls_per_second");
    for kind in ["rust", "c_abi"] {
        let (median, min, max, calls) = measure(kind, source);
        println!(
            "{kind},{calls},{:.3},{:.3},{:.3},{:.0}",
            median * 1e3,
            min * 1e3,
            max * 1e3,
            calls as f64 / median
        );
    }
}
