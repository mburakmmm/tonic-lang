use tonic_compiler::{compile, compile_modules, discover_imports, parse, ModuleSource};
use tonic_core::{
    ast::{
        BinaryOp, CompareOp, ComprehensionKind, ExprKind, FormatConversion, PatternKind, StmtKind,
        Target, TypeParamKind,
    },
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
fn f_strings_have_owned_ast_and_verified_format_bytecode() {
    let source = "width=6\nresult=f'value={42!r:>{width}}'";
    let ast = parse(source, "f-string").unwrap();
    let StmtKind::Assign(_, expression) = &ast.body[1].kind else {
        panic!("f-string assignment")
    };
    let ExprKind::JoinedString(parts) = &expression.kind else {
        panic!("joined string")
    };
    assert_eq!(parts.len(), 2);
    let ExprKind::FormattedValue {
        conversion,
        format_spec,
        ..
    } = &parts[1].kind
    else {
        panic!("formatted value")
    };
    assert_eq!(*conversion, FormatConversion::Repr);
    assert!(matches!(
        format_spec.as_deref().map(|spec| &spec.kind),
        Some(ExprKind::JoinedString(_))
    ));

    let program = compile(source, "f-string").unwrap();
    let operations = program.program().code[0]
        .instructions
        .iter()
        .filter_map(|instruction| Op::try_from(instruction.opcode).ok())
        .collect::<Vec<_>>();
    assert!(operations.contains(&Op::Convert));
    assert!(operations.contains(&Op::FormatValue));
}

#[test]
fn identity_and_membership_comparisons_are_tonic_owned() {
    let source = "result = left is right is not other in values not in excluded";
    let ast = parse(source, "comparisons").unwrap();
    let StmtKind::Assign(_, expression) = &ast.body[0].kind else {
        panic!("comparison assignment")
    };
    let ExprKind::Compare(_, pairs) = &expression.kind else {
        panic!("comparison expression")
    };
    assert!(matches!(pairs[0].0, CompareOp::Is));
    assert!(matches!(pairs[1].0, CompareOp::IsNot));
    assert!(matches!(pairs[2].0, CompareOp::In));
    assert!(matches!(pairs[3].0, CompareOp::NotIn));

    let program = compile(source, "comparisons").unwrap();
    let operations = program.program().code[0]
        .instructions
        .iter()
        .filter_map(|instruction| Op::try_from(instruction.opcode).ok())
        .collect::<Vec<_>>();
    for expected in [Op::Is, Op::IsNot, Op::Contains, Op::NotContains] {
        assert!(operations.contains(&expected), "missing {expected:?}");
    }
}

#[test]
fn comprehensions_have_hidden_scopes_and_owned_bytecode() {
    let source = "result=[x*2 for x in source if x]\nunique={x%3 for x in source}\nmapping={x:x+1 for x in source}\nstream=(x for x in source)";
    let ast = parse(source, "comprehensions").unwrap();
    for (statement, expected) in ast.body.iter().zip([
        ComprehensionKind::List,
        ComprehensionKind::Set,
        ComprehensionKind::Dict,
        ComprehensionKind::Generator,
    ]) {
        let StmtKind::Assign(_, expression) = &statement.kind else {
            panic!("comprehension assignment")
        };
        let ExprKind::Comprehension(comprehension) = &expression.kind else {
            panic!("comprehension expression")
        };
        assert_eq!(comprehension.kind, expected);
        assert_eq!(comprehension.clauses.len(), 1);
    }

    let program = compile(source, "comprehensions").unwrap();
    let list = program
        .program()
        .code
        .iter()
        .find(|code| code.name.ends_with("<listcomp>"))
        .expect("list comprehension code");
    assert!(list
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::ListAppend as u16));
    let set = program
        .program()
        .code
        .iter()
        .find(|code| code.name.ends_with("<setcomp>"))
        .expect("set comprehension code");
    assert!(set
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::SetAdd as u16));
    let generator = program
        .program()
        .code
        .iter()
        .find(|code| code.name.ends_with("<genexpr>"))
        .expect("generator expression code");
    assert!(generator.generator);
    assert!(!generator.coroutine);
}

#[test]
fn async_comprehensions_have_coroutine_scopes_and_owned_bytecode() {
    let source = "async def collect(source):\n    values=[await transform(x) async for x in source if x]\n    awaited=[await transform(x) for x in [1,2]]\n    unique={await transform(x) async for x in source}\n    mapping={x:await transform(x) async for x in source}\n    return values,awaited,unique,mapping,(await transform(x) async for x in source)\nstream=(x async for x in source)";
    let ast = parse(source, "async-comprehensions").unwrap();
    let StmtKind::Function { body, .. } = &ast.body[0].kind else {
        panic!("async function")
    };
    for statement in &body[..4] {
        let StmtKind::Assign(_, expression) = &statement.kind else {
            panic!("comprehension assignment")
        };
        let ExprKind::Comprehension(comprehension) = &expression.kind else {
            panic!("comprehension expression")
        };
        assert!(comprehension.coroutine);
    }

    let program = compile(source, "async-comprehensions").unwrap();
    let list = program
        .program()
        .code
        .iter()
        .find(|code| code.name.ends_with("<listcomp>"))
        .expect("async list comprehension code");
    assert!(list.coroutine);
    assert!(!list.generator);
    assert!(list
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::GetANext as u16));
    let async_generators = program
        .program()
        .code
        .iter()
        .filter(|code| code.name.ends_with("<genexpr>"))
        .collect::<Vec<_>>();
    assert_eq!(async_generators.len(), 2);
    assert!(async_generators
        .iter()
        .all(|code| code.coroutine && code.generator));
    assert!(async_generators.iter().all(|code| code
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == Op::AsyncYield as u16)));

    let error = compile("result=[x async for x in source]", "invalid-async-comp")
        .expect_err("eager async comprehension outside async function");
    assert_eq!(error.kind, "SyntaxError");
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
fn basic_match_patterns_are_owned_and_lowered() {
    let source = "match subject:\n    case [head, *middle, tail] if allowed(head):\n        result=middle\n    case {'value': value, **rest}:\n        result=(value,rest)\n    case Point(x, y=2):\n        result=x\n    case 1 | 2 as selected:\n        result=selected\n    case None:\n        result=0\n    case other:\n        result=other\n";
    let ast = parse(source, "match-basic").unwrap();
    let StmtKind::Match { cases, .. } = &ast.body[0].kind else {
        panic!("match statement")
    };
    assert_eq!(cases.len(), 6);
    let PatternKind::Sequence(sequence) = &cases[0].pattern.kind else {
        panic!("sequence pattern")
    };
    assert_eq!(sequence.len(), 3);
    assert!(matches!(sequence[1].kind, PatternKind::Star(_)));
    assert!(cases[0].guard.is_some());
    let PatternKind::Mapping { rest, .. } = &cases[1].pattern.kind else {
        panic!("mapping pattern")
    };
    assert!(rest.is_some());
    assert!(matches!(cases[2].pattern.kind, PatternKind::Class { .. }));
    let PatternKind::As {
        pattern: Some(pattern),
        name: Some(_),
    } = &cases[3].pattern.kind
    else {
        panic!("as pattern")
    };
    assert!(matches!(pattern.kind, PatternKind::Or(_)));
    assert!(matches!(cases[4].pattern.kind, PatternKind::Singleton(_)));

    let program = compile(source, "match-basic").unwrap();
    let operations = program.program().code[0]
        .instructions
        .iter()
        .filter_map(|instruction| Op::try_from(instruction.opcode).ok())
        .collect::<Vec<_>>();
    assert!(operations.contains(&Op::Eq));
    assert!(operations.contains(&Op::Is));
    assert!(operations.contains(&Op::JumpFalse));
    assert!(operations.contains(&Op::MatchSequence));
    assert!(operations.contains(&Op::MatchMapping));
    assert!(operations.contains(&Op::MatchKey));
    assert!(operations.contains(&Op::MatchClass));
    assert!(operations.contains(&Op::MatchAttr));
    assert!(operations.contains(&Op::MatchArgs));
    assert!(operations.contains(&Op::MatchClassItem));
}

#[test]
fn function_annotations_are_owned_and_lowered() {
    let source = "def f(a: int, /, b: str = 'x', *args: tuple, c: float = 1.0, **kwargs: dict) -> bool:\n    return True\n";
    let ast = parse(source, "annotations").unwrap();
    let StmtKind::Function {
        params, returns, ..
    } = &ast.body[0].kind
    else {
        panic!("function")
    };
    assert!(params.positional[0].annotation.is_some());
    assert!(params.positional[1].annotation.is_some());
    assert!(params.vararg_annotation.is_some());
    assert!(params.keyword_only[0].annotation.is_some());
    assert!(params.kwarg_annotation.is_some());
    assert!(returns.is_some());

    let program = compile(source, "annotations").unwrap();
    let site = &program.program().code[0].functions[0];
    assert_eq!(site.annotations.len(), 6);
}

#[test]
fn variable_annotations_are_owned_and_lowered_by_scope() {
    let source = "value: int = 1\nmissing: str\nclass Holder:\n    item: float = 2.0\ndef local():\n    hidden: bytes\n    return 1\n";
    let ast = parse(source, "variable-annotations").unwrap();
    let StmtKind::AnnAssign {
        target,
        annotation,
        value,
        simple,
    } = &ast.body[0].kind
    else {
        panic!("annotated assignment")
    };
    assert!(matches!(target, Target::Name(_)));
    assert!(matches!(annotation.kind, ExprKind::Name(_)));
    assert!(value.is_some());
    assert!(*simple);

    let program = compile(source, "variable-annotations").unwrap();
    let module_ops = program.program().code[0]
        .instructions
        .iter()
        .filter_map(|instruction| Op::try_from(instruction.opcode).ok())
        .collect::<Vec<_>>();
    assert!(module_ops.contains(&Op::Dict));
    assert!(module_ops.contains(&Op::StoreGlobal));
    assert!(module_ops.contains(&Op::SetItem));

    let class_code = program
        .program()
        .code
        .iter()
        .find(|code| code.class_body)
        .unwrap();
    assert!(class_code
        .instructions
        .iter()
        .filter_map(|instruction| Op::try_from(instruction.opcode).ok())
        .any(|op| op == Op::SetItem));
    let local_code = program
        .program()
        .code
        .iter()
        .find(|code| code.name.ends_with("local"))
        .unwrap();
    assert!(!local_code
        .instructions
        .iter()
        .filter_map(|instruction| Op::try_from(instruction.opcode).ok())
        .any(|op| op == Op::SetItem));

    for invalid in [
        "def f():\n    global value\n    value: int\n",
        "def outer():\n    value=1\n    def inner():\n        nonlocal value\n        value: int\n",
    ] {
        assert_eq!(
            compile(invalid, "invalid-annotation").unwrap_err().kind,
            "SyntaxError"
        );
    }
}

#[test]
fn type_parameters_and_aliases_are_owned_and_lowered() {
    let source = "def identity[T: int, *Ts, **P](value: T) -> T:\n    return value\nclass Box[T]:\n    item: T\ntype Pair[T] = (T, T)\n";
    let ast = parse(source, "type-parameters").unwrap();
    let StmtKind::Function { type_params, .. } = &ast.body[0].kind else {
        panic!("generic function")
    };
    assert_eq!(type_params.len(), 3);
    assert!(matches!(
        type_params[0].kind,
        TypeParamKind::TypeVar { bound: Some(_) }
    ));
    assert!(matches!(type_params[1].kind, TypeParamKind::TypeVarTuple));
    assert!(matches!(type_params[2].kind, TypeParamKind::ParamSpec));
    let StmtKind::Class { type_params, .. } = &ast.body[1].kind else {
        panic!("generic class")
    };
    assert_eq!(type_params.len(), 1);
    let StmtKind::TypeAlias {
        type_params, value, ..
    } = &ast.body[2].kind
    else {
        panic!("generic alias")
    };
    assert_eq!(type_params.len(), 1);
    assert!(matches!(value.kind, ExprKind::Tuple(_)));

    let program = compile(source, "type-parameters").unwrap();
    let module = &program.program().code[0];
    let operations = module
        .instructions
        .iter()
        .filter_map(|instruction| Op::try_from(instruction.opcode).ok())
        .collect::<Vec<_>>();
    assert!(operations.contains(&Op::TypeParam));
    assert!(operations.contains(&Op::TypeAlias));
    assert_eq!(module.functions[0].type_params.len(), 3);
    let identity = program
        .program()
        .code
        .iter()
        .find(|code| code.name.ends_with("identity"))
        .unwrap();
    assert_eq!(identity.type_params.len(), 3);
    program.program().clone().verify().unwrap();
}

#[test]
fn matrix_multiplication_is_owned_and_lowered() {
    let source = "result = left @ right\nresult @= next_value\n";
    let ast = parse(source, "matrix-multiplication").unwrap();
    let StmtKind::Assign(_, value) = &ast.body[0].kind else {
        panic!("matrix assignment")
    };
    assert!(matches!(
        value.kind,
        ExprKind::Binary(_, BinaryOp::MatrixMultiply, _)
    ));
    assert!(matches!(
        ast.body[1].kind,
        StmtKind::AugAssign(_, BinaryOp::MatrixMultiply, _)
    ));

    let program = compile(source, "matrix-multiplication").unwrap();
    let operations = program.program().code[0]
        .instructions
        .iter()
        .filter_map(|instruction| Op::try_from(instruction.opcode).ok())
        .collect::<Vec<_>>();
    assert!(operations.contains(&Op::MatMul));
    assert!(operations.contains(&Op::InplaceMatMul));
    program.program().clone().verify().unwrap();
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
fn async_functions_and_await_have_owned_ast_and_coroutine_metadata() {
    let source = "async def inner():\n    return 1\nasync def outer():\n    return await inner()";
    let ast = parse(source, "coroutine").unwrap();
    let StmtKind::Function { is_async, body, .. } = &ast.body[1].kind else {
        panic!("async function")
    };
    assert!(*is_async);
    let StmtKind::Return(Some(value)) = &body[0].kind else {
        panic!("async return")
    };
    assert!(matches!(value.kind, ExprKind::Await(_)));

    let program = compile(source, "coroutine").unwrap();
    assert!(!program.program().code[0].coroutine);
    assert!(program.program().code[1].coroutine);
    assert!(program.program().code[2].coroutine);
    assert!(program.program().code[2]
        .instructions
        .iter()
        .any(|instruction| Op::try_from(instruction.opcode) == Ok(Op::GetAwaitable)));

    let program = compile("async def values():\n    yield 1", "async-generator").unwrap();
    let code = &program.program().code[1];
    assert!(code.generator);
    assert!(code.coroutine);
    assert!(code
        .instructions
        .iter()
        .any(|instruction| Op::try_from(instruction.opcode) == Ok(Op::AsyncYield)));

    for (source, message) in [
        (
            "async def values():\n    yield from []",
            "yield from inside async function",
        ),
        (
            "async def values():\n    yield 1\n    return 2",
            "return with value in async generator",
        ),
    ] {
        let error = compile(source, "invalid-async-generator").unwrap_err();
        assert_eq!(error.kind, "SyntaxError");
        assert!(error.message.contains(message), "{error:?}");
    }
}

#[test]
fn async_for_has_owned_ast_and_verified_coroutine_bytecode() {
    let source = "async def consume(items):\n    async for item in items:\n        pass\n    else:\n        return 1\n    return 2";
    let ast = parse(source, "async-for").unwrap();
    let StmtKind::Function { body, .. } = &ast.body[0].kind else {
        panic!("async function")
    };
    assert!(matches!(body[0].kind, StmtKind::AsyncFor(_, _, _, _)));

    let program = compile(source, "async-for").unwrap();
    let code = &program.program().code[1];
    assert!(code.coroutine);
    for expected in [
        Op::GetAIter,
        Op::GetANext,
        Op::GetAwaitable,
        Op::EndAsyncFor,
    ] {
        assert!(code
            .instructions
            .iter()
            .any(|instruction| Op::try_from(instruction.opcode) == Ok(expected)));
    }
    assert!(!code.exception_regions.is_empty());

    assert!(compile("async for item in items:\n    pass", "bad-async-for").is_err());
}

#[test]
fn async_with_has_owned_ast_and_verified_coroutine_bytecode() {
    let source = "async def use(manager):\n    async with manager as value:\n        return value";
    let ast = parse(source, "async-with").unwrap();
    let StmtKind::Function { body, .. } = &ast.body[0].kind else {
        panic!("async function")
    };
    let async_with = &body[0];
    let StmtKind::AsyncWith { items, body } = &async_with.kind else {
        panic!("async with")
    };
    assert_eq!((items.len(), body.len()), (1, 1));
    assert!(items[0].target.is_some());
    assert_eq!(
        &source[async_with.span.start as usize..async_with.span.end as usize],
        "async with manager as value:\n        return value"
    );

    let program = compile(source, "async-with").unwrap();
    let code = &program.program().code[1];
    assert!(code.coroutine);
    for expected in [
        Op::AsyncContextEnter,
        Op::AsyncContextExit,
        Op::GetAwaitable,
        Op::YieldFrom,
    ] {
        assert!(code
            .instructions
            .iter()
            .any(|instruction| Op::try_from(instruction.opcode) == Ok(expected)));
    }
    assert!(!code.exception_regions.is_empty());

    let error = compile("async with manager:\n    pass", "bad-async-with").unwrap_err();
    assert_eq!(error.kind, "SyntaxError");
    assert!(error.span.is_some());
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
