use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_runtime::Vm;

const WARMUP: usize = 5;
const SAMPLES: usize = 30;

const CASES: &[(&str, &str, &str)] = &[
    (
        "integer_loop",
        include_str!("../../../benches/comparison/integer_loop.py"),
        "4999950000\n",
    ),
    (
        "fib_calls",
        include_str!("../../../benches/comparison/fib_calls.py"),
        "102334155000\n",
    ),
    (
        "known_calls",
        include_str!("../../../benches/comparison/known_calls.py"),
        "50015000\n",
    ),
    (
        "float_loop",
        include_str!("../../../benches/comparison/float_loop.py"),
        "5000.0\n",
    ),
    (
        "list_iteration",
        include_str!("../../../benches/comparison/list_iteration.py"),
        "20000\n",
    ),
    (
        "closure_calls",
        include_str!("../../../benches/comparison/closure_calls.py"),
        "10000\n",
    ),
    (
        "keyword_calls",
        include_str!("../../../benches/comparison/keyword_calls.py"),
        "50045000\n",
    ),
    (
        "dict_lookup",
        include_str!("../../../benches/comparison/dict_lookup.py"),
        "20000\n",
    ),
    (
        "dict_insert",
        include_str!("../../../benches/comparison/dict_insert.py"),
        "10000\n",
    ),
    (
        "attribute_load",
        include_str!("../../../benches/comparison/attribute_load.py"),
        "10000\n",
    ),
    (
        "bound_method_calls",
        include_str!("../../../benches/comparison/bound_method_calls.py"),
        "50005000\n",
    ),
    (
        "descriptor_load",
        include_str!("../../../benches/comparison/descriptor_load.py"),
        "10000\n",
    ),
    (
        "super_calls",
        include_str!("../../../benches/comparison/super_calls.py"),
        "50015000\n",
    ),
];

fn main() {
    println!("engine,case,phase,sample,seconds");
    for &(name, source, expected) in CASES {
        let program = compile(source, name).expect("comparison source must compile");
        let mut output = Vec::new();
        Vm::new()
            .expect("VM")
            .run(&program, &mut output)
            .expect("comparison source must execute");
        assert_eq!(output, expected.as_bytes(), "checksum mismatch for {name}");

        for sample in 0..WARMUP + SAMPLES {
            let start = Instant::now();
            let compiled = compile(black_box(source), name).expect("compile");
            let seconds = start.elapsed().as_secs_f64();
            black_box(compiled);
            if sample >= WARMUP {
                println!("tonic,{name},compile,{},{seconds:.9}", sample - WARMUP);
            }
        }
        for sample in 0..WARMUP + SAMPLES {
            let mut vm = Vm::new().expect("VM setup is outside timed region");
            let start = Instant::now();
            vm.run(black_box(&program), &mut io::sink()).expect("run");
            let seconds = start.elapsed().as_secs_f64();
            if sample >= WARMUP {
                println!("tonic,{name},warm_run,{},{seconds:.9}", sample - WARMUP);
            }
        }
    }
}
