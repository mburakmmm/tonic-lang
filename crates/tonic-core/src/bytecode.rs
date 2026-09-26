use crate::{
    ast::{Constant, SymbolId},
    diagnostic::{Diagnostic, Result, Span},
};

pub const BYTECODE_VERSION: u16 = 15;
/// Explicit wire opcode numbers. Never serialize Rust enum layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Op {
    Const = 1,
    Move = 2,
    LoadGlobal = 3,
    StoreGlobal = 4,
    LoadCell = 5,
    StoreCell = 6,
    Add = 10,
    Sub = 11,
    Mul = 12,
    FloorDiv = 13,
    Mod = 14,
    Div = 15,
    Neg = 16,
    Pos = 17,
    Not = 18,
    InplaceAdd = 19,
    Eq = 20,
    Ne = 21,
    Lt = 22,
    Le = 23,
    Gt = 24,
    Ge = 25,
    Pow = 80,
    BitOr = 81,
    BitXor = 82,
    BitAnd = 83,
    LeftShift = 84,
    RightShift = 85,
    Invert = 86,
    InplaceSub = 87,
    InplaceMul = 88,
    InplaceDiv = 89,
    InplaceFloorDiv = 90,
    InplaceMod = 91,
    InplacePow = 92,
    InplaceBitOr = 93,
    InplaceBitXor = 94,
    InplaceBitAnd = 95,
    InplaceLeftShift = 96,
    InplaceRightShift = 97,
    Jump = 30,
    JumpFalse = 31,
    JumpTrue = 32,
    Call = 40,
    Return = 41,
    Function = 42,
    BeginArgs = 43,
    ArgPos = 44,
    ArgStar = 45,
    ArgNamed = 46,
    ArgMapping = 47,
    CallExpanded = 48,
    Tuple = 50,
    List = 51,
    Item = 52,
    Unpack = 53,
    Iter = 54,
    Next = 55,
    Dict = 56,
    SetItem = 57,
    DictMerge = 58,
    ImportFrom = 59,
    Import = 60,
    Attr = 61,
    SetAttr = 62,
    Class = 63,
    LoadName = 64,
    StoreName = 65,
    ClassDeref = 66,
    Slice = 67,
    DelAttr = 68,
    DelItem = 69,
    Raise = 70,
    ExceptionMatch = 71,
    ClearException = 72,
    ClearBinding = 73,
    PushException = 74,
    ContextEnter = 75,
    ContextExit = 76,
    Yield = 77,
}
impl TryFrom<u16> for Op {
    type Error = Diagnostic;
    fn try_from(value: u16) -> Result<Self> {
        Ok(match value {
            1 => Self::Const,
            2 => Self::Move,
            3 => Self::LoadGlobal,
            4 => Self::StoreGlobal,
            5 => Self::LoadCell,
            6 => Self::StoreCell,
            10 => Self::Add,
            11 => Self::Sub,
            12 => Self::Mul,
            13 => Self::FloorDiv,
            14 => Self::Mod,
            15 => Self::Div,
            16 => Self::Neg,
            17 => Self::Pos,
            18 => Self::Not,
            19 => Self::InplaceAdd,
            20 => Self::Eq,
            21 => Self::Ne,
            22 => Self::Lt,
            23 => Self::Le,
            24 => Self::Gt,
            25 => Self::Ge,
            80 => Self::Pow,
            81 => Self::BitOr,
            82 => Self::BitXor,
            83 => Self::BitAnd,
            84 => Self::LeftShift,
            85 => Self::RightShift,
            86 => Self::Invert,
            87 => Self::InplaceSub,
            88 => Self::InplaceMul,
            89 => Self::InplaceDiv,
            90 => Self::InplaceFloorDiv,
            91 => Self::InplaceMod,
            92 => Self::InplacePow,
            93 => Self::InplaceBitOr,
            94 => Self::InplaceBitXor,
            95 => Self::InplaceBitAnd,
            96 => Self::InplaceLeftShift,
            97 => Self::InplaceRightShift,
            30 => Self::Jump,
            31 => Self::JumpFalse,
            32 => Self::JumpTrue,
            40 => Self::Call,
            41 => Self::Return,
            42 => Self::Function,
            43 => Self::BeginArgs,
            44 => Self::ArgPos,
            45 => Self::ArgStar,
            46 => Self::ArgNamed,
            47 => Self::ArgMapping,
            48 => Self::CallExpanded,
            50 => Self::Tuple,
            51 => Self::List,
            52 => Self::Item,
            53 => Self::Unpack,
            54 => Self::Iter,
            55 => Self::Next,
            56 => Self::Dict,
            57 => Self::SetItem,
            58 => Self::DictMerge,
            59 => Self::ImportFrom,
            60 => Self::Import,
            61 => Self::Attr,
            62 => Self::SetAttr,
            63 => Self::Class,
            64 => Self::LoadName,
            65 => Self::StoreName,
            66 => Self::ClassDeref,
            67 => Self::Slice,
            68 => Self::DelAttr,
            69 => Self::DelItem,
            70 => Self::Raise,
            71 => Self::ExceptionMatch,
            72 => Self::ClearException,
            73 => Self::ClearBinding,
            74 => Self::PushException,
            75 => Self::ContextEnter,
            76 => Self::ContextExit,
            77 => Self::Yield,
            _ => {
                return Err(Diagnostic::new(
                    "BytecodeError",
                    format!("unknown opcode {value}"),
                ))
            }
        })
    }
}
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Instr {
    pub opcode: u16,
    pub a: u16,
    pub b: u16,
    pub c: u16,
}
impl Instr {
    pub fn new(op: Op, a: u16, b: u16, c: u16) -> Self {
        Self {
            opcode: op as u16,
            a,
            b,
            c,
        }
    }
    pub fn to_bytes(self) -> [u8; 8] {
        let mut bytes = [0; 8];
        for (i, x) in [self.opcode, self.a, self.b, self.c].iter().enumerate() {
            bytes[i * 2..i * 2 + 2].copy_from_slice(&x.to_le_bytes());
        }
        bytes
    }
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        let word = |i| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        Self {
            opcode: word(0),
            a: word(2),
            b: word(4),
            c: word(6),
        }
    }
}
#[derive(Clone, Debug)]
pub struct CallSite {
    pub first: u16,
    pub count: u16,
    /// Keyword values follow positional values in the same register window.
    pub keywords: Vec<SymbolId>,
}
#[derive(Clone, Debug)]
pub struct FunctionSite {
    pub code: u16,
    /// Indices into the creating frame's local-cell + free-cell window.
    pub captures: Vec<u16>,
    pub defaults: Vec<u16>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExceptionRegion {
    pub start: u16,
    pub end: u16,
    pub target: u16,
    pub exception: u16,
}
#[derive(Clone, Debug, Default)]
pub struct Signature {
    pub positional: u16,
    pub posonly: u16,
    pub keyword_only: u16,
    pub vararg: Option<u16>,
    pub kwarg: Option<u16>,
    /// Bound parameter slots, in definition-time default evaluation order.
    pub defaults: Vec<u16>,
}
#[derive(Clone, Debug)]
pub struct CodeObject {
    pub class_body: bool,
    pub generator: bool,
    pub name: String,
    pub params: u16,
    pub signature: Signature,
    pub locals: Vec<SymbolId>,
    pub registers: u16,
    pub instructions: Vec<Instr>,
    pub spans: Vec<Span>,
    pub constants: Vec<Constant>,
    pub calls: Vec<CallSite>,
    pub cell_locals: Vec<u16>,
    pub free_vars: Vec<SymbolId>,
    pub functions: Vec<FunctionSite>,
    pub exception_regions: Vec<ExceptionRegion>,
}
#[derive(Clone, Debug)]
pub struct Program {
    pub version: u16,
    pub symbols: Vec<String>,
    pub code: Vec<CodeObject>,
    pub modules: Vec<ModuleInfo>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleInfo {
    pub name: String,
    pub filename: String,
    pub code: u16,
    pub code_count: u16,
    pub globals: Vec<SymbolId>,
}
/// Immutable after validation; execution never accepts unverified instructions.
#[derive(Clone, Debug)]
pub struct VerifiedProgram(Program);
impl VerifiedProgram {
    pub fn program(&self) -> &Program {
        &self.0
    }
}
impl Program {
    pub fn verify(self) -> Result<VerifiedProgram> {
        let bad = |msg| Diagnostic::new("BytecodeError", msg);
        if self.version != BYTECODE_VERSION
            || self.code.is_empty()
            || self.modules.is_empty()
            || self.code.len() > u16::MAX as usize
            || self.symbols.len() > u16::MAX as usize
        {
            return Err(bad("invalid program header"));
        }
        let mut module_names = std::collections::HashSet::new();
        let mut module_entries = std::collections::HashSet::new();
        let mut module_globals = std::collections::HashSet::new();
        for (index, module) in self.modules.iter().enumerate() {
            if module.name.is_empty()
                || module.filename.is_empty()
                || !module_names.insert(module.name.as_str())
                || !module_entries.insert(module.code)
                || module.code as usize >= self.code.len()
                || module.code_count == 0
                || module.code as usize + module.code_count as usize > self.code.len()
                || module.globals.iter().any(|symbol| {
                    symbol.0 as usize >= self.symbols.len() || !module_globals.insert(symbol.0)
                })
                || (index == 0 && module.code != 0)
            {
                return Err(bad("invalid module table"));
            }
            let entry = &self.code[module.code as usize];
            if entry.params != 0
                || entry.class_body
                || entry.generator
                || !entry.cell_locals.is_empty()
                || !entry.free_vars.is_empty()
            {
                return Err(bad("module cannot have parameters"));
            }
        }
        let mut covered_code = vec![false; self.code.len()];
        for module in &self.modules {
            for slot in &mut covered_code
                [module.code as usize..module.code as usize + module.code_count as usize]
            {
                if *slot {
                    return Err(bad("overlapping module code ranges"));
                }
                *slot = true;
            }
        }
        if covered_code.iter().any(|covered| !covered) {
            return Err(bad("module code ranges do not cover the program"));
        }
        for code in &self.code {
            if code.class_body && (code.params != 0 || code.generator) {
                return Err(bad("class body cannot be a function"));
            }
            if code.instructions.is_empty()
                || code.instructions.len() > u16::MAX as usize
                || code.spans.len() != code.instructions.len()
                || code.registers == 0
                || code.params as usize > code.locals.len()
                || code.locals.len() > code.registers as usize
            {
                return Err(bad("invalid code metadata"));
            }
            let sig = &code.signature;
            let named = sig.positional as usize + sig.keyword_only as usize;
            let total =
                named + usize::from(sig.vararg.is_some()) + usize::from(sig.kwarg.is_some());
            if sig.posonly > sig.positional
                || total != code.params as usize
                || sig.vararg.is_some_and(|v| v as usize != named)
                || sig
                    .kwarg
                    .is_some_and(|v| v as usize != named + usize::from(sig.vararg.is_some()))
            {
                return Err(bad("invalid function signature"));
            }
            let mut defaults = std::collections::HashSet::new();
            for slot in &sig.defaults {
                if *slot as usize >= named || !defaults.insert(*slot) {
                    return Err(bad("invalid default parameter slot"));
                }
            }
            if code.locals[..code.params as usize]
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != code.params as usize
            {
                return Err(bad("duplicate parameter symbol"));
            }
            for local in &code.locals {
                if local.0 as usize >= self.symbols.len() {
                    return Err(bad("invalid local symbol"));
                }
            }
            let cell_count = code.cell_locals.len() + code.free_vars.len();
            if cell_count > u16::MAX as usize {
                return Err(bad("too many cells"));
            }
            let mut seen = std::collections::HashSet::new();
            for local in &code.cell_locals {
                if *local as usize >= code.locals.len() || !seen.insert(*local) {
                    return Err(bad("invalid or duplicate cell local"));
                }
            }
            if code.class_body
                && (code.cell_locals.len() > 1
                    || code.cell_locals.iter().any(|local| {
                        self.symbols[code.locals[*local as usize].0 as usize] != "__class__"
                    }))
            {
                return Err(bad("invalid class cell"));
            }
            for name in &code.free_vars {
                if name.0 as usize >= self.symbols.len() {
                    return Err(bad("invalid free variable symbol"));
                }
            }
            for site in &code.functions {
                if module_entries.contains(&site.code) || site.code as usize >= self.code.len() {
                    return Err(bad("function code out of bounds"));
                }
                let child = &self.code[site.code as usize];
                if site.captures.len() != child.free_vars.len() {
                    return Err(bad("closure capture count mismatch"));
                }
                if site.defaults.len() != child.signature.defaults.len()
                    || site.defaults.iter().any(|r| *r >= code.registers)
                {
                    return Err(bad("invalid function defaults"));
                }
                for capture in &site.captures {
                    if *capture as usize >= cell_count {
                        return Err(bad("closure capture out of bounds"));
                    }
                }
            }
            for region in &code.exception_regions {
                if region.start >= region.end
                    || region.end as usize > code.instructions.len()
                    || region.target as usize >= code.instructions.len()
                    || region.exception >= code.registers
                {
                    return Err(bad("invalid exception region"));
                }
            }
            for (index, left) in code.exception_regions.iter().enumerate() {
                for right in &code.exception_regions[index + 1..] {
                    let overlaps = left.start < right.end && right.start < left.end;
                    let nested = (left.start <= right.start && right.end <= left.end)
                        || (right.start <= left.start && left.end <= right.end);
                    if overlaps && !nested {
                        return Err(bad("partially overlapping exception regions"));
                    }
                }
            }
            let reg = |r: u16| -> Result<()> {
                if r < code.registers {
                    Ok(())
                } else {
                    Err(bad("register out of bounds"))
                }
            };
            let sym = |s: u16| -> Result<()> {
                if (s as usize) < self.symbols.len() {
                    Ok(())
                } else {
                    Err(bad("symbol out of bounds"))
                }
            };
            let jump = |j: u16| -> Result<()> {
                if (j as usize) < code.instructions.len() {
                    Ok(())
                } else {
                    Err(bad("jump out of bounds"))
                }
            };
            let range = |start: u16, count: u16| -> Result<()> {
                if start as u32 + count as u32 <= code.registers as u32 {
                    Ok(())
                } else {
                    Err(bad("register window out of bounds"))
                }
            };
            for call in &code.calls {
                let count = (call.count as usize)
                    .checked_add(call.keywords.len())
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| bad("call window too large"))?;
                range(call.first, count)?;
                let mut seen = std::collections::HashSet::new();
                for name in &call.keywords {
                    sym(name.0)?;
                    if !seen.insert(*name) {
                        return Err(bad("duplicate keyword argument"));
                    }
                }
            }
            for (pc, i) in code.instructions.iter().enumerate() {
                let op = Op::try_from(i.opcode)?;
                match op {
                    Op::LoadName | Op::StoreName | Op::ClassDeref => {
                        if !code.class_body {
                            return Err(bad("namespace opcode outside class body"));
                        }
                        reg(i.a)?;
                        sym(i.b)?;
                        if op == Op::ClassDeref {
                            if i.c as usize >= code.free_vars.len() {
                                return Err(bad("class closure operand out of bounds"));
                            }
                        } else if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::BeginArgs => {
                        if i.a != 0 || i.b != 0 || i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::ArgStar => {
                        reg(i.a)?;
                        if i.b != 0 || i.c > 1 {
                            return Err(bad("invalid star argument flags"));
                        }
                    }
                    Op::ArgPos | Op::ArgMapping | Op::Dict => {
                        reg(i.a)?;
                        if i.b != 0 || i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::ArgNamed => {
                        reg(i.a)?;
                        sym(i.b)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::CallExpanded | Op::DictMerge => {
                        reg(i.a)?;
                        reg(i.b)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::LoadCell | Op::StoreCell => {
                        reg(i.a)?;
                        if i.b as usize >= cell_count || i.c != 0 {
                            return Err(bad("invalid cell operand"));
                        }
                    }
                    Op::Const => {
                        reg(i.a)?;
                        if i.b as usize >= code.constants.len() {
                            return Err(bad("constant out of bounds"));
                        }
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::Move | Op::Neg | Op::Pos | Op::Invert | Op::Not | Op::Iter => {
                        reg(i.a)?;
                        reg(i.b)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::LoadGlobal | Op::StoreGlobal | Op::Import => {
                        reg(i.a)?;
                        sym(i.b)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::ImportFrom => {
                        reg(i.a)?;
                        reg(i.b)?;
                        sym(i.c)?;
                    }
                    Op::DelAttr => {
                        reg(i.a)?;
                        sym(i.b)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::DelItem => {
                        reg(i.a)?;
                        reg(i.b)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::Add
                    | Op::InplaceAdd
                    | Op::Sub
                    | Op::Mul
                    | Op::FloorDiv
                    | Op::Mod
                    | Op::Div
                    | Op::Eq
                    | Op::Ne
                    | Op::Lt
                    | Op::Le
                    | Op::Gt
                    | Op::Ge
                    | Op::Pow
                    | Op::BitOr
                    | Op::BitXor
                    | Op::BitAnd
                    | Op::LeftShift
                    | Op::RightShift
                    | Op::InplaceSub
                    | Op::InplaceMul
                    | Op::InplaceDiv
                    | Op::InplaceFloorDiv
                    | Op::InplaceMod
                    | Op::InplacePow
                    | Op::InplaceBitOr
                    | Op::InplaceBitXor
                    | Op::InplaceBitAnd
                    | Op::InplaceLeftShift
                    | Op::InplaceRightShift
                    | Op::Item
                    | Op::SetItem => {
                        reg(i.a)?;
                        reg(i.b)?;
                        reg(i.c)?;
                    }
                    Op::Slice => {
                        reg(i.a)?;
                        range(i.b, 3)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::Jump => {
                        jump(i.a)?;
                        if i.b != 0 || i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::JumpFalse | Op::JumpTrue => {
                        reg(i.a)?;
                        jump(i.b)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::Return => {
                        reg(i.a)?;
                        if i.b != 0 || i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::Yield => {
                        if !code.generator {
                            return Err(bad("yield outside generator code"));
                        }
                        reg(i.a)?;
                        reg(i.b)?;
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::Raise => {
                        if i.b > 2 || (i.b != 2 && i.c != 0) {
                            return Err(bad("invalid raise operand"));
                        }
                        match i.b {
                            0 => reg(i.a)?,
                            1 if i.a != 0 => return Err(bad("nonzero bare raise operand")),
                            1 => {}
                            2 => {
                                reg(i.a)?;
                                reg(i.c)?;
                            }
                            _ => unreachable!(),
                        }
                    }
                    Op::ExceptionMatch => {
                        reg(i.a)?;
                        reg(i.b)?;
                        reg(i.c)?;
                    }
                    Op::ClearException => {
                        if i.a != 0 || i.b != 0 || i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::PushException => {
                        reg(i.a)?;
                        if i.b != 0 || i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::ContextEnter | Op::ContextExit => {
                        reg(i.a)?;
                        reg(i.b)?;
                        reg(i.c)?;
                    }
                    Op::ClearBinding => {
                        if i.a > 3 || i.c != 0 {
                            return Err(bad("invalid clear-binding operand"));
                        }
                        match i.a {
                            0 => reg(i.b)?,
                            1 => sym(i.b)?,
                            2 if i.b as usize >= cell_count => {
                                return Err(bad("clear cell out of bounds"));
                            }
                            2 => {}
                            3 if !code.class_body => {
                                return Err(bad("namespace clear outside class body"));
                            }
                            3 => sym(i.b)?,
                            _ => unreachable!(),
                        }
                    }
                    Op::Function => {
                        reg(i.a)?;
                        if i.b as usize >= code.functions.len() {
                            return Err(bad("function site out of bounds"));
                        }
                        if i.c != 0 {
                            return Err(bad("nonzero reserved operand"));
                        }
                    }
                    Op::Call | Op::Class => {
                        reg(i.a)?;
                        reg(i.b)?;
                        if i.c as usize >= code.calls.len() {
                            return Err(bad("call site out of bounds"));
                        }
                        if op == Op::Class {
                            let keywords = &code.calls[i.c as usize].keywords;
                            if keywords.len() > 1
                                || keywords
                                    .iter()
                                    .any(|name| self.symbols[name.0 as usize] != "metaclass")
                            {
                                return Err(bad("invalid class keyword window"));
                            }
                        }
                    }
                    Op::Tuple | Op::List => {
                        reg(i.a)?;
                        range(i.b, i.c)?;
                    }
                    Op::Unpack => {
                        reg(i.b)?;
                        range(i.a, i.c)?;
                    }
                    Op::Next => {
                        reg(i.a)?;
                        reg(i.b)?;
                        jump(i.c)?;
                    }
                    Op::Attr | Op::SetAttr => {
                        reg(i.a)?;
                        reg(i.b)?;
                        sym(i.c)?;
                    }
                }
                if pc + 1 == code.instructions.len()
                    && !matches!(op, Op::Jump | Op::Return | Op::Raise)
                {
                    return Err(bad("code can fall off end"));
                }
            }
        }
        for code in &self.code {
            verify_argument_stack(code)?;
        }
        Ok(VerifiedProgram(self))
    }
    pub fn disassemble(&self) -> String {
        let mut text = format!("Tonic bytecode v{}\n", self.version);
        for (id, code) in self.code.iter().enumerate() {
            text.push_str(&format!(
                "code {id} {}: {} registers, {} parameters\n",
                code.name, code.registers, code.params
            ));
            for (pc, i) in code.instructions.iter().enumerate() {
                text.push_str(&format!(
                    "  {pc:04} {:?} {} {} {}\n",
                    Op::try_from(i.opcode),
                    i.a,
                    i.b,
                    i.c
                ));
            }
        }
        text
    }
}

/// Verify scratch argument-stack depth on all reachable CFG edges.
fn verify_argument_stack(code: &CodeObject) -> Result<()> {
    let bad = || Diagnostic::new("BytecodeError", "unbalanced expanded argument stack");
    let mut depths = vec![None; code.instructions.len()];
    let mut work = vec![(0usize, 0usize)];
    work.extend(
        code.exception_regions
            .iter()
            .map(|region| (region.target as usize, 0usize)),
    );
    while let Some((pc, mut depth)) = work.pop() {
        if let Some(previous) = depths[pc] {
            if previous != depth {
                return Err(bad());
            }
            continue;
        }
        depths[pc] = Some(depth);
        let i = code.instructions[pc];
        let op = Op::try_from(i.opcode)?;
        match op {
            Op::BeginArgs => depth += 1,
            Op::CallExpanded => {
                depth = depth.checked_sub(1).ok_or_else(bad)?;
            }
            Op::ArgPos | Op::ArgStar | Op::ArgNamed | Op::ArgMapping if depth == 0 => {
                return Err(bad())
            }
            _ => {}
        }
        match op {
            Op::Return | Op::Raise | Op::Yield => {
                if depth != 0 {
                    return Err(bad());
                }
            }
            Op::Jump => work.push((i.a as usize, depth)),
            Op::JumpFalse | Op::JumpTrue => {
                work.push((i.b as usize, depth));
                work.push((pc + 1, depth));
            }
            Op::Next => {
                work.push((i.c as usize, depth));
                work.push((pc + 1, depth));
            }
            _ => work.push((pc + 1, depth)),
        }
    }
    Ok(())
}
