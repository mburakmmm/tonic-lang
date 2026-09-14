use std::{hint::black_box, io, time::Instant};
use tonic_compiler::compile;
use tonic_runtime::{Stats, Vm};

const WARMUP: usize = 2;
const SAMPLES: usize = 15;

fn main() {
    let cases = [
        ("integer_loop", "i=0\ns=0\nwhile i<100000:\n    s+=i\n    i+=1\n"),
        ("fib_40_x1000", "def fib(n):\n    a=0\n    b=1\n    while n>0:\n        a,b=b,a+b\n        n-=1\n    return a\ni=0\nwhile i<1000:\n    fib(40)\n    i+=1\n"),
        ("known_calls_10000", "def add(a,b):\n    return a+b\ni=0\nwhile i<10000:\n    add(i,2)\n    i+=1\n"),
        ("native_calls_10000", "import fastmath\ni=0\nwhile i<10000:\n    fastmath.add(i,2)\n    i+=1\n"),
        ("float_loop_10000", "i=0\nx=0.0\nwhile i<10000:\n    x+=0.5\n    i+=1\n"),
        ("list_iteration_1000", "i=0\nxs=[1,2,3,4,5]\nwhile i<1000:\n    for x in xs:\n        y=x+1\n    i+=1\n"),
        ("slice_copy_1000", "i=0\nxs=[0,1,2,3,4,5,6,7,8,9]\nwhile i<1000:\n    y=xs[1:9:2]\n    i+=1\n"),
        ("closure_calls_10000", "def counter(n):\n    def inc():\n        nonlocal n\n        n+=1\n        return n\n    return inc\nf=counter(0)\ni=0\nwhile i<10000:\n    f()\n    i+=1\n"),
        ("closure_creation_5000", "def make(n):\n    def f():\n        return n\n    return f\ni=0\nwhile i<5000:\n    f=make(i)\n    f()\n    i+=1\n"),
        ("keyword_calls_10000", "def add(a,/,b=2,*,bias=3):\n    return a+b+bias\ni=0\nwhile i<10000:\n    add(i,b=2,bias=3)\n    i+=1\n"),
        ("variadic_calls_10000", "def f(a,*args,**kw):\n    return a+args[0]+kw['b']\ni=0\nwhile i<10000:\n    f(i,2,b=3)\n    i+=1\n"),
        ("expanded_calls_10000", "def f(a,b,*,c):\n    return a+b+c\nxs=(1,2)\nkw={'c':3}\ni=0\nwhile i<10000:\n    f(*xs,**kw)\n    i+=1\n"),
        ("dict_lookup_10000", "d={'a':1,'b':2,'c':3}\ni=0\ns=0\nwhile i<10000:\n    s+=d['b']\n    i+=1\n"),
        ("dict_insert_10000", "d={}\ni=0\nwhile i<10000:\n    d[i]=i+1\n    i+=1\n"),
        ("cyclic_garbage_10000", "i=0\nwhile i<10000:\n    x=[]\n    x+=(x,)\n    i+=1\n"),
        ("instance_creation_10000", "class C:\n    pass\ni=0\nwhile i<10000:\n    c=C()\n    i+=1\n"),
        ("initialized_instances_10000", "class C:\n    def __init__(self,x):\n        self.x=x\ni=0\nwhile i<10000:\n    c=C(i)\n    i+=1\n"),
        ("attribute_load_10000", "class C:\n    pass\nc=C()\nc.x=1\ni=0\ns=0\nwhile i<10000:\n    s+=c.x\n    i+=1\n"),
        ("attribute_store_10000", "class C:\n    pass\nc=C()\nc.x=0\ni=0\nwhile i<10000:\n    c.x+=1\n    i+=1\n"),
        ("bound_method_calls_10000", "class C:\n    def add(self,x):\n        return x+1\nc=C()\ni=0\nwhile i<10000:\n    c.add(i)\n    i+=1\n"),
        ("saved_method_calls_10000", "class C:\n    def add(self,x):\n        return x+1\nc=C()\nf=c.add\ni=0\nwhile i<10000:\n    f(i)\n    i+=1\n"),
        ("static_method_calls_10000", "class C:\n    @staticmethod\n    def add(x):\n        return x+1\ni=0\nwhile i<10000:\n    C.add(i)\n    i+=1\n"),
        ("class_method_calls_10000", "class C:\n    bias=1\n    @classmethod\n    def add(cls,x):\n        return cls.bias+x\ni=0\nwhile i<10000:\n    C.add(i)\n    i+=1\n"),
        ("property_load_10000", "class C:\n    def __init__(self):\n        self._x=1\n    @property\n    def x(self):\n        return self._x\nc=C()\ni=0\ns=0\nwhile i<10000:\n    s+=c.x\n    i+=1\n"),
        ("property_store_10000", "class C:\n    def __init__(self):\n        self._x=0\n    @property\n    def x(self):\n        return self._x\n    @x.setter\n    def x(self,value):\n        self._x=value\nc=C()\ni=0\nwhile i<10000:\n    c.x=i\n    i+=1\n"),
        ("descriptor_load_10000", "class D:\n    def __get__(self,obj,owner):\n        return obj._x\nclass C:\n    x=D()\nc=C()\nc._x=1\ni=0\ns=0\nwhile i<10000:\n    s+=c.x\n    i+=1\n"),
        ("descriptor_store_10000", "class D:\n    def __set__(self,obj,value):\n        obj._x=value\nclass C:\n    x=D()\nc=C()\nc._x=0\ni=0\nwhile i<10000:\n    c.x=i\n    i+=1\n"),
        ("inherited_attribute_10000", "class A:\n    x=1\nclass B(A):\n    pass\nc=B()\ni=0\ns=0\nwhile i<10000:\n    s+=c.x\n    i+=1\n"),
        ("dictionary_attribute_10000", "class C:\n    pass\nc=C()\nc.x=1\nname='p'\nfor i in range(70):\n    setattr(c,name,i)\n    name+='p'\ni=0\ns=0\nwhile i<10000:\n    s+=c.x\n    i+=1\n"),
    ];
    let mut raw = String::from("case,phase,gc,sample,seconds,gc_pause_ns,gc_max_pause_ns\n");
    println!("case,gc,compile_median_us,median_us,min_us,max_us,dispatches,dispatches_per_s,guest_allocations,estimated_heap_bytes,peak_registers,bytecode_bytes,gc_collections,gc_reclaimed,gc_moved,resident_objects,peak_heap_bytes,gc_pause_median_us,gc_pause_max_us");
    for (name, source) in cases {
        let mut compile_times = Vec::new();
        for sample in 0..WARMUP + SAMPLES {
            let start = Instant::now();
            let code = compile(black_box(source), "<bench>").unwrap();
            let seconds = start.elapsed().as_secs_f64();
            black_box(&code);
            if sample >= WARMUP {
                compile_times.push(seconds);
                raw.push_str(&format!(
                    "{name},parse_compile_verify,na,{},{seconds:.9},0,0\n",
                    sample - WARMUP
                ));
            }
        }
        compile_times.sort_by(f64::total_cmp);
        let program = compile(source, "<bench>").unwrap();
        let code_bytes: usize = program
            .program()
            .code
            .iter()
            .map(|c| c.instructions.len() * 8)
            .sum();
        for (mode, interval) in [("default", Some(1024)), ("disabled", None)] {
            let mut samples: Vec<(f64, Stats)> = Vec::new();
            for sample in 0..WARMUP + SAMPLES {
                // Fresh VM per sample; parse/compile/verify, VM creation and Drop
                // are outside this timer. Vm::run setup and GC are included.
                let mut vm = Vm::new().unwrap();
                vm.gc_interval = interval;
                let start = Instant::now();
                vm.run(black_box(&program), &mut io::sink()).unwrap();
                let seconds = start.elapsed().as_secs_f64();
                if sample >= WARMUP {
                    raw.push_str(&format!(
                        "{name},run,{mode},{},{seconds:.9},{},{}\n",
                        sample - WARMUP,
                        vm.stats.gc_pause_ns,
                        vm.stats.gc_max_pause_ns
                    ));
                    samples.push((seconds, black_box(vm.stats)));
                }
            }
            samples.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (median, s) = samples[SAMPLES / 2];
            let mut pauses: Vec<_> = samples.iter().map(|(_, s)| s.gc_pause_ns).collect();
            pauses.sort_unstable();
            let max_pause = samples
                .iter()
                .map(|(_, s)| s.gc_max_pause_ns)
                .max()
                .unwrap();
            println!("{name},{mode},{:.3},{:.3},{:.3},{:.3},{},{:.0},{},{},{},{},{},{},{},{},{},{:.3},{:.3}",
                compile_times[SAMPLES/2]*1e6, median*1e6, samples[0].0*1e6, samples[SAMPLES-1].0*1e6,
                s.instructions,s.instructions as f64/median,s.heap_allocations,s.estimated_heap_bytes,
                s.peak_registers,code_bytes,s.gc_collections,s.gc_reclaimed,s.gc_moved,s.live_objects,
                s.peak_heap_bytes,pauses[SAMPLES/2] as f64/1e3,max_pause as f64/1e3);
        }
    }
    let mut times = Vec::new();
    for sample in 0..WARMUP + SAMPLES {
        let mut vm = Vm::new().unwrap();
        let start = Instant::now();
        for _ in 0..100000 {
            let mut ctx = vm.context().unwrap();
            let h = ctx.from_i64(black_box(42)).unwrap();
            black_box(ctx.to_i64(h).unwrap());
        }
        let seconds = start.elapsed().as_secs_f64();
        assert_eq!(vm.active_handles(), 0);
        if sample >= WARMUP {
            times.push(seconds);
            raw.push_str(&format!(
                "native_scope_100000,native_scope,na,{},{seconds:.9},0,0\n",
                sample - WARMUP
            ));
        }
    }
    times.sort_by(f64::total_cmp);
    eprintln!("native_scope_create_resolve_drop_100000: median {:.3} ms, min {:.3}, max {:.3}; active_handles=0",
        times[SAMPLES/2]*1e3,times[0]*1e3,times[SAMPLES-1]*1e3);
    if let Some(path) = std::env::var_os("TONIC_BENCH_RAW") {
        std::fs::write(path, raw).unwrap();
    }
}
