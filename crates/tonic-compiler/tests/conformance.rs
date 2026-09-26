use tonic_compiler::{compile, compile_modules, discover_imports, parse, ModuleSource};
use tonic_core::{
    ast::{ExprKind, StmtKind},
    bytecode::Op,
};
#[test]
fn spans_unicode_and_precedence() {
    let src = "x = 1 + 2 * 3\nprint('é')\n";
    let ast = parse(src, "x").unwrap();
    let StmtKind::Assign(_, expr) = &ast.body[0].kind else {
        panic!("assignment")
    };
    assert_eq!(
        &src[expr.span.start as usize..expr.span.end as usize],
        "1 + 2 * 3"
    );
    let ExprKind::Binary(_, _, right) = &expr.kind else {
        panic!("binary")
    };
    assert!(matches!(right.kind, ExprKind::Binary(..)));
    assert_eq!(ast.body[1].span.start, 14);
}
#[test]
fn valid_python_forms() {
    for src in [
        "if True:\n\tprint(1)\n",
        "x=(1+\n 2)\n",
        "x=0xff + 1_000\n",
        "x='''multi\nline'''\n",
        "x=1; y=2\n",
        "def f(a, /, b):\n    return a+b\nprint(f(1,2))",
        "f=lambda a,/,b=2,*args,c=3,**kw:(a,b,args,c,kw)",
        "print(-1*2+3, not 1==2)",
        "print(2**8,5|2,5^3,5&3,1<<4,8>>2,~5)",
        "print([0,1,2,3][1:3], 'abc'[::-1])",
        "del object.attr",
        "class Meta(type):\n    pass\nclass X(metaclass=Meta):\n    pass",
        "def fail():\n    raise ValueError('boom')",
        "import package.child\nimport package.child as child\nfrom package import value as answer",
    ] {
        compile(src, "x").unwrap();
    }
}

#[test]
fn raise_ast_and_bytecode_are_tonic_owned() {
    let source = "raise ValueError('boom')";
    let ast = parse(source, "raise").unwrap();
    assert!(matches!(
        ast.body[0].kind,
        StmtKind::Raise {
            value: Some(_),
            cause: None
        }
    ));
    let program = compile(source, "raise").unwrap();
    assert!(program.program().code[0]
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::Raise as u16));
    let chained = compile("raise ValueError('x') from cause", "raise-from").unwrap();
    assert!(chained.program().code[0]
        .instructions
        .iter()
        .any(|instruction| { instruction.opcode == Op::Raise as u16 && instruction.b == 2 }));
}

#[test]
fn try_except_has_tonic_owned_handlers_and_regions() {
    let source = "try:\n    int('bad')\nexcept (TypeError, ValueError) as error:\n    print(error)\nelse:\n    print('ok')";
    let ast = parse(source, "try").unwrap();
    let StmtKind::Try {
        body,
        handlers,
        otherwise,
        finalbody,
    } = &ast.body[0].kind
    else {
        panic!("try statement")
    };
    assert_eq!((body.len(), handlers.len(), otherwise.len()), (1, 1, 1));
    assert!(finalbody.is_empty());
    assert!(handlers[0].type_.is_some());
    assert!(handlers[0].name.is_some());
    let program = compile(source, "try").unwrap();
    let code = &program.program().code[0];
    assert_eq!(code.exception_regions.len(), 2);
    assert!(code
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::ExceptionMatch as u16));
    assert!(code
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::ClearException as u16));
    assert!(code
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::PushException as u16));
    let program = compile(
        "try:\n    print('body')\nfinally:\n    print('cleanup')",
        "finally",
    )
    .unwrap();
    assert!(!program.program().code[0].exception_regions.is_empty());
}

#[test]
fn with_has_tonic_owned_items_and_context_opcodes() {
    let source = "with first() as (a,b), second():\n    print(a,b)";
    let ast = parse(source, "with").unwrap();
    let StmtKind::With { items, body } = &ast.body[0].kind else {
        panic!("with statement")
    };
    assert_eq!((items.len(), body.len()), (2, 1));
    assert!(items[0].target.is_some());
    assert!(items[1].target.is_none());
    let program = compile(source, "with").unwrap();
    let code = &program.program().code[0];
    assert!(code
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::ContextEnter as u16));
    assert!(code
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::ContextExit as u16));
    assert!(code.exception_regions.len() >= 2);
}

#[test]
fn invalid_python_forms() {
    for src in [
        "if True\n    pass",
        "if True:\npass",
        "def f(a,a):\n    pass",
        "a + = 1",
        "  x=1\n y=2",
        "(1+2",
        "break",
        "continue",
        "return 1",
        "1 = 2",
    ] {
        assert!(compile(src, "x").is_err(), "accepted {src:?}");
    }
}
#[test]
fn unsupported_syntax_is_explicit() {
    for src in [
        "class X(extra=1):\n    pass",
        "async def f():\n    pass",
        "x=[i for i in range(3)]",
        "print(f'{1}')",
        "match x:\n    case 1:\n        pass",
        "x=[1,2]\nx[:]=[3]",
        "del x",
        "from . import sibling",
        "from package import *",
    ] {
        let e = compile(src, "x").unwrap_err();
        assert_eq!(e.kind, "UnsupportedSyntax", "{src}");
        assert!(e.span.is_some());
    }
}

#[test]
fn slice_ast_and_bytecode_are_tonic_owned() {
    let source = "x = values[1:4:2]\n";
    let ast = parse(source, "slice").unwrap();
    let StmtKind::Assign(_, value) = &ast.body[0].kind else {
        panic!("assignment")
    };
    let ExprKind::Subscript(_, item) = &value.kind else {
        panic!("subscript")
    };
    assert!(matches!(item.kind, ExprKind::Slice { .. }));
    assert_eq!(
        &source[item.span.start as usize..item.span.end as usize],
        "1:4:2"
    );
    let program = compile(source, "slice").unwrap();
    assert!(program.program().code[0]
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::Slice as u16));
}
#[test]
fn temporary_registers_reused() {
    let source = "x = 1+2\n".repeat(1000);
    let p = compile(&source, "x").unwrap();
    assert!(p.program().code[0].registers < 10);
}
#[test]
fn bytecode_pipeline_has_local_and_global_operations() {
    let p = compile("x=1\ndef f(a):\n    return a+x\nprint(f(2))", "x").unwrap();
    assert_eq!(p.program().code.len(), 2);
    let f = &p.program().code[1];
    assert_eq!(f.params, 1);
    assert!(f
        .instructions
        .iter()
        .any(|i| i.opcode == Op::LoadGlobal as u16));
    assert!(f.instructions.iter().any(|i| i.opcode == Op::Move as u16));
}

#[test]
fn yield_marks_only_its_lexical_function_as_generator() {
    let program = compile(
        "def outer():\n    def inner():\n        yield 1\n    return inner\n",
        "generator",
    )
    .unwrap();
    assert!(!program.program().code[0].generator);
    assert!(!program.program().code[1].generator);
    assert!(program.program().code[2].generator);
    assert!(program.program().code[2]
        .instructions
        .iter()
        .any(|instruction| Op::try_from(instruction.opcode) == Ok(Op::Yield)));
    for source in ["yield 1", "class C:\n    yield 1"] {
        assert!(compile(source, "bad-yield").is_err(), "accepted {source:?}");
    }
}
#[test]
fn resource_limits_reject_deep_ast_before_parsing() {
    let s = format!("x={}1{}", "(".repeat(300), ")".repeat(300));
    assert_eq!(compile(&s, "x").unwrap_err().kind, "ResourceError");
    let s = format!("x={}", vec!["1"; 300].join("+"));
    assert_eq!(compile(&s, "x").unwrap_err().kind, "ResourceError");
}
#[test]
fn class_scopes_and_target_spans() {
    let source = "class Café:\n    def f(self):\n        self.x=1\n";
    let ast = parse(source, "class").unwrap();
    let StmtKind::Class { body, .. } = &ast.body[0].kind else {
        panic!("class")
    };
    let StmtKind::Function { body, .. } = &body[0].kind else {
        panic!("method")
    };
    let span = body[0].span;
    assert_eq!(&source[span.start as usize..span.end as usize], "self.x=1");
    let p = compile(source, "class").unwrap();
    assert!(p.program().code[1].class_body);
    assert!(!p.program().code[2].class_body);
    for bad in [
        "class C:\n    return 1",
        "def f():\n    class C:\n        return 1",
        "while True:\n    class C:\n        break",
        "class C:\n    nonlocal x",
    ] {
        assert_eq!(
            compile(bad, "bad").unwrap_err().kind,
            "SyntaxError",
            "{bad}"
        );
    }
    let p = compile(
        "class C:\n    def f(self):\n        return __class__",
        "class-cell",
    )
    .unwrap();
    assert_eq!(p.program().code[1].cell_locals.len(), 1);
    assert_eq!(p.program().code[2].free_vars.len(), 1);
}

#[test]
fn decorator_ast_and_bytecode_preserve_application_order() {
    let source = "@outer\n@factory(1)\ndef f(x=default()):\n    return x\n";
    let ast = parse(source, "decorators").unwrap();
    let StmtKind::Function { decorators, .. } = &ast.body[0].kind else {
        panic!("decorated function")
    };
    assert_eq!(decorators.len(), 2);
    assert_eq!(
        &source[decorators[0].span.start as usize..decorators[0].span.end as usize],
        "outer"
    );
    let program = compile(source, "decorators").unwrap();
    let module = &program.program().code[0];
    assert_eq!(
        module
            .instructions
            .iter()
            .filter(|i| i.opcode == Op::Call as u16)
            .count(),
        4
    );
}

#[test]
fn lambda_has_own_scope_span_and_function_bytecode() {
    let source = "x=2\nf=lambda a=1: lambda b:a+b+x\n";
    let ast = parse(source, "lambda").unwrap();
    let StmtKind::Assign(_, expression) = &ast.body[1].kind else {
        panic!("lambda assignment")
    };
    let ExprKind::Lambda { body, .. } = &expression.kind else {
        panic!("outer lambda")
    };
    assert_eq!(
        &source[expression.span.start as usize..expression.span.end as usize],
        "lambda a=1: lambda b:a+b+x"
    );
    assert!(matches!(body.kind, ExprKind::Lambda { .. }));
    let program = compile(source, "lambda").unwrap();
    assert_eq!(program.program().code.len(), 3);
    assert!(program.program().code[0]
        .instructions
        .iter()
        .any(|i| i.opcode == Op::Function as u16));
    let class_lambda = compile("class C:\n    f=lambda: __class__", "class-cell").unwrap();
    assert_eq!(class_lambda.program().code[1].cell_locals.len(), 1);
    assert_eq!(class_lambda.program().code[2].free_vars.len(), 1);
}

#[test]
fn module_graph_links_symbols_code_and_nested_import_discovery() {
    assert_eq!(
        discover_imports(
            "import first\nif True:\n    import package.second\ndef later():\n    from package import helper",
            "main.tonic",
        )
        .unwrap(),
        ["first", "package", "package.second", "package.helper"]
    );
    let linked = compile_modules(
        "__main__",
        &[
            ModuleSource::new(
                "__main__",
                "main.tonic",
                "import helper\nprint(helper.answer())",
            ),
            ModuleSource::new(
                "helper",
                "helper.tonic",
                "value=40\ndef answer():\n    return value+2",
            ),
        ],
    )
    .unwrap();
    let program = linked.program();
    assert_eq!(program.modules.len(), 2);
    assert_eq!(program.modules[0].code, 0);
    assert!(program.modules[1].code > 0);
    let helper_entry = usize::from(program.modules[1].code);
    let child = program.code[helper_entry]
        .functions
        .first()
        .expect("helper function site");
    assert!(child.code > program.modules[1].code);
    assert!(program.symbols.iter().any(|symbol| symbol == "helper"));
    assert!(program.symbols.iter().any(|symbol| symbol == "value"));

    let from = compile("from helper import answer as result", "from.tonic").unwrap();
    assert!(from.program().code[0]
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::ImportFrom as u16));
}
