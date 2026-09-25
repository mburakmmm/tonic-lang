use std::fmt;

/// UTF-8 byte offsets, kept through AST, bytecode and runtime errors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: String,
    pub message: String,
    pub filename: Option<String>,
    pub span: Option<Span>,
    pub trace: Vec<(String, Span)>,
}
impl Diagnostic {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            filename: None,
            span: None,
            trace: Vec::new(),
        }
    }
    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
    pub fn in_file(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
    pub fn render(&self, filename: &str, source: &str) -> String {
        let mut text = format!("{}: {}", self.kind, self.message);
        if let Some(span) = self.span {
            let mut offset = (span.start as usize).min(source.len());
            while !source.is_char_boundary(offset) {
                offset -= 1;
            }
            let prefix = &source[..offset];
            let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            let content = source.lines().nth(line - 1).unwrap_or("");
            text.push_str(&format!(
                "\n  --> {filename}:{line}:{column}\n   | {content}\n   | {}^",
                " ".repeat(column - 1)
            ));
        }
        for (name, _) in &self.trace {
            text.push_str(&format!("\n  in {name}"));
        }
        text
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}
impl std::error::Error for Diagnostic {}
pub type Result<T> = std::result::Result<T, Diagnostic>;
