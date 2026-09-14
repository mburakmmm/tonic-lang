use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_cpython::register;
use tonic_runtime::{Stats, Vm};

const WARMUP: usize = 3;
const SAMPLES: usize = 15;
const CALLS: usize = 100_000;

fn measure(source: &str, bridge: bool) -> (f64, f64, f64, Stats) {
    let program = compile(source, "<cpython-bridge-bench>").unwrap();
    let mut samples = Vec::new();
    for sample in 0..WARMUP + SAMPLES {
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        if bridge {
            register(&mut vm).unwrap();
        }
        let started = Instant::now();
        vm.run(black_box(&program), &mut io::sink()).unwrap();
        let elapsed = started.elapsed().as_secs_f64();
        if sample >= WARMUP {
            samples.push((elapsed, vm.stats));
        }
    }
    samples.sort_by(|left, right| left.0.total_cmp(&right.0));
    let (median, stats) = samples[SAMPLES / 2];
    (median, samples[0].0, samples[SAMPLES - 1].0, stats)
}

fn main() {
    let tonic = measure(
        &format!("i=0\nwhile i<{CALLS}:\n    abs(-42)\n    i+=1"),
        false,
    );
    let python = measure(
        &format!("import python\ni=0\nwhile i<{CALLS}:\n    python.abs(-42)\n    i+=1"),
        true,
    );
    let python_named = measure(
        &format!(
            "import python\ni=0\nwhile i<{CALLS}:\n    python.call_int1('builtins','abs',-42)\n    i+=1"
        ),
        true,
    );
    let python_generic = measure(
        &format!(
            "import python\ni=0\nwhile i<{CALLS}:\n    python.call1('builtins','abs',-42)\n    i+=1"
        ),
        true,
    );
    let tonic_callback = measure(
        &format!(
            "import python\ndef identity(value):\n    return value\nproxy=python.proxy(identity)\ni=0\nwhile i<{CALLS}:\n    python.invoke_proxy_int(proxy,42)\n    i+=1"
        ),
        true,
    );
    let python_keyword = measure(
        &format!(
            "import python\nargs=[1.25]\nkwargs={{'ndigits':1}}\ni=0\nwhile i<{CALLS}:\n    python.call('builtins','round',args,kwargs)\n    i+=1"
        ),
        true,
    );
    let tonic_keyword_callback = measure(
        &format!(
            "import python\ndef add(left,right=0):\n    return left+right\nproxy=python.proxy(add)\nargs=[20]\nkwargs={{'right':22}}\ni=0\nwhile i<{CALLS}:\n    python.invoke(proxy,args,kwargs)\n    i+=1"
        ),
        true,
    );
    let proxy_attribute = measure(
        &format!(
            "import python\nclass Box:\n    def __init__(self,value):\n        self.value=value\nbox=Box(42)\nproxy=python.proxy(box)\nargs=[proxy,'value']\nkwargs={{}}\ni=0\nwhile i<{CALLS}:\n    python.call('builtins','getattr',args,kwargs)\n    i+=1"
        ),
        true,
    );
    let cached_proxy_repr = measure(
        &format!(
            "import python\nclass Box:\n    pass\nbox=Box()\nsysmod=python.call1('builtins','__import__','sys')\npython.call('builtins','setattr',[sysmod,'_tonic_bench_proxy',box],{{}})\nargs=[box]\nkwargs={{}}\ni=0\nwhile i<{CALLS}:\n    python.call('builtins','repr',args,kwargs)\n    i+=1\npython.call('builtins','delattr',[sysmod,'_tonic_bench_proxy'],{{}})"
        ),
        true,
    );
    println!("path,calls,median_ms,min_ms,max_ms,calls_per_second,native_calls,guest_allocations");
    for (path, measurement) in [
        ("tonic_builtin", tonic),
        ("cpython_number_abs", python),
        ("cpython_named_call", python_named),
        ("cpython_generic_call1", python_generic),
        ("cpython_tonic_callback", tonic_callback),
        ("cpython_keyword_call", python_keyword),
        ("cpython_tonic_keyword_callback", tonic_keyword_callback),
        ("cpython_proxy_attribute", proxy_attribute),
        ("cpython_cached_proxy_repr", cached_proxy_repr),
    ] {
        let (median, min, max, stats) = measurement;
        println!(
            "{path},{CALLS},{:.3},{:.3},{:.3},{:.0},{},{}",
            median * 1e3,
            min * 1e3,
            max * 1e3,
            CALLS as f64 / median,
            stats.native_calls,
            stats.heap_allocations,
        );
    }
}
