use tonic_compiler::compile;
use tonic_runtime::Vm;
fn output(s: &str) -> String {
    let mut out = Vec::new();
    Vm::new()
        .unwrap()
        .run(&compile(s, "calls").unwrap(), &mut out)
        .unwrap();
    String::from_utf8(out).unwrap()
}
fn error(s: &str) -> String {
    Vm::new()
        .unwrap()
        .run(&compile(s, "calls").unwrap(), &mut Vec::new())
        .unwrap_err()
        .kind
        .into()
}
#[test]
fn positional_keyword_and_defaults() {
    assert_eq!(output("def f(a,b=2,*,c=3):\n    return a+b+c\nprint(f(1),f(c=5,a=2),f(2,4,c=6))\ndef g(a,/,b):\n    return a+b\nprint(g(1,b=2))"),"6 9 12\n3\n");
}
#[test]
fn mutable_defaults_evaluated_once() {
    assert_eq!(output("def stamp():\n    print('default')\n    return []\ndef f(x=stamp()):\n    x += (1,)\n    return x\nprint(f())\nprint(f())\nprint(f([]))"),"default\n[1]\n[1, 1]\n[1]\n");
}
#[test]
fn defaults_capture_definition_environment() {
    assert_eq!(output("def make(x):\n    def f(a=x):\n        return a,x\n    x=9\n    return f\nf=make(2)\nprint(f(),f(5))"),"(2, 9) (5, 9)\n");
}
#[test]
fn variadic_and_expanded_calls() {
    assert_eq!(output("def f(a,/,b=2,*args,c=3,**kw):\n    print(a,b,args,c,kw)\nf(1,4,5,6,c=7,x=8,a=9)\nf(*[1],**{'c':5,'z':6})\nf(1,*[2,3],*[4],**{'c':5},y=6)"),"1 4 (5, 6) 7 {'x': 8, 'a': 9}\n1 2 () 5 {'z': 6}\n1 2 (3, 4) 5 {'y': 6}\n");
}
#[test]
fn nested_argument_builders() {
    assert_eq!(
        output("def f(*args,**kw):\n    return args,kw\nprint(f(*[1],x=f(*[2],y=3),**{'z':4}))"),
        "((1,), {'x': ((2,), {'y': 3}), 'z': 4})\n"
    );
}
#[test]
fn keyword_print() {
    assert_eq!(
        output("print(1,2,sep=':',end='!')\nprint(3,4,sep=None,end=None)"),
        "1:2!3 4\n"
    );
}
#[test]
fn binding_errors() {
    for s in [
        "def f(a):\n    pass\nf()",
        "def f(a):\n    pass\nf(1,2)",
        "def f(a):\n    pass\nf(1,a=2)",
        "def f(a,/):\n    pass\nf(a=1)",
        "def f(*,a):\n    pass\nf()",
        "def f():\n    pass\nf(x=1)",
        "def f(**kw):\n    pass\nf(x=1,**{'x':2})",
        "def f(**kw):\n    pass\nf(**{1:2})",
        "print(*1)",
        "print(**[])",
    ] {
        assert_eq!(error(s), "TypeError", "{s}");
    }
}
#[test]
fn expanded_argument_limit() {
    let p = compile("print(*range(100))", "x").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.limits.arguments = 10;
    assert_eq!(
        vm.run(&p, &mut Vec::new()).unwrap_err().kind,
        "ResourceError"
    );
}
#[test]
fn cells_include_keyword_and_variadic_parameters() {
    assert_eq!(output("def make(*args,x=3,**kw):\n    def f():\n        return args,x,kw\n    return f\nprint(make(1,2,x=5,y=6)())"),"((1, 2), 5, {'y': 6})\n");
}
#[test]
fn error_paths_preserve_argument_evaluation_order() {
    for interval in [None, Some(1)] {
        for (call, expected) in [
            ("f(**{'x':1},x=print(2),y=print(3))", "2\n3\n"),
            ("f(**{1:1},x=print(2))", "2\n"),
            ("f(**{1:1},**{True:2},x=print(2))", ""),
            ("f(**{1:1},**{2:2},x=print(2))", "2\n"),
            ("f(*None,x=print(2))", "2\n"),
            ("f(0,*None,x=print(2))", ""),
        ] {
            let p = compile(&format!("def f(*a,**kw):\n    pass\n{call}"), "x").unwrap();
            let mut vm = Vm::new().unwrap();
            vm.gc_interval = interval;
            let mut out = Vec::new();
            assert_eq!(vm.run(&p, &mut out).unwrap_err().kind, "TypeError");
            assert_eq!(out, expected.as_bytes(), "{call}");
            assert_eq!(vm.active_handles(), 0);
        }
    }
}

#[test]
fn monomorphic_tonic_call_cache_guards_callee_identity() {
    let source = "def add(a,b):\n    return a+b\ndef sub(a,b):\n    return a-b\nf=add\ni=0\ntotal=0\nwhile i<20:\n    if i==10:\n        f=sub\n    total+=f(100,i)\n    i+=1\nprint(total)";
    let program = tonic_compiler::compile(source, "call-cache").unwrap();
    let mut vm = Vm::new().unwrap();
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"1900\n");
    assert_eq!(vm.stats.calls, 21);
    assert_eq!(vm.stats.call_quickened, 2);
    assert_eq!(vm.stats.call_cache_misses, 1);
}

#[test]
fn two_target_tonic_call_pic_handles_alternating_callees() {
    let source = "def add(a,b):\n    return a+b\ndef sub(a,b):\n    return a-b\nf=add\ni=0\ntotal=0\nwhile i<30:\n    if i>=10:\n        if i%2==0:\n            f=add\n        else:\n            f=sub\n    total+=f(i,1)\n    i+=1\nprint(total)";
    let program = tonic_compiler::compile(source, "call-pic").unwrap();
    let mut vm = Vm::new().unwrap();
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"445\n");
    assert_eq!(vm.stats.call_cache_misses, 1);
    assert_eq!(vm.stats.call_pic_promotions, 1);
    assert_eq!(vm.stats.call_quickened, 2);
}
