//! Lexical scope resolution independent of parser types and runtime layout.
//! Captured bindings are cells; intermediate scopes forward cells, never values.
use std::collections::HashSet;
use tonic_core::{
    ast::*,
    diagnostic::{Diagnostic, Result, Span},
};

#[derive(Debug)]
pub(crate) struct Scope {
    pub class_body: bool,
    pub generator: bool,
    pub coroutine: bool,
    class_cell: Option<SymbolId>,
    pub globals: HashSet<SymbolId>,
    pub nonlocals: HashSet<SymbolId>,
    pub locals: Vec<SymbolId>,
    pub cells: Vec<SymbolId>,
    pub free: Vec<SymbolId>,
    pub children: Vec<(u32, Scope)>,
}
impl Scope {
    pub fn resolve(
        params: &Parameters,
        body: &[Stmt],
        module: bool,
        bound: &HashSet<SymbolId>,
    ) -> Result<Self> {
        Self::resolve_kind(params, body, module, false, false, None, bound)
    }
    fn resolve_kind(
        params: &Parameters,
        body: &[Stmt],
        module: bool,
        class_body: bool,
        coroutine: bool,
        class_cell: Option<SymbolId>,
        bound: &HashSet<SymbolId>,
    ) -> Result<Self> {
        let mut scan = Scan {
            params: params.names(),
            locals: params.names(),
            reads: Vec::new(),
            seen: HashSet::new(),
            globals: HashSet::new(),
            nonlocals: HashSet::new(),
            children: Vec::new(),
            module,
            yield_span: None,
            yield_from_span: None,
            return_value_span: None,
        };
        scan.block(body)?;
        if module || class_body {
            if let Some(span) = scan.yield_span {
                return Err(Diagnostic::new("SyntaxError", "yield outside function").at(span));
            }
        }
        if coroutine && scan.yield_span.is_some() {
            if let Some(span) = scan.yield_from_span {
                return Err(
                    Diagnostic::new("SyntaxError", "yield from inside async function").at(span),
                );
            }
            if let Some(span) = scan.return_value_span {
                return Err(
                    Diagnostic::new("SyntaxError", "return with value in async generator").at(span),
                );
            }
        }
        for name in &scan.nonlocals {
            if !bound.contains(name) {
                return Err(Diagnostic::new(
                    "SyntaxError",
                    "no binding for nonlocal name in enclosing functions",
                )
                .at(Scan::declaration_span(body, *name).unwrap_or_default()));
            }
        }
        let mut scope = Self {
            class_body,
            generator: scan.yield_span.is_some(),
            coroutine,
            class_cell,
            globals: scan.globals.clone(),
            nonlocals: scan.nonlocals.clone(),
            locals: scan
                .locals
                .into_iter()
                .filter(|n| !module && !scan.globals.contains(n) && !scan.nonlocals.contains(n))
                .collect(),
            cells: Vec::new(),
            free: Vec::new(),
            children: Vec::new(),
        };
        for name in scan.reads.into_iter().chain(scan.nonlocals.iter().copied()) {
            if !module
                && !scope.locals.contains(&name)
                && !scan.globals.contains(&name)
                && bound.contains(&name)
            {
                unique(&mut scope.free, name);
            }
        }
        let mut child_bound = bound.clone();
        if !module && !class_body {
            child_bound.extend(scope.locals.iter().copied());
        }
        if let Some(class_cell) = class_cell {
            child_bound.insert(class_cell);
        }
        if !class_body {
            for name in &scan.globals {
                child_bound.remove(name);
            }
        }
        for (span, child) in scan.children {
            let child = match child {
                Child::Function(params, body, coroutine) => {
                    Self::resolve_kind(params, body, false, false, coroutine, None, &child_bound)?
                }
                Child::Class(body, class_cell) => Self::resolve_kind(
                    &Parameters::default(),
                    body,
                    false,
                    true,
                    false,
                    Some(class_cell),
                    &child_bound,
                )?,
                Child::Lambda(params, body) => {
                    let statement = Stmt {
                        kind: StmtKind::Return(Some(body.clone())),
                        span: body.span,
                    };
                    Self::resolve(params, &[statement], false, &child_bound)?
                }
                Child::Comprehension(comprehension) => {
                    let (params, body) = comprehension_scope(comprehension);
                    Self::resolve_kind(
                        &params,
                        &body,
                        false,
                        false,
                        comprehension.coroutine,
                        None,
                        &child_bound,
                    )?
                }
            };
            for name in &child.free {
                if class_body && scope.class_cell == Some(*name) {
                    unique(&mut scope.locals, *name);
                    unique(&mut scope.cells, *name);
                } else if !class_body && scope.locals.contains(name) {
                    unique(&mut scope.cells, *name);
                } else if !module && bound.contains(name) {
                    unique(&mut scope.free, *name);
                }
            }
            scope.children.push((span, child));
        }
        // Canonical deterministic cell order follows local register order.
        scope
            .cells
            .sort_by_key(|name| scope.locals.iter().position(|n| n == name));
        scope.free.sort_by_key(|name| name.0);
        Ok(scope)
    }
    pub fn local(&self, name: SymbolId) -> Option<u16> {
        self.locals
            .iter()
            .position(|n| *n == name)
            .map(|n| n as u16)
    }
    pub fn cell(&self, name: SymbolId) -> Option<u16> {
        self.cells
            .iter()
            .chain(&self.free)
            .position(|n| *n == name)
            .map(|n| n as u16)
    }
    pub fn take_child(&mut self, span: u32) -> Scope {
        let i = self
            .children
            .iter()
            .position(|(s, _)| *s == span)
            .expect("resolved child scope");
        self.children.remove(i).1
    }
}
fn unique(names: &mut Vec<SymbolId>, name: SymbolId) {
    if !names.contains(&name) {
        names.push(name);
    }
}
struct Scan<'a> {
    params: Vec<SymbolId>,
    locals: Vec<SymbolId>,
    reads: Vec<SymbolId>,
    seen: HashSet<SymbolId>,
    globals: HashSet<SymbolId>,
    nonlocals: HashSet<SymbolId>,
    children: Vec<(u32, Child<'a>)>,
    module: bool,
    yield_span: Option<Span>,
    yield_from_span: Option<Span>,
    return_value_span: Option<Span>,
}
enum Child<'a> {
    Function(&'a Parameters, &'a [Stmt], bool),
    Class(&'a [Stmt], SymbolId),
    Lambda(&'a Parameters, &'a Expr),
    Comprehension(&'a Comprehension),
}
impl<'a> Scan<'a> {
    fn read(&mut self, n: SymbolId) {
        unique(&mut self.reads, n);
        self.seen.insert(n);
    }
    fn bind(&mut self, n: SymbolId) {
        unique(&mut self.locals, n);
        self.seen.insert(n);
    }
    fn target(&mut self, t: &'a Target) {
        match t {
            Target::Name(n) => self.bind(*n),
            Target::Item(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Target::Attribute(owner, _) => self.expr(owner),
            Target::Tuple(ts) => {
                for t in ts {
                    self.target(t);
                }
            }
        }
    }
    fn pattern(&mut self, pattern: &'a Pattern) {
        match &pattern.kind {
            PatternKind::Value(value) => self.expr(value),
            PatternKind::Singleton(_) => {}
            PatternKind::Sequence(patterns) | PatternKind::Or(patterns) => {
                for pattern in patterns {
                    self.pattern(pattern);
                }
            }
            PatternKind::Mapping {
                keys,
                patterns,
                rest,
            } => {
                for key in keys {
                    self.expr(key);
                }
                for pattern in patterns {
                    self.pattern(pattern);
                }
                if let Some(name) = rest {
                    self.bind(*name);
                }
            }
            PatternKind::Class {
                class,
                positional,
                keyword_patterns,
                ..
            } => {
                self.expr(class);
                for pattern in positional.iter().chain(keyword_patterns) {
                    self.pattern(pattern);
                }
            }
            PatternKind::Star(name) => {
                if let Some(name) = name {
                    self.bind(*name);
                }
            }
            PatternKind::As { pattern, name } => {
                if let Some(pattern) = pattern {
                    self.pattern(pattern);
                }
                if let Some(name) = name {
                    self.bind(*name);
                }
            }
        }
    }
    fn expr(&mut self, e: &'a Expr) {
        match &e.kind {
            ExprKind::Name(n) => self.read(*n),
            ExprKind::Constant(_) => {}
            ExprKind::Dict(items) => {
                for (k, v) in items {
                    if let Some(k) = k {
                        self.expr(k);
                    }
                    self.expr(v);
                }
            }
            ExprKind::Tuple(es)
            | ExprKind::List(es)
            | ExprKind::Set(es)
            | ExprKind::Bool(_, es) => {
                for e in es {
                    self.expr(e);
                }
            }
            ExprKind::Binary(a, _, b) | ExprKind::Subscript(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::Slice { start, stop, step } => {
                for e in [start, stop, step].into_iter().flatten() {
                    self.expr(e);
                }
            }
            ExprKind::Unary(_, e) | ExprKind::Attribute(e, _) => self.expr(e),
            ExprKind::Compare(e, pairs) => {
                self.expr(e);
                for (_, e) in pairs {
                    self.expr(e);
                }
            }
            ExprKind::Call(e, args) => {
                self.expr(e);
                if let Some(class_cell) = args.implicit_class {
                    self.read(class_cell);
                }
                for (_, arg) in &args.positional {
                    self.expr(arg);
                }
                for (_, arg) in &args.keywords {
                    self.expr(arg);
                }
            }
            ExprKind::Conditional(a, b, c) => {
                self.expr(a);
                self.expr(b);
                self.expr(c);
            }
            ExprKind::Yield(value) => {
                self.yield_span.get_or_insert(e.span);
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            ExprKind::YieldFrom(value) => {
                self.yield_span.get_or_insert(e.span);
                self.yield_from_span.get_or_insert(e.span);
                self.expr(value);
            }
            ExprKind::Await(value) => self.expr(value),
            ExprKind::Lambda { params, body } => {
                for (_, default) in params.defaults() {
                    self.expr(default);
                }
                self.children
                    .push((e.span.start, Child::Lambda(params, body)));
            }
            ExprKind::Comprehension(comprehension) => {
                self.expr(&comprehension.clauses[0].iterable);
                self.children
                    .push((e.span.start, Child::Comprehension(comprehension)));
            }
        }
    }
    fn block(&mut self, body: &'a [Stmt]) -> Result<()> {
        for s in body {
            match &s.kind {
                StmtKind::Assign(ts, e) => {
                    self.expr(e);
                    for t in ts {
                        self.target(t);
                    }
                }
                StmtKind::AugAssign(target, _, e) => {
                    if let Target::Name(n) = target {
                        self.read(*n);
                    }
                    self.target(target);
                    self.expr(e);
                }
                StmtKind::DeleteTargets(targets) => {
                    for target in targets {
                        self.target(target);
                    }
                }
                StmtKind::Expr(e) => self.expr(e),
                StmtKind::Return(Some(e)) => {
                    self.return_value_span.get_or_insert(s.span);
                    self.expr(e);
                }
                StmtKind::Raise { value, cause } => {
                    if let Some(value) = value {
                        self.expr(value);
                    }
                    if let Some(cause) = cause {
                        self.expr(cause);
                    }
                }
                StmtKind::Try {
                    body,
                    handlers,
                    otherwise,
                    finalbody,
                } => {
                    self.block(body)?;
                    for handler in handlers {
                        if let Some(type_) = &handler.type_ {
                            self.expr(type_);
                        }
                        if let Some(name) = handler.name {
                            self.bind(name);
                        }
                        self.block(&handler.body)?;
                    }
                    self.block(otherwise)?;
                    self.block(finalbody)?;
                }
                StmtKind::With { items, body } | StmtKind::AsyncWith { items, body } => {
                    for item in items {
                        self.expr(&item.context);
                        if let Some(target) = &item.target {
                            self.target(target);
                        }
                    }
                    self.block(body)?;
                }
                StmtKind::If(e, a, b) | StmtKind::While(e, a, b) => {
                    self.expr(e);
                    self.block(a)?;
                    self.block(b)?;
                }
                StmtKind::For(t, e, a, b) | StmtKind::AsyncFor(t, e, a, b) => {
                    self.expr(e);
                    self.target(t);
                    self.block(a)?;
                    self.block(b)?;
                }
                StmtKind::Match { subject, cases } => {
                    self.expr(subject);
                    for case in cases {
                        self.pattern(&case.pattern);
                        if let Some(guard) = &case.guard {
                            self.expr(guard);
                        }
                        self.block(&case.body)?;
                    }
                }
                StmtKind::Function {
                    name,
                    is_async,
                    decorators,
                    params,
                    returns,
                    body,
                    ..
                } => {
                    for decorator in decorators {
                        self.expr(decorator);
                    }
                    for (_, default) in params.defaults() {
                        self.expr(default);
                    }
                    for parameter in params.positional.iter().chain(&params.keyword_only) {
                        if let Some(annotation) = &parameter.annotation {
                            self.expr(annotation);
                        }
                    }
                    if let Some(annotation) = &params.vararg_annotation {
                        self.expr(annotation);
                    }
                    if let Some(annotation) = &params.kwarg_annotation {
                        self.expr(annotation);
                    }
                    if let Some(annotation) = returns {
                        self.expr(annotation);
                    }
                    self.bind(*name);
                    self.children
                        .push((s.span.start, Child::Function(params, body, *is_async)));
                }
                StmtKind::Class {
                    name,
                    class_cell,
                    decorators,
                    bases,
                    metaclass,
                    body,
                    ..
                } => {
                    for decorator in decorators {
                        self.expr(decorator);
                    }
                    for base in bases {
                        self.expr(base);
                    }
                    if let Some((_, metaclass)) = metaclass {
                        self.expr(metaclass);
                    }
                    self.bind(*name);
                    self.children
                        .push((s.span.start, Child::Class(body, *class_cell)));
                }
                StmtKind::Import(names) => {
                    for alias in names {
                        self.bind(alias.bound);
                    }
                }
                StmtKind::ImportFrom { names, .. } => {
                    for (_, bound) in names {
                        self.bind(*bound);
                    }
                }
                StmtKind::Global(names) | StmtKind::Nonlocal(names) => {
                    let nonlocal = matches!(s.kind, StmtKind::Nonlocal(_));
                    if nonlocal && self.module {
                        return Err(Diagnostic::new(
                            "SyntaxError",
                            "nonlocal declaration at module scope",
                        )
                        .at(s.span));
                    }
                    for name in names {
                        if self.params.contains(name) {
                            return Err(Diagnostic::new(
                                "SyntaxError",
                                "parameter conflicts with global/nonlocal declaration",
                            )
                            .at(s.span));
                        }
                        if self.seen.contains(name) {
                            return Err(Diagnostic::new(
                                "SyntaxError",
                                "name used or assigned before global/nonlocal declaration",
                            )
                            .at(s.span));
                        }
                        if (nonlocal && self.globals.contains(name))
                            || (!nonlocal && self.nonlocals.contains(name))
                        {
                            return Err(Diagnostic::new(
                                "SyntaxError",
                                "name is both global and nonlocal",
                            )
                            .at(s.span));
                        }
                        if nonlocal {
                            self.nonlocals.insert(*name);
                        } else {
                            self.globals.insert(*name);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn declaration_span(body: &[Stmt], name: SymbolId) -> Option<Span> {
        for s in body {
            match &s.kind {
                StmtKind::Nonlocal(names) if names.contains(&name) => return Some(s.span),
                StmtKind::If(_, a, b)
                | StmtKind::While(_, a, b)
                | StmtKind::For(_, _, a, b)
                | StmtKind::AsyncFor(_, _, a, b) => {
                    if let Some(s) =
                        Self::declaration_span(a, name).or_else(|| Self::declaration_span(b, name))
                    {
                        return Some(s);
                    }
                }
                StmtKind::Try {
                    body,
                    handlers,
                    otherwise,
                    finalbody,
                } => {
                    if let Some(span) = Self::declaration_span(body, name)
                        .or_else(|| Self::declaration_span(otherwise, name))
                        .or_else(|| Self::declaration_span(finalbody, name))
                        .or_else(|| {
                            handlers
                                .iter()
                                .find_map(|handler| Self::declaration_span(&handler.body, name))
                        })
                    {
                        return Some(span);
                    }
                }
                StmtKind::With { body, .. } | StmtKind::AsyncWith { body, .. } => {
                    if let Some(span) = Self::declaration_span(body, name) {
                        return Some(span);
                    }
                }
                StmtKind::Match { cases, .. } => {
                    for case in cases {
                        if let Some(span) = Self::declaration_span(&case.body, name) {
                            return Some(span);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
}

fn comprehension_scope(comprehension: &Comprehension) -> (Parameters, Vec<Stmt>) {
    let span = comprehension.element.span;
    let params = Parameters {
        positional: vec![Parameter {
            name: comprehension.iterator_parameter,
            default: None,
            annotation: None,
        }],
        ..Parameters::default()
    };
    let tail = match comprehension.kind {
        ComprehensionKind::List | ComprehensionKind::Set => Stmt {
            kind: StmtKind::Expr((*comprehension.element).clone()),
            span,
        },
        ComprehensionKind::Dict => Stmt {
            kind: StmtKind::Expr(Expr {
                kind: ExprKind::Tuple(vec![
                    (*comprehension.element).clone(),
                    (**comprehension
                        .value
                        .as_ref()
                        .expect("dict comprehension value"))
                    .clone(),
                ]),
                span,
            }),
            span,
        },
        ComprehensionKind::Generator => Stmt {
            kind: StmtKind::Expr(Expr {
                kind: ExprKind::Yield(Some(comprehension.element.clone())),
                span,
            }),
            span,
        },
    };
    let mut body = vec![tail];
    for (index, clause) in comprehension.clauses.iter().enumerate().rev() {
        for filter in clause.filters.iter().rev() {
            body = vec![Stmt {
                kind: StmtKind::If(filter.clone(), body, Vec::new()),
                span: filter.span,
            }];
        }
        let iterable = if index == 0 {
            Expr {
                kind: ExprKind::Name(comprehension.iterator_parameter),
                span: clause.iterable.span,
            }
        } else {
            clause.iterable.clone()
        };
        body = vec![Stmt {
            kind: if clause.is_async {
                StmtKind::AsyncFor(clause.target.clone(), iterable, body, Vec::new())
            } else {
                StmtKind::For(clause.target.clone(), iterable, body, Vec::new())
            },
            span: clause.iterable.span,
        }];
    }
    if comprehension.kind != ComprehensionKind::Generator {
        body.insert(
            0,
            Stmt {
                kind: StmtKind::Assign(
                    vec![Target::Name(comprehension.accumulator)],
                    Expr {
                        kind: ExprKind::Constant(Constant::None),
                        span,
                    },
                ),
                span,
            },
        );
        body.push(Stmt {
            kind: StmtKind::Return(Some(Expr {
                kind: ExprKind::Name(comprehension.accumulator),
                span,
            })),
            span,
        });
    }
    (params, body)
}
