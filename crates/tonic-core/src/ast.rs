use crate::diagnostic::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u16);
#[derive(Clone, Debug)]
pub struct Module {
    pub body: Vec<Stmt>,
    pub symbols: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum StmtKind {
    Assign(Vec<Target>, Expr),
    AugAssign(Target, BinaryOp, Expr),
    DeleteAttributes(Vec<(Expr, SymbolId)>),
    Expr(Expr),
    Function {
        name: SymbolId,
        label: String,
        decorators: Vec<Expr>,
        params: Parameters,
        body: Vec<Stmt>,
    },
    Class {
        name: SymbolId,
        /// Synthetic lexical binding populated with the completed class.
        class_cell: SymbolId,
        label: String,
        decorators: Vec<Expr>,
        bases: Vec<Expr>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>, Vec<Stmt>),
    For(Target, Expr, Vec<Stmt>, Vec<Stmt>),
    Import(Vec<(SymbolId, SymbolId)>),
    Global(Vec<SymbolId>),
    Nonlocal(Vec<SymbolId>),
    Break,
    Continue,
    Pass,
}
#[derive(Clone, Debug)]
pub enum Target {
    Name(SymbolId),
    Tuple(Vec<Target>),
    Item(Expr, Expr),
    Attribute(Expr, SymbolId),
}
#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum ExprKind {
    Constant(Constant),
    Name(SymbolId),
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Bool(bool, Vec<Expr>),
    Compare(Box<Expr>, Vec<(CompareOp, Expr)>),
    Call(Box<Expr>, CallArguments),
    Dict(Vec<(Option<Expr>, Expr)>),
    Attribute(Box<Expr>, SymbolId),
    Subscript(Box<Expr>, Box<Expr>),
    Slice {
        start: Option<Box<Expr>>,
        stop: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Lambda {
        params: Parameters,
        body: Box<Expr>,
    },
}
#[derive(Clone, Debug, Default)]
pub struct Parameters {
    pub positional: Vec<Parameter>,
    pub posonly: u16,
    pub keyword_only: Vec<Parameter>,
    pub vararg: Option<SymbolId>,
    pub kwarg: Option<SymbolId>,
}
#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: SymbolId,
    pub default: Option<Expr>,
}
impl Parameters {
    pub fn names(&self) -> Vec<SymbolId> {
        self.positional
            .iter()
            .chain(&self.keyword_only)
            .map(|p| p.name)
            .chain(self.vararg)
            .chain(self.kwarg)
            .collect()
    }
    pub fn defaults(&self) -> impl Iterator<Item = (usize, &Expr)> {
        self.positional
            .iter()
            .chain(&self.keyword_only)
            .enumerate()
            .filter_map(|(i, p)| p.default.as_ref().map(|d| (i, d)))
    }
}
#[derive(Clone, Debug, Default)]
pub struct CallArguments {
    pub positional: Vec<(bool, Expr)>,
    pub keywords: Vec<(Option<SymbolId>, Expr)>,
    /// A zero-argument `super()` in a class-nested function needs the class cell.
    pub implicit_class: Option<SymbolId>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Constant {
    None,
    Bool(bool),
    Int(String),
    Float(f64),
    Str(String),
}
#[derive(Clone, Copy, Debug)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    FloorDivide,
    Modulo,
    Divide,
}
#[derive(Clone, Copy, Debug)]
pub enum UnaryOp {
    Negative,
    Positive,
    Not,
}
#[derive(Clone, Copy, Debug)]
pub enum CompareOp {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
