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
pub struct ImportAlias {
    /// Increasing absolute module prefixes (`a`, `a.b`, `a.b.c`).
    pub modules: Vec<SymbolId>,
    pub bound: SymbolId,
    /// An explicit alias binds the leaf module; otherwise Python binds the
    /// top-level package for a dotted import.
    pub bind_leaf: bool,
}
#[derive(Clone, Debug)]
pub enum StmtKind {
    Assign(Vec<Target>, Expr),
    AnnAssign {
        target: Target,
        annotation: Expr,
        value: Option<Expr>,
        simple: bool,
    },
    AugAssign(Target, BinaryOp, Expr),
    DeleteTargets(Vec<Target>),
    Expr(Expr),
    TypeAlias {
        name: SymbolId,
        type_params: Vec<TypeParam>,
        value: Expr,
    },
    Function {
        name: SymbolId,
        label: String,
        is_async: bool,
        decorators: Vec<Expr>,
        type_params: Vec<TypeParam>,
        params: Parameters,
        returns: Option<Expr>,
        body: Vec<Stmt>,
    },
    Class {
        name: SymbolId,
        /// Synthetic lexical binding populated with the completed class.
        class_cell: SymbolId,
        label: String,
        decorators: Vec<Expr>,
        type_params: Vec<TypeParam>,
        bases: Vec<Expr>,
        metaclass: Option<(SymbolId, Expr)>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    Raise {
        value: Option<Expr>,
        cause: Option<Expr>,
    },
    Try {
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        otherwise: Vec<Stmt>,
        finalbody: Vec<Stmt>,
    },
    With {
        items: Vec<WithItem>,
        body: Vec<Stmt>,
    },
    AsyncWith {
        items: Vec<WithItem>,
        body: Vec<Stmt>,
    },
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>, Vec<Stmt>),
    For(Target, Expr, Vec<Stmt>, Vec<Stmt>),
    AsyncFor(Target, Expr, Vec<Stmt>, Vec<Stmt>),
    Match {
        subject: Expr,
        cases: Vec<MatchCase>,
    },
    Import(Vec<ImportAlias>),
    ImportFrom {
        /// Increasing absolute module prefixes, ending in the source module.
        modules: Vec<SymbolId>,
        names: Vec<(SymbolId, SymbolId)>,
    },
    Global(Vec<SymbolId>),
    Nonlocal(Vec<SymbolId>),
    Break,
    Continue,
    Pass,
}
#[derive(Clone, Debug)]
pub struct TypeParam {
    pub name: SymbolId,
    pub kind: TypeParamKind,
    pub default: Option<Expr>,
    /// `*Ts = *tuple[int, str]` preserves the unpack marker separately from
    /// the default expression so lowering never needs a general starred
    /// expression node outside an expansion context.
    pub unpacked_default: bool,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum TypeParamKind {
    TypeVar { bound: Option<Expr> },
    ParamSpec,
    TypeVarTuple,
}
#[derive(Clone, Debug)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum PatternKind {
    Value(Expr),
    Singleton(Constant),
    Sequence(Vec<Pattern>),
    Mapping {
        keys: Vec<Expr>,
        patterns: Vec<Pattern>,
        rest: Option<SymbolId>,
    },
    Class {
        class: Expr,
        positional: Vec<Pattern>,
        keyword_names: Vec<SymbolId>,
        keyword_patterns: Vec<Pattern>,
    },
    Star(Option<SymbolId>),
    As {
        pattern: Option<Box<Pattern>>,
        name: Option<SymbolId>,
    },
    Or(Vec<Pattern>),
}
#[derive(Clone, Debug)]
pub struct ExceptHandler {
    pub type_: Option<Expr>,
    pub name: Option<SymbolId>,
    pub body: Vec<Stmt>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct WithItem {
    pub context: Expr,
    pub target: Option<Target>,
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
    Set(Vec<Expr>),
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
    Yield(Option<Box<Expr>>),
    YieldFrom(Box<Expr>),
    Await(Box<Expr>),
    JoinedString(Vec<Expr>),
    FormattedValue {
        value: Box<Expr>,
        conversion: FormatConversion,
        format_spec: Option<Box<Expr>>,
    },
    Lambda {
        params: Parameters,
        body: Box<Expr>,
    },
    Comprehension(Comprehension),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatConversion {
    None,
    Str,
    Repr,
    Ascii,
}
#[derive(Clone, Debug)]
pub struct Comprehension {
    pub kind: ComprehensionKind,
    /// The hidden comprehension code suspends through `await` and/or an
    /// asynchronous `for` clause.
    pub coroutine: bool,
    /// Hidden positional parameter receiving the already-created outer iterator.
    pub iterator_parameter: SymbolId,
    /// Hidden accumulator local used by eager list/dict comprehensions.
    pub accumulator: SymbolId,
    /// List/gen element, or the key for a dict comprehension.
    pub element: Box<Expr>,
    /// Present only for a dict comprehension.
    pub value: Option<Box<Expr>>,
    pub clauses: Vec<ComprehensionClause>,
}
#[derive(Clone, Debug)]
pub struct ComprehensionClause {
    pub target: Target,
    pub iterable: Expr,
    pub filters: Vec<Expr>,
    pub is_async: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComprehensionKind {
    List,
    Set,
    Dict,
    Generator,
}
#[derive(Clone, Debug, Default)]
pub struct Parameters {
    pub positional: Vec<Parameter>,
    pub posonly: u16,
    pub keyword_only: Vec<Parameter>,
    pub vararg: Option<SymbolId>,
    pub kwarg: Option<SymbolId>,
    pub vararg_annotation: Option<Box<Expr>>,
    pub kwarg_annotation: Option<Box<Expr>>,
}
#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: SymbolId,
    pub default: Option<Expr>,
    pub annotation: Option<Expr>,
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
    MatrixMultiply,
    Power,
    BitOr,
    BitXor,
    BitAnd,
    LeftShift,
    RightShift,
    FloorDivide,
    Modulo,
    Divide,
}
#[derive(Clone, Copy, Debug)]
pub enum UnaryOp {
    Negative,
    Positive,
    Invert,
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
    Is,
    IsNot,
    In,
    NotIn,
}
