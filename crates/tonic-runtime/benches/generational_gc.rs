use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_runtime::{Stats, Vm};

const WARMUP: usize = 3;
const SAMPLES: usize = 15;

fn main() {
    let cases = [
        (
            "ephemeral_lists_100000",
            "i=0\nwhile i<100000:\n    value=[i]\n    i+=1\nprint(i)",
        ),
        (
            "old_to_young_slot_100000",
            "owner=[None]\ni=0\nwhile i<100000:\n    owner[0]=[i]\n    i+=1\nprint(owner[0])",
        ),
        (
            "cyclic_garbage_100000",
            "i=0\nwhile i<100000:\n    value=[]\n    value+=(value,)\n    i+=1\nprint(i)",
        ),
    ];
    println!("case,median_us,min_us,max_us,collections,minor,major,promoted,reclaimed,moved,pause_us,max_pause_us");
    for (name, source) in cases {
        let program = compile(source, "<generational-gc-bench>").unwrap();
        let mut samples: Vec<(f64, Stats)> = Vec::new();
        for sample in 0..WARMUP + SAMPLES {
            let mut vm = Vm::new().unwrap();
            vm.gc_interval = Some(128);
            let started = Instant::now();
            vm.run(black_box(&program), &mut io::sink()).unwrap();
            let elapsed = started.elapsed().as_secs_f64();
            if sample >= WARMUP {
                samples.push((elapsed, black_box(vm.stats)));
            }
        }
        samples.sort_by(|left, right| left.0.total_cmp(&right.0));
        let (median, stats) = samples[SAMPLES / 2];
        println!(
            "{name},{:.3},{:.3},{:.3},{},{},{},{},{},{},{:.3},{:.3}",
            median * 1e6,
            samples[0].0 * 1e6,
            samples[SAMPLES - 1].0 * 1e6,
            stats.gc_collections,
            stats.gc_minor_collections,
            stats.gc_major_collections,
            stats.gc_promoted,
            stats.gc_reclaimed,
            stats.gc_moved,
            stats.gc_pause_ns as f64 / 1e3,
            stats.gc_max_pause_ns as f64 / 1e3,
        );
    }
}
