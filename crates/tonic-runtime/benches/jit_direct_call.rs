use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_runtime::{ExecutionMode, Stats, Vm};

const WARMUP: usize = 3;
const SAMPLES: usize = 15;

fn main() {
    println!(
        "case,mode,median_us,min_us,max_us,instructions,jit_compile_us,jit_code_bytes,jit_side_exits,jit_resumes,jit_direct_call_sites,jit_direct_method_sites,jit_direct_calls"
    );
    for (case, source) in [
        (
            "positional_exact",
            "def add(a,b):\n    return a+b\ndef loop(n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,1)\n        i+=1\n    return total\nprint(loop(100000))",
        ),
        (
            "keyword_defaults",
            "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,bias=0)\n        i+=1\n    return total\nprint(loop(100000))",
        ),
        (
            "bound_method",
            "class Counter:\n    def add(self,a,/,b=1):\n        return a+b\ndef loop(counter,n):\n    i=0\n    total=0\n    while i<n:\n        total=counter.add(total,b=1)\n        i+=1\n    return total\nprint(loop(Counter(),100000))",
        ),
        (
            "staticmethod",
            "class Math:\n    @staticmethod\n    def add(a,/,b=1):\n        return a+b\ndef loop(math,n):\n    i=0\n    total=0\n    while i<n:\n        total=math.add(total,b=1)\n        i+=1\n    return total\nprint(loop(Math(),100000))",
        ),
        (
            "classmethod",
            "class Math:\n    @classmethod\n    def add(cls,a,/,b=1):\n        return a+b\ndef loop(math,n):\n    i=0\n    total=0\n    while i<n:\n        total=math.add(total,b=1)\n        i+=1\n    return total\nprint(loop(Math(),100000))",
        ),
        (
            "custom_descriptor",
            "def add(a,/,b=1):\n    return a+b\nclass Forward:\n    def __get__(self,obj,owner):\n        return add\nclass Math:\n    op=Forward()\ndef loop(math,n):\n    i=0\n    total=0\n    while i<n:\n        total=math.op(total,b=1)\n        i+=1\n    return total\nprint(loop(Math(),100000))",
        ),
        (
            "expanded_call",
            "def add(a,b):\n    return a+b\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values)\n        i+=1\n    return total\nprint(loop([1],100000))",
        ),
        (
            "expanded_named",
            "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values,bias=0)\n        i+=1\n    return total\nprint(loop([1],100000))",
        ),
        (
            "expanded_mapping",
            "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(mapping,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,**mapping)\n        i+=1\n    return total\nprint(loop({'b':1,'bias':0},100000))",
        ),
        (
            "unused_variadic",
            "def add(a,b,*rest,**kw):\n    return a+b\ndef loop(n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,1)\n        i+=1\n    return total\nprint(loop(100000))",
        ),
        (
            "observed_varargs",
            "def collect(*args):\n    return args\ndef loop(n):\n    i=0\n    result=None\n    while i<n:\n        result=collect(1,2)\n        i+=1\n    return result\nprint(loop(100000))",
        ),
        (
            "observed_kwargs",
            "def collect(**kw):\n    return kw\ndef loop(n):\n    i=0\n    result=None\n    while i<n:\n        result=collect(x=1,y=2)\n        i+=1\n    return result\nprint(loop(100000))",
        ),
    ] {
        let program = compile(source, "<jit-direct-call-bench>").unwrap();
        for (name, mode, direct_calls) in [
            ("interpreter-adaptive", ExecutionMode::Interpreter, false),
            ("jit-side-exit", ExecutionMode::Jit, false),
            ("jit-direct-call", ExecutionMode::Jit, true),
        ] {
            let mut samples: Vec<(f64, Stats)> = Vec::new();
            for sample in 0..WARMUP + SAMPLES {
                let mut vm = Vm::new().unwrap();
                vm.execution_mode = mode;
                vm.jit_direct_call_inlining = direct_calls;
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
                "{case},{name},{:.3},{:.3},{:.3},{},{:.3},{},{},{},{},{},{}",
                median * 1e6,
                samples[0].0 * 1e6,
                samples[SAMPLES - 1].0 * 1e6,
                stats.instructions,
                stats.jit_compile_ns as f64 / 1e3,
                stats.jit_code_bytes,
                stats.jit_side_exits,
                stats.jit_resumes,
                stats.jit_direct_call_sites,
                stats.jit_direct_method_sites,
                stats.jit_direct_calls,
            );
        }
    }
}
