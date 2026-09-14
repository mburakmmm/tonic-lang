mod hir;
mod lower;
mod parser;

pub use parser::parse;
use tonic_core::{bytecode::VerifiedProgram, diagnostic::Result};

pub fn compile(source: &str, filename: &str) -> Result<VerifiedProgram> {
    lower::compile(parse(source, filename)?)?.verify()
}
