use tonic_compiler::compile;
use tonic_core::diagnostic::{Diagnostic, Result};
use tonic_runtime::{Context, ExecutionMode, Handle, Vm};
fn output(source: &str) -> String {
    let p = compile(source, "test.tonic").unwrap();
    let mut out = Vec::new();
    Vm::new().unwrap().run(&p, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}
fn error(source: &str) -> Diagnostic {
    let p = compile(source, "test.tonic").unwrap();
    Vm::new().unwrap().run(&p, &mut Vec::new()).unwrap_err()
}
#[test]
fn fib() {
    assert_eq!(
        output(include_str!("../../../examples/fib.tonic")),
        "102334155\n"
    );
}
#[test]
fn native_module() {
    assert_eq!(
        output(include_str!("../../../examples/fastmath.tonic")),
        "42\n"
    );
}
#[test]
fn integers_promote_without_overflow() {
    assert_eq!(
        output("x = 1152921504606846975\nprint(x+1, x*x, -x*3)\n"),
        "1152921504606846976 1329227995784915870597964051066650625 -3458764513820540925\n"
    );
}
#[test]
fn calls_recursion_and_scopes() {
    assert_eq!(output("def f(n):\n    if n <= 1:\n        return 1\n    return n*f(n-1)\nprint(f(20))\nx=1\ndef g():\n    return x\nx=8\nprint(g())\n"),"2432902008176640000\n8\n");
}
#[test]
fn local_binding_is_whole_function() {
    assert_eq!(
        error("x=3\ndef f():\n    print(x)\n    x=4\nf()\n").kind,
        "UnboundLocalError"
    );
    assert_eq!(
        error("def f():\n    if False:\n        x=1\n    return x\nf()\n").kind,
        "UnboundLocalError"
    );
}
#[test]
fn builtin_rebinding_and_function_aliases() {
    assert_eq!(
        output("def f(x):\n    return x+10\np = print\nprint = f\np(print(2))\n"),
        "12\n"
    );
}
#[test]
fn short_circuit_and_chains() {
    assert_eq!(output("def tick(n):\n    print(n)\n    return n\nprint(3 < tick(2) < tick(1))\nprint(0 and missing, 5 or missing, not [])\nprint(tick(1) if True else missing)\n"),"2\nFalse\n0 5 True\n1\n1\n");
}
#[test]
fn loop_else_break_continue_nested() {
    assert_eq!(output("for i in range(3):\n    for j in range(2):\n        break\n    if i==1:\n        continue\n    print(i)\nelse:\n    print('done')\nwhile True:\n    break\nelse:\n    print('bad')\n"),"0\n2\ndone\n");
}
#[test]
fn loop_rebinding_does_not_change_iterator() {
    assert_eq!(
        output("xs=[1,2,3]\nfor x in xs:\n    xs=[9]\n    print(x)\n"),
        "1\n2\n3\n"
    );
}
#[test]
fn unpack_and_evaluation_order() {
    assert_eq!(output("a,b=1,2\na,b=b,a\nprint(a,b)\na,(b,c)=[1,[2,3]]\nprint(a,b,c)\na,b='é字'\nprint(a,b)\n"),"2 1\n1 2 3\né 字\n");
    assert_eq!(error("a,b=[1]\n").kind, "ValueError");
    assert_eq!(error("a,b=[1,2,3]\n").kind, "ValueError");
}
#[test]
fn indexing_and_iteration() {
    assert_eq!(output("print([10,20][-1], 'é字'[1], len('é字'))\nfor i in range(5,-1,-2):\n    print(i)\nprint(range(2,9,2)[-1])\n"),"20 字 2\n5\n3\n1\n8\n");
    assert_eq!(error("print([1][2])").kind, "IndexError");
    assert_eq!(error("range(0,1,0)").kind, "ValueError");
}
#[test]
fn list_tuple_and_unicode_slicing() {
    assert_eq!(
        output(
            "xs=[0,1,2,3,4]\nprint(xs[:],xs[1:4],xs[::-1],xs[4:0:-2])\nprint((0,1,2,3)[-3:-1])\nprint('aé字z'[::-2])\nprint(xs[-999999999999999999999999999999:],xs[:999999999999999999999999999999],xs[::-999999999999999999999999999999])"
        ),
        "[0, 1, 2, 3, 4] [1, 2, 3] [4, 3, 2, 1, 0] [4, 2]\n(1, 2)\nzé\n[0, 1, 2, 3, 4] [0, 1, 2, 3, 4] [4]\n"
    );
    assert_eq!(error("[1,2][::0]").kind, "ValueError");
    assert_eq!(error("[1,2][::1.0]").kind, "TypeError");
}
#[test]
fn negative_division() {
    assert_eq!(
        output("print(-7//3,-7%3,7//-3,7%-3,-7//-3,-7%-3)\nprint(True+True, False==0)\n"),
        "-3 2 -3 -2 2 -1\n2 True\n"
    );
}
#[test]
fn floats_and_exact_mixed_comparisons() {
    assert_eq!(output("print(1.5+2.5, -7.0//3.0, -7.0%3.0)\nprint(9007199254740993 == 9007199254740992.0, 9007199254740993 > 9007199254740992.0)\n"),"4.0 -3.0 2.0\nFalse True\n");
}
#[test]
fn runtime_errors_have_spans() {
    let e = error("def f():\n    return 1//0\nf()\n");
    assert_eq!(e.kind, "ZeroDivisionError");
    assert_eq!(e.trace.len(), 2);
    assert!(e
        .render("test.tonic", "def f():\n    return 1//0\nf()\n")
        .contains("test.tonic:2:"));
}
#[test]
fn arity_name_type_import_errors() {
    for (src, kind) in [
        ("def f(x):\n    return x\nf()", "TypeError"),
        ("missing", "NameError"),
        ("1+'a'", "TypeError"),
        ("import missing", "ModuleNotFoundError"),
        ("import fastmath\nfastmath.missing", "AttributeError"),
        ("import fastmath\nfastmath.add(1)", "TypeError"),
    ] {
        assert_eq!(error(src).kind, kind);
    }
}
#[test]
fn fuel_and_recursion_limits() {
    let mut vm = Vm::new().unwrap();
    vm.limits.instructions = Some(100);
    assert_eq!(
        vm.run(
            &compile("while True:\n    pass", "x").unwrap(),
            &mut Vec::new()
        )
        .unwrap_err()
        .kind,
        "ResourceError"
    );
    vm.limits.instructions = None;
    vm.limits.frames = 16;
    assert_eq!(
        vm.run(
            &compile("def f():\n    return f()\nf()", "x").unwrap(),
            &mut Vec::new()
        )
        .unwrap_err()
        .kind,
        "RecursionError"
    );
    let mut out = Vec::new();
    vm.run(&compile("print(42)", "x").unwrap(), &mut out)
        .unwrap();
    assert_eq!(out, b"42\n");
}
#[test]
fn native_exception_cleans_local_handles() {
    let mut vm = Vm::new().unwrap();
    let p = compile("import fastmath\nfastmath.add('bad',2)", "x").unwrap();
    assert_eq!(vm.run(&p, &mut Vec::new()).unwrap_err().kind, "TypeError");
    assert_eq!(vm.active_handles(), 0);
}
#[test]
fn native_registration() {
    fn twice(ctx: &mut Context<'_>, args: &[Handle]) -> Result<Handle> {
        let n = ctx.to_i64(args[0])?;
        ctx.from_i64(n * 2)
    }
    let mut vm = Vm::new().unwrap();
    vm.register_native("demo", "twice", 1, twice).unwrap();
    let p = compile("import demo as d\nprint(d.twice(21))", "x").unwrap();
    let mut out = Vec::new();
    vm.run(&p, &mut out).unwrap();
    assert_eq!(out, b"42\n");
    assert_eq!(vm.stats.native_calls, 1);
    assert_eq!(vm.active_handles(), 0);
}
#[test]
fn small_integer_loop_has_no_per_iteration_heap_allocations() {
    let p = compile("i=0\ntotal=0\nwhile i<10000:\n    total+=i\n    i+=1", "x").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.run(&p, &mut Vec::new()).unwrap();
    assert_eq!(vm.stats.heap_allocations, 0);
    assert_eq!(vm.stats.backedges, 10000);
}

#[test]
fn cranelift_leaf_loop_returns_and_guard_deopts_through_normal_frames() {
    let source = "def sum_to(n):\n    total=0\n    i=0\n    while i<n:\n        total+=i\n        i+=1\n    return total\ndef add(a,b):\n    return a+b\ni=0\nwhile i<7:\n    add(i,2)\n    i+=1\nprint(sum_to(100),add(1.5,2),add(1152921504606846975,1))";
    let program = compile(source, "jit-integration").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"4950 3.5 1152921504606846976\n");
    assert_eq!(vm.stats.jit_compiled, 2);
    assert_eq!(vm.stats.jit_calls, 3);
    assert_eq!(vm.stats.jit_returns, 1);
    assert_eq!(vm.stats.jit_deopts, 2);
    assert_eq!(vm.stats.jit_deferred, 8);
    assert_eq!(vm.stats.jit_osr_entries, 1);
    assert!(vm.stats.jit_code_bytes > 0);
    assert!(vm.stats.jit_compile_ns > 0);
}

#[test]
fn cold_loop_stays_adaptive_below_osr_threshold() {
    let source = "def sum_to(n):\n    total=0\n    i=0\n    while i<n:\n        total+=i\n        i+=1\n    return total\nprint(sum_to(10))";
    let program = compile(source, "jit-cold-loop").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"45\n");
    assert_eq!(vm.stats.jit_compiled, 0);
    assert_eq!(vm.stats.jit_osr_entries, 0);
    assert_eq!(vm.stats.jit_deferred, 1);
}

#[test]
fn cranelift_integer_mul_floor_div_and_mod_match_python_semantics() {
    let source = "def arithmetic(a,b):\n    return a*b, a//b, a%b\nprint(arithmetic(-7,3))\nprint(arithmetic(7,-3))";
    let program = compile(source, "jit-integer-ops").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"(-21, -3, 2)\n(-21, -3, -2)\n");
    assert_eq!(vm.stats.jit_compiled, 0);
    assert_eq!(vm.stats.jit_fallbacks, 1);

    let source = "def arithmetic(a,b):\n    return a*b + a//b + a%b\nprint(arithmetic(-7,3))\nprint(arithmetic(7,-3))";
    let program = compile(source, "jit-integer-ops-leaf").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"-22\n-26\n");
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_calls, 2);
    assert_eq!(vm.stats.jit_returns, 2);
}

#[test]
fn cranelift_runtime_division_survives_collecting_safepoints() {
    let source = "def divide(a,b,c):\n    first=a/b\n    return first/c\nprint(divide(20,2,4))";
    let program = compile(source, "jit-runtime-helper").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"2.5\n");
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_returns, 1);
    assert_eq!(vm.stats.jit_helper_calls, 2);
    assert_eq!(vm.stats.jit_safepoints, 2);
    assert!(vm.stats.jit_gc_collections > 0);
}

#[test]
fn cranelift_float_loop_stays_unboxed_through_poll_safepoints() {
    let source = "def accumulate(value,step,n):\n    i=0\n    while i<n:\n        value+=step\n        i+=1\n    return value\nprint(accumulate(0.0,0.5,5000))";
    let program = compile(source, "jit-float-loop").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_osr_threshold = 2;
    vm.jit_min_instructions = 0;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"2500.0\n");
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_osr_entries, 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_helper_calls >= 4900);
    assert!(vm.stats.heap_allocations < 100);
}

#[test]
fn cranelift_keeps_profiled_float_leaf_unboxed_until_return() {
    let source = "def fused(a,b):\n    x=a+b\n    x=x*b\n    return x-b\ndef loop(n):\n    i=0\n    x=1.0\n    while i<n:\n        x=fused(x,1.000001)\n        i+=1\n    return x\nprint(loop(5000))";
    let program = compile(source, "jit-direct-float").unwrap();

    let mut generic = Vm::new().unwrap();
    generic.execution_mode = ExecutionMode::Jit;
    generic.jit_direct_call_inlining = false;
    generic.gc_interval = Some(1);
    let mut generic_out = Vec::new();
    generic.run(&program, &mut generic_out).unwrap();

    let mut direct = Vm::new().unwrap();
    direct.execution_mode = ExecutionMode::Jit;
    direct.gc_interval = Some(1);
    let mut direct_out = Vec::new();
    direct.run(&program, &mut direct_out).unwrap();

    assert_eq!(direct_out, generic_out);
    assert_eq!(direct.stats.jit_direct_call_sites, 1);
    assert!(direct.stats.jit_direct_calls >= 4_900);
    assert_eq!(direct.stats.jit_deopts, 0);
    assert_eq!(direct.stats.jit_side_exits, 0);
    assert!(generic.stats.heap_allocations >= direct.stats.heap_allocations + 9_500);
    assert!(direct.stats.jit_gc_collections > 0);
}

#[test]
fn cranelift_float_leaf_guard_deopts_before_mixed_type_side_effects() {
    let source = "def fused(a,b):\n    x=a+b\n    x=x*b\n    return x-b\ndef apply(x,step,n):\n    i=0\n    while i<n:\n        x=fused(x,step)\n        i+=1\n    return x\nprint(apply(1.0,1.000001,5000))\nprint(apply(1,2,10))";
    let program = compile(source, "jit-direct-float-guard").unwrap();

    let mut interpreter = Vm::new().unwrap();
    let mut expected = Vec::new();
    interpreter.run(&program, &mut expected).unwrap();

    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut actual = Vec::new();
    vm.run(&program, &mut actual).unwrap();

    assert_eq!(actual, expected);
    assert!(actual.ends_with(b"3070\n"));
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_float_leaf_matches_ieee_edge_results_under_stress_gc() {
    let source = "def mul(a,b):\n    return a*b\ndef sub(a,b):\n    return a-b\ndef mul_loop(x,step,n):\n    i=0\n    while i<n:\n        x=mul(x,step)\n        i+=1\n    return x\ndef sub_loop(x,step,n):\n    i=0\n    while i<n:\n        x=sub(x,step)\n        i+=1\n    return x\ninf=1e308*1e308\nprint(mul_loop(inf,2.0,500))\nprint(sub_loop(inf,inf,500))\nprint(mul_loop(-0.0,1.0,500))";
    let program = compile(source, "jit-direct-float-ieee").unwrap();

    let mut interpreter = Vm::new().unwrap();
    let mut expected = Vec::new();
    interpreter.run(&program, &mut expected).unwrap();

    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut actual = Vec::new();
    vm.run(&program, &mut actual).unwrap();

    assert_eq!(actual, expected);
    assert_eq!(actual, b"inf\nnan\n-0.0\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 2);
    assert!(vm.stats.jit_direct_calls >= 1_300);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_gc_collections > 0);
}

#[test]
fn cranelift_inlines_profiled_exact_callee_integer_leaf() {
    let source = "def add(a,b):\n    return a+b\ndef loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=add(s,1)\n        i+=1\n    return s\nprint(loop(5000))";
    let program = compile(source, "jit-direct-call").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert_eq!(vm.stats.calls, 5_002);
    assert_eq!(vm.stats.jit_direct_calls, 4_937);
    assert!(vm.stats.gc_collections > 0);
    assert!(vm.stats.instructions < 2_000);
}

#[test]
fn cranelift_inlines_keyword_and_default_bound_integer_leaf() {
    let source = "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=add(s,bias=0)\n        i+=1\n    return s\nprint(loop(5000))";
    let program = compile(source, "jit-direct-call-binding").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert_eq!(vm.stats.calls, 5_002);
    assert_eq!(vm.stats.jit_direct_calls, 4_937);
    assert!(vm.stats.gc_collections > 0);
    assert!(vm.stats.instructions < 2_000);
}

#[test]
fn cranelift_fuses_plain_bound_method_lookup_and_leaf_call() {
    let source = "class Counter:\n    def add(self,a,/,b=1):\n        return a+b\ndef loop(counter,n):\n    i=0\n    s=0\n    while i<n:\n        s=counter.add(s,b=1)\n        i+=1\n    return s\ncounter=Counter()\nprint(loop(counter,5000))\ndef replacement(a,/,b=1):\n    return a+b+b\ncounter.add=replacement\nprint(loop(counter,10))";
    let program = compile(source, "jit-direct-method").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n20\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert!(vm.stats.jit_deopts >= 1);
    assert!(vm.stats.gc_collections > 0);
}

#[test]
fn cranelift_direct_method_guard_observes_class_rebinding() {
    let source = "class Counter:\n    def add(self,a,/,b=1):\n        return a+b\ndef replacement(self,a,/,b=1):\n    return a+b+b\ndef loop(counter,n):\n    i=0\n    s=0\n    while i<n:\n        s=counter.add(s,b=1)\n        i+=1\n    return s\ncounter=Counter()\nprint(loop(counter,5000))\nCounter.add=replacement\nprint(loop(counter,10))";
    let program = compile(source, "jit-direct-method-rebind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n20\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_direct_method_binds_receiver_without_hot_bound_method_allocations() {
    let source = "class Token:\n    def identity(self):\n        return self\ndef loop(token,n):\n    i=0\n    result=None\n    while i<n:\n        result=token.identity()\n        i+=1\n    return result\ntoken=Token()\nprint(isinstance(loop(token,5000),Token))";
    let program = compile(source, "jit-direct-method-receiver").unwrap();

    let mut generic = Vm::new().unwrap();
    generic.execution_mode = ExecutionMode::Jit;
    generic.jit_direct_call_inlining = false;
    let mut generic_out = Vec::new();
    generic.run(&program, &mut generic_out).unwrap();

    let mut direct = Vm::new().unwrap();
    direct.execution_mode = ExecutionMode::Jit;
    let mut direct_out = Vec::new();
    direct.run(&program, &mut direct_out).unwrap();

    assert_eq!(generic_out, b"True\n");
    assert_eq!(direct_out, generic_out);
    assert_eq!(direct.stats.jit_direct_method_sites, 1);
    assert!(direct.stats.jit_direct_calls >= 4_900);
    assert!(generic.stats.heap_allocations >= direct.stats.heap_allocations + 4_800);
}

#[test]
fn cranelift_fuses_staticmethod_without_binding_receiver() {
    let source = "class Math:\n    @staticmethod\n    def add(a,/,b=1):\n        return a+b\ndef loop(math,n):\n    i=0\n    s=0\n    while i<n:\n        s=math.add(s,b=1)\n        i+=1\n    return s\nprint(loop(Math(),5000))";
    let program = compile(source, "jit-direct-staticmethod").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert!(vm.stats.jit_helper_calls < 50);
}

#[test]
fn cranelift_method_entry_cache_guards_owner_changes() {
    let source = "class Counter:\n    def add(self,a,b):\n        return a+b\ndef loop(first,second,n):\n    i=0\n    total=0\n    while i<n:\n        if i%2==0:\n            owner=first\n        else:\n            owner=second\n        total=owner.add(total,1)\n        i+=1\n    return total\nprint(loop(Counter(),Counter(),5000))";
    let program = compile(source, "jit-method-owner-guard").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_staticmethod_guard_includes_binding_kind() {
    let source = "class Math:\n    @staticmethod\n    def add(a,/,b=1):\n        return a+b\ndef loop(math,n):\n    i=0\n    s=0\n    while i<n:\n        s=math.add(s,b=1)\n        i+=1\n    return s\nmath=Math()\nraw=Math.add\nprint(loop(math,5000))\nMath.add=raw\nloop(math,1)";
    let program = compile(source, "jit-direct-staticmethod-kind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    let error = vm.run(&program, &mut out).unwrap_err();
    assert_eq!(out, b"5000\n");
    assert_eq!(error.kind, "TypeError");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_fuses_classmethod_with_dynamic_subclass_receiver() {
    let source = "class Base:\n    @classmethod\n    def identity(cls):\n        return cls\nclass Sub(Base):\n    marker=1\ndef loop(value,n):\n    i=0\n    result=None\n    while i<n:\n        result=value.identity()\n        i+=1\n    return result\nprint(loop(Sub(),5000).__name__)";
    let program = compile(source, "jit-direct-classmethod").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"Sub\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);
}

#[test]
fn cranelift_classmethod_guard_includes_binding_kind() {
    let source = "class Base:\n    @classmethod\n    def identity(cls):\n        return cls\ndef loop(value,n):\n    i=0\n    result=None\n    while i<n:\n        result=value.identity()\n        i+=1\n    return result\nvalue=Base()\nraw=Base.identity.__func__\nprint(loop(value,5000).__name__)\nBase.identity=raw\nprint(isinstance(loop(value,1),Base))";
    let program = compile(source, "jit-direct-classmethod-kind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"Base\nTrue\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_fuses_class_level_function_and_classmethod_access() {
    let source = "class Math:\n    def add(a,/,b=1):\n        return a+b\nclass Base:\n    @classmethod\n    def identity(cls):\n        return cls\nclass Sub(Base):\n    marker=1\ndef sum_loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=Math.add(s,b=1)\n        i+=1\n    return s\ndef class_loop(n):\n    i=0\n    result=None\n    while i<n:\n        result=Sub.identity()\n        i+=1\n    return result\nprint(sum_loop(5000))\nprint(class_loop(5000).__name__)";
    let program = compile(source, "jit-direct-class-access").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\nSub\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 2);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_direct_calls >= 9_800);
}

#[test]
fn cranelift_resumes_custom_descriptor_then_inlines_returned_leaf() {
    let source = "def add(a,/,b=1):\n    return a+b\nclass Forward:\n    def __get__(self,obj,owner):\n        return add\nclass Math:\n    op=Forward()\ndef loop(math,n):\n    i=0\n    total=0\n    while i<n:\n        total=math.op(total,b=1)\n        i+=1\n    return total\nprint(loop(Math(),5000))";
    let program = compile(source, "jit-custom-descriptor").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_side_exits >= 4_900);
    assert!(vm.stats.jit_resumes >= 4_900);
    assert!(vm.stats.jit_direct_calls >= 4_900);
}

#[test]
fn cranelift_custom_descriptor_result_change_deopts_at_call() {
    let source = "def add(a,b):\n    return a+b\ndef other(a,b):\n    return a+b+b\ntarget=add\nclass Forward:\n    def __get__(self,obj,owner):\n        return target\nclass Math:\n    op=Forward()\ndef loop(math,n):\n    i=0\n    total=0\n    while i<n:\n        total=math.op(total,1)\n        i+=1\n    return total\nmath=Math()\nprint(loop(math,5000))\ntarget=other\nprint(loop(math,1))";
    let program = compile(source, "jit-custom-descriptor-rebind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n2\n");
    assert_eq!(vm.stats.jit_direct_method_sites, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_inlines_flat_expanded_call_under_stress_gc() {
    let source = "def add(a,b):\n    return a+b\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values)\n        i+=1\n    return total\nprint(loop([1],5000))";
    let program = compile(source, "jit-expanded-call").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert!(vm.stats.jit_compiled >= 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_resumes, 0);
}

#[test]
fn cranelift_expanded_sequence_guard_observes_items_and_length_changes() {
    let source = "def add(a,b,c):\n    return a+b+c\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values)\n        i+=1\n    return total\nvalues=[1,0]\nprint(loop(values,5000))\nvalues[0]=2\nprint(loop(values,5000))";
    let program = compile(source, "jit-expanded-item-mutation").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n10000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 4_800);
    assert_eq!(vm.stats.jit_deopts, 0);

    let source = "def add(a,b):\n    return a+b\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values)\n        i+=1\n    return total\nprint(loop([1],5000))\nprint(loop([1,2],5000))";
    let program = compile(source, "jit-expanded-length-guard").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "TypeError");
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_defers_non_loop_expansion_until_profile_is_ready() {
    let source = "def add(a,b):\n    return a+b\ndef invoke(values):\n    return add(1,*values)\ni=0\ntotal=0\nwhile i<20:\n    total+=invoke([1])\n    i+=1\nprint(total)";
    let program = compile(source, "jit-expanded-entry-profile").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"40\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 10);
    assert_eq!(vm.stats.jit_side_exits, 0);
}

#[test]
fn cranelift_binds_named_and_default_slots_in_expanded_direct_call() {
    let source = "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(values,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*values,bias=0)\n        i+=1\n    return total\nprint(loop([1],5000))";
    let program = compile(source, "jit-expanded-named").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 4_900);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
}

#[test]
fn cranelift_binds_mapping_values_in_expanded_direct_call() {
    let source = "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(mapping,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,**mapping)\n        i+=1\n    return total\nmapping={'b':1,'bias':0}\nprint(loop(mapping,5000))\nmapping['bias']=1\nprint(loop(mapping,5000))";
    let program = compile(source, "jit-expanded-mapping").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n10000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 9_800);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
}

#[test]
fn cranelift_mapping_guard_deopts_when_key_set_changes() {
    let source = "def add(a,/,b=1,*,bias=0):\n    return a+b+bias\ndef loop(mapping,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,b=1,**mapping)\n        i+=1\n    return total\nprint(loop({'bias':0},5000))\nprint(loop({'bias':0,'other':1},5000))";
    let program = compile(source, "jit-expanded-mapping-keys").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "TypeError");
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_expanded_segment_waits_for_matching_nested_builder() {
    let source = "def pack(*args,**kw):\n    return 0\ndef add(a,b,**kw):\n    return a+b\ndef loop(one,inner,extra,n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,*one,x=pack(*inner,y=3),**extra)\n        i+=1\n    return total\nprint(loop([1],[2],{'z':4},5000))";
    let program = compile(source, "jit-expanded-nested").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert!(vm.stats.jit_compiled >= 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_side_exits >= 4_900);
    assert!(vm.stats.jit_side_exits < 5_100);
    assert!(vm.stats.jit_resumes >= 4_900);
}

#[test]
fn cranelift_omits_unobserved_and_materializes_observed_variadics() {
    let source = "def add(a,b,*rest,**kw):\n    return a+b\ndef loop(n):\n    i=0\n    total=0\n    while i<n:\n        total=add(total,1)\n        i+=1\n    return total\nprint(loop(5000))";
    let program = compile(source, "jit-unused-variadic").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"5000\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_direct_calls >= 4_900);

    let source = "def reveal(a,*rest,**kw):\n    return rest\ndef loop(n):\n    i=0\n    result=None\n    while i<n:\n        result=reveal(1,2,3)\n        i+=1\n    return result\nprint(loop(100))";
    let program = compile(source, "jit-observed-variadic").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"(2, 3)\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_direct_calls >= 20);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
}

#[test]
fn cranelift_materializes_observed_variadic_parameters_in_direct_calls() {
    let source = "def collect_args(a,*args,b=0):\n    return args\ndef collect_kw(a=0,/,*,b=0,**kw):\n    return kw\ndef loop_args(n):\n    i=0\n    result=None\n    while i<n:\n        result=collect_args(0,1,2,b=9)\n        i+=1\n    return result\ndef loop_kw(n):\n    i=0\n    result=None\n    while i<n:\n        result=collect_kw(b=9,a=1,y=2)\n        i+=1\n    return result\nprint(loop_args(5000))\nprint(loop_kw(5000))";
    let program = compile(source, "jit-materialized-variadics").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"(1, 2)\n{'a': 1, 'y': 2}\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 2);
    assert!(vm.stats.jit_direct_calls >= 9_800);
    assert_eq!(vm.stats.jit_side_exits, 0);
    assert_eq!(vm.stats.jit_deopts, 0);
    assert!(vm.stats.jit_gc_collections >= 9_800);
}

#[test]
fn cranelift_direct_call_guards_callee_and_argument_tags() {
    let source = "def add(a,b):\n    return a+b\ndef other(a,b):\n    return a+b+b\ndef loop(n):\n    i=0\n    s=0\n    while i<n:\n        s=add(s,1)\n        i+=1\n    return s\nprint(loop(200))\nadd=other\nprint(loop(10))";
    let program = compile(source, "jit-direct-call-rebind").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"200\n20\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert_eq!(vm.stats.jit_deopts, 1);
    assert_eq!(vm.stats.call_cache_misses, 1);

    let source = "def add(a,b):\n    return a+b\ndef apply(a,b):\n    return add(a,b)\ni=0\nwhile i<10:\n    apply(i,1)\n    i+=1\nprint(apply(1.5,2.5))";
    let program = compile(source, "jit-direct-call-types").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"4.0\n");
    assert_eq!(vm.stats.jit_direct_call_sites, 1);
    assert!(vm.stats.jit_deopts >= 1);
}

#[test]
fn cranelift_runtime_error_preserves_kind_and_source_span() {
    let source = "def divide(a,b):\n    return a/b\nprint(divide(1,0))";
    let program = compile(source, "jit-runtime-error").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "ZeroDivisionError");
    assert_eq!(error.span, Some(program.program().code[1].spans[2]));
    assert_eq!(vm.stats.jit_helper_calls, 1);
    assert_eq!(vm.stats.jit_runtime_errors, 1);
}

#[test]
fn cranelift_recursive_calls_resume_through_vm_frames_and_rebinding() {
    let source = "def recur(n):\n    if n<1:\n        return 0\n    return target(n-1)\ntarget=recur\ni=0\nwhile i<8:\n    recur(2)\n    i+=1\ndef replacement(n):\n    return 40+n\ntarget=replacement\nprint(recur(2))";
    let program = compile(source, "jit-recursive-resume").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    vm.gc_interval = Some(1);
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"41\n");
    assert!(vm.stats.jit_compiled >= 2);
    assert!(vm.stats.jit_side_exits > 0);
    assert_eq!(vm.stats.jit_side_exits, vm.stats.jit_resumes);
    assert_eq!(vm.stats.jit_helper_calls, 0);
}

#[test]
fn cranelift_global_helper_preserves_name_error_location() {
    let source = "def read_missing():\n    return missing\nprint(read_missing())";
    let program = compile(source, "jit-global-error").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_threshold = 1;
    vm.jit_min_instructions = 0;
    let error = vm.run(&program, &mut Vec::new()).unwrap_err();
    assert_eq!(error.kind, "NameError");
    assert_eq!(error.span, Some(program.program().code[1].spans[0]));
    assert_eq!(vm.stats.jit_runtime_errors, 1);
}

#[test]
fn unstable_jit_site_despecializes_after_bounded_guard_failures() {
    let source = "def add(a,b):\n    return a+b\ni=0\nwhile i<20:\n    add(1.5,i)\n    i+=1";
    let program = compile(source, "jit-despecialize").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    vm.jit_min_instructions = 0;
    vm.run(&program, &mut Vec::new()).unwrap();
    assert_eq!(vm.stats.jit_compiled, 1);
    assert_eq!(vm.stats.jit_calls, 8);
    assert_eq!(vm.stats.jit_deopts, 8);
    assert_eq!(vm.stats.jit_despecialized, 1);
    assert_eq!(vm.stats.jit_deferred, 7);
    assert_eq!(vm.stats.jit_fallbacks, 0);
}

#[test]
fn tiny_leaf_function_stays_in_profitable_adaptive_tier() {
    let source =
        "def add(a,b):\n    return a+b\ni=0\ns=0\nwhile i<20:\n    s=add(s,1)\n    i+=1\nprint(s)";
    let program = compile(source, "jit-profitability").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.execution_mode = ExecutionMode::Jit;
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"20\n");
    assert_eq!(vm.stats.jit_compiled, 0);
    assert_eq!(vm.stats.jit_unprofitable, 1);
    assert_eq!(vm.stats.jit_fallbacks, 0);
    assert_eq!(vm.stats.call_quickened, 1);
}

#[test]
fn adaptive_integer_binary_sites_quicken_and_despecialize() {
    let source = "def add(a,b):\n    return a+b\ni=0\nwhile i<10:\n    add(i,2)\n    i+=1\nprint(add(1.5,2))\ni=0\nwhile i<9:\n    add(i,3)\n    i+=1\nprint(add(4,5))";
    let program = compile(source, "adaptive-binary").unwrap();
    let mut vm = Vm::new().unwrap();
    let mut out = Vec::new();
    vm.run(&program, &mut out).unwrap();
    assert_eq!(out, b"3.5\n9\n");
    assert_eq!(vm.stats.quickened, 4);
    assert_eq!(vm.stats.quickened_misses, 1);
}

#[test]
fn persistent_callable_cannot_alias_code_in_another_execution() {
    use std::sync::Mutex;
    use tonic_runtime::PersistentHandle;
    static SAVED: Mutex<Option<PersistentHandle>> = Mutex::new(None);
    fn save(ctx: &mut Context<'_>, args: &[Handle]) -> Result<Handle> {
        *SAVED.lock().unwrap() = Some(ctx.persist(args[0])?);
        ctx.none()
    }
    fn load(ctx: &mut Context<'_>, _: &[Handle]) -> Result<Handle> {
        let saved = SAVED.lock().unwrap().take().unwrap();
        let local = ctx.borrow_persistent(&saved)?;
        ctx.release_persistent(&saved)?;
        Ok(local)
    }
    let mut vm = Vm::new().unwrap();
    vm.register_native("cache", "save", 1, save).unwrap();
    vm.register_native("cache", "load", 0, load).unwrap();
    let first = compile(
        "import cache\ndef f():\n    return 42\ncache.save(f)",
        "first",
    )
    .unwrap();
    vm.run(&first, &mut Vec::new()).unwrap();
    let second = compile("import cache\ncache.load()()", "second").unwrap();
    assert_eq!(
        vm.run(&second, &mut Vec::new()).unwrap_err().kind,
        "RuntimeError"
    );
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn augmented_list_add_preserves_aliases_and_self_extension() {
    assert_eq!(
        output("a=[1]\nb=a\na += (2,3)\na += a\nprint(a,b)\na += range(2)\nprint(b)"),
        "[1, 2, 3, 1, 2, 3] [1, 2, 3, 1, 2, 3]\n[1, 2, 3, 1, 2, 3, 0, 1]\n"
    );
}

#[test]
fn cyclic_container_repr_and_trace_edges() {
    assert_eq!(output("a=[]\na += (a,)\nprint(a)"), "[[...]]\n");
    let mut vm = Vm::new().unwrap();
    vm.run(&compile("a=[]\na += (a,)", "x").unwrap(), &mut Vec::new())
        .unwrap();
    assert!(vm.root_and_edge_counts().1 >= 2);
}
