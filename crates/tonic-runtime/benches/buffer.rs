use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_runtime::{Stats, Vm};

const WARMUP: usize = 3;
const SAMPLES: usize = 15;
const ELEMENTS: usize = 1024;
const SUMS: usize = 10_000;

fn source(buffer: bool) -> String {
    let convert = if buffer {
        "values=fastmath.array(values)\n"
    } else {
        ""
    };
    format!(
        "import fastmath\nvalues=[]\nj=0\nwhile j<{ELEMENTS}:\n    values+=(j%16,)\n    j+=1\n{convert}i=0\nwhile i<{SUMS}:\n    fastmath.sum(values)\n    i+=1\n"
    )
}

fn measure(buffer: bool) -> (f64, f64, f64, Stats) {
    let program = compile(&source(buffer), "<buffer-bench>").unwrap();
    let mut samples = Vec::new();
    for sample in 0..WARMUP + SAMPLES {
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        let start = Instant::now();
        vm.run(black_box(&program), &mut io::sink()).unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        assert_eq!(vm.active_handles(), 0);
        if sample >= WARMUP {
            samples.push((elapsed, vm.stats));
        }
    }
    samples.sort_by(|left, right| left.0.total_cmp(&right.0));
    let (median, stats) = samples[SAMPLES / 2];
    (median, samples[0].0, samples[SAMPLES - 1].0, stats)
}

fn main() {
    println!(
        "storage,elements,sums,median_ms,min_ms,max_ms,elements_per_second,buffer_exports,buffer_copies,guest_allocations,peak_heap_bytes"
    );
    for (name, buffer) in [("boxed_list", false), ("f64_buffer", true)] {
        let (median, min, max, stats) = measure(buffer);
        println!(
            "{name},{ELEMENTS},{SUMS},{:.3},{:.3},{:.3},{:.0},{},{},{},{}",
            median * 1e3,
            min * 1e3,
            max * 1e3,
            (ELEMENTS * SUMS) as f64 / median,
            stats.buffer_exports,
            stats.buffer_copies,
            stats.heap_allocations,
            stats.peak_heap_bytes
        );
    }
}
