//! Replaceable parser adapter. No RustPython types escape this module.
use py::Ranged;
use rustpython_parser::{ast as py, lexer, Mode, Parse, Tok};
use std::collections::{HashMap, VecDeque};
use tonic_core::{
    ast::*,
    diagnostic::{Diagnostic, Result, Span},
};

#[derive(Debug)]
struct RawTypeParamDefault {
    source: String,
    span: Span,
    unpacked: bool,
}

/// RustPython 0.4 owns the replaceable bootstrap grammar but predates PEP 696.
/// Mask only type-parameter defaults, preserving every byte offset, and feed
/// their expressions through the same adapter after the surrounding 3.12 AST
/// has been parsed. This keeps the compatibility shim local to the parser
/// boundary and lets it disappear when the upstream AST grows `default_value`.
fn type_param_defaults(
    source: &str,
    filename: &str,
) -> Result<(String, VecDeque<Vec<Option<RawTypeParamDefault>>>)> {
    let mut tokens = Vec::new();
    for token in lexer::lex(source, Mode::Module) {
        let Ok(token) = token else { break };
        tokens.push(token);
    }
    let mut masked = source.as_bytes().to_vec();
    let mut groups = VecDeque::new();
    let mut i = 0;
    while i + 2 < tokens.len() {
        if !matches!(tokens[i].0, Tok::Def | Tok::Class | Tok::Type)
            || !matches!(tokens[i + 1].0, Tok::Name { .. })
            || !matches!(tokens[i + 2].0, Tok::Lsqb)
        {
            i += 1;
            continue;
        }
        let open = i + 2;
        let mut depth = 1usize;
        let mut lambda_parameters = false;
        let mut segments = Vec::new();
        let mut segment_start = open + 1;
        let mut close = None;
        let mut cursor = open + 1;
        while cursor < tokens.len() {
            let token = &tokens[cursor].0;
            if depth == 1 {
                if matches!(token, Tok::Lambda) {
                    lambda_parameters = true;
                } else if lambda_parameters && matches!(token, Tok::Colon) {
                    lambda_parameters = false;
                } else if !lambda_parameters && matches!(token, Tok::Comma) {
                    segments.push((segment_start, cursor));
                    segment_start = cursor + 1;
                    cursor += 1;
                    continue;
                }
            }
            match token {
                Tok::Lpar | Tok::Lsqb | Tok::Lbrace => depth += 1,
                Tok::Rpar | Tok::Rbrace => depth = depth.saturating_sub(1),
                Tok::Rsqb => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        segments.push((segment_start, cursor));
                        close = Some(cursor);
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        let Some(close) = close else {
            break;
        };
        let mut defaults = Vec::new();
        let mut saw_default = false;
        let mut previous_was_variadic = false;
        for (start, end) in segments {
            if start == end {
                continue;
            }
            let mut nested = 0usize;
            let mut equal = None;
            let mut name = None;
            let mut prefix = 0u8;
            for (offset, (token, _)) in tokens[start..end].iter().enumerate() {
                if offset == 0 {
                    prefix = match token {
                        Tok::Star => 1,
                        Tok::DoubleStar => 2,
                        _ => 0,
                    };
                }
                if name.is_none() {
                    if let Tok::Name { name: value } = token {
                        name = Some(value.as_str());
                    }
                }
                if nested == 0 && matches!(token, Tok::Equal) {
                    equal = Some(start + offset);
                    break;
                }
                match token {
                    Tok::Lpar | Tok::Lsqb | Tok::Lbrace => nested += 1,
                    Tok::Rpar | Tok::Rsqb | Tok::Rbrace => nested = nested.saturating_sub(1),
                    _ => {}
                }
            }
            let default = if let Some(equal) = equal {
                if previous_was_variadic && prefix == 0 {
                    return Err(Diagnostic::new(
                        "SyntaxError",
                        "type parameter with a default follows TypeVarTuple",
                    )
                    .in_file(filename)
                    .at(Span {
                        start: tokens[start].1.start().into(),
                        end: tokens[end - 1].1.end().into(),
                    }));
                }
                saw_default = true;
                let mut expression_start: usize = tokens[equal].1.end().into();
                let expression_end: usize = tokens[end - 1].1.end().into();
                let mut unpacked = false;
                if prefix == 1 && equal + 1 < end && matches!(tokens[equal + 1].0, Tok::Star) {
                    unpacked = true;
                    expression_start = tokens[equal + 1].1.end().into();
                }
                while expression_start < expression_end
                    && source.as_bytes()[expression_start].is_ascii_whitespace()
                {
                    expression_start += 1;
                }
                if expression_start == expression_end {
                    return Err(
                        Diagnostic::new("SyntaxError", "expected type parameter default")
                            .in_file(filename)
                            .at(Span {
                                start: tokens[equal].1.start().into(),
                                end: tokens[equal].1.end().into(),
                            }),
                    );
                }
                let mask_start: usize = tokens[equal].1.start().into();
                for byte in &mut masked[mask_start..expression_end] {
                    if !matches!(*byte, b'\n' | b'\r') {
                        *byte = b' ';
                    }
                }
                Some(RawTypeParamDefault {
                    source: source[expression_start..expression_end]
                        .trim_end()
                        .to_owned(),
                    span: Span {
                        start: expression_start as u32,
                        end: expression_end as u32,
                    },
                    unpacked,
                })
            } else {
                if saw_default {
                    return Err(Diagnostic::new(
                        "SyntaxError",
                        "non-default type parameter follows default type parameter",
                    )
                    .in_file(filename)
                    .at(Span {
                        start: tokens[start].1.start().into(),
                        end: tokens[end - 1].1.end().into(),
                    }));
                }
                None
            };
            if name.is_none() {
                return Err(
                    Diagnostic::new("SyntaxError", "expected type parameter name")
                        .in_file(filename),
                );
            }
            previous_was_variadic = prefix == 1;
            defaults.push(default);
        }
        groups.push_back(defaults);
        i = close + 1;
    }
    // Only ASCII spaces replace source bytes, so valid UTF-8 remains valid.
    Ok((
        String::from_utf8(masked).expect("masked UTF-8 source"),
        groups,
    ))
}

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
    let (parser_source, type_param_defaults) = type_param_defaults(source, filename)?;
    let suite = py::Suite::parse(&parser_source, filename).map_err(|e| {
        let start = u32::from(e.offset);
        Diagnostic::new("SyntaxError", e.error.to_string())
            .in_file(filename)
            .at(Span { start, end: start })
    })?;
    let mut adapter = Adapter {
        symbols: Vec::new(),
        names: HashMap::new(),
        depth: 0,
        async_function: false,
        class_name: None,
        type_param_defaults,
        filename: filename.to_owned(),
        span_offset: 0,
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
    async_function: bool,
    class_name: Option<String>,
    type_param_defaults: VecDeque<Vec<Option<RawTypeParamDefault>>>,
    filename: String,
    span_offset: u32,
}
impl Adapter {
    fn node_span(&self, node: &impl Ranged) -> Span {
        let mut span = span(node);
        span.start = span.start.saturating_add(self.span_offset);
        span.end = span.end.saturating_add(self.span_offset);
        span
    }
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
        let s = self.node_span(&node);
        let kind = match node {
            py::Stmt::Assign(a) => StmtKind::Assign(
                a.targets
                    .into_iter()
                    .map(|t| self.target(t))
                    .collect::<Result<_>>()?,
                self.expr(*a.value)?,
            ),
            py::Stmt::AnnAssign(a) => {
                let simple = a.simple;
                let target = self.target(*a.target)?;
                if simple && !matches!(target, Target::Name(_)) {
                    return Err(Diagnostic::new(
                        "SyntaxError",
                        "simple annotated assignment must target a name",
                    )
                    .at(s));
                }
                if simple {
                    self.symbol("__annotations__")?;
                }
                StmtKind::AnnAssign {
                    target,
                    annotation: self.expr(*a.annotation)?,
                    value: a.value.map(|value| self.expr(*value)).transpose()?,
                    simple,
                }
            }
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
                    let target_span = self.node_span(&target);
                    let target = self.target(target)?;
                    if !matches!(target, Target::Attribute(..) | Target::Item(..)) {
                        return Err(unsupported(target_span, "delete target"));
                    }
                    targets.push(target);
                }
                StmtKind::DeleteTargets(targets)
            }
            py::Stmt::Expr(e) => StmtKind::Expr(self.expr(*e.value)?),
            py::Stmt::TypeAlias(alias) => {
                let py::Expr::Name(name) = *alias.name else {
                    return Err(
                        Diagnostic::new("SyntaxError", "type alias name must be a name").at(s),
                    );
                };
                StmtKind::TypeAlias {
                    name: self.symbol(name.id.as_str())?,
                    type_params: self.type_params(alias.type_params, s)?,
                    value: self.expr(*alias.value)?,
                }
            }
            py::Stmt::ClassDef(c) => {
                let type_params = self.type_params(c.type_params, s)?;
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
                let previous_async = std::mem::replace(&mut self.async_function, false);
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
                self.async_function = previous_async;
                self.class_name = previous_class;
                StmtKind::Class {
                    name,
                    class_cell,
                    label,
                    decorators,
                    type_params,
                    bases,
                    metaclass,
                    body,
                }
            }
            py::Stmt::FunctionDef(f) => {
                let type_params = self.type_params(f.type_params, s)?;
                let label = f.name.to_string();
                // Decorator expressions precede defaults; applications happen
                // in reverse order after the Function object is constructed.
                let decorators = f
                    .decorator_list
                    .into_iter()
                    .map(|d| self.expr(d))
                    .collect::<Result<_>>()?;
                let params = self.parameters(*f.args, s)?;
                let returns = f
                    .returns
                    .map(|e| {
                        self.symbol("return")?;
                        self.expr(*e)
                    })
                    .transpose()?;
                let name = self.symbol(f.name.as_str())?;
                let previous_async = std::mem::replace(&mut self.async_function, false);
                self.depth += 1;
                let body = self.block(f.body)?;
                self.depth -= 1;
                self.async_function = previous_async;
                StmtKind::Function {
                    name,
                    label,
                    is_async: false,
                    decorators,
                    type_params,
                    params,
                    returns,
                    body,
                }
            }
            py::Stmt::AsyncFunctionDef(f) => {
                let type_params = self.type_params(f.type_params, s)?;
                let label = f.name.to_string();
                let decorators = f
                    .decorator_list
                    .into_iter()
                    .map(|d| self.expr(d))
                    .collect::<Result<_>>()?;
                let params = self.parameters(*f.args, s)?;
                let returns = f
                    .returns
                    .map(|e| {
                        self.symbol("return")?;
                        self.expr(*e)
                    })
                    .transpose()?;
                let name = self.symbol(f.name.as_str())?;
                let previous_async = std::mem::replace(&mut self.async_function, true);
                self.depth += 1;
                let body = self.block(f.body)?;
                self.depth -= 1;
                self.async_function = previous_async;
                StmtKind::Function {
                    name,
                    label,
                    is_async: true,
                    decorators,
                    type_params,
                    params,
                    returns,
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
                        let handler_span = self.node_span(&handler);
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
            py::Stmt::AsyncWith(w) => {
                if !self.async_function {
                    return Err(Diagnostic::new(
                        "SyntaxError",
                        "async with outside async function",
                    )
                    .at(s));
                }
                if w.type_comment.is_some() {
                    return Err(unsupported(s, "async with type comment"));
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
                StmtKind::AsyncWith {
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
            py::Stmt::AsyncFor(f) => {
                if !self.async_function {
                    return Err(
                        Diagnostic::new("SyntaxError", "async for outside async function").at(s),
                    );
                }
                StmtKind::AsyncFor(
                    self.target(*f.target)?,
                    self.expr(*f.iter)?,
                    self.block(f.body)?,
                    self.block(f.orelse)?,
                )
            }
            py::Stmt::Match(m) => StmtKind::Match {
                subject: self.expr(*m.subject)?,
                cases: m
                    .cases
                    .into_iter()
                    .map(|case| {
                        let case_span = self.node_span(&case.pattern);
                        Ok(MatchCase {
                            pattern: self.pattern(case.pattern)?,
                            guard: case.guard.map(|guard| self.expr(*guard)).transpose()?,
                            body: self.block(case.body)?,
                            span: case_span,
                        })
                    })
                    .collect::<Result<_>>()?,
            },
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
    fn pattern(&mut self, node: py::Pattern) -> Result<Pattern> {
        let s = self.node_span(&node);
        let kind = match node {
            py::Pattern::MatchValue(value) => PatternKind::Value(self.expr(*value.value)?),
            py::Pattern::MatchSingleton(singleton) => {
                let value = match singleton.value {
                    py::Constant::None => Constant::None,
                    py::Constant::Bool(value) => Constant::Bool(value),
                    _ => return Err(unsupported(s, "match singleton")),
                };
                PatternKind::Singleton(value)
            }
            py::Pattern::MatchSequence(sequence) => PatternKind::Sequence(
                sequence
                    .patterns
                    .into_iter()
                    .map(|pattern| self.pattern(pattern))
                    .collect::<Result<_>>()?,
            ),
            py::Pattern::MatchMapping(mapping) => PatternKind::Mapping {
                keys: mapping
                    .keys
                    .into_iter()
                    .map(|key| self.expr(key))
                    .collect::<Result<_>>()?,
                patterns: mapping
                    .patterns
                    .into_iter()
                    .map(|pattern| self.pattern(pattern))
                    .collect::<Result<_>>()?,
                rest: mapping
                    .rest
                    .map(|name| self.symbol(name.as_str()))
                    .transpose()?,
            },
            py::Pattern::MatchClass(class) => PatternKind::Class {
                class: self.expr(*class.cls)?,
                positional: class
                    .patterns
                    .into_iter()
                    .map(|pattern| self.pattern(pattern))
                    .collect::<Result<_>>()?,
                keyword_names: class
                    .kwd_attrs
                    .into_iter()
                    .map(|name| self.symbol(name.as_str()))
                    .collect::<Result<_>>()?,
                keyword_patterns: class
                    .kwd_patterns
                    .into_iter()
                    .map(|pattern| self.pattern(pattern))
                    .collect::<Result<_>>()?,
            },
            py::Pattern::MatchStar(star) => PatternKind::Star(
                star.name
                    .map(|name| self.symbol(name.as_str()))
                    .transpose()?,
            ),
            py::Pattern::MatchAs(as_) => PatternKind::As {
                pattern: as_
                    .pattern
                    .map(|pattern| self.pattern(*pattern).map(Box::new))
                    .transpose()?,
                name: as_
                    .name
                    .map(|name| self.symbol(name.as_str()))
                    .transpose()?,
            },
            py::Pattern::MatchOr(or) => PatternKind::Or(
                or.patterns
                    .into_iter()
                    .map(|pattern| self.pattern(pattern))
                    .collect::<Result<_>>()?,
            ),
        };
        Ok(Pattern { kind, span: s })
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
            params.positional.push(Parameter {
                name: self.symbol(arg.def.arg.as_str())?,
                default: arg.default.map(|e| self.expr(*e)).transpose()?,
                annotation: arg.def.annotation.map(|e| self.expr(*e)).transpose()?,
            });
        }
        for arg in args.kwonlyargs {
            params.keyword_only.push(Parameter {
                name: self.symbol(arg.def.arg.as_str())?,
                default: arg.default.map(|e| self.expr(*e)).transpose()?,
                annotation: arg.def.annotation.map(|e| self.expr(*e)).transpose()?,
            });
        }
        if let Some(arg) = args.vararg {
            params.vararg = Some(self.symbol(arg.arg.as_str())?);
            params.vararg_annotation = arg
                .annotation
                .map(|e| self.expr(*e).map(Box::new))
                .transpose()?;
        }
        if let Some(arg) = args.kwarg {
            params.kwarg = Some(self.symbol(arg.arg.as_str())?);
            params.kwarg_annotation = arg
                .annotation
                .map(|e| self.expr(*e).map(Box::new))
                .transpose()?;
        }
        let mut seen = std::collections::HashSet::new();
        if params.names().iter().any(|n| !seen.insert(*n)) {
            return Err(Diagnostic::new("SyntaxError", "duplicate parameter").at(s));
        }
        Ok(params)
    }
    fn target(&mut self, node: py::Expr) -> Result<Target> {
        let s = self.node_span(&node);
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

    fn type_params(&mut self, params: Vec<py::TypeParam>, s: Span) -> Result<Vec<TypeParam>> {
        let mut defaults = if params.is_empty() {
            Vec::new()
        } else {
            self.type_param_defaults.pop_front().ok_or_else(|| {
                Diagnostic::new("SyntaxError", "missing type parameter metadata").at(s)
            })?
        };
        if defaults.len() != params.len() {
            return Err(Diagnostic::new("SyntaxError", "invalid type parameter metadata").at(s));
        }
        let mut result = Vec::with_capacity(params.len());
        let mut seen = std::collections::HashSet::new();
        for (param, default) in params.into_iter().zip(defaults.drain(..)) {
            let param_span = self.node_span(&param);
            let (name, kind) = match param {
                py::TypeParam::TypeVar(param) => (
                    self.symbol(param.name.as_str())?,
                    TypeParamKind::TypeVar {
                        bound: param
                            .bound
                            .map(|bound| self.expr(*bound).map(Box::new))
                            .transpose()?
                            .map(|bound| *bound),
                    },
                ),
                py::TypeParam::ParamSpec(param) => {
                    (self.symbol(param.name.as_str())?, TypeParamKind::ParamSpec)
                }
                py::TypeParam::TypeVarTuple(param) => (
                    self.symbol(param.name.as_str())?,
                    TypeParamKind::TypeVarTuple,
                ),
            };
            if !seen.insert(name) {
                return Err(Diagnostic::new("SyntaxError", "duplicate type parameter").at(s));
            }
            let (default, unpacked_default) = if let Some(default) = default {
                let parsed = py::Expr::parse(&default.source, &self.filename).map_err(|error| {
                    Diagnostic::new("SyntaxError", error.error.to_string()).at(default.span)
                })?;
                let previous_offset = std::mem::replace(&mut self.span_offset, default.span.start);
                let expression = self.expr(parsed);
                self.span_offset = previous_offset;
                let mut expression = expression?;
                expression.span = default.span;
                (Some(expression), default.unpacked)
            } else {
                (None, false)
            };
            result.push(TypeParam {
                name,
                kind,
                default,
                unpacked_default,
                span: param_span,
            });
        }
        if !result.is_empty() {
            self.symbol("__type_params__")?;
        }
        Ok(result)
    }
    fn expr(&mut self, node: py::Expr) -> Result<Expr> {
        let s = self.node_span(&node);
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
            py::Expr::Set(set) => ExprKind::Set(
                set.elts
                    .into_iter()
                    .map(|element| self.expr(element))
                    .collect::<Result<_>>()?,
            ),
            py::Expr::ListComp(c) => {
                self.comprehension(ComprehensionKind::List, *c.elt, None, c.generators, s)?
            }
            py::Expr::DictComp(c) => self.comprehension(
                ComprehensionKind::Dict,
                *c.key,
                Some(*c.value),
                c.generators,
                s,
            )?,
            py::Expr::GeneratorExp(c) => {
                self.comprehension(ComprehensionKind::Generator, *c.elt, None, c.generators, s)?
            }
            py::Expr::SetComp(c) => {
                self.comprehension(ComprehensionKind::Set, *c.elt, None, c.generators, s)?
            }
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
            py::Expr::Await(await_) => {
                if !self.async_function {
                    return Err(
                        Diagnostic::new("SyntaxError", "await outside async function").at(s),
                    );
                }
                ExprKind::Await(Box::new(self.expr(*await_.value)?))
            }
            py::Expr::JoinedStr(joined) => ExprKind::JoinedString(
                joined
                    .values
                    .into_iter()
                    .map(|value| self.expr(value))
                    .collect::<Result<_>>()?,
            ),
            py::Expr::FormattedValue(formatted) => ExprKind::FormattedValue {
                value: Box::new(self.expr(*formatted.value)?),
                conversion: match formatted.conversion {
                    py::ConversionFlag::None => FormatConversion::None,
                    py::ConversionFlag::Str => FormatConversion::Str,
                    py::ConversionFlag::Repr => FormatConversion::Repr,
                    py::ConversionFlag::Ascii => FormatConversion::Ascii,
                },
                format_spec: formatted
                    .format_spec
                    .map(|spec| self.expr(*spec).map(Box::new))
                    .transpose()?,
            },
            py::Expr::Lambda(lambda) => {
                let params = self.parameters(*lambda.args, s)?;
                let previous_async = std::mem::replace(&mut self.async_function, false);
                self.depth += 1;
                let body = self.expr(*lambda.body)?;
                self.depth -= 1;
                self.async_function = previous_async;
                ExprKind::Lambda {
                    params,
                    body: Box::new(body),
                }
            }
            _ => return Err(unsupported(s, "expression")),
        };
        Ok(Expr { kind, span: s })
    }
    fn comprehension(
        &mut self,
        kind: ComprehensionKind,
        element: py::Expr,
        value: Option<py::Expr>,
        generators: Vec<py::Comprehension>,
        s: Span,
    ) -> Result<ExprKind> {
        if generators.is_empty() {
            return Err(Diagnostic::new(
                "SyntaxError",
                "comprehension requires at least one for clause",
            )
            .at(s));
        }

        let mut generators = generators.into_iter();
        let first = generators.next().expect("checked nonempty comprehension");
        // Python evaluates and iterates the outermost iterable in the enclosing
        // scope. Every target, filter, later iterable and result expression is
        // owned by the hidden comprehension scope.
        let first_iterable = self.expr(first.iter)?;
        let enclosing_async = self.async_function;
        let hidden_may_suspend = enclosing_async || kind == ComprehensionKind::Generator;
        let previous_async = std::mem::replace(&mut self.async_function, hidden_may_suspend);
        self.depth += 1;
        let mut clauses = vec![ComprehensionClause {
            target: self.target(first.target)?,
            iterable: first_iterable,
            filters: first
                .ifs
                .into_iter()
                .map(|filter| self.expr(filter))
                .collect::<Result<_>>()?,
            is_async: first.is_async,
        }];
        for generator in generators {
            clauses.push(ComprehensionClause {
                target: self.target(generator.target)?,
                iterable: self.expr(generator.iter)?,
                filters: generator
                    .ifs
                    .into_iter()
                    .map(|filter| self.expr(filter))
                    .collect::<Result<_>>()?,
                is_async: generator.is_async,
            });
        }
        let element = self.expr(element).map(Box::new);
        let value = value
            .map(|value| self.expr(value).map(Box::new))
            .transpose();
        self.depth -= 1;
        self.async_function = previous_async;

        let element = element?;
        let value = value?;
        let coroutine = clauses.iter().any(|clause| clause.is_async)
            || Self::expression_suspends(&element)
            || value.as_deref().is_some_and(Self::expression_suspends)
            || clauses.iter().enumerate().any(|(index, clause)| {
                (index != 0 && Self::expression_suspends(&clause.iterable))
                    || clause.filters.iter().any(Self::expression_suspends)
            });
        if coroutine && kind != ComprehensionKind::Generator && !enclosing_async {
            return Err(Diagnostic::new(
                "SyntaxError",
                "asynchronous comprehension outside of an asynchronous function",
            )
            .at(s));
        }

        Ok(ExprKind::Comprehension(Comprehension {
            kind,
            coroutine,
            iterator_parameter: self.symbol(".0")?,
            accumulator: self.symbol(".result")?,
            element,
            value,
            clauses,
        }))
    }
    fn expression_suspends(expression: &Expr) -> bool {
        match &expression.kind {
            ExprKind::Await(_) => true,
            ExprKind::Tuple(values)
            | ExprKind::List(values)
            | ExprKind::Set(values)
            | ExprKind::Bool(_, values) => values.iter().any(Self::expression_suspends),
            ExprKind::Binary(left, _, right) | ExprKind::Subscript(left, right) => {
                Self::expression_suspends(left) || Self::expression_suspends(right)
            }
            ExprKind::Unary(_, value)
            | ExprKind::Attribute(value, _)
            | ExprKind::YieldFrom(value) => Self::expression_suspends(value),
            ExprKind::Compare(left, comparisons) => {
                Self::expression_suspends(left)
                    || comparisons
                        .iter()
                        .any(|(_, value)| Self::expression_suspends(value))
            }
            ExprKind::Call(callee, arguments) => {
                Self::expression_suspends(callee)
                    || arguments
                        .positional
                        .iter()
                        .any(|(_, value)| Self::expression_suspends(value))
                    || arguments
                        .keywords
                        .iter()
                        .any(|(_, value)| Self::expression_suspends(value))
            }
            ExprKind::Dict(entries) => entries.iter().any(|(key, value)| {
                key.as_ref().is_some_and(Self::expression_suspends)
                    || Self::expression_suspends(value)
            }),
            ExprKind::Slice { start, stop, step } => [start, stop, step]
                .into_iter()
                .flatten()
                .any(|value| Self::expression_suspends(value)),
            ExprKind::Conditional(condition, then_value, else_value) => {
                Self::expression_suspends(condition)
                    || Self::expression_suspends(then_value)
                    || Self::expression_suspends(else_value)
            }
            ExprKind::Yield(value) => value.as_deref().is_some_and(Self::expression_suspends),
            ExprKind::JoinedString(values) => values.iter().any(Self::expression_suspends),
            ExprKind::FormattedValue {
                value, format_spec, ..
            } => {
                Self::expression_suspends(value)
                    || format_spec
                        .as_deref()
                        .is_some_and(Self::expression_suspends)
            }
            ExprKind::Comprehension(comprehension) => {
                comprehension.coroutine && comprehension.kind != ComprehensionKind::Generator
            }
            ExprKind::Lambda { params, .. } => {
                params
                    .positional
                    .iter()
                    .chain(&params.keyword_only)
                    .any(|parameter| {
                        parameter
                            .default
                            .as_ref()
                            .is_some_and(Self::expression_suspends)
                            || parameter
                                .annotation
                                .as_ref()
                                .is_some_and(Self::expression_suspends)
                    })
                    || params
                        .vararg_annotation
                        .as_deref()
                        .is_some_and(Self::expression_suspends)
                    || params
                        .kwarg_annotation
                        .as_deref()
                        .is_some_and(Self::expression_suspends)
            }
            ExprKind::Constant(_) | ExprKind::Name(_) => false,
        }
    }
    fn binary(op: py::Operator, _s: Span) -> Result<BinaryOp> {
        Ok(match op {
            py::Operator::Add => BinaryOp::Add,
            py::Operator::Sub => BinaryOp::Subtract,
            py::Operator::Mult => BinaryOp::Multiply,
            py::Operator::MatMult => BinaryOp::MatrixMultiply,
            py::Operator::Pow => BinaryOp::Power,
            py::Operator::BitOr => BinaryOp::BitOr,
            py::Operator::BitXor => BinaryOp::BitXor,
            py::Operator::BitAnd => BinaryOp::BitAnd,
            py::Operator::LShift => BinaryOp::LeftShift,
            py::Operator::RShift => BinaryOp::RightShift,
            py::Operator::FloorDiv => BinaryOp::FloorDivide,
            py::Operator::Mod => BinaryOp::Modulo,
            py::Operator::Div => BinaryOp::Divide,
        })
    }
    fn compare(op: py::CmpOp, _s: Span) -> Result<CompareOp> {
        Ok(match op {
            py::CmpOp::Eq => CompareOp::Equal,
            py::CmpOp::NotEq => CompareOp::NotEqual,
            py::CmpOp::Lt => CompareOp::Less,
            py::CmpOp::LtE => CompareOp::LessEqual,
            py::CmpOp::Gt => CompareOp::Greater,
            py::CmpOp::GtE => CompareOp::GreaterEqual,
            py::CmpOp::Is => CompareOp::Is,
            py::CmpOp::IsNot => CompareOp::IsNot,
            py::CmpOp::In => CompareOp::In,
            py::CmpOp::NotIn => CompareOp::NotIn,
        })
    }
}
