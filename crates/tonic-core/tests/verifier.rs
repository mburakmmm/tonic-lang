use tonic_core::{ast::Constant, bytecode::*, diagnostic::Span};
fn program() -> Program {
    Program {
        version: BYTECODE_VERSION,
        symbols: vec!["x".into()],
        code: vec![CodeObject {
            class_body: false,
            name: "test".into(),
            params: 0,
            signature: Default::default(),
            locals: vec![],
            registers: 2,
            cell_locals: vec![],
            free_vars: vec![],
            functions: vec![],
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
    }];
    let mut child = p.code[0].clone();
    child.params = 1;
    child.signature.positional = 1;
    child.signature.defaults = vec![0];
    child.locals = vec![SymbolId(0)];
    child.free_vars = vec![SymbolId(0)];
    p.code.extend([parent, child]);
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
    assert!(bad.verify().is_err());
    valid.code[1].free_vars.push(tonic_core::ast::SymbolId(0));
    valid.code[1].instructions[0] = Instr::new(Op::ClassDeref, 0, 0, 0);
    valid.verify().unwrap();
}
