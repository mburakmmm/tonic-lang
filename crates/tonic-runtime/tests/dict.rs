use tonic_compiler::compile;
use tonic_runtime::Vm;
fn output(s: &str) -> String {
    let mut out = Vec::new();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    vm.run(&compile(s, "dict").unwrap(), &mut out).unwrap();
    String::from_utf8(out).unwrap()
}
#[test]
fn literal_update_order_and_numeric_keys() {
    assert_eq!(output("d={True:1, 1.0:2, 'x':3}\nd[1]=4\nd['y']=5\nprint(d,len(d),d[True])\nprint({'x':1,'y':2}=={'y':2,'x':1})"),"{True: 4, 'x': 3, 'y': 5} 3 4\nTrue\n");
}
#[test]
fn dict_unpack_tuple_keys_and_cycles() {
    assert_eq!(output("d={'x':1}\nx={**d,'y':2,**{'x':3}}\nprint(x)\nd[(1,'a')]=5\nprint(d[(1.0,'a')])\na={}\na['self']=a\nprint(a)"),"{'x': 3, 'y': 2}\n5\n{'self': {...}}\n");
}
#[test]
fn iteration_allows_value_update_but_not_size_change() {
    assert_eq!(
        output("d={'a':1,'b':2}\nfor key in d:\n    d[key]=3\n    print(key,d[key])"),
        "a 3\nb 3\n"
    );
    let p = compile("d={'a':1}\nfor key in d:\n    d['b']=2", "x").unwrap();
    assert_eq!(
        Vm::new()
            .unwrap()
            .run(&p, &mut Vec::new())
            .unwrap_err()
            .kind,
        "RuntimeError"
    );
}
#[test]
fn missing_and_unhashable_keys() {
    for (s, kind) in [
        ("{}['x']", "KeyError"),
        ("{[]:1}", "TypeError"),
        ("{(1,[]):2}", "TypeError"),
        ("d={}\nd[{}]=1", "TypeError"),
    ] {
        let e = Vm::new()
            .unwrap()
            .run(&compile(s, "x").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(e.kind, kind);
    }
}
#[test]
fn list_item_assignment_and_rhs_order() {
    assert_eq!(
        output("a=[1,2]\nb=a\na[-1]=3\nprint(b)\ni=0\na[i],i=4,1\nprint(a,i)"),
        "[1, 3]\n[4, 3] 1\n"
    );
}

#[test]
fn augmented_item_target_is_evaluated_once() {
    assert_eq!(output("d={'x':1}\ndef owner():\n    print('owner')\n    return d\ndef key():\n    print('key')\n    return 'x'\nowner()[key()] += 2\nprint(d)"),"owner\nkey\n{'x': 3}\n");
}
#[test]
fn literal_group_expressions_run_before_hash_errors() {
    for source in ["{[]:print(1),2:print(2)}", "{**{},[]:print(1),2:print(2)}"] {
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = Some(1);
        let mut out = Vec::new();
        assert_eq!(
            vm.run(&compile(source, "x").unwrap(), &mut out)
                .unwrap_err()
                .kind,
            "TypeError"
        );
        assert_eq!(out, b"1\n2\n");
    }
}

#[test]
fn custom_hash_and_collision_equality_suspend_for_dictionary_operations() {
    assert_eq!(
        output(
            r#"class Truth:
    def __init__(self,value): self.value=value
    def __bool__(self):
        scratch=0.0
        for i in range(20): scratch+=0.5
        return self.value
class Key:
    def __init__(self,value): self.value=value
    def __hash__(self):
        scratch=0.0
        for i in range(20): scratch+=0.5
        return 7
    def __eq__(self,other): return Truth(self.value==other.value)
a=Key(1); same=Key(1); other=Key(2)
d={a:'first'}
print(d[same],len(d))
d[same]='updated'
d[other]='other'
print(d[a],d[other],len(d))
del d[same]
print(d[other],len(d))
print({Key(4):Key(5)}=={Key(4):Key(5)})
print({Key(4):Key(5)}!={Key(4):Key(6)})
built=dict([(Key(8),'eight')])
print(built[Key(8)])
copied=dict({Key(9):'nine'})
unpacked={**{Key(10):'ten'}}
print(copied[Key(9)],unpacked[Key(10)])
initialized={}
dict.__init__(initialized,{Key(11):'eleven'},named='keyword')
print(initialized[Key(11)],initialized['named'])
class One:
    def __hash__(self): return hash(1)
    def __eq__(self,other): return other==1
print({1:'integer'}[One()])"#,
        ),
        "first 1\nupdated other 2\nother 1\nTrue\nTrue\neight\nnine ten\neleven keyword\ninteger\n"
    );

    let error = Vm::new()
        .unwrap()
        .run(
            &compile(
                "a={}\na['self']=a\nb={}\nb['self']=b\nprint(a==b)",
                "cyclic-dict-comparison",
            )
            .unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "RecursionError");
}
