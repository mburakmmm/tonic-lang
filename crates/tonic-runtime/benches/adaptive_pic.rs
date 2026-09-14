use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_runtime::Vm;

const WARMUP: usize = 3;
const SAMPLES: usize = 15;

fn main() {
    let cases = [
        (
            "alternating_calls_100000",
            "def add(a,b):\n    return a+b\ndef sub(a,b):\n    return a-b\nf=add\ni=0\ns=0\nwhile i<100000:\n    if i>=10:\n        if i%2==0:\n            f=add\n        else:\n            f=sub\n    s+=f(i,1)\n    i+=1\nprint(s)",
        ),
        (
            "alternating_attrs_100000",
            "class A:\n    pass\nclass B:\n    pass\na=A()\nb=B()\na.x=1\nb.x=2\no=a\ni=0\ns=0\nwhile i<100000:\n    if i>=10:\n        if i%2==0:\n            o=a\n        else:\n            o=b\n    s+=o.x\n    i+=1\nprint(s)",
        ),
        (
            "unrelated_class_mutation_100000",
            "class Target:\n    pass\nclass Noise:\n    pass\nc=Target()\nc.x=1\ni=0\ns=0\nwhile i<100000:\n    Noise.y=i\n    s+=c.x\n    i+=1\nprint(s)",
        ),
    ];
    println!(
        "case,mode,median_us,min_us,max_us,call_quickened,call_pic_promotions,call_misses,attr_quickened,attr_pic_promotions,attr_misses"
    );
    for (name, source) in cases {
        let program = compile(source, "<adaptive-pic-bench>").unwrap();
        for (mode, adaptive) in [("generic", false), ("adaptive", true)] {
            let mut samples = Vec::new();
            for sample in 0..WARMUP + SAMPLES {
                let mut vm = Vm::new().unwrap();
                vm.adaptive_specialization = adaptive;
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
                "{name},{mode},{:.3},{:.3},{:.3},{},{},{},{},{},{}",
                median * 1e6,
                samples[0].0 * 1e6,
                samples[SAMPLES - 1].0 * 1e6,
                stats.call_quickened,
                stats.call_pic_promotions,
                stats.call_cache_misses,
                stats.attr_quickened,
                stats.attr_pic_promotions,
                stats.attr_cache_misses,
            );
        }
    }
}
