use tonic_compiler::compile;
use tonic_runtime::Vm;
fn output(source: &str) -> String {
    let mut out = Vec::new();
    Vm::new()
        .unwrap()
        .run(&compile(source, "closure").unwrap(), &mut out)
        .unwrap();
    String::from_utf8(out).unwrap()
}
#[test]
fn escaping_shared_cells() {
    assert_eq!(output("def counter():\n    n=0\n    def inc():\n        nonlocal n\n        n+=1\n        return n\n    def read():\n        return n\n    return inc,read\na,b=counter()\nprint(a(),a(),b())\nc,d=counter()\nprint(c(),d(),b())"),"1 2 2\n1 1 2\n");
}
#[test]
fn forwarding_and_shadowing() {
    assert_eq!(output("x=99\ndef a(x):\n    def b():\n        def c():\n            return x\n        return c\n    return b\nprint(a(7)()())\ndef f():\n    x=1\n    def g():\n        x=2\n        def h():\n            return x\n        return h\n    return g()\nprint(f()())"),"7\n2\n");
}
#[test]
fn late_binding_and_recursive_nested_function() {
    assert_eq!(output("def outer():\n    funcs=[]\n    for i in range(3):\n        def f():\n            return i\n        funcs += (f,)\n    return funcs\nx=outer()\nprint(x[0](),x[1](),x[2]())\ndef make():\n    def fact(n):\n        if n<2:\n            return 1\n        return n*fact(n-1)\n    return fact\nprint(make()(6))"),"2 2 2\n720\n");
}
#[test]
fn lambdas_share_function_binding_and_closure_rules() {
    assert_eq!(output("f=lambda a,b=2,/,*args,c=3,**kw:(a,b,args,c,kw)\nprint(f(1,4,5,c=6,x=7))\ndef make(x):\n    return lambda y=2:x+y\ng=make(8)\nprint(g())\nfuncs=[]\nfor i in range(3):\n    funcs+=(lambda:i,)\nprint(funcs[0](),funcs[2]())\nfact=lambda n:1 if n<2 else n*fact(n-1)\nprint(fact(6))"),"(1, 4, (5,), 6, {'x': 7})\n10\n2 2\n720\n");
}
#[test]
fn global_rebinding_and_barrier() {
    assert_eq!(output("x=10\ndef outer():\n    x=20\n    def middle():\n        global x\n        x+=1\n        def inner():\n            return x\n        return inner\n    return middle\nprint(outer()()(),x)"),"11 11\n");
}
#[test]
fn free_and_local_unbound_errors() {
    for (src, kind) in [
        (
            "def f():\n    def g():\n        return x\n    g()\n    x=1\nf()",
            "NameError",
        ),
        (
            "def f():\n    def g():\n        return x\n    print(x)\n    x=1\nf()",
            "UnboundLocalError",
        ),
    ] {
        let e = Vm::new()
            .unwrap()
            .run(&compile(src, "x").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(e.kind, kind);
    }
}
#[test]
fn declaration_errors() {
    for src in ["nonlocal x","def f():\n    nonlocal x","def f(x):\n    global x","def f():\n    print(x)\n    global x","def f():\n    x=1\n    global x","def f():\n    global x\n    nonlocal x","def a():\n    x=1\n    def b():\n        global x\n        def c():\n            nonlocal x"]{let e=compile(src,"x").unwrap_err();assert_eq!(e.kind,"SyntaxError", "{src}");assert!(e.span.is_some());}
}
#[test]
fn ordinary_functions_do_not_allocate_cells() {
    let source = "def f(x):\n    return x+1\ni=0\nwhile i<1000:\n    f(i)\n    i+=1";
    let p = compile(source, "x").unwrap();
    assert!(p.program().code[1].cell_locals.is_empty());
    let mut vm = Vm::new().unwrap();
    vm.run(&p, &mut Vec::new()).unwrap();
    assert_eq!(vm.stats.heap_allocations, 1);
}
