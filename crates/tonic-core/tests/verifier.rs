use tonic_core::{ast::Constant, bytecode::*, diagnostic::Span};
fn program() -> Program {
    Program {
        version: BYTECODE_VERSION,
        symbols: vec!["x".into()],
        modules: vec![ModuleInfo {
            name: "__main__".into(),
            filename: "test.tonic".into(),
            code: 0,
            code_count: 1,
            globals: vec![tonic_core::ast::SymbolId(0)],
        }],
        code: vec![CodeObject {
            class_body: false,
            generator: false,
            coroutine: false,
            name: "test".into(),
            params: 0,
            signature: Default::default(),
            locals: vec![],
            registers: 2,
            cell_locals: vec![],
            free_vars: vec![],
            functions: vec![],
            exception_regions: vec![],
            instructions: vec![
                Instr::new(Op::Const, 0, 0, 0),
                Instr::new(Op::Return, 0, 0, 0),
            ],
            spans: vec![Span::default(); 2],
            constants: vec![Constant::None],
            calls: vec![],
        }],
    }
}
#[test]
fn explicit_encoding_roundtrip() {
    assert_eq!(std::mem::size_of::<Instr>(), 8);
    let i = Instr::new(Op::Add, 1, 256, 65535);
    assert_eq!(i.to_bytes(), [10, 0, 1, 0, 0, 1, 255, 255]);
    assert_eq!(Instr::from_bytes(i.to_bytes()).to_bytes(), i.to_bytes());
}
#[test]
fn valid_program() {
    program().verify().unwrap();
    let mut raised = program();
    raised.code[0].instructions = vec![Instr::new(Op::Raise, 0, 0, 0)];
    raised.code[0].spans = vec![Span::default()];
    raised.verify().unwrap();
    let mut chained = program();
    chained.code[0].instructions = vec![Instr::new(Op::Raise, 0, 2, 1)];
    chained.code[0].spans = vec![Span::default()];
    chained.verify().unwrap();
}

#[test]
fn identity_and_membership_operands_are_verified() {
    for op in [Op::Is, Op::IsNot, Op::Contains, Op::NotContains] {
        let mut valid = program();
        valid.code[0]
            .instructions
            .insert(1, Instr::new(op, 0, 0, 1));
        valid.code[0].spans.insert(1, Span::default());
        valid.clone().verify().unwrap();

        let mut invalid = valid;
        invalid.code[0].instructions[1].c = invalid.code[0].registers;
        assert!(
            invalid.verify().is_err(),
            "{op:?} accepted an invalid register"
        );
    }
}

#[test]
fn list_append_operands_are_verified() {
    let mut valid = program();
    valid.code[0]
        .instructions
        .insert(1, Instr::new(Op::ListAppend, 0, 1, 0));
    valid.code[0].spans.insert(1, Span::default());
    valid.clone().verify().unwrap();

    let mut invalid = valid.clone();
    invalid.code[0].instructions[1].b = invalid.code[0].registers;
    assert!(invalid.verify().is_err());
    let mut invalid = valid;
    invalid.code[0].instructions[1].c = 1;
    assert!(invalid.verify().is_err());
}

#[test]
fn set_construction_operands_are_verified() {
    let mut valid = program();
    valid.code[0]
        .instructions
        .insert(1, Instr::new(Op::Set, 0, 0, 0));
    valid.code[0]
        .instructions
        .insert(2, Instr::new(Op::SetAdd, 0, 1, 0));
    valid.code[0].spans.insert(1, Span::default());
    valid.code[0].spans.insert(2, Span::default());
    valid.clone().verify().unwrap();

    let mut invalid = valid.clone();
    invalid.code[0].instructions[1].b = 1;
    assert!(invalid.verify().is_err());
    let mut invalid = valid;
    invalid.code[0].instructions[2].b = invalid.code[0].registers;
    assert!(invalid.verify().is_err());
}

#[test]
fn structural_match_operands_are_verified() {
    for op in [
        Op::MatchKey,
        Op::MatchClass,
        Op::MatchAttr,
        Op::MatchArgs,
        Op::MatchClassItem,
    ] {
        let mut valid = program();
        valid.code[0]
            .instructions
            .insert(1, Instr::new(op, 0, 1, 0));
        valid.code[0].spans.insert(1, Span::default());
        valid.clone().verify().unwrap();
        for operand in 0..3 {
            let mut invalid = valid.clone();
            let registers = invalid.code[0].registers;
            let instruction = &mut invalid.code[0].instructions[1];
            match operand {
                0 => instruction.a = registers,
                1 => instruction.b = registers,
                _ => instruction.c = registers,
            }
            assert!(invalid.verify().is_err(), "{op:?} operand {operand}");
        }
    }

    let mut sequence = program();
    sequence.code[0]
        .instructions
        .insert(1, Instr::new(Op::MatchSequence, 0, 1, 0x8001));
    sequence.code[0].spans.insert(1, Span::default());
    sequence.clone().verify().unwrap();
    sequence.code[0].instructions[1].b = sequence.code[0].registers;
    assert!(sequence.verify().is_err());

    let mut unique = program();
    unique.code[0]
        .instructions
        .insert(1, Instr::new(Op::MatchUnique, 0, 1, 0));
    unique.code[0].spans.insert(1, Span::default());
    unique.clone().verify().unwrap();
    unique.code[0].instructions[1].c = 2;
    assert!(unique.verify().is_err());

    let mut mapping = program();
    mapping.code[0]
        .instructions
        .insert(1, Instr::new(Op::MatchMapping, 0, 1, u16::MAX));
    mapping.code[0].spans.insert(1, Span::default());
    mapping.clone().verify().unwrap();
    mapping.code[0].instructions[1].b = mapping.code[0].registers;
    assert!(mapping.verify().is_err());
}

#[test]
fn yield_requires_generator_function_metadata() {
    let mut generator = program();
    generator.modules[0].code_count = 2;
    let mut code = generator.code[0].clone();
    code.generator = true;
    code.name = "generate".into();
    code.instructions = vec![
        Instr::new(Op::Yield, 0, 1, 0),
        Instr::new(Op::Return, 0, 0, 0),
    ];
    code.spans = vec![Span::default(); 2];
    generator.code.push(code);
    generator.clone().verify().unwrap();

    let mut delegating = generator.clone();
    delegating.code[1].instructions[0] = Instr::new(Op::YieldFrom, 0, 1, 1);
    delegating.clone().verify().unwrap();

    let mut outside = program();
    outside.code[0].instructions[0] = Instr::new(Op::Yield, 0, 1, 0);
    assert!(outside.verify().is_err());

    let mut outside = program();
    outside.code[0].instructions[0] = Instr::new(Op::YieldFrom, 0, 1, 1);
    assert!(outside.verify().is_err());

    let mut module = program();
    module.code[0].generator = true;
    assert!(module.verify().is_err());

    let mut class_body = generator;
    class_body.code[1].class_body = true;
    assert!(class_body.verify().is_err());
}

#[test]
fn await_requires_coroutine_metadata() {
    let mut coroutine = program();
    coroutine.modules[0].code_count = 2;
    let mut code = coroutine.code[0].clone();
    code.coroutine = true;
    code.name = "coroutine".into();
    code.instructions = vec![
        Instr::new(Op::GetAwaitable, 0, 1, 0),
        Instr::new(Op::YieldFrom, 0, 1, 2),
        Instr::new(Op::Return, 0, 0, 0),
    ];
    code.spans = vec![Span::default(); 3];
    coroutine.code.push(code);
    coroutine.clone().verify().unwrap();

    let mut outside = program();
    outside.code[0].instructions[0] = Instr::new(Op::GetAwaitable, 0, 1, 0);
    assert!(outside.verify().is_err());

    let mut async_generator = coroutine;
    async_generator.code[1].generator = true;
    async_generator.code[1]
        .instructions
        .insert(2, Instr::new(Op::AsyncYield, 0, 1, 0));
    async_generator.code[1].spans.push(Span::default());
    async_generator.verify().unwrap();

    let mut outside = program();
    outside.code[0].instructions[0] = Instr::new(Op::AsyncYield, 0, 1, 0);
    assert!(outside.verify().is_err());

    let mut coroutine_only = program();
    coroutine_only.code[0].coroutine = true;
    coroutine_only.code[0].instructions[0] = Instr::new(Op::AsyncYield, 0, 1, 0);
    assert!(coroutine_only.verify().is_err());
}

#[test]
fn async_iteration_requires_coroutine_metadata_and_valid_targets() {
    let mut eager_aiter = program();
    eager_aiter.code[0].instructions[0] = Instr::new(Op::GetAIter, 0, 1, 0);
    eager_aiter.clone().verify().unwrap();
    eager_aiter.code[0].instructions[0].b = eager_aiter.code[0].registers;
    assert!(eager_aiter.verify().is_err());

    let mut async_for = program();
    async_for.modules[0].code_count = 2;
    let mut code = async_for.code[0].clone();
    code.coroutine = true;
    code.name = "async_for".into();
    code.instructions = vec![
        Instr::new(Op::GetAIter, 0, 1, 0),
        Instr::new(Op::GetANext, 0, 1, 0),
        Instr::new(Op::EndAsyncFor, 0, 3, 0),
        Instr::new(Op::Return, 0, 0, 0),
    ];
    code.spans = vec![Span::default(); 4];
    async_for.code.push(code);
    async_for.clone().verify().unwrap();

    let mut outside = async_for.clone();
    outside.code[1].coroutine = false;
    assert!(outside.verify().is_err());

    let mut target = async_for.clone();
    target.code[1].instructions[2].b = 4;
    assert!(target.verify().is_err());

    let mut reserved = async_for;
    reserved.code[1].instructions[0].c = 1;
    assert!(reserved.verify().is_err());
}

#[test]
fn async_context_requires_coroutine_metadata_and_valid_registers() {
    let mut async_with = program();
    async_with.modules[0].code_count = 2;
    let mut code = async_with.code[0].clone();
    code.coroutine = true;
    code.name = "async_with".into();
    code.registers = 3;
    code.instructions = vec![
        Instr::new(Op::AsyncContextEnter, 0, 1, 2),
        Instr::new(Op::AsyncContextExit, 0, 1, 2),
        Instr::new(Op::Return, 0, 0, 0),
    ];
    code.spans = vec![Span::default(); 3];
    async_with.code.push(code);
    async_with.clone().verify().unwrap();

    let mut outside = async_with.clone();
    outside.code[1].coroutine = false;
    assert!(outside.verify().is_err());

    let mut invalid = async_with;
    invalid.code[1].instructions[1].c = 3;
    assert!(invalid.verify().is_err());
}
#[test]
fn rejects_opcode_operands_and_metadata() {
    let mut variants = Vec::new();
    let mut p = program();
    p.version += 1;
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0].opcode = 65535;
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0].a = 2;
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0].b = 2;
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0].c = 1;
    variants.push(p);
    let mut p = program();
    p.code[0].spans.clear();
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::Jump, 2, 0, 0);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::Call, 0, 1, 0);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::Function, 0, 42, 0);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::LoadGlobal, 0, 2, 0);
    variants.push(p);
    let mut p = program();
    p.code[0].calls.push(CallSite {
        first: 65535,
        count: 65535,
        keywords: vec![],
    });
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::Tuple, 0, 1, 2);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[1] = Instr::new(Op::Move, 0, 1, 0);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::Slice, 0, 0, 0);
    variants.push(p);
    let mut p = program();
    p.code[0].registers = 4;
    p.code[0].instructions[0] = Instr::new(Op::Slice, 0, 1, 1);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::DelAttr, 0, 1, 0);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::Raise, 0, 2, 2);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::Raise, 1, 1, 0);
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::ContextEnter, 0, 1, 2);
    variants.push(p);
    let mut p = program();
    p.modules[0].globals.push(tonic_core::ast::SymbolId(0));
    variants.push(p);
    let mut p = program();
    p.code[0].instructions[0] = Instr::new(Op::ImportFrom, 0, 1, 1);
    variants.push(p);
    for p in variants {
        assert!(p.verify().is_err());
    }
}
#[test]
fn deterministic_decoder_mutation_sweep() {
    let mut seed = 1u64;
    for _ in 0..10000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let i = Instr::from_bytes(seed.to_le_bytes());
        let mut p = program();
        p.code[0].instructions[0] = i;
        let _ = p.verify();
    }
}

#[test]
fn closure_and_signature_metadata() {
    use tonic_core::ast::SymbolId;
    let mut p = program();
    let mut parent = p.code[0].clone();
    parent.locals = vec![SymbolId(0)];
    parent.cell_locals = vec![0];
    parent.functions = vec![FunctionSite {
        code: 2,
        captures: vec![0],
        defaults: vec![1],
        annotations: vec![(SymbolId(0), 1)],
    }];
    let mut child = p.code[0].clone();
    child.params = 1;
    child.signature.positional = 1;
    child.signature.defaults = vec![0];
    child.locals = vec![SymbolId(0)];
    child.free_vars = vec![SymbolId(0)];
    p.code.extend([parent, child]);
    p.modules[0].code_count = 3;
    p.clone().verify().unwrap();

    let mut variants = Vec::new();
    let mut bad = p.clone();
    bad.code[1].functions[0].captures.clear();
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[1].functions[0].captures[0] = 1;
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[1].functions[0].defaults[0] = 2;
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[1].functions[0].defaults.clear();
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[1].functions[0].annotations[0].1 = 2;
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[1].functions[0].annotations[0].0 = SymbolId(1);
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[1].functions[0].annotations.push((SymbolId(0), 0));
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[1].cell_locals.push(0);
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[2].signature.posonly = 2;
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[2].signature.defaults = vec![1];
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[2].signature.defaults.push(0);
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[2].signature.vararg = Some(0);
    variants.push(bad);
    let mut bad = p.clone();
    bad.code[1].instructions[0] = Instr::new(Op::LoadCell, 0, 1, 0);
    variants.push(bad);
    for p in variants {
        assert!(p.verify().is_err());
    }
}

#[test]
fn expanded_argument_control_flow_balance() {
    let begin = Instr::new(Op::BeginArgs, 0, 0, 0);
    let arg = Instr::new(Op::ArgPos, 0, 0, 0);
    let call = Instr::new(Op::CallExpanded, 0, 1, 0);
    let ret = Instr::new(Op::Return, 0, 0, 0);
    let with = |instructions: Vec<Instr>| {
        let mut p = program();
        p.code[0].spans = vec![Span::default(); instructions.len()];
        p.code[0].instructions = instructions;
        p
    };
    with(vec![begin, begin, arg, call, arg, call, ret])
        .verify()
        .unwrap();
    for code in [
        vec![arg, ret],
        vec![call, ret],
        vec![begin, ret],
        vec![begin, Instr::new(Op::Jump, 0, 0, 0)],
        vec![Instr::new(Op::JumpFalse, 0, 2, 0), begin, call, ret],
    ] {
        assert!(with(code).verify().is_err());
    }
}

#[test]
fn keyword_windows_and_symbols() {
    use tonic_core::ast::SymbolId;
    let mut p = program();
    p.code[0].calls.push(CallSite {
        first: 0,
        count: 1,
        keywords: vec![SymbolId(0)],
    });
    p.clone().verify().unwrap();
    let mut bad = p.clone();
    bad.code[0].calls[0].keywords[0] = SymbolId(1);
    assert!(bad.verify().is_err());
    let mut bad = p.clone();
    bad.code[0].calls[0].first = 1;
    assert!(bad.verify().is_err());
    p.code[0].calls[0].count = 0;
    p.code[0].calls[0].keywords.push(SymbolId(0));
    assert!(p.verify().is_err());
}
#[test]
fn class_namespace_opcodes_and_metadata_are_checked() {
    let mut valid = program();
    let mut body = valid.code[0].clone();
    body.class_body = true;
    body.instructions[0] = Instr::new(Op::LoadName, 0, 0, 0);
    valid.code.push(body);
    valid.modules[0].code_count = 2;
    valid.clone().verify().unwrap();
    let mut bad = valid.clone();
    bad.code[0].instructions[0] = Instr::new(Op::LoadName, 0, 0, 0);
    assert!(bad.verify().is_err());
    let mut bad = valid.clone();
    bad.code[0].class_body = true;
    assert!(bad.verify().is_err());
    let mut bad = valid.clone();
    bad.code[1].instructions[0] = Instr::new(Op::ClassDeref, 0, 0, 0);
    assert!(bad.verify().is_err());
    let mut bad = valid.clone();
    bad.code[1].locals.push(tonic_core::ast::SymbolId(0));
    bad.code[1].cell_locals.push(0);
    assert!(bad.verify().is_err());
    let mut bad = valid.clone();
    bad.code[1].instructions[0] = Instr::new(Op::SetAttr, 0, 1, 1);
    assert!(bad.verify().is_err());
    let mut bad = valid.clone();
    bad.code[1].calls.push(CallSite {
        first: 0,
        count: 0,
        keywords: vec![tonic_core::ast::SymbolId(0)],
    });
    bad.code[1].instructions[0] = Instr::new(Op::Class, 0, 1, 0);
    assert!(bad.verify().is_err()); // symbol zero is not named `metaclass`
    valid.code[1].free_vars.push(tonic_core::ast::SymbolId(0));
    valid.code[1].instructions[0] = Instr::new(Op::ClassDeref, 0, 0, 0);
    valid.verify().unwrap();
}

#[test]
fn exception_regions_and_handler_operands_are_checked() {
    let mut valid = program();
    valid.code[0].exception_regions.push(ExceptionRegion {
        start: 0,
        end: 1,
        target: 1,
        exception: 1,
    });
    valid.clone().verify().unwrap();
    let mut bad = valid.clone();
    bad.code[0].exception_regions[0].end = 3;
    assert!(bad.verify().is_err());
    let mut bad = valid.clone();
    bad.code[0].exception_regions[0].exception = 2;
    assert!(bad.verify().is_err());
    let mut bad = valid.clone();
    bad.code[0].instructions[0] = Instr::new(Op::ExceptionMatch, 0, 1, 2);
    assert!(bad.verify().is_err());
    let mut bad = valid;
    bad.code[0].instructions[0] = Instr::new(Op::ClearException, 1, 0, 0);
    assert!(bad.verify().is_err());
}
