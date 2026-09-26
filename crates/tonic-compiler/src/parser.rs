//! Replaceable parser adapter. No RustPython types escape this module.
use py::Ranged;
use rustpython_parser::{ast as py, lexer, Mode, Parse, Tok};
use std::collections::HashMap;
use tonic_core::{
    ast::*,
    diagnostic::{Diagnostic, Result, Span},
};

pub fn parse(source: &str, filename: &str) -> Result<Module> {
    if source.len() > 1_048_576 {
        return Err(
            Diagnostic::new("ResourceError", "bootstrap source limit is 1 MiB").in_file(filename),
        );
    }
    // BOOTSTRAP: bound recursive parser/AST lowering and destruction depth.
    // Logical lines include bracket continuations; large flat modules remain valid.
    let (mut tokens, mut indent, mut compounds) = (0usize, 0usize, 0usize);
    for token in lexer::lex(source, Mode::Module) {
        let Ok((token, range)) = token else {
            break;
        }; // Parser reports lexical errors.
        match token {
            Tok::Newline => tokens = 0,
            Tok::Indent => indent += 1,
            Tok::Dedent => indent = indent.saturating_sub(1),
            Tok::If | Tok::Elif | Tok::While | Tok::For | Tok::Def | Tok::Class | Tok::Try => {
                compounds += 1
            }
            _ => {}
        }
        tokens += 1;
        if tokens > 256 || indent > 64 || compounds > 128 {
            return Err(Diagnostic::new(
                "ResourceError",
                "bootstrap syntax complexity limit exceeded",
            )
            .in_file(filename)
            .at(Span {
                start: range.start().into(),
                end: range.end().into(),
            }));
        }
    }
    let suite = py::Suite::parse(source, filename).map_err(|e| {
        let start = u32::from(e.offset);
        Diagnostic::new("SyntaxError", e.error.to_string())
            .in_file(filename)
            .at(Span { start, end: start })
    })?;
    let mut adapter = Adapter {
        symbols: Vec::new(),
        names: HashMap::new(),
        depth: 0,
        class_name: None,
    };
    let body = adapter
        .block(suite)
        .map_err(|error| error.in_file(filename))?;
    Ok(Module {
        body,
        symbols: adapter.symbols,
    })
}
fn span(node: &impl Ranged) -> Span {
    Span {
        start: node.start().into(),
        end: node.end().into(),
    }
}
fn unsupported(s: Span, feature: &str) -> Diagnostic {
    Diagnostic::new(
        "UnsupportedSyntax",
        format!("{feature} is not implemented in the bootstrap compiler"),
    )
    .at(s)
}
struct Adapter {
    symbols: Vec<String>,
    names: HashMap<String, SymbolId>,
    depth: usize,
    class_name: Option<String>,
}
impl Adapter {
    fn symbol(&mut self, name: &str) -> Result<SymbolId> {
        let mangled;
        let name = if name.starts_with("__") && !name.ends_with("__") && !name.contains('.') {
            if let Some(class) = self
                .class_name
                .as_deref()
                .map(|n| n.trim_start_matches('_'))
                .filter(|n| !n.is_empty())
            {
                mangled = format!("_{class}{name}");
                mangled.as_str()
            } else {
                name
            }
        } else {
            name
        };
        if let Some(id) = self.names.get(name) {
            return Ok(*id);
        }
        let id = SymbolId(
            u16::try_from(self.symbols.len())
                .map_err(|_| Diagnostic::new("ResourceError", "too many symbols"))?,
        );
        self.names.insert(name.to_owned(), id);
        self.symbols.push(name.to_owned());
        Ok(id)
    }
    fn block(&mut self, suite: Vec<py::Stmt>) -> Result<Vec<Stmt>> {
        suite.into_iter().map(|s| self.stmt(s)).collect()
    }
    fn stmt(&mut self, node: py::Stmt) -> Result<Stmt> {
        let s = span(&node);
        let kind = match node {
            py::Stmt::Assign(a) => StmtKind::Assign(
                a.targets
                    .into_iter()
                    .map(|t| self.target(t))
                    .collect::<Result<_>>()?,
                self.expr(*a.value)?,
            ),
            py::Stmt::AugAssign(a) => {
                let target = self.target(*a.target)?;
                if matches!(target, Target::Tuple(_)) {
                    return Err(unsupported(s, "augmented unpacking target"));
                }
                StmtKind::AugAssign(target, Self::binary(a.op, s)?, self.expr(*a.value)?)
            }
            py::Stmt::Delete(d) => {
                let mut targets = Vec::new();
                for target in d.targets {
                    let target_span = span(&target);
                    let target = self.target(target)?;
                    if !matches!(target, Target::Attribute(..) | Target::Item(..)) {
                        return Err(unsupported(target_span, "delete target"));
                    }
                    targets.push(target);
                }
                StmtKind::DeleteTargets(targets)
            }
            py::Stmt::Expr(e) => StmtKind::Expr(self.expr(*e.value)?),
            py::Stmt::ClassDef(c) => {
                if !c.type_params.is_empty() {
                    return Err(unsupported(s, "class type parameters"));
                }
                let mut metaclass = None;
                for keyword in c.keywords {
                    if keyword.arg.as_ref().map(|name| name.as_str()) != Some("metaclass") {
                        return Err(unsupported(
                            s,
                            "class keyword arguments other than metaclass",
                        ));
                    }
                    if metaclass.is_some() {
                        return Err(
                            Diagnostic::new("SyntaxError", "duplicate metaclass keyword").at(s),
                        );
                    }
                    metaclass = Some((self.symbol("metaclass")?, self.expr(keyword.value)?));
                }
                let label = c.name.to_string();
                let name = self.symbol(&label)?;
                let class_cell = self.symbol("__class__")?;
                // Python evaluates decorator expressions before bases/class body.
                let decorators = c
                    .decorator_list
                    .into_iter()
                    .map(|d| self.expr(d))
                    .collect::<Result<_>>()?;
                let bases = c
                    .bases
                    .into_iter()
                    .map(|b| self.expr(b))
                    .collect::<Result<_>>()?;
                let previous_class = self.class_name.replace(label.clone());
                let previous_depth = std::mem::replace(&mut self.depth, 0);
                self.symbol("__module__")?;
                self.symbol("__qualname__")?;
                self.symbol("__doc__")?;
                let mut body = self.block(c.body)?;
                if let Some(Stmt {
                    kind:
                        StmtKind::Expr(Expr {
                            kind: ExprKind::Constant(Constant::Str(_)),
                            ..
                        }),
                    ..
                }) = body.first()
                {
                    let doc = self.symbol("__doc__")?;
                    let StmtKind::Expr(expr) = body[0].kind.clone() else {
                        unreachable!()
                    };
                    body[0].kind = StmtKind::Assign(vec![Target::Name(doc)], expr);
                }
                self.depth = previous_depth;
                self.class_name = previous_class;
                StmtKind::Class {
                    name,
                    class_cell,
                    label,
                    decorators,
                    bases,
                    metaclass,
                    body,
                }
            }
            py::Stmt::FunctionDef(f) => {
                if f.returns.is_some() || !f.type_params.is_empty() {
                    return Err(unsupported(s, "annotations/type parameters"));
                }
                let label = f.name.to_string();
                // Decorator expressions precede defaults; applications happen
                // in reverse order after the Function object is constructed.
                let decorators = f
                    .decorator_list
                    .into_iter()
                    .map(|d| self.expr(d))
                    .collect::<Result<_>>()?;
                let params = self.parameters(*f.args, s)?;
                let name = self.symbol(f.name.as_str())?;
                self.depth += 1;
                let body = self.block(f.body)?;
                self.depth -= 1;
                StmtKind::Function {
                    name,
                    label,
                    decorators,
                    params,
                    body,
                }
            }
            py::Stmt::Return(r) => {
                if self.depth == 0 {
                    return Err(Diagnostic::new("SyntaxError", "return outside function").at(s));
                }
                StmtKind::Return(r.value.map(|v| self.expr(*v)).transpose()?)
            }
            py::Stmt::Raise(r) => StmtKind::Raise {
                value: r.exc.map(|value| self.expr(*value)).transpose()?,
                cause: r.cause.map(|value| self.expr(*value)).transpose()?,
            },
            py::Stmt::Try(t) => {
                let handlers = t
                    .handlers
                    .into_iter()
                    .map(|handler| {
                        let py::ExceptHandler::ExceptHandler(handler) = handler;
                        let handler_span = span(&handler);
                        Ok(ExceptHandler {
                            type_: handler.type_.map(|value| self.expr(*value)).transpose()?,
                            name: handler
                                .name
                                .map(|name| self.symbol(name.as_str()))
                                .transpose()?,
                            body: self.block(handler.body)?,
                            span: handler_span,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                StmtKind::Try {
                    body: self.block(t.body)?,
                    handlers,
                    otherwise: self.block(t.orelse)?,
                    finalbody: self.block(t.finalbody)?,
                }
            }
            py::Stmt::With(w) => {
                if w.type_comment.is_some() {
                    return Err(unsupported(s, "with type comment"));
                }
                let items = w
                    .items
                    .into_iter()
                    .map(|item| {
                        Ok(WithItem {
                            context: self.expr(item.context_expr)?,
                            target: item
                                .optional_vars
                                .map(|target| self.target(*target))
                                .transpose()?,
                        })
                    })
                    .collect::<Result<_>>()?;
                StmtKind::With {
                    items,
                    body: self.block(w.body)?,
                }
            }
            py::Stmt::If(i) => StmtKind::If(
                self.expr(*i.test)?,
                self.block(i.body)?,
                self.block(i.orelse)?,
            ),
            py::Stmt::While(w) => StmtKind::While(
                self.expr(*w.test)?,
                self.block(w.body)?,
                self.block(w.orelse)?,
            ),
            py::Stmt::For(f) => StmtKind::For(
                self.target(*f.target)?,
                self.expr(*f.iter)?,
                self.block(f.body)?,
                self.block(f.orelse)?,
            ),
            py::Stmt::Import(i) => {
                let mut names = Vec::new();
                for alias in i.names {
                    let modules = self.module_prefixes(alias.name.as_str())?;
                    let explicit_alias = alias.asname.is_some();
                    let bound_name = alias.asname.as_ref().map_or_else(
                        || alias.name.as_str().split('.').next().unwrap(),
                        |n| n.as_str(),
                    );
                    names.push(ImportAlias {
                        modules,
                        bound: self.symbol(bound_name)?,
                        bind_leaf: explicit_alias,
                    });
                }
                StmtKind::Import(names)
            }
            py::Stmt::ImportFrom(i) => {
                if i.level.is_some_and(|level| level.to_u32() != 0) {
                    return Err(unsupported(s, "relative imports"));
                }
                let module = i
                    .module
                    .as_ref()
                    .ok_or_else(|| unsupported(s, "relative imports"))?;
                let modules = self.module_prefixes(module.as_str())?;
                let mut names = Vec::with_capacity(i.names.len());
                for alias in i.names {
                    if alias.name.as_str() == "*" {
                        return Err(unsupported(s, "star imports"));
                    }
                    let name = self.symbol(alias.name.as_str())?;
                    let bound =
                        self.symbol(alias.asname.as_ref().unwrap_or(&alias.name).as_str())?;
                    names.push((name, bound));
                }
                StmtKind::ImportFrom { modules, names }
            }
            py::Stmt::Break(_) => StmtKind::Break,
            py::Stmt::Global(g) => StmtKind::Global(
                g.names
                    .iter()
                    .map(|n| self.symbol(n.as_str()))
                    .collect::<Result<_>>()?,
            ),
            py::Stmt::Nonlocal(n) => StmtKind::Nonlocal(
                n.names
                    .iter()
                    .map(|n| self.symbol(n.as_str()))
                    .collect::<Result<_>>()?,
            ),
            py::Stmt::Continue(_) => StmtKind::Continue,
            py::Stmt::Pass(_) => StmtKind::Pass,
            _ => return Err(unsupported(s, "statement")),
        };
        Ok(Stmt { kind, span: s })
    }
    fn module_prefixes(&mut self, name: &str) -> Result<Vec<SymbolId>> {
        let mut prefixes = Vec::new();
        let mut prefix = String::new();
        for component in name.split('.') {
            if component.is_empty() {
                return Err(Diagnostic::new("SyntaxError", "empty import component"));
            }
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(component);
            prefixes.push(self.symbol(&prefix)?);
        }
        Ok(prefixes)
    }
    fn parameters(&mut self, args: py::Arguments, s: Span) -> Result<Parameters> {
        let mut params = Parameters {
            posonly: args.posonlyargs.len() as u16,
            ..Parameters::default()
        };
        for arg in args.posonlyargs.into_iter().chain(args.args) {
            if arg.def.annotation.is_some() {
                return Err(unsupported(s, "parameter annotations"));
            }
            params.positional.push(Parameter {
                name: self.symbol(arg.def.arg.as_str())?,
                default: arg.default.map(|e| self.expr(*e)).transpose()?,
            });
        }
        for arg in args.kwonlyargs {
            if arg.def.annotation.is_some() {
                return Err(unsupported(s, "parameter annotations"));
            }
            params.keyword_only.push(Parameter {
                name: self.symbol(arg.def.arg.as_str())?,
                default: arg.default.map(|e| self.expr(*e)).transpose()?,
            });
        }
        for (arg, dest) in [
            (args.vararg, &mut params.vararg),
            (args.kwarg, &mut params.kwarg),
        ] {
            if let Some(arg) = arg {
                if arg.annotation.is_some() {
                    return Err(unsupported(s, "parameter annotations"));
                }
                *dest = Some(self.symbol(arg.arg.as_str())?);
            }
        }
        let mut seen = std::collections::HashSet::new();
        if params.names().iter().any(|n| !seen.insert(*n)) {
            return Err(Diagnostic::new("SyntaxError", "duplicate parameter").at(s));
        }
        Ok(params)
    }
    fn target(&mut self, node: py::Expr) -> Result<Target> {
        let s = span(&node);
        match node {
            py::Expr::Name(n) => Ok(Target::Name(self.symbol(n.id.as_str())?)),
            py::Expr::Attribute(a) => Ok(Target::Attribute(
                self.expr(*a.value)?,
                self.symbol(a.attr.as_str())?,
            )),
            py::Expr::Subscript(subscript) => {
                if matches!(*subscript.slice, py::Expr::Slice(_)) {
                    return Err(unsupported(s, "slice assignment target"));
                }
                Ok(Target::Item(
                    self.expr(*subscript.value)?,
                    self.expr(*subscript.slice)?,
                ))
            }
            py::Expr::Tuple(t) => Ok(Target::Tuple(
                t.elts
                    .into_iter()
                    .map(|e| self.target(e))
                    .collect::<Result<_>>()?,
            )),
            py::Expr::List(l) => Ok(Target::Tuple(
                l.elts
                    .into_iter()
                    .map(|e| self.target(e))
                    .collect::<Result<_>>()?,
            )),
            _ => Err(unsupported(s, "assignment target")),
        }
    }
    fn expr(&mut self, node: py::Expr) -> Result<Expr> {
        let s = span(&node);
        let kind = match node {
            py::Expr::Constant(c) => ExprKind::Constant(match c.value {
                py::Constant::None => Constant::None,
                py::Constant::Bool(b) => Constant::Bool(b),
                py::Constant::Int(n) => Constant::Int(n.to_string()),
                py::Constant::Float(n) => Constant::Float(n),
                py::Constant::Str(t) => Constant::Str(t),
                _ => return Err(unsupported(s, "literal type")),
            }),
            py::Expr::Name(n) => ExprKind::Name(self.symbol(n.id.as_str())?),
            py::Expr::Tuple(t) => ExprKind::Tuple(
                t.elts
                    .into_iter()
                    .map(|e| self.expr(e))
                    .collect::<Result<_>>()?,
            ),
            py::Expr::List(l) => ExprKind::List(
                l.elts
                    .into_iter()
                    .map(|e| self.expr(e))
                    .collect::<Result<_>>()?,
            ),
            py::Expr::BinOp(b) => ExprKind::Binary(
                Box::new(self.expr(*b.left)?),
                Self::binary(b.op, s)?,
                Box::new(self.expr(*b.right)?),
            ),
            py::Expr::UnaryOp(u) => ExprKind::Unary(
                match u.op {
                    py::UnaryOp::USub => UnaryOp::Negative,
                    py::UnaryOp::UAdd => UnaryOp::Positive,
                    py::UnaryOp::Invert => UnaryOp::Invert,
                    py::UnaryOp::Not => UnaryOp::Not,
                },
                Box::new(self.expr(*u.operand)?),
            ),
            py::Expr::BoolOp(b) => ExprKind::Bool(
                matches!(b.op, py::BoolOp::And),
                b.values
                    .into_iter()
                    .map(|e| self.expr(e))
                    .collect::<Result<_>>()?,
            ),
            py::Expr::Compare(c) => ExprKind::Compare(
                Box::new(self.expr(*c.left)?),
                c.ops
                    .into_iter()
                    .zip(c.comparators)
                    .map(|(op, e)| Ok((Self::compare(op, s)?, self.expr(e)?)))
                    .collect::<Result<_>>()?,
            ),
            py::Expr::Call(c) => {
                let mut args = CallArguments::default();
                if c.args.is_empty()
                    && c.keywords.is_empty()
                    && self.class_name.is_some()
                    && self.depth > 0
                    && matches!(&*c.func, py::Expr::Name(n) if n.id.as_str() == "super")
                {
                    args.implicit_class = Some(self.symbol("__class__")?);
                }
                for arg in c.args {
                    match arg {
                        py::Expr::Starred(s) => args.positional.push((true, self.expr(*s.value)?)),
                        arg => args.positional.push((false, self.expr(arg)?)),
                    }
                }
                for kw in c.keywords {
                    let name = kw
                        .arg
                        .as_ref()
                        .map(|n| self.symbol(n.as_str()))
                        .transpose()?;
                    if name.is_some() && args.keywords.iter().any(|(n, _)| *n == name) {
                        return Err(
                            Diagnostic::new("SyntaxError", "repeated keyword argument").at(s)
                        );
                    }
                    args.keywords.push((name, self.expr(kw.value)?));
                }
                ExprKind::Call(Box::new(self.expr(*c.func)?), args)
            }
            py::Expr::Dict(d) => ExprKind::Dict(
                d.keys
                    .into_iter()
                    .zip(d.values)
                    .map(|(k, v)| Ok((k.map(|e| self.expr(e)).transpose()?, self.expr(v)?)))
                    .collect::<Result<_>>()?,
            ),
            py::Expr::Attribute(a) => ExprKind::Attribute(
                Box::new(self.expr(*a.value)?),
                self.symbol(a.attr.as_str())?,
            ),
            py::Expr::Subscript(a) => ExprKind::Subscript(
                Box::new(self.expr(*a.value)?),
                Box::new(self.expr(*a.slice)?),
            ),
            py::Expr::Slice(a) => ExprKind::Slice {
                start: a.lower.map(|e| self.expr(*e).map(Box::new)).transpose()?,
                stop: a.upper.map(|e| self.expr(*e).map(Box::new)).transpose()?,
                step: a.step.map(|e| self.expr(*e).map(Box::new)).transpose()?,
            },
            py::Expr::IfExp(i) => ExprKind::Conditional(
                Box::new(self.expr(*i.test)?),
                Box::new(self.expr(*i.body)?),
                Box::new(self.expr(*i.orelse)?),
            ),
            py::Expr::Yield(y) => {
                if self.depth == 0 {
                    return Err(Diagnostic::new("SyntaxError", "yield outside function").at(s));
                }
                ExprKind::Yield(
                    y.value
                        .map(|value| self.expr(*value).map(Box::new))
                        .transpose()?,
                )
            }
            py::Expr::YieldFrom(y) => {
                if self.depth == 0 {
                    return Err(Diagnostic::new("SyntaxError", "yield outside function").at(s));
                }
                ExprKind::YieldFrom(Box::new(self.expr(*y.value)?))
            }
            py::Expr::Lambda(lambda) => {
                let params = self.parameters(*lambda.args, s)?;
                self.depth += 1;
                let body = self.expr(*lambda.body)?;
                self.depth -= 1;
                ExprKind::Lambda {
                    params,
                    body: Box::new(body),
                }
            }
            _ => return Err(unsupported(s, "expression")),
        };
        Ok(Expr { kind, span: s })
    }
    fn binary(op: py::Operator, s: Span) -> Result<BinaryOp> {
        Ok(match op {
            py::Operator::Add => BinaryOp::Add,
            py::Operator::Sub => BinaryOp::Subtract,
            py::Operator::Mult => BinaryOp::Multiply,
            py::Operator::Pow => BinaryOp::Power,
            py::Operator::BitOr => BinaryOp::BitOr,
            py::Operator::BitXor => BinaryOp::BitXor,
            py::Operator::BitAnd => BinaryOp::BitAnd,
            py::Operator::LShift => BinaryOp::LeftShift,
            py::Operator::RShift => BinaryOp::RightShift,
            py::Operator::FloorDiv => BinaryOp::FloorDivide,
            py::Operator::Mod => BinaryOp::Modulo,
            py::Operator::Div => BinaryOp::Divide,
            py::Operator::MatMult => return Err(unsupported(s, "matrix multiplication")),
        })
    }
    fn compare(op: py::CmpOp, s: Span) -> Result<CompareOp> {
        Ok(match op {
            py::CmpOp::Eq => CompareOp::Equal,
            py::CmpOp::NotEq => CompareOp::NotEqual,
            py::CmpOp::Lt => CompareOp::Less,
            py::CmpOp::LtE => CompareOp::LessEqual,
            py::CmpOp::Gt => CompareOp::Greater,
            py::CmpOp::GtE => CompareOp::GreaterEqual,
            _ => return Err(unsupported(s, "comparison operator")),
        })
    }
}
