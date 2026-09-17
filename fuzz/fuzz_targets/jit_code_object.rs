#![no_main]

use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;
use tonic_compiler::compile;
use tonic_jit::{
    compile as compile_jit, is_direct_call_inlineable, is_direct_float_leaf_inlineable,
};

fn seed() -> &'static tonic_core::bytecode::CodeObject {
    static SEED: OnceLock<tonic_core::bytecode::CodeObject> = OnceLock::new();
    SEED.get_or_init(|| {
        compile(
            "def kernel(a,b):\n    total=a+b\n    while total<20:\n        total+=b\n    return total",
            "fuzz-jit",
        )
        .expect("static fuzz seed compiles")
        .program()
        .code[1]
        .clone()
    })
}

fuzz_target!(|data: &[u8]| {
    let mut code = seed().clone();
    if data.is_empty() {
        return;
    }
    code.registers = u16::from_le_bytes([data[0], *data.get(1).unwrap_or(&0)]);
    code.params = u16::from_le_bytes([*data.get(2).unwrap_or(&0), *data.get(3).unwrap_or(&0)]);
    for (index, instruction) in code.instructions.iter_mut().enumerate() {
        let offset = 4 + index * 8;
        let word = |at| {
            u16::from_le_bytes([
                *data.get(offset + at).unwrap_or(&0),
                *data.get(offset + at + 1).unwrap_or(&0),
            ])
        };
        instruction.opcode = word(0);
        instruction.a = word(2);
        instruction.b = word(4);
        instruction.c = word(6);
    }
    if data.last().is_some_and(|byte| byte & 1 != 0) {
        code.constants.clear();
    }
    if data.last().is_some_and(|byte| byte & 2 != 0) {
        code.calls.clear();
    }

    let _ = is_direct_call_inlineable(&code);
    let _ = is_direct_float_leaf_inlineable(&code);
    let _ = compile_jit(&code);
});
