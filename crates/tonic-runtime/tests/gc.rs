use tonic_compiler::compile;
use tonic_runtime::Vm;
fn baseline_objects() -> usize {
    Vm::new().unwrap().collect_garbage().unwrap().survivors
}
fn stress(source: &str) -> (String, Vm) {
    let p = compile(source, "gc").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&p, &mut out).unwrap();
    (String::from_utf8(out).unwrap(), vm)
}
#[test]
fn closures_defaults_and_suspended_caller_roots() {
    let (out,vm)=stress("def maker(x):\n    n=[x]\n    def inc(step=[1]):\n        n[0]+=step[0]\n        return n\n    return inc\nf=maker(4)\nprint(f(),f())\ndef outer(x):\n    return x, maker(10)()\nprint(outer(['keep']))");
    assert_eq!(out, "[6] [6]\n(['keep'], [11])\n");
    assert!(vm.stats.gc_collections > 5);
}
#[test]
fn pending_expansion_roots() {
    let (out,_)=stress("def f(*a,**k):\n    return a,k\ndef mk():\n    return ['inner']\nprint(f(*[['outer']],nested=f(*mk(),**{'x':['value']}),**{'tail':['end']}))");
    assert_eq!(
        out,
        "((['outer'],), {'nested': (('inner',), {'x': ['value']}), 'tail': ['end']})\n"
    );
}
#[test]
fn native_persistent_handle_survives_movement() {
    let mut vm = Vm::new().unwrap();
    let persistent = {
        let mut ctx = vm.context().unwrap();
        for _ in 0..20 {
            ctx.from_str("dead").unwrap();
        }
        let value = ctx.from_str("kept").unwrap();
        ctx.persist(value).unwrap()
    };
    let stats = vm.collect_garbage().unwrap();
    assert!(stats.reclaimed >= 20);
    assert!(stats.moved > 0);
    {
        let mut ctx = vm.context().unwrap();
        let value = ctx.borrow_persistent(&persistent).unwrap();
        assert_eq!(ctx.as_str(value).unwrap(), "kept");
        ctx.release_persistent(&persistent).unwrap();
    }
    assert_eq!(vm.collect_garbage().unwrap().reclaimed, 1);
}
#[test]
fn native_result_and_error_cleanup() {
    let (out, mut vm) = stress("import fastmath\nprint(fastmath.add(20,22))");
    assert_eq!(out, "42\n");
    let bad = compile("import fastmath\nfastmath.add(['bad'],1)", "bad").unwrap();
    assert!(vm.run(&bad, &mut Vec::new()).is_err());
    assert_eq!(vm.active_handles(), 0);
    vm.run(&compile("pass", "empty").unwrap(), &mut Vec::new())
        .unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.live_objects(), baseline_objects());
}
#[test]
fn native_allocated_result_and_failed_scope_roots() {
    use tonic_core::diagnostic::{Diagnostic, Result};
    use tonic_runtime::{Context, Handle};
    fn make(ctx: &mut Context<'_>, _: &[Handle]) -> Result<Handle> {
        for _ in 0..2000 {
            ctx.from_str("temporary")?;
        }
        ctx.from_str("native result")
    }
    fn fail(ctx: &mut Context<'_>, _: &[Handle]) -> Result<Handle> {
        for _ in 0..2000 {
            ctx.from_str("failed temporary")?;
        }
        Err(Diagnostic::new("ValueError", "native failure"))
    }
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    vm.register_native("host", "make", 0, make).unwrap();
    vm.register_native("host", "fail", 0, fail).unwrap();
    let baseline = vm.collect_garbage().unwrap().survivors;
    let mut out = Vec::new();
    vm.run(
        &compile("import host\nprint(host.make())", "native").unwrap(),
        &mut out,
    )
    .unwrap();
    assert_eq!(out, b"native result\n");
    assert!(vm.stats.gc_reclaimed >= 2000);
    assert_eq!(vm.active_handles(), 0);
    let p = compile("import host\nhost.fail()", "native").unwrap();
    assert_eq!(vm.run(&p, &mut out).unwrap_err().kind, "ValueError");
    assert_eq!(vm.active_handles(), 0);
    assert!(vm.collect_garbage().unwrap().reclaimed >= 2000);
    assert!(vm.live_objects() <= baseline + 2);
}
#[test]
fn unreachable_container_and_function_cycles_are_collected() {
    let mut vm = Vm::new().unwrap();
    let p=compile("a=[]\na += (a,)\nd={}\nd['d']=d\ndef outer():\n    def f():\n        return f\n    return f\nf=outer()","cycles").unwrap();
    vm.run(&p, &mut Vec::new()).unwrap();
    let before = vm.live_objects();
    vm.run(&compile("pass", "empty").unwrap(), &mut Vec::new())
        .unwrap();
    let stats = vm.collect_garbage().unwrap();
    assert!(stats.reclaimed >= before - baseline_objects());
    assert_eq!(vm.live_objects(), baseline_objects());
}
#[test]
fn allocation_loop_has_bounded_live_heap() {
    let p = compile(
        "i=0\nx=0.0\nwhile i<10000:\n    x+=0.5\n    i+=1\nprint(x)",
        "loop",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(16);
    let mut out = Vec::new();
    vm.run(&p, &mut out).unwrap();
    assert_eq!(out, b"5000.0\n");
    assert!(vm.stats.gc_reclaimed > 9900);
    // Dead temporary registers can retain one nursery cohort until the next
    // scheduled major collection; the bound includes that deliberate window.
    assert!(
        vm.live_objects() < 80,
        "live objects: {}",
        vm.live_objects()
    );
}

#[test]
fn automatic_collection_tiers_between_minor_and_major() {
    let program = compile(
        "owner=[None]\ni=0\nwhile i<200:\n    owner[0]=[i]\n    i+=1\nprint(owner[0])",
        "gc-tiers",
    )
    .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"[199]\n");
    assert!(vm.stats.gc_minor_collections > 0);
    assert!(vm.stats.gc_major_collections > 0);
    assert_eq!(
        vm.stats.gc_collections,
        vm.stats.gc_minor_collections + vm.stats.gc_major_collections
    );
    assert!(vm.stats.gc_promoted > 0);
}
#[test]
fn mutable_containers_and_iterators_survive_collection() {
    let(out,_)=stress("d={'x':[1],'y':[2]}\nfor key in d:\n    d[key] += [3]\n    print(key,d[key])\na=[]\na += (a,)\nprint(a)");
    assert_eq!(out, "x [1, 3]\ny [2, 3]\n[[...]]\n");
}
