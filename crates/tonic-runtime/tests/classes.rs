use tonic_compiler::compile;
use tonic_runtime::Vm;
fn run(source: &str) -> String {
    let code = compile(source, "classes").unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    let mut output = Vec::new();
    vm.run(&code, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}
#[test]
fn class_dict_is_a_live_read_only_mappingproxy() {
    assert_eq!(
        run(
            "def make_view():\n    class Hidden:\n        secret=42\n    return Hidden.__dict__\nhidden=make_view()\nprint(hidden['secret'])\nclass C:\n    x=1\nview=C.__dict__\nprint(view['x'],len(view))\nC.x=7\nC.y=9\nprint(view['x'],view['y'],len(view))\nnames=''\nfor name in view:\n    if name=='x' or name=='y':\n        names+=name\nprint(names)\nprint(view)"
        ),
        "42\n1 4\n7 9 5\nxy\nmappingproxy({'__module__': '__main__', '__qualname__': 'C', '__doc__': None, 'x': 7, 'y': 9})\n"
    );

    let error = Vm::new()
        .unwrap()
        .run(
            &compile("class C:\n    x=1\nC.__dict__['x']=2", "mappingproxy-write").unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "TypeError");

    let error = Vm::new()
        .unwrap()
        .run(
            &compile(
                "class C:\n    x=1\nview=C.__dict__\nfor name in view:\n    C.y=2",
                "mappingproxy-iteration",
            )
            .unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "RuntimeError");
}
#[test]
fn explicit_metaclasses_select_the_most_derived_compatible_type() {
    assert_eq!(
        run(
            "class Meta(type):\n    marker='meta'\n    @classmethod\n    def __prepare__(mcls,name,bases):\n        print('prepare',name,len(bases))\n        return {'seed': 5}\nclass ChildMeta(Meta):\n    pass\ndef choose():\n    print('choose')\n    return Meta\nclass A(metaclass=choose()):\n    print('body',seed)\n    value=seed+1\nclass B(A,metaclass=ChildMeta):\n    pass\nclass C(B):\n    pass\ndef local_class():\n    class LocalMeta(type):\n        pass\n    class Local(metaclass=LocalMeta):\n        pass\n    return Local\nLocal=local_class()\nlocal=Local()\nprint(type.__class__==type,object.__class__==type,Meta.__class__==type)\nprint(A.__class__==Meta,B.__class__==ChildMeta,C.__class__==ChildMeta)\nprint(type(Local)==Local.__class__,type(local)==Local,Local.__class__.__name__)\nprint(A.seed,A.value,isinstance(A,Meta),isinstance(B,Meta),issubclass(ChildMeta,Meta))"
        ),
        "choose\nprepare A 0\nbody 5\nprepare B 1\nprepare C 1\nTrue True True\nTrue True True\nTrue True LocalMeta\n5 6 True True True\n"
    );

    for (source, kind) in [
        (
            "class M1(type):\n    pass\nclass M2(type):\n    pass\nclass A(metaclass=M1):\n    pass\nclass B(metaclass=M2):\n    pass\nclass C(A,B):\n    pass",
            "TypeError",
        ),
        ("class C(metaclass=object):\n    pass", "TypeError"),
        (
            "class Meta(type):\n    @classmethod\n    def __prepare__(mcls,name,bases):\n        return 1\nclass C(metaclass=Meta):\n    pass",
            "TypeError",
        ),
        (
            "class Meta(type):\n    pass\nMeta()",
            "UnsupportedFeature",
        ),
    ] {
        let error = Vm::new()
            .unwrap()
            .run(
                &compile(source, "metaclass-errors").unwrap(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind, kind);
    }
}
#[test]
fn custom_prepare_mapping_can_be_converted_by_metaclass_new() {
    assert_eq!(
        run(
            "class Namespace:\n    def __init__(self):\n        self.data={}\n    def __getitem__(self,key):\n        return self.data[key]\n    def __setitem__(self,key,value):\n        self.data[key]=value\nclass Meta(type):\n    @classmethod\n    def __prepare__(mcls,name,bases):\n        return Namespace()\n    def __new__(mcls,name,bases,namespace):\n        copied={'__module__':namespace['__module__'],'__qualname__':namespace['__qualname__'],'__doc__':namespace['__doc__'],'value':namespace['value']}\n        return super().__new__(mcls,name,bases,copied)\nseed=40\nclass C(metaclass=Meta):\n    value=seed+1\nprint(C.value,C.__module__,C.__qualname__,C.__doc__)"
        ),
        "41 __main__ C None\n"
    );
}
#[test]
fn builtin_type_objects_share_type_checks_and_constructors() {
    assert_eq!(
        run(
            "print(type(1).__name__,type(True).__name__,type(None).__name__,type(1.5).__name__,type('x').__name__,type([]).__name__,type(()).__name__,type({}).__name__)\nprint(type(1)==int,isinstance(True,bool),isinstance(True,int),isinstance(1,object),issubclass(bool,int),isinstance(1,(str,int)))\nprint(type(range(3))==range,isinstance(range(3),range),issubclass(range,object))\nprint(int(),int(True),int(3.9),int('42'))\nprint(int('101',2),int('0xff',0),int('10',base=2))\nprint(float(),float(2),float('2.5'))\nclass Truth:\n    def __bool__(self):\n        total=0.0\n        for i in range(20):\n            total+=0.5\n        return True\nprint(bool(),bool([]),bool([1]),bool(Truth()))\nprint(str(),str(12),list('ab'),tuple([1,2]),list(range(3)))\nprint(dict({'x':3}),dict(a=1),dict({'a':1},b=2),dict([('a',1),('b',2)]))"
        ),
        "int bool NoneType float str list tuple dict\nTrue True True True True True\nTrue True True\n0 1 3 42\n5 255 2\n0.0 2.0 2.5\nFalse False True True\n 12 ['a', 'b'] (1, 2) [0, 1, 2]\n{'x': 3} {'a': 1} {'a': 1, 'b': 2} {'a': 1, 'b': 2}\n"
    );
    for (source, kind) in [
        ("int('bad')", "ValueError"),
        ("float('bad')", "ValueError"),
        ("int(1.0,2)", "TypeError"),
        ("list(1)", "TypeError"),
        ("dict(1)", "TypeError"),
        ("int('10',1)", "ValueError"),
        ("int(10,2)", "TypeError"),
        ("dict([(1,)])", "ValueError"),
    ] {
        let error = Vm::new()
            .unwrap()
            .run(
                &compile(source, "builtin-type-errors").unwrap(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind, kind);
    }
}
#[test]
fn getattr_fallback_uses_instance_class_and_metaclass_protocols() {
    assert_eq!(
        run(
            "class Missing:\n    def __init__(self):\n        self.present=7\n    def __getattr__(self,name):\n        scratch=0.0\n        for i in range(20):\n            scratch+=0.5\n        return name+'!'\nm=Missing()\nm.__getattr__=lambda name:'shadow'\nprint(m.present,m.absent,getattr(m,'other'))\nclass StaticMissing:\n    __getattr__=staticmethod(lambda name:'static-'+name)\nclass ClassMissing:\n    @classmethod\n    def __getattr__(cls,name):\n        return cls.__name__+'-'+name\nprint(StaticMissing().x,ClassMissing().y)\nclass Meta(type):\n    def __getattr__(cls,name):\n        return cls.__name__+'-'+name\nclass C(metaclass=Meta):\n    present=3\nprint(C.present,C.missing)"
        ),
        "7 absent! other!\nstatic-x ClassMissing-y\n3 C-missing\n"
    );
    let error = Vm::new()
        .unwrap()
        .run(
            &compile("class C:\n    __getattr__=1\nC().missing", "getattr-error").unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "TypeError");
}
#[test]
fn metaclass_new_init_chain_preserves_namespace_and_order() {
    assert_eq!(
        run(
            "class Descriptor:\n    def __set_name__(self,owner,name):\n        print('set_name',owner.__name__,name)\nclass Meta(type):\n    @classmethod\n    def __prepare__(mcls,name,bases):\n        print('prepare',mcls.__name__,name,len(bases))\n        return {'seed': 4}\n    def __new__(mcls,name,bases,namespace):\n        print('new',mcls.__name__,name,len(bases),namespace['seed'])\n        namespace['made']=namespace['seed']+1\n        cls=super().__new__(mcls,name,bases,namespace)\n        print('after_new',cls.__name__)\n        return cls\n    def __init__(cls,name,bases,namespace):\n        print('init',cls.__name__,name,namespace['made'])\n        cls.ready=namespace['made']+1\nclass C(metaclass=Meta):\n    print('body',seed)\n    item=Descriptor()\nprint(C.made,C.ready,type(C)==Meta)\nclass InitOnly(type):\n    def __init__(cls,name,bases,namespace):\n        cls.copied=namespace['value']\nclass I(metaclass=InitOnly):\n    value=9\nprint(I.copied)\nclass Nested(type):\n    def __new__(mcls,name,bases,namespace):\n        if name=='Outer':\n            class Inner(metaclass=mcls):\n                marker=7\n            print('nested',Inner.marker)\n        return super().__new__(mcls,name,bases,namespace)\nclass Outer(metaclass=Nested):\n    pass\nprint(Outer.__name__)"
        ),
        "prepare Meta C 0\nbody 4\nnew Meta C 0 4\nset_name C item\nafter_new C\ninit C C 5\n5 6 True\n9\nnested 7\nOuter\n"
    );

    for (source, kind) in [
        (
            "class Meta(type):\n    def __new__(mcls,name,bases,namespace):\n        return 42\n    def __init__(cls,name,bases,namespace):\n        print('must not run')\nclass C(metaclass=Meta):\n    pass\nprint(C)",
            None,
        ),
        (
            "class Meta(type):\n    def __init__(cls,name,bases,namespace):\n        return 1\nclass C(metaclass=Meta):\n    pass",
            Some("TypeError"),
        ),
        (
            "type.__new__(type,'C',(),{})",
            Some("UnsupportedFeature"),
        ),
    ] {
        let result = Vm::new().unwrap().run(
            &compile(source, "metaclass-new-init-errors").unwrap(),
            &mut Vec::new(),
        );
        match kind {
            Some(kind) => assert_eq!(result.unwrap_err().kind, kind),
            None => result.unwrap(),
        }
    }
}
#[test]
fn three_argument_type_copies_namespace_and_runs_set_name() {
    assert_eq!(
        run(
            "class D:\n    def __set_name__(self,owner,name):\n        print('set_name',owner.__name__,name)\nns={'x':3,'d':D()}\nC=type('C',(),ns)\nprint(C.__name__,C.__bases__[0]==object,C.x,C.__module__,C.__qualname__,C.__doc__)\nprint(len(ns),hasattr(ns,'__module__'))\nclass Base:\n    value=5\nChild=type('Child',(Base,),{'extra':7,'__module__':'custom'})\nprint(Child().value,Child.extra,Child.__module__)\nMixed=type('Mixed',(),{1:2,'x':3})\nview=Mixed.__dict__\nseen=0\nfor key in view:\n    if key==1:\n        seen+=view[key]\nprint(view[1],view[1.0],seen,Mixed.x)\nMixed.y=4\ndel Mixed.x\nprint(view['y'],hasattr(Mixed,'x'))"
        ),
        "set_name C d\nC True 3 __main__ C None\n2 False\n5 7 custom\n2 2 2 3\n4 False\n"
    );
    for source in [
        "type(1,(),{})",
        "type('C',[],{})",
        "type('C',(),[])",
        "type('C',(1,),{})",
        "type()",
        "type('C',())",
    ] {
        let error = Vm::new()
            .unwrap()
            .run(
                &compile(source, "dynamic-type-errors").unwrap(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind, "TypeError");
    }
}
#[test]
fn constructors_methods_and_binding() {
    assert_eq!(run("class Point:\n    tag='point'\n    def __init__(self,x,y=2):\n        self.x=x\n        self.y=y\n    def total(self,/,*,scale=1):\n        return (self.x+self.y)*scale\np=Point(3,y=4)\nprint(p.x,p.tag,p.total(scale=2))\nf=p.total\np.x+=10\nprint(f(),Point.total(p),f.__self__==p,f.__func__==Point.total)"),"3 point 14\n17 17 True True\n");
}
#[test]
fn implicit_class_cells_and_super_follow_c3_mro() {
    assert_eq!(
        run(
            "class A:\n    def f(self):\n        return 'A'\n    @classmethod\n    def who(cls):\n        return cls.__name__\n    @property\n    def value(self):\n        return 4\nclass B(A):\n    def f(self):\n        return 'B'+super().f()\nclass C(A):\n    def f(self):\n        return 'C'+super().f()\nclass D(B,C):\n    def f(self):\n        return 'D'+getattr(super(),'f')()\n    @classmethod\n    def who(cls):\n        return super().who()\n    @property\n    def value(self):\n        return super().value+1\n    def defining(self):\n        def nested():\n            return __class__\n        return nested()\nd=D()\nprint(d.f(),D.who(),d.value,d.defining()==D)\nprint(super(D,d).f(),super(D,D).who(),super(D,d).__self__==d,super(D,D).__self_class__==D)\nclass Descriptor:\n    def __get__(self,obj,owner):\n        return owner.__name__\nclass Parent:\n    label=Descriptor()\nclass Child(Parent):\n    def read(self):\n        return super().label\nprint(Child().read())\nclass Lambda:\n    owner=lambda self: __class__\nprint(Lambda().owner()==Lambda)"
        ),
        "DBCA D 5 True\nBCA D True True\nChild\nTrue\n"
    );
    for source in [
        "super()",
        "class C:\n    def f():\n        return super()\nC.f()",
        "class A:\n    pass\nclass B:\n    pass\nsuper(A,B())",
    ] {
        let error = Vm::new()
            .unwrap()
            .run(&compile(source, "super-errors").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert!(matches!(error.kind.as_str(), "RuntimeError" | "TypeError"));
    }
}
#[test]
fn new_controls_allocation_and_preserves_initializer_arguments() {
    assert_eq!(
        run(
            "class C:\n    def __new__(cls,x):\n        print('new',cls.__name__,x)\n        self=super().__new__(cls)\n        self.before=x+1\n        return self\n    def __init__(self,x):\n        print('init',x)\n        self.after=x+2\nc=C(4)\nprint(c.before,c.after,c.__new__==C.__new__)\nclass Skip:\n    def __new__(cls):\n        return 42\n    def __init__(self):\n        print('must not run')\nprint(Skip())\nclass Base:\n    def __new__(cls,x):\n        value=object.__new__(cls)\n        value.x=x\n        return value\n    def __init__(self,x):\n        self.x+=1\nclass Child(Base):\n    pass\nprint(Child(7).x)\nclass Payload:\n    pass\nclass Rooted:\n    def __new__(cls,p,*,tag):\n        x=0.0\n        for i in range(50):\n            x+=0.5\n        return object.__new__(cls)\n    def __init__(self,p,*,tag):\n        self.p=p\n        self.tag=tag\np=Payload()\nr=Rooted(p,tag='kept')\nprint(r.p==p,r.tag)"
        ),
        "new C 4\ninit 4\n5 6 True\n42\n8\nTrue kept\n"
    );
    for source in [
        "object.__new__(1)",
        "class C:\n    __new__=1\nC()",
        "class C:\n    pass\nobject.__new__(C,1)",
    ] {
        let error = Vm::new()
            .unwrap()
            .run(&compile(source, "new-errors").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(error.kind, "TypeError");
    }
}
#[test]
fn callable_and_length_protocols_use_class_lookup_and_normal_frames() {
    assert_eq!(
        run(
            "class Callable:\n    def __init__(self,bias):\n        self.bias=bias\n    def __call__(self,x=1,*,scale=1):\n        return (self.bias+x)*scale\nclass Child(Callable):\n    pass\nc=Child(2)\nprint(c(),c(3,scale=2))\nc.__call__=lambda x: 99\nprint(c(4),c.__call__(4))\nclass Sized:\n    def __len__(self):\n        total=0.0\n        for i in range(40):\n            total+=0.5\n        return 5\ns=Sized()\ns.__len__=lambda: 8\nprint(len(s))\nclass BoolSized:\n    def __len__(self):\n        return True\nprint(len(BoolSized()))\nclass StaticCall:\n    __call__=staticmethod(lambda x: x+1)\nclass ClassCall:\n    @classmethod\n    def __call__(cls):\n        return cls.__name__\nclass StaticSized:\n    __len__=staticmethod(lambda: 6)\nclass ClassSized:\n    @classmethod\n    def __len__(cls):\n        return 7\nprint(StaticCall()(4),ClassCall()(),len(StaticSized()),len(ClassSized()))"
        ),
        "3 10\n6 99\n5\n1\n5 ClassCall 6 7\n"
    );
    for (source, kind) in [
        ("class C:\n    pass\nC()()", "TypeError"),
        ("class C:\n    __call__=1\nC()()", "TypeError"),
        (
            "class C:\n    def __len__(self):\n        return -1\nlen(C())",
            "ValueError",
        ),
        (
            "class C:\n    def __len__(self):\n        return 1.0\nlen(C())",
            "TypeError",
        ),
        (
            "class C:\n    pass\nc=C()\nC.__call__=c\nc()",
            "RecursionError",
        ),
    ] {
        let error = Vm::new()
            .unwrap()
            .run(
                &compile(source, "protocol-errors").unwrap(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind, kind);
    }
    fn allocations(iterations: usize) -> u64 {
        let source = format!(
            "class C:\n    def __call__(self):\n        return 1\n    def __len__(self):\n        return 2\nc=C()\ni=0\nwhile i<{iterations}:\n    x=c()\n    y=len(c)\n    i+=1"
        );
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        vm.run(
            &compile(&source, "protocol-allocation").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.stats.heap_allocations
    }
    assert_eq!(allocations(1), allocations(1_000));
}
#[test]
fn item_protocols_use_special_lookup_and_setter_continuations() {
    assert_eq!(
        run(
            "class Bag:\n    def __init__(self):\n        self.data={}\n    def __getitem__(self,key):\n        scratch=0.0\n        for i in range(20):\n            scratch+=0.5\n        return self.data[key]+1\n    def __setitem__(self,key,value):\n        self.data[key]=value+2\n        return 99\nb=Bag()\nb['x']=4\nprint(b['x'],b.data['x'])\nb.__getitem__=lambda key: 100\nprint(b['x'],b.__getitem__('x'))\nclass Static:\n    __getitem__=staticmethod(lambda key:key+3)\n    __setitem__=staticmethod(lambda key,value: print('static-set',key,value))\nclass ByClass:\n    @classmethod\n    def __getitem__(cls,key):\n        return cls.__name__+key\nprint(Static()[4],ByClass()['!'])\nStatic()[1]=2"
        ),
        "7 6\n7 100\n7 ByClass!\nstatic-set 1 2\n"
    );
    for source in [
        "class C:\n    __getitem__=1\nC()[0]",
        "class C:\n    __setitem__=1\nC()[0]=2",
        "class C:\n    pass\nC()[0]",
        "class C:\n    pass\nC()[0]=2",
    ] {
        let error = Vm::new()
            .unwrap()
            .run(
                &compile(source, "item-protocol-errors").unwrap(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind, "TypeError");
    }
}
#[test]
fn item_deletion_supports_builtin_and_custom_protocols() {
    assert_eq!(
        run(
            "values=[1,2,3]\ndel values[-2]\nprint(values)\ndata={'a':1,'b':2}\ndel data['a']\nprint(data)\nclass Bag:\n    def __init__(self):\n        self.data={'x':4,'y':5}\n    def __delitem__(self,key):\n        scratch=0.0\n        for i in range(20):\n            scratch+=0.5\n        del self.data[key]\n        print('deleted',key)\n        return 99\nb=Bag()\ndel b['x']\nprint(b.data)\nb.__delitem__=lambda key: print('shadow',key)\ndel b['y']\nprint(b.data)\nclass Static:\n    __delitem__=staticmethod(lambda key:print('static',key))\ndel Static()[7]"
        ),
        "[1, 3]\n{'b': 2}\ndeleted x\n{'y': 5}\ndeleted y\n{}\nstatic 7\n"
    );
    for (source, kind) in [
        ("x=[]\ndel x[0]", "IndexError"),
        ("x={}\ndel x['missing']", "KeyError"),
        ("del (1)[0]", "TypeError"),
        ("class C:\n    __delitem__=1\ndel C()[0]", "TypeError"),
    ] {
        let error = Vm::new()
            .unwrap()
            .run(
                &compile(source, "delete-item-errors").unwrap(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind, kind);
    }
}
#[test]
fn truth_protocols_suspend_and_preserve_boolean_operands() {
    assert_eq!(
        run(
            "class Toggle:\n    def __init__(self):\n        self.calls=0\n    def __bool__(self):\n        self.calls+=1\n        scratch=0.0\n        for i in range(20):\n            scratch+=0.5\n        return self.calls<3\nt=Toggle()\nwhile t:\n    print('tick')\nprint(t.calls,not t)\nt.__bool__=lambda: True\nprint('yes' if t else 'no',t.calls)\nprint((t and 7)==t,(t or 7)==t,t.calls)\nclass Length:\n    def __init__(self,n):\n        self.n=n\n    def __len__(self):\n        return self.n\nclass Child(Length):\n    pass\nprint(not Child(0),not Child(2),'yes' if Child(3) else 'no')\nclass Both:\n    def __bool__(self):\n        return False\n    def __len__(self):\n        return 4\nprint(not Both())\nclass StaticTruth:\n    __bool__=staticmethod(lambda: True)\nclass ClassTruth:\n    @classmethod\n    def __bool__(cls):\n        return cls.__name__=='ClassTruth'\nprint(not StaticTruth(),not ClassTruth())\nclass Dynamic:\n    def __bool__(self):\n        return True\n    def __len__(self):\n        return 0\nd=Dynamic()\nprint(not d)\ndel Dynamic.__bool__\nprint(not d,not object())"
        ),
        "tick\ntick\n3 True\nno 5\nTrue False 7\nTrue False yes\nTrue\nFalse False\nFalse\nTrue False\n"
    );
    for (source, kind) in [
        (
            "class C:\n    def __bool__(self):\n        return 1\nif C():\n    pass",
            "TypeError",
        ),
        (
            "class C:\n    def __len__(self):\n        return -1\nif C():\n    pass",
            "ValueError",
        ),
        (
            "class C:\n    def __len__(self):\n        return 1.0\nnot C()",
            "TypeError",
        ),
        ("class C:\n    __bool__=1\nC() and 1", "TypeError"),
    ] {
        let error = Vm::new()
            .unwrap()
            .run(&compile(source, "truth-errors").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(error.kind, kind);
    }
    fn allocations(iterations: usize) -> u64 {
        let source = format!(
            "class B:\n    def __bool__(self):\n        return True\nclass L:\n    def __len__(self):\n        return 1\nb=B()\nl=L()\ni=0\nwhile i<{iterations}:\n    if b:\n        x=not l\n    i+=1"
        );
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        vm.run(
            &compile(&source, "truth-allocation").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.stats.heap_allocations
    }
    assert_eq!(allocations(1), allocations(1_000));
}

#[test]
fn instance_slot_cache_guards_shape_class_and_dependency_version() {
    let source = "class C:\n    pass\ndef get_x(self):\n    return 9\nc=C()\nc.x=1\np=property(get_x)\ni=0\ntotal=0\nwhile i<20:\n    if i==10:\n        setattr(C,'x',p)\n    total+=c.x\n    i+=1\nprint(total)";
    let program = tonic_compiler::compile(source, "attr-cache").unwrap();
    let mut vm = Vm::new().unwrap();
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"100\n");
    assert_eq!(vm.stats.attr_quickened, 1);
    assert_eq!(vm.stats.attr_cache_misses, 1);
}
#[test]
fn two_shape_attribute_pic_handles_alternating_instances() {
    let source = "class A:\n    pass\nclass B:\n    pass\na=A()\nb=B()\na.x=1\nb.x=2\no=a\ni=0\ntotal=0\nwhile i<30:\n    if i>=10:\n        if i%2==0:\n            o=a\n        else:\n            o=b\n    total+=o.x\n    i+=1\nprint(total)";
    let program = tonic_compiler::compile(source, "attr-pic").unwrap();
    let mut vm = Vm::new().unwrap();
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"40\n");
    assert_eq!(vm.stats.attr_cache_misses, 1);
    assert_eq!(vm.stats.attr_pic_promotions, 1);
    assert_eq!(vm.stats.attr_quickened, 2);
}
#[test]
fn attribute_cache_invalidation_tracks_only_class_and_mro_dependencies() {
    let source = "class Base:\n    pass\nclass Child(Base):\n    pass\nclass Noise:\n    pass\nc=Child()\nc.x=1\ni=0\ntotal=0\nwhile i<30:\n    if i==10:\n        Noise.y=4\n    if i==20:\n        Base.x=property(lambda self: 9)\n    total+=c.x\n    i+=1\nprint(total)";
    let program = tonic_compiler::compile(source, "attr-dependencies").unwrap();
    let mut vm = Vm::new().unwrap();
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"110\n");
    assert_eq!(vm.stats.attr_cache_misses, 1);
    assert_eq!(vm.stats.attr_quickened, 1);
}
#[test]
fn attribute_cache_does_not_hide_late_descriptor_class_mutation() {
    let source = "class Marker:\n    pass\nclass C:\n    x=Marker()\nc=C()\nc.x=1\ni=0\nwhile i<10:\n    y=c.x\n    i+=1\ndef get(self,obj,owner):\n    return 9\ndef set_value(self,obj,value):\n    pass\nMarker.__get__=get\nMarker.__set__=set_value\nprint(c.x)";
    let program = tonic_compiler::compile(source, "attr-descriptor-dependency").unwrap();
    let mut vm = Vm::new().unwrap();
    let mut output = Vec::new();
    vm.run(&program, &mut output).unwrap();
    assert_eq!(output, b"9\n");
    assert_eq!(vm.stats.attr_quickened, 0);
    assert_eq!(vm.stats.attr_cache_misses, 0);
}
#[test]
fn decorators_static_methods_and_class_methods() {
    assert_eq!(run("def mark(tag):\n    print('eval',tag)\n    def apply(value):\n        print('apply',tag)\n        return value\n    return apply\ndef default():\n    print('default')\n    return 2\n@mark('outer')\n@mark('inner')\ndef decorated(x=default()):\n    return x+1\nprint(decorated())\nclass A:\n    x=4\n    @staticmethod\n    def add(a,b=1):\n        return a+b\n    @classmethod\n    def read(cls,n):\n        return cls.x+n\nclass B(A):\n    x=10\nprint(A.add(2),A().add(3,4),A.read(3),B.read(5),B().read(6))"),"eval outer\neval inner\ndefault\napply inner\napply outer\n3\n3 7 7 15 16\n");

    assert_eq!(run("def decorate(tag):\n    print('eval',tag)\n    def apply(cls):\n        print('apply',tag)\n        cls.tag=tag\n        return cls\n    return apply\n@decorate('outer')\n@decorate('inner')\nclass C:\n    pass\nprint(C.tag)\ndef free(x):\n    return x+1\nsm=staticmethod(free)\ncm=classmethod(free)\nprint(sm(4),sm.__func__==free,cm.__func__==free)"),"eval outer\neval inner\napply inner\napply outer\nouter\n5 True True\n");
}
#[test]
fn properties_use_data_descriptor_priority_and_setters() {
    assert_eq!(run("class C:\n    def __init__(self,x):\n        self._x=x\n    @property\n    def x(self):\n        return self._x\n    @x.setter\n    def x(self,value):\n        self._x=value\n        return 99\nc=C(2)\nprint(c.x,C.x.fget==C.x.fget,C.x.fset==C.x.fset)\nc.x=7\nprint(getattr(c,'x'))\nsetattr(c,'x',9)\nprint(c.x)\nclass D:\n    pass\nd=D()\nd.x=100\ndef read(self):\n    return 5\nD.x=property(read)\nprint(d.x)"),"2 True True\n7\n9\n5\n");
    for source in [
        "class C:\n    @property\n    def x(self):\n        return 1\nC().x=2",
        "class C:\n    x=property()\nC().x",
    ] {
        let error = Vm::new()
            .unwrap()
            .run(&compile(source, "property").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(error.kind, "AttributeError");
    }
}
#[test]
fn custom_data_and_non_data_descriptors_follow_precedence() {
    assert_eq!(
        run(
            "class Data:\n    def __init__(self):\n        self.value=0\n    def __get__(self,obj,owner):\n        if obj==None:\n            return self\n        print('get',owner.__name__)\n        return self.value\n    def __set__(self,obj,value):\n        print('set',value)\n        self.value=value\n        return 99\nclass Base:\n    x=Data()\nclass Child(Base):\n    pass\nc=Child()\nc.x=4\nprint(c.x,Child.x==Base.x)\nsetattr(c,'x',8)\nprint(getattr(c,'x'))\nclass NonData:\n    def __get__(self,obj,owner):\n        return 7\nclass C:\n    y=NonData()\nn=C()\nprint(n.y,C.y)\nn.y=9\nprint(n.y,C.y)\nclass WriteOnly:\n    def __set__(self,obj,value):\n        pass\nclass W:\n    z=WriteOnly()\nw=W()\nprint(w.z==W.z)"
        ),
        "set 4\nget Child\n4 True\nset 8\nget Child\n8\n7 7\n9 7\nTrue\n"
    );
    for source in [
        "class D:\n    __get__=1\nclass C:\n    x=D()\nC().x",
        "class D:\n    __set__=1\nclass C:\n    x=D()\nC().x=2",
    ] {
        let error = Vm::new()
            .unwrap()
            .run(&compile(source, "descriptor").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(error.kind, "TypeError");
    }
}
#[test]
fn descriptor_calls_do_not_allocate_temporary_bound_methods() {
    fn allocations(iterations: usize) -> u64 {
        let source = format!(
            "class D:\n    def __get__(self,obj,owner):\n        return 1\nclass C:\n    x=D()\nc=C()\ni=0\nwhile i<{iterations}:\n    y=c.x\n    i+=1"
        );
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        vm.run(
            &compile(&source, "descriptor-allocation").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.stats.heap_allocations
    }
    assert_eq!(allocations(1), allocations(1_000));
}
#[test]
fn descriptor_set_name_runs_in_definition_order_before_decorators() {
    assert_eq!(
        run(
            "class D:\n    def __init__(self,tag):\n        self.tag=tag\n    def __set_name__(self,owner,name):\n        print('set_name',self.tag,owner.__name__,name)\n        self.name=name\n        x=0.0\n        for i in range(20):\n            x+=0.5\n    def __get__(self,obj,owner):\n        return getattr(self,'name','unset')\ndef decorate(cls):\n    print('decorate',cls.__name__)\n    return cls\n@decorate\nclass Base:\n    print('body')\n    a=D('first')\n    b=D('second')\nclass Child(Base):\n    pass\nprint(Base.a,Base.b,Child.a)\nlate=D('late')\nBase.c=late\nprint(Base.c)"
        ),
        "body\nset_name first Base a\nset_name second Base b\ndecorate Base\na b a\nunset\n"
    );
    for source in [
        "class D:\n    def __set_name__(self,owner,name):\n        return missing\nclass C:\n    x=D()",
        "class D:\n    __set_name__=1\nclass C:\n    x=D()",
    ] {
        let error = Vm::new()
            .unwrap()
            .run(&compile(source, "set-name").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert!(matches!(error.kind.as_str(), "NameError" | "TypeError"));
    }
}
#[test]
fn descriptor_and_property_deleters_use_delete_continuations() {
    assert_eq!(
        run(
            "class D:\n    def __get__(self,obj,owner):\n        return 'managed'\n    def __delete__(self,obj):\n        print('descriptor delete')\n        obj.deleted=True\n        return 99\nclass C:\n    x=D()\nc=C()\nprint(c.x)\ndel c.x\nprint(c.deleted,c.x)\nclass P:\n    def __init__(self):\n        self._x=4\n    @property\n    def x(self):\n        return self._x\n    @x.deleter\n    def x(self):\n        print('property delete')\n        del self._x\np=P()\nprint(p.x)\ndel p.x\nprint(getattr(p,'_x','gone'),P.x.fdel==P.x.fdel)\nclass E:\n    x=1\ne=E()\ne.y=2\ndel e.y\ndel E.x\nprint(getattr(e,'y','missing'),getattr(E,'x','missing'))"
        ),
        "managed\ndescriptor delete\nTrue managed\n4\nproperty delete\ngone True\nmissing missing\n"
    );
    for source in [
        "class C:\n    @property\n    def x(self):\n        return 1\ndel C().x",
        "class C:\n    pass\ndel C().missing",
        "class D:\n    def __delete__(self,obj):\n        pass\nclass C:\n    x=D()\nC().x=1",
    ] {
        let error = Vm::new()
            .unwrap()
            .run(&compile(source, "delete").unwrap(), &mut Vec::new())
            .unwrap_err();
        assert_eq!(error.kind, "AttributeError");
    }
}
#[test]
fn delete_targets_are_ordered_and_three_argument_property_deletes() {
    assert_eq!(
        run(
            "class C:\n    pass\na=C()\nb=C()\na.x=1\nb.x=2\ndef owner(tag,value):\n    print('owner',tag)\n    return value\ndel owner(1,a).x,owner(2,b).x\nprint(getattr(a,'x','gone'),getattr(b,'x','gone'))\ndef get(self):\n    return 1\ndef delete(self):\n    self.deleted=True\nclass P:\n    x=property(get,None,delete)\np=P()\ndel p.x\nprint(p.deleted)"
        ),
        "owner 1\nowner 2\ngone gone\nTrue\n"
    );
}
#[test]
fn bound_variadic_defaults_and_inherited_initializer() {
    assert_eq!(run("class Base:\n    def __init__(self,a=2,/,*args,b=3,**kw):\n        self.data=(a,args,b,kw)\n    def read(self,*args,**kw):\n        return self.data,args,kw\nclass Child(Base):\n    pass\nc=Child(*[4,5],**{'b':6,'a':7})\nprint(c.read(*[8],**{'x':9}))\nprint(Child().data)"),"((4, (5,), 6, {'a': 7}), (8,), {'x': 9})\n(2, (), 3, {})\n");
}
#[test]
fn attribute_builtins_and_class_checks() {
    assert_eq!(run("class A:\n    x=2\nclass B(A):\n    pass\nb=B()\nsetattr(b,'y',3)\nprint(getattr(b,'x'),getattr(b,'y'),getattr(b,'missing',4),hasattr(b,'missing'))\nprint(isinstance(b,A),isinstance(b,B),isinstance(1,object),issubclass(B,A),issubclass(A,B))\nprint(isinstance(b,(B,1)),issubclass(B,(A,1)))\nsetattr(b,'__private',9)\nprint(getattr(b,'__private'))"),"2 3 4 False\nTrue True True True False\nTrue True\n9\n");
}
#[test]
fn class_scope_skips_method_bindings_and_forwards_cells() {
    assert_eq!(run("x='global'\ndef make():\n    x='outer'\n    class C:\n        print(x)\n        x='class'\n        def read(self):\n            return x,C\n    return C\nC=make()\nprint(C.x,C().read()[0],C().read()[1]==C)"),"global\nclass outer True\n");
    assert_eq!(run("x='global'\ndef make():\n    x='outer'\n    class C:\n        global x\n        x='changed'\n        def read(self):\n            return x\n    return C\nprint(make()().read(),x)"),"outer changed\n");
    assert_eq!(run("def make():\n    x=1\n    class C:\n        nonlocal x\n        x+=1\n        y=x\n        def f(self):\n            nonlocal x\n            x+=1\n            return x\n    return C\nC=make()\nprint(C.y,C().f())"),"2 3\n");
}
#[test]
fn multiple_inheritance_c3_and_rebinding() {
    assert_eq!(run("class A:\n    x=1\n    def f(self):\n        return 'A'\nclass B(A):\n    pass\nclass C(A):\n    def f(self):\n        return 'C'\nclass D(B,C):\n    pass\nd=D()\nprint(d.f(),d.x)\nA.x=2\nprint(d.x)\nD.f=A.f\nprint(d.f())\nprint(D.__mro__[1]==B,D.__mro__[2]==C,D.__mro__[3]==A,D.__mro__[4]==object)"),"C 1\n2\nA\nTrue True True True\n");
}
#[test]
fn private_names_qualified_names_and_class_docstrings() {
    assert_eq!(run("class A:\n    'A doc'\n    __x=2\n    def __init__(self):\n        self.__y=3\n    def f(self):\n        return self.__x+self.__y\n    class B:\n        pass\nprint(A().f(),A._A__x,A.__doc__,A.B.__qualname__)\ndef factory():\n    class Local:\n        pass\n    return Local\nprint(factory().__name__,factory().__qualname__)"),"5 2 A doc A.B\nLocal factory.<locals>.Local\n");
}
#[test]
fn method_identity_and_instance_dictionary_keys() {
    assert_eq!(run("class C:\n    def f(self):\n        return 1\na=C()\nb=C()\nd={a:2,a.f:3}\nprint(a==a,a==b,a.f==a.f,a.f==b.f,d[a],d[a.f])\ndef free():\n    return 7\na.f=free\nprint(a.f(),b.f())"),"True False True False 2 3\n7 1\n");
}
#[test]
fn attribute_augassign_evaluates_owner_once() {
    assert_eq!(run("class C:\n    pass\nc=C()\nc.x=1\ndef owner():\n    print('owner')\n    return c\ndef rhs():\n    print('rhs')\n    return 2\nowner().x+=rhs()\nprint(c.x)"),"owner\nrhs\n3\n");
}
#[test]
fn class_namespace_and_initializer_roots() {
    assert_eq!(run("def garbage():\n    x=0.0\n    for i in range(100):\n        x+=0.5\nclass C:\n    kept=['namespace']\n    garbage()\n    def __init__(self):\n        self.data=['instance']\n        self=0\n        garbage()\n    def method(self):\n        return self.data\nc=C()\nc.saved=c.method\nc.cycle=c\nprint(c.saved(),C.kept)"),"['instance'] ['namespace']\n");
}
#[test]
fn shape_sharing_and_dictionary_fallback() {
    let mut source = String::from("class C:\n    pass\na=C()\nb=C()\na.x=1\nb.x=2\n");
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    vm.run(&compile(&source, "shape").unwrap(), &mut Vec::new())
        .unwrap();
    assert_eq!(vm.shape_count(), 2);
    for i in 0..80 {
        source.push_str(&format!("a.v{i}={i}\n"));
    }
    source.push_str("a.v0=100\nprint(a.x,b.x,a.v0,a.v79)\n");
    let mut out = Vec::new();
    vm.run(&compile(&source, "shape").unwrap(), &mut out)
        .unwrap();
    assert_eq!(out, b"1 2 100 79\n");
    assert_eq!(vm.shape_count(), 65);
}
#[test]
fn class_and_bound_method_cycles_are_collectible() {
    let mut vm = Vm::new().unwrap();
    let baseline = vm.collect_garbage().unwrap().survivors;
    let code=compile("def factory():\n    class C:\n        @staticmethod\n        def s():\n            return C\n        @classmethod\n        def c(cls):\n            return cls\n        def f(self):\n            return C\n    return C\nC=factory()\nx=C()\nx.f=x.f\nx.cycle=x\nC.loop=C","cycles").unwrap();
    vm.run(&code, &mut Vec::new()).unwrap();
    let expected = vm.live_objects() - baseline;
    vm.run(&compile("pass", "empty").unwrap(), &mut Vec::new())
        .unwrap();
    assert!(expected > 0);
    assert_eq!(vm.collect_garbage().unwrap().reclaimed, expected);
    assert_eq!(vm.live_objects(), baseline);
}
#[test]
fn class_errors_and_unsupported_protocols_are_explicit() {
    for (source,kind) in [
        ("class C:\n    pass\nC(1)","TypeError"),
        ("class C:\n    def __init__(self):\n        return 3\nC()","TypeError"),
        ("class C:\n    pass\nC().missing","AttributeError"),
        ("class C(1):\n    pass","TypeError"),
        ("class A:\n    pass\nclass B(A,A):\n    pass","TypeError"),
        ("class A:\n    pass\nclass B:\n    pass\nclass X(A,B):\n    pass\nclass Y(B,A):\n    pass\nclass Z(X,Y):\n    pass","TypeError"),
        ("class C:\n    pass\nC.__eq__=1","UnsupportedFeature"),
        ("x=object()\nx.a=1","AttributeError"),
        ("object.x=1","TypeError"),
        ("class C:\n    x=classmethod(1)\nd={C.x:2}\nprint(d[C.x],C.x==C.x)\nC.x()","TypeError"),
    ] {
        let mut vm=Vm::new().unwrap();vm.gc_interval=Some(1);
        assert_eq!(vm.run(&compile(source,"error").unwrap(),&mut Vec::new()).unwrap_err().kind,kind,"{source}");
    }
}
