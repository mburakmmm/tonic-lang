use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_cpython::register;
use tonic_runtime::Vm;

const WARMUP: usize = 3;
const SAMPLES: usize = 15;
const COLLECTIONS: usize = 10_000;

const GRAPH: &str = "import python\ntypes=python.call1('builtins','__import__','types')\nnamespace_type=python.call('builtins','getattr',[types,'SimpleNamespace'],{})\nholder=python.invoke(namespace_type,[],{})\nclass Box:\n    pass\nbox=Box()\npython.call('builtins','setattr',[holder,'proxy',box],{})\nbox.holder=holder";

fn main() {
    let program = compile(GRAPH, "<cross-runtime-gc-bench>").unwrap();
    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..WARMUP + SAMPLES {
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        register(&mut vm).unwrap();
        vm.run(&program, &mut io::sink()).unwrap();
        let started = Instant::now();
        for _ in 0..COLLECTIONS {
            black_box(vm.collect_garbage().unwrap());
        }
        let elapsed = started.elapsed().as_secs_f64();
        if sample >= WARMUP {
            samples.push(elapsed);
        }
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[SAMPLES / 2];
    println!("case,collections,median_ms,min_ms,max_ms,collections_per_second");
    println!(
        "reachable_foreign_graph,{COLLECTIONS},{:.3},{:.3},{:.3},{:.0}",
        median * 1e3,
        samples[0] * 1e3,
        samples[SAMPLES - 1] * 1e3,
        COLLECTIONS as f64 / median,
    );
}
