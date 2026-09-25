use crate::{compile_named, parse};
use std::collections::{HashMap, HashSet};
use tonic_core::{
    ast::{Module, Stmt, StmtKind, SymbolId},
    bytecode::{ModuleInfo, Op, Program, VerifiedProgram, BYTECODE_VERSION},
    diagnostic::{Diagnostic, Result},
};

#[derive(Clone, Debug)]
pub struct ModuleSource {
    pub name: String,
    pub filename: String,
    pub source: String,
}

impl ModuleSource {
    pub fn new(
        name: impl Into<String>,
        filename: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            filename: filename.into(),
            source: source.into(),
        }
    }
}

pub fn discover_imports(source: &str, filename: &str) -> Result<Vec<String>> {
    let module = parse(source, filename)?;
    let mut imports = Vec::new();
    collect_imports(&module.body, &module, &mut imports);
    let mut seen = HashSet::new();
    imports.retain(|name| seen.insert(name.clone()));
    Ok(imports)
}

fn collect_imports(statements: &[Stmt], module: &Module, imports: &mut Vec<String>) {
    for statement in statements {
        match &statement.kind {
            StmtKind::Import(names) => {
                for alias in names {
                    imports.extend(
                        alias
                            .modules
                            .iter()
                            .map(|name| module.symbols[usize::from(name.0)].clone()),
                    );
                }
            }
            StmtKind::ImportFrom { modules, names } => {
                imports.extend(
                    modules
                        .iter()
                        .map(|name| module.symbols[usize::from(name.0)].clone()),
                );
                let base = &module.symbols[usize::from(modules.last().expect("module").0)];
                imports.extend(
                    names
                        .iter()
                        .map(|(name, _)| format!("{base}.{}", module.symbols[usize::from(name.0)])),
                );
            }
            StmtKind::Function { body, .. } | StmtKind::Class { body, .. } => {
                collect_imports(body, module, imports);
            }
            StmtKind::Try {
                body,
                handlers,
                otherwise,
                finalbody,
            } => {
                collect_imports(body, module, imports);
                for handler in handlers {
                    collect_imports(&handler.body, module, imports);
                }
                collect_imports(otherwise, module, imports);
                collect_imports(finalbody, module, imports);
            }
            StmtKind::With { body, .. } => collect_imports(body, module, imports),
            StmtKind::While(_, body, otherwise) | StmtKind::For(_, _, body, otherwise) => {
                collect_imports(body, module, imports);
                collect_imports(otherwise, module, imports);
            }
            StmtKind::If(_, body, otherwise) => {
                collect_imports(body, module, imports);
                collect_imports(otherwise, module, imports);
            }
            _ => {}
        }
    }
}

pub fn compile_modules(entry: &str, sources: &[ModuleSource]) -> Result<VerifiedProgram> {
    if sources.is_empty() {
        return Err(Diagnostic::new("ImportError", "module source set is empty"));
    }
    let mut names = HashSet::new();
    for source in sources {
        if source.name.is_empty() || !names.insert(source.name.as_str()) {
            return Err(Diagnostic::new(
                "ImportError",
                "module names must be non-empty and unique",
            ));
        }
    }
    if !names.contains(entry) {
        return Err(Diagnostic::new(
            "ImportError",
            format!("entry module '{entry}' is missing"),
        ));
    }
    let mut ordered = Vec::with_capacity(sources.len());
    ordered.push(
        sources
            .iter()
            .find(|source| source.name == entry)
            .expect("entry checked above"),
    );
    ordered.extend(sources.iter().filter(|source| source.name != entry));
    let programs = ordered
        .iter()
        .map(|source| compile_named(&source.source, &source.filename, &source.name))
        .collect::<Result<Vec<_>>>()?;
    link(&programs)
}

fn link(programs: &[VerifiedProgram]) -> Result<VerifiedProgram> {
    let mut symbols = Vec::<String>::new();
    let mut symbol_ids = HashMap::<String, u16>::new();
    let mut maps = Vec::with_capacity(programs.len());
    for verified in programs {
        let mut map = Vec::with_capacity(verified.program().symbols.len());
        for symbol in &verified.program().symbols {
            let id = if let Some(id) = symbol_ids.get(symbol) {
                *id
            } else {
                let id = u16::try_from(symbols.len()).map_err(|_| module_limit())?;
                symbols.push(symbol.clone());
                symbol_ids.insert(symbol.clone(), id);
                id
            };
            map.push(id);
        }
        maps.push(map);
    }
    let mut global_maps = Vec::with_capacity(programs.len());
    for verified in programs {
        let mut map = Vec::with_capacity(verified.program().symbols.len());
        for symbol in &verified.program().symbols {
            let id = u16::try_from(symbols.len()).map_err(|_| module_limit())?;
            symbols.push(symbol.clone());
            map.push(id);
        }
        global_maps.push(map);
    }

    let mut offsets = Vec::with_capacity(programs.len());
    let mut code_count = 0usize;
    for verified in programs {
        offsets.push(u16::try_from(code_count).map_err(|_| module_limit())?);
        code_count = code_count
            .checked_add(verified.program().code.len())
            .ok_or_else(module_limit)?;
        if code_count > u16::MAX as usize {
            return Err(module_limit());
        }
    }

    let mut code = Vec::with_capacity(code_count);
    let mut modules = Vec::with_capacity(programs.len());
    for (program_index, verified) in programs.iter().enumerate() {
        let program = verified.program();
        let map = &maps[program_index];
        let global_map = &global_maps[program_index];
        let offset = offsets[program_index];
        let source_module = program
            .modules
            .first()
            .ok_or_else(|| Diagnostic::new("BytecodeError", "linked program has no module"))?;
        modules.push(ModuleInfo {
            name: source_module.name.clone(),
            filename: source_module.filename.clone(),
            code: offset,
            code_count: u16::try_from(program.code.len()).map_err(|_| module_limit())?,
            globals: global_map.iter().copied().map(SymbolId).collect(),
        });
        for original in &program.code {
            let mut linked = original.clone();
            linked
                .locals
                .iter_mut()
                .for_each(|symbol| remap(symbol, map));
            linked
                .free_vars
                .iter_mut()
                .for_each(|symbol| remap(symbol, map));
            for call in &mut linked.calls {
                call.keywords
                    .iter_mut()
                    .for_each(|symbol| remap(symbol, map));
            }
            for site in &mut linked.functions {
                site.code = site.code.checked_add(offset).ok_or_else(module_limit)?;
            }
            for instruction in &mut linked.instructions {
                let op = Op::try_from(instruction.opcode)?;
                match op {
                    Op::LoadGlobal | Op::StoreGlobal | Op::LoadName => {
                        instruction.b = global_map[usize::from(instruction.b)];
                    }
                    Op::Import | Op::DelAttr | Op::StoreName | Op::ClassDeref | Op::ArgNamed => {
                        instruction.b = map[usize::from(instruction.b)]
                    }
                    Op::ImportFrom => {
                        instruction.c = map[usize::from(instruction.c)];
                    }
                    Op::ClearBinding if instruction.a == 1 => {
                        instruction.b = global_map[usize::from(instruction.b)];
                    }
                    Op::ClearBinding if instruction.a == 3 => {
                        instruction.b = map[usize::from(instruction.b)];
                    }
                    Op::Attr | Op::SetAttr => {
                        instruction.c = map[usize::from(instruction.c)];
                    }
                    _ => {}
                }
            }
            code.push(linked);
        }
    }
    Program {
        version: BYTECODE_VERSION,
        symbols,
        code,
        modules,
    }
    .verify()
}

fn remap(symbol: &mut SymbolId, map: &[u16]) {
    symbol.0 = map[usize::from(symbol.0)];
}

fn module_limit() -> Diagnostic {
    Diagnostic::new(
        "ResourceError",
        "linked module graph exceeds the 16-bit bytecode limit",
    )
}
