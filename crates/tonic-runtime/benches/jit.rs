use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_runtime::{ExecutionMode, Stats, Vm};

const WARMUP: usize = 3;
const SAMPLES: usize = 15;

fn main() {
    let cases = [
        (
            "sum_to_1000000",
            "def sum_to(n):\n    total=0\n    i=0\n    while i<n:\n        total+=i\n        i+=1\n    return total\nprint(sum_to(1000000))",
        ),
        (
            "leaf_add_calls_100000",
            "def add(a,b):\n    return a+b\ni=0\ns=0\nwhile i<100000:\n    s=add(s,1)\n    i+=1\nprint(s)",
        ),
        (
            "jit_caller_leaf_add_100000",
            "def add(a,b):\n    return a+b\ndef loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=add(s,1)\n        i+=1\n    return s\nprint(loop(100000))",
        ),
        (
            "leaf_mul_floor_mod_100000",
            "def arithmetic(a,b):\n    return a*b+a//b+a%b\ni=0\ns=0\nwhile i<100000:\n    s+=arithmetic(i+1,7)\n    i+=1\nprint(s)",
        ),
        (
            "leaf_runtime_div_100000",
            "def divide(a,b):\n    x=a/b\n    x=x/1\n    return x/1\ni=0\ns=0.0\nwhile i<100000:\n    s+=divide(i+1,7)\n    i+=1\nprint(s)",
        ),
        (
            "runtime_div_loop_100000",
            "def repeated_div(n):\n    i=1\n    x=0\n    while i<=n:\n        x=i/7\n        i+=1\n    return x\nprint(repeated_div(100000))",
        ),
        (
            "recursive_fib_20",
            "def fib(n):\n    if n<2:\n        return n\n    return fib(n-1)+fib(n-2)\nprint(fib(20))",
        ),
        (
            "unstable_float_add_100000",
            "def add(a,b):\n    return a+b\ni=0\nx=0.0\nwhile i<100000:\n    x=add(x,0.5)\n    i+=1\nprint(x)",
        ),
        (
            "float_add_loop_100000",
            "def accumulate(value,step,n):\n    i=0\n    while i<n:\n        value+=step\n        i+=1\n    return value\nprint(accumulate(0.0,0.5,100000))",
        ),
        (
            "jit_caller_float_chain_100000",
            "def fused(a,b):\n    x=a+b\n    x=x*b\n    return x-b\ndef loop(n):\n    i=0\n    x=1.0\n    while i<n:\n        x=fused(x,1.000001)\n        i+=1\n    return x\nprint(loop(100000))",
        ),
        (
            "instance_attr_load_100000",
            "class C:\n    pass\nc=C()\nc.x=1\ni=0\ns=0\nwhile i<100000:\n    s+=c.x\n    i+=1\nprint(s)",
        ),
    ];
    println!(
        "case,mode,median_us,min_us,max_us,instructions,heap_allocations,jit_compile_us,jit_code_bytes,jit_calls,jit_returns,jit_deopts,jit_despecialized,jit_fallbacks,jit_helper_calls,jit_gc_collections,jit_side_exits,jit_resumes,jit_direct_call_sites,jit_direct_method_sites,jit_direct_calls"
    );
    let mut raw = String::from("case,mode,sample,seconds\n");
    for (name, source) in cases {
        let program = compile(source, "<jit-bench>").unwrap();
        for (mode_name, mode, adaptive) in [
            ("interpreter-generic", ExecutionMode::Interpreter, false),
            ("interpreter-adaptive", ExecutionMode::Interpreter, true),
            ("jit", ExecutionMode::Jit, true),
        ] {
            let mut samples: Vec<(f64, Stats)> = Vec::new();
            for sample in 0..WARMUP + SAMPLES {
                let mut vm = Vm::new().unwrap();
                vm.execution_mode = mode;
                vm.adaptive_specialization = adaptive;
                let started = Instant::now();
                vm.run(black_box(&program), &mut io::sink()).unwrap();
                let seconds = started.elapsed().as_secs_f64();
                if sample >= WARMUP {
                    raw.push_str(&format!(
                        "{name},{mode_name},{},{seconds:.9}\n",
                        sample - WARMUP
                    ));
                    samples.push((seconds, black_box(vm.stats)));
                }
            }
            samples.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (median, stats) = samples[SAMPLES / 2];
            println!(
                "{name},{mode_name},{:.3},{:.3},{:.3},{},{},{:.3},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                median * 1e6,
                samples[0].0 * 1e6,
                samples[SAMPLES - 1].0 * 1e6,
                stats.instructions,
                stats.heap_allocations,
                stats.jit_compile_ns as f64 / 1e3,
                stats.jit_code_bytes,
                stats.jit_calls,
                stats.jit_returns,
                stats.jit_deopts,
                stats.jit_despecialized,
                stats.jit_fallbacks,
                stats.jit_helper_calls,
                stats.jit_gc_collections,
                stats.jit_side_exits,
                stats.jit_resumes,
                stats.jit_direct_call_sites,
                stats.jit_direct_method_sites,
                stats.jit_direct_calls,
            );
        }
    }
    if let Some(path) = std::env::var_os("TONIC_BENCH_RAW") {
        std::fs::write(path, raw).unwrap();
    }
}
