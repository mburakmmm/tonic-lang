use tonic_core::{
    ast::Constant,
    bytecode::{CallSite, CodeObject, Instr, Op, Program, BYTECODE_VERSION},
    diagnostic::Span,
};
use tonic_runtime::Vm;
fn program(instructions: Vec<Instr>) -> Program {
    Program {
        version: BYTECODE_VERSION,
        symbols: vec!["x".into()],
        code: vec![CodeObject {
            class_body: false,
            name: "direct".into(),
            params: 0,
            signature: Default::default(),
            locals: vec![],
            registers: 4,
            cell_locals: vec![],
            free_vars: vec![],
            functions: vec![],
            exception_regions: vec![],
            spans: vec![Span::default(); instructions.len()],
            instructions,
            constants: vec![Constant::Int("1".into())],
            calls: vec![CallSite {
                first: 0,
                count: 1,
                keywords: vec![],
            }],
        }],
    }
}
#[test]
fn verified_but_uninitialized_register_is_a_guest_error() {
    let p = program(vec![Instr::new(Op::Return, 0, 0, 0)])
        .verify()
        .unwrap();
    assert_eq!(
        Vm::new()
            .unwrap()
            .run(&p, &mut Vec::new())
            .unwrap_err()
            .kind,
        "UnboundLocalError"
    );
}
#[test]
fn verified_infinite_loop_is_interruptible() {
    let p = program(vec![Instr::new(Op::Jump, 0, 0, 0)])
        .verify()
        .unwrap();
    let mut vm = Vm::new().unwrap();
    vm.limits.instructions = Some(10);
    assert_eq!(
        vm.run(&p, &mut Vec::new()).unwrap_err().kind,
        "ResourceError"
    );
}
#[test]
fn invalid_integer_payload_is_rejected_without_panicking() {
    let mut p = program(vec![
        Instr::new(Op::Const, 0, 0, 0),
        Instr::new(Op::Return, 0, 0, 0),
    ]);
    p.code[0].constants[0] = Constant::Int("not an integer".into());
    let p = p.verify().unwrap();
    assert_eq!(
        Vm::new()
            .unwrap()
            .run(&p, &mut Vec::new())
            .unwrap_err()
            .kind,
        "BytecodeError"
    );
}
#[test]
fn bounded_execution_of_mutated_bytecode() {
    let opcodes = [
        1, 2, 3, 4, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 30, 31, 32, 40,
        41, 42, 50, 51, 52, 53, 54, 55, 60, 61, 67, 68,
    ];
    let mut seed = 19u64;
    let mut accepted = 0;
    for _ in 0..5000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mutation = Instr {
            opcode: opcodes[seed as usize % opcodes.len()],
            a: ((seed >> 8) % 5) as u16,
            b: ((seed >> 16) % 5) as u16,
            c: ((seed >> 24) % 5) as u16,
        };
        let p = program(vec![
            Instr::new(Op::Const, 0, 0, 0),
            mutation,
            Instr::new(Op::Return, 0, 0, 0),
        ]);
        if let Ok(p) = p.verify() {
            accepted += 1;
            let mut vm = Vm::new().unwrap();
            vm.limits.instructions = Some(30);
            let _ = vm.run(&p, &mut Vec::new());
        }
    }
    assert!(accepted > 100);
}
