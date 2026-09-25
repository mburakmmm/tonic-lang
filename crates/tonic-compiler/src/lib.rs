mod hir;
mod lower;
mod modules;
mod parser;

pub use modules::{compile_modules, discover_imports, ModuleSource};
pub use parser::parse;
use tonic_core::{bytecode::VerifiedProgram, diagnostic::Result};

pub fn compile(source: &str, filename: &str) -> Result<VerifiedProgram> {
    compile_named(source, filename, "__main__")
}

pub fn compile_named(source: &str, filename: &str, module: &str) -> Result<VerifiedProgram> {
    lower::compile(parse(source, filename)?, module, filename)
        .and_then(|program| program.verify())
        .map_err(|error| {
            if error.filename.is_some() {
                error
            } else {
                error.in_file(filename)
            }
        })
}
