use crate::hir::Scope;
use tonic_core::{
    ast::*,
    bytecode::*,
    diagnostic::{Diagnostic, Result, Span},
};

fn limit() -> Diagnostic {
    Diagnostic::new(
        "ResourceError",
        "bootstrap bytecode limit exceeded (16-bit indices)",
    )
}
fn index(n: usize) -> Result<u16> {
    u16::try_from(n).map_err(|_| limit())
}
pub fn compile(module: Module) -> Result<Program> {
    let mut program = Program {
        version: BYTECODE_VERSION,
        symbols: module.symbols,
        code: Vec::new(),
    };
    let scope = Scope::resolve(
        &Parameters::default(),
        &module.body,
        true,
        &Default::default(),
    )?;
    build(
        &mut program,
        "<module>".to_owned(),
        &Parameters::default(),
        &module.body,
        scope,
    )?;
    Ok(program)
}
fn build(
    program: &mut Program,
    name: String,
    params: &Parameters,
    body: &[Stmt],
    scope: Scope,
) -> Result<u16> {
    let id = index(program.code.len())?;
    let next = index(scope.locals.len())?;
    let code = CodeObject {
        class_body: scope.class_body,
        name,
        params: index(params.names().len())?,
        signature: Signature {
            positional: index(params.positional.len())?,
            posonly: params.posonly,
            keyword_only: index(params.keyword_only.len())?,
            vararg: params
                .vararg
                .map(|_| (params.positional.len() + params.keyword_only.len()) as u16),
            kwarg: params.kwarg.map(|_| {
                (params.positional.len()
                    + params.keyword_only.len()
                    + usize::from(params.vararg.is_some())) as u16
            }),
            defaults: params.defaults().map(|(slot, _)| slot as u16).collect(),
        },
        locals: scope.locals.clone(),
        registers: 0,
        instructions: Vec::new(),
        spans: Vec::new(),
        constants: Vec::new(),
        calls: Vec::new(),
        cell_locals: scope
            .cells
            .iter()
            .map(|n| scope.local(*n).expect("cell local"))
            .collect(),
        free_vars: scope.free.clone(),
        functions: Vec::new(),
        exception_regions: Vec::new(),
    };
    // Reserve identity before recursively compiling functions.
    program.code.push(code.clone());
    let mut lower = Lower {
        program,
        code,
        scope,
        next,
        high: next,
        loops: Vec::new(),
        cleanups: Vec::new(),
        finally_bypasses: Vec::new(),
        with_bypasses: Vec::new(),
    };
    if lower.scope.class_body {
        let span = body
            .first()
            .map(|statement| statement.span)
            .unwrap_or_default();
        let module = lower.constant(Constant::Str("__main__".into()), span)?;
        let qualname = lower.constant(Constant::Str(lower.code.name.clone()), span)?;
        let doc = lower.constant(Constant::None, span)?;
        for (name, value) in [
            ("__module__", module),
            ("__qualname__", qualname),
            ("__doc__", doc),
        ] {
            let symbol = lower
                .program
                .symbols
                .iter()
                .position(|candidate| candidate == name)
                .ok_or_else(|| Diagnostic::new("BytecodeError", "missing class symbol"))?;
            lower.emit(Op::StoreName, value, index(symbol)?, 0, span)?;
        }
    }
    lower.block(body)?;
    let r = lower.constant(Constant::None, Span::default())?;
    lower.emit(Op::Return, r, 0, 0, Span::default())?;
    lower.code.registers = lower.high;
    lower.program.code[id as usize] = lower.code;
    Ok(id)
}
struct Loop {
    start: u16,
    breaks: Vec<usize>,
    cleanup_depth: usize,
}
#[derive(Clone, Copy)]
struct ExceptionCleanup {
    name: Option<SymbolId>,
    span: Span,
}
#[derive(Clone)]
enum ControlCleanup {
    Exception(ExceptionCleanup),
    Finally {
        id: usize,
        body: Vec<Stmt>,
        span: Span,
    },
    With {
        id: usize,
        token: u16,
        span: Span,
    },
}
struct FinallyBypass {
    start: u16,
    end: u16,
    exception: u16,
    span: Span,
}
struct Lower<'a> {
    program: &'a mut Program,
    code: CodeObject,
    scope: Scope,
    next: u16,
    high: u16,
    loops: Vec<Loop>,
    cleanups: Vec<ControlCleanup>,
    finally_bypasses: Vec<Vec<FinallyBypass>>,
    with_bypasses: Vec<Vec<FinallyBypass>>,
}
impl Lower<'_> {
    fn apply_decorators(&mut self, mut value: u16, decorators: &[(u16, Span)]) -> Result<u16> {
        // Expressions were evaluated top-to-bottom. Applications are
        // bottom-to-top and use the ordinary call path, preserving arbitrary
        // user callable semantics without a decorator-specific runtime ABI.
        for &(decorator, span) in decorators.iter().rev() {
            let first = self.alloc(1)?;
            self.emit(Op::Move, first, value, 0, span)?;
            let site = index(self.code.calls.len())?;
            self.code.calls.push(CallSite {
                first,
                count: 1,
                keywords: Vec::new(),
            });
            let result = self.alloc(1)?;
            self.emit(Op::Call, result, decorator, site, span)?;
            value = result;
        }
        Ok(value)
    }
    fn qualified_name(&self, label: &str) -> String {
        if self.code.name == "<module>" {
            label.to_owned()
        } else if self.scope.class_body {
            format!("{}.{label}", self.code.name)
        } else {
            format!("{}.<locals>.{label}", self.code.name)
        }
    }
    fn alloc(&mut self, count: u16) -> Result<u16> {
        let r = self.next;
        self.next = self.next.checked_add(count).ok_or_else(limit)?;
        self.high = self.high.max(self.next);
        Ok(r)
    }
    fn pc(&self) -> Result<u16> {
        index(self.code.instructions.len())
    }
    fn emit(&mut self, op: Op, a: u16, b: u16, c: u16, span: Span) -> Result<usize> {
        if self.code.instructions.len() >= u16::MAX as usize {
            return Err(limit());
        }
        let pc = self.code.instructions.len();
        self.code.instructions.push(Instr::new(op, a, b, c));
        self.code.spans.push(span);
        Ok(pc)
    }
    fn patch(&mut self, pc: usize, target: u16) {
        let i = &mut self.code.instructions[pc];
        match Op::try_from(i.opcode).expect("compiler opcode") {
            Op::Jump => i.a = target,
            Op::Next => i.c = target,
            _ => i.b = target,
        }
    }
    fn constant(&mut self, c: Constant, s: Span) -> Result<u16> {
        let r = self.alloc(1)?;
        let id = index(self.code.constants.len())?;
        self.code.constants.push(c);
        self.emit(Op::Const, r, id, 0, s)?;
        Ok(r)
    }
    fn load(&mut self, n: SymbolId, s: Span) -> Result<u16> {
        let r = self.alloc(1)?;
        if self.scope.globals.contains(&n) {
            self.emit(Op::LoadGlobal, r, n.0, 0, s)?;
            return Ok(r);
        }
        if self.scope.class_body
            && !self.scope.globals.contains(&n)
            && !self.scope.nonlocals.contains(&n)
        {
            if self.scope.local(n).is_none() {
                if let Some(cell) = self.scope.cell(n) {
                    self.emit(Op::ClassDeref, r, n.0, cell, s)?;
                    return Ok(r);
                }
            }
            self.emit(Op::LoadName, r, n.0, 0, s)?;
            return Ok(r);
        }
        if let Some(cell) = self.scope.cell(n) {
            self.emit(Op::LoadCell, r, cell, 0, s)?;
        } else if let Some(local) = self.scope.local(n) {
            self.emit(Op::Move, r, local, 0, s)?;
        } else {
            self.emit(Op::LoadGlobal, r, n.0, 0, s)?;
        }
        Ok(r)
    }
    fn store(&mut self, n: SymbolId, r: u16, s: Span) -> Result<()> {
        if self.scope.globals.contains(&n) {
            self.emit(Op::StoreGlobal, r, n.0, 0, s)?;
            return Ok(());
        }
        if self.scope.class_body
            && !self.scope.globals.contains(&n)
            && !self.scope.nonlocals.contains(&n)
        {
            self.emit(Op::StoreName, r, n.0, 0, s)?;
            return Ok(());
        }
        if let Some(cell) = self.scope.cell(n) {
            self.emit(Op::StoreCell, r, cell, 0, s)?;
        } else if let Some(local) = self.scope.local(n) {
            self.emit(Op::Move, local, r, 0, s)?;
        } else {
            self.emit(Op::StoreGlobal, r, n.0, 0, s)?;
        }
        Ok(())
    }
    fn clear(&mut self, n: SymbolId, s: Span) -> Result<()> {
        if self.scope.globals.contains(&n) {
            self.emit(Op::ClearBinding, 1, n.0, 0, s)?;
        } else if self.scope.class_body && !self.scope.nonlocals.contains(&n) {
            self.emit(Op::ClearBinding, 3, n.0, 0, s)?;
        } else if let Some(cell) = self.scope.cell(n) {
            self.emit(Op::ClearBinding, 2, cell, 0, s)?;
        } else if let Some(local) = self.scope.local(n) {
            self.emit(Op::ClearBinding, 0, local, 0, s)?;
        } else {
            self.emit(Op::ClearBinding, 1, n.0, 0, s)?;
        }
        Ok(())
    }
    fn emit_exception_cleanup(&mut self, cleanup: ExceptionCleanup) -> Result<()> {
        if let Some(name) = cleanup.name {
            self.clear(name, cleanup.span)?;
        }
        self.emit(Op::ClearException, 0, 0, 0, cleanup.span)?;
        Ok(())
    }
    fn emit_cleanups_from(&mut self, depth: usize) -> Result<()> {
        let saved = self.cleanups.clone();
        for index in (depth..saved.len()).rev() {
            self.cleanups.truncate(index);
            match saved[index].clone() {
                ControlCleanup::Exception(cleanup) => {
                    self.emit_exception_cleanup(cleanup)?;
                }
                ControlCleanup::Finally { id, body, span } => {
                    let exception = self.alloc(1)?;
                    let start = self.pc()?;
                    self.block(&body)?;
                    let end = self.pc()?;
                    if start != end {
                        self.finally_bypasses[id].push(FinallyBypass {
                            start,
                            end,
                            exception,
                            span,
                        });
                    }
                }
                ControlCleanup::With { id, token, span } => {
                    let failure = self.alloc(1)?;
                    let result = self.alloc(1)?;
                    let none = self.constant(Constant::None, span)?;
                    let start = self.pc()?;
                    self.emit(Op::ContextExit, result, token, none, span)?;
                    let end = self.pc()?;
                    self.with_bypasses[id].push(FinallyBypass {
                        start,
                        end,
                        exception: failure,
                        span,
                    });
                }
            }
        }
        self.cleanups = saved;
        Ok(())
    }
    fn lower_try_except(
        &mut self,
        body: &[Stmt],
        handlers: &[ExceptHandler],
        otherwise: &[Stmt],
        span: Span,
    ) -> Result<()> {
        let exception = self.alloc(1)?;
        let start = self.pc()?;
        self.block(body)?;
        let end = self.pc()?;
        self.block(otherwise)?;
        let skip_handlers = self.emit(Op::Jump, 0, 0, 0, span)?;
        let target = self.pc()?;
        if start != end {
            self.code.exception_regions.push(ExceptionRegion {
                start,
                end,
                target,
                exception,
            });
        }
        let mut completed = Vec::new();
        for handler in handlers {
            let mismatch = if let Some(type_) = &handler.type_ {
                let class = self.expr(type_)?;
                let matched = self.alloc(1)?;
                self.emit(Op::ExceptionMatch, matched, exception, class, handler.span)?;
                Some(self.emit(Op::JumpFalse, matched, 0, 0, handler.span)?)
            } else {
                None
            };
            self.emit(Op::PushException, exception, 0, 0, handler.span)?;
            if let Some(name) = handler.name {
                self.store(name, exception, handler.span)?;
            }
            let cleanup = ExceptionCleanup {
                name: handler.name,
                span: handler.span,
            };
            let body_start = self.pc()?;
            self.cleanups.push(ControlCleanup::Exception(cleanup));
            self.block(&handler.body)?;
            self.cleanups.pop();
            let body_end = self.pc()?;
            self.emit_exception_cleanup(cleanup)?;
            completed.push(self.emit(Op::Jump, 0, 0, 0, handler.span)?);
            let cleanup_target = self.pc()?;
            if body_start != body_end {
                self.code.exception_regions.push(ExceptionRegion {
                    start: body_start,
                    end: body_end,
                    target: cleanup_target,
                    exception,
                });
            }
            self.emit_exception_cleanup(cleanup)?;
            self.emit(Op::Raise, exception, 0, 0, handler.span)?;
            if let Some(mismatch) = mismatch {
                self.patch(mismatch, self.pc()?);
            }
        }
        self.emit(Op::Raise, exception, 0, 0, span)?;
        let done = self.pc()?;
        self.patch(skip_handlers, done);
        for jump in completed {
            self.patch(jump, done);
        }
        Ok(())
    }
    fn lower_with(&mut self, items: &[WithItem], body: &[Stmt], span: Span) -> Result<()> {
        let Some((item, remaining)) = items.split_first() else {
            return self.block(body);
        };
        let manager = self.expr(&item.context)?;
        let token = self.alloc(1)?;
        let entered = self.alloc(1)?;
        self.emit(Op::ContextEnter, entered, token, manager, item.context.span)?;

        let id = self.with_bypasses.len();
        self.with_bypasses.push(Vec::new());
        let cleanup = ControlCleanup::With {
            id,
            token,
            span: item.context.span,
        };
        let start = self.pc()?;
        self.cleanups.push(cleanup);
        if let Some(target) = &item.target {
            self.target(target, entered, item.context.span)?;
        }
        self.lower_with(remaining, body, span)?;
        self.cleanups.pop();
        let end = self.pc()?;

        let none = self.constant(Constant::None, span)?;
        let normal_result = self.alloc(1)?;
        self.emit(Op::ContextExit, normal_result, token, none, span)?;
        let skip_exception_paths = self.emit(Op::Jump, 0, 0, 0, span)?;

        let exception = self.alloc(1)?;
        let target = self.pc()?;
        if start != end {
            self.code.exception_regions.push(ExceptionRegion {
                start,
                end,
                target,
                exception,
            });
        }
        self.emit(Op::PushException, exception, 0, 0, span)?;
        let exit_result = self.alloc(1)?;
        let replacement = self.alloc(1)?;
        let exit_start = self.pc()?;
        self.emit(Op::ContextExit, exit_result, token, exception, span)?;
        let suppressed = self.emit(Op::JumpTrue, exit_result, 0, 0, span)?;
        let exit_end = self.pc()?;
        self.emit(Op::ClearException, 0, 0, 0, span)?;
        self.emit(Op::Raise, exception, 0, 0, span)?;

        let suppressed_target = self.pc()?;
        self.patch(suppressed, suppressed_target);
        self.emit(Op::ClearException, 0, 0, 0, span)?;
        let suppressed_done = self.emit(Op::Jump, 0, 0, 0, span)?;

        let cleanup_target = self.pc()?;
        if exit_start != exit_end {
            self.code.exception_regions.push(ExceptionRegion {
                start: exit_start,
                end: exit_end,
                target: cleanup_target,
                exception: replacement,
            });
        }
        self.emit(Op::ClearException, 0, 0, 0, span)?;
        self.emit(Op::Raise, replacement, 0, 0, span)?;

        for bypass in std::mem::take(&mut self.with_bypasses[id]) {
            let target = self.pc()?;
            self.code.exception_regions.push(ExceptionRegion {
                start: bypass.start,
                end: bypass.end,
                target,
                exception: bypass.exception,
            });
            self.emit(Op::Raise, bypass.exception, 0, 0, bypass.span)?;
        }
        let done = self.pc()?;
        self.patch(skip_exception_paths, done);
        self.patch(suppressed_done, done);
        Ok(())
    }
    fn target(&mut self, t: &Target, r: u16, s: Span) -> Result<()> {
        match t {
            Target::Attribute(owner, name) => {
                let owner = self.expr(owner)?;
                self.emit(Op::SetAttr, owner, r, name.0, s)?;
            }
            Target::Name(n) => self.store(*n, r, s)?,
            Target::Item(owner, key) => {
                let owner = self.expr(owner)?;
                let key = self.expr(key)?;
                self.emit(Op::SetItem, owner, key, r, s)?;
            }
            Target::Tuple(ts) => {
                let count = index(ts.len())?;
                let base = self.alloc(count)?;
                self.emit(Op::Unpack, base, r, count, s)?;
                for (i, t) in ts.iter().enumerate() {
                    self.target(t, base + i as u16, s)?;
                }
            }
        }
        Ok(())
    }
    fn block(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            let mark = self.next;
            self.stmt(s)?;
            self.next = mark;
        }
        Ok(())
    }
    fn stmt(&mut self, stmt: &Stmt) -> Result<()> {
        let s = stmt.span;
        match &stmt.kind {
            StmtKind::Assign(ts, e) => {
                // Parallel literal assignment stages all RHS values before any store.
                // Only flat names are optimized: nested unpack errors have observable order.
                if let ([Target::Tuple(targets)], ExprKind::Tuple(values)) =
                    (ts.as_slice(), &e.kind)
                {
                    if targets.len() == values.len()
                        && targets.iter().all(|t| matches!(t, Target::Name(_)))
                    {
                        let regs = values
                            .iter()
                            .map(|v| self.expr(v))
                            .collect::<Result<Vec<_>>>()?;
                        for (t, r) in targets.iter().zip(regs) {
                            self.target(t, r, s)?;
                        }
                        return Ok(());
                    }
                }
                let r = self.expr(e)?;
                for t in ts {
                    self.target(t, r, s)?;
                }
            }
            StmtKind::AugAssign(target, op, e) => {
                let (left, item) = match target {
                    Target::Name(n) => (self.load(*n, s)?, None),
                    Target::Item(owner, key) => {
                        let owner = self.expr(owner)?;
                        let key = self.expr(key)?;
                        let left = self.alloc(1)?;
                        self.emit(Op::Item, left, owner, key, s)?;
                        (left, Some((Op::SetItem, owner, key)))
                    }
                    Target::Attribute(owner, name) => {
                        let owner = self.expr(owner)?;
                        let left = self.alloc(1)?;
                        self.emit(Op::Attr, left, owner, name.0, s)?;
                        (left, Some((Op::SetAttr, owner, name.0)))
                    }
                    Target::Tuple(_) => unreachable!("parser rejects augmented unpack"),
                };
                let right = self.expr(e)?;
                let r = self.alloc(1)?;
                self.emit(
                    if matches!(op, BinaryOp::Add) {
                        Op::InplaceAdd
                    } else {
                        binary(*op)
                    },
                    r,
                    left,
                    right,
                    s,
                )?;
                if let Some((op, owner, key)) = item {
                    if op == Op::SetAttr {
                        self.emit(op, owner, r, key, s)?;
                    } else {
                        self.emit(op, owner, key, r, s)?;
                    }
                } else if let Target::Name(n) = target {
                    self.store(*n, r, s)?;
                }
            }
            StmtKind::DeleteTargets(targets) => {
                for target in targets {
                    match target {
                        Target::Attribute(owner, name) => {
                            let owner = self.expr(owner)?;
                            self.emit(Op::DelAttr, owner, name.0, 0, s)?;
                        }
                        Target::Item(owner, key) => {
                            let owner = self.expr(owner)?;
                            let key = self.expr(key)?;
                            self.emit(Op::DelItem, owner, key, 0, s)?;
                        }
                        Target::Name(_) | Target::Tuple(_) => {
                            unreachable!("parser rejects this delete target")
                        }
                    }
                }
            }
            StmtKind::Expr(e) => {
                self.expr(e)?;
            }
            StmtKind::Class {
                name,
                class_cell: _,
                label,
                decorators,
                bases,
                metaclass,
                body,
            } => {
                let decorators = decorators
                    .iter()
                    .map(|d| self.expr(d).map(|r| (r, d.span)))
                    .collect::<Result<Vec<_>>>()?;
                let child = self.scope.take_child(s.start);
                let captures = child
                    .free
                    .iter()
                    .map(|n| self.scope.cell(*n).expect("class capture"))
                    .collect();
                let label = self.qualified_name(label);
                let id = build(self.program, label, &Parameters::default(), body, child)?;
                let site = index(self.code.functions.len())?;
                self.code.functions.push(FunctionSite {
                    code: id,
                    captures,
                    defaults: Vec::new(),
                });
                let function = self.alloc(1)?;
                self.emit(Op::Function, function, site, 0, s)?;
                let count = index(bases.len())?;
                let keyword_count = u16::from(metaclass.is_some());
                let first = self.alloc(count.checked_add(keyword_count).ok_or_else(limit)?)?;
                for (offset, base) in bases.iter().enumerate() {
                    let value = self.expr(base)?;
                    self.emit(Op::Move, first + offset as u16, value, 0, base.span)?;
                }
                let keywords = if let Some((name, metaclass)) = metaclass {
                    let value = self.expr(metaclass)?;
                    self.emit(Op::Move, first + count, value, 0, metaclass.span)?;
                    vec![*name]
                } else {
                    Vec::new()
                };
                let site = index(self.code.calls.len())?;
                self.code.calls.push(CallSite {
                    first,
                    count,
                    keywords,
                });
                let result = self.alloc(1)?;
                self.emit(Op::Class, result, function, site, s)?;
                let result = self.apply_decorators(result, &decorators)?;
                self.store(*name, result, s)?;
            }
            StmtKind::Return(e) => {
                let r = if let Some(e) = e {
                    self.expr(e)?
                } else {
                    self.constant(Constant::None, s)?
                };
                self.emit_cleanups_from(0)?;
                self.emit(Op::Return, r, 0, 0, s)?;
            }
            StmtKind::Raise { value, cause } => {
                if let Some(value) = value {
                    let value = self.expr(value)?;
                    if let Some(cause) = cause {
                        let cause = self.expr(cause)?;
                        self.emit(Op::Raise, value, 2, cause, s)?;
                    } else {
                        self.emit(Op::Raise, value, 0, 0, s)?;
                    }
                } else {
                    if cause.is_some() {
                        return Err(Diagnostic::new(
                            "SyntaxError",
                            "bare raise cannot specify a cause",
                        )
                        .at(s));
                    }
                    self.emit(Op::Raise, 0, 1, 0, s)?;
                }
            }
            StmtKind::Function {
                name,
                label,
                decorators,
                params,
                body,
            } => {
                let decorators = decorators
                    .iter()
                    .map(|d| self.expr(d).map(|r| (r, d.span)))
                    .collect::<Result<Vec<_>>>()?;
                let label = self.qualified_name(label);
                let child = self.scope.take_child(s.start);
                let captures = child
                    .free
                    .iter()
                    .map(|n| self.scope.cell(*n).expect("captured cell"))
                    .collect();
                let id = build(self.program, label, params, body, child)?;
                let site = index(self.code.functions.len())?;
                let defaults = params
                    .defaults()
                    .map(|(_, e)| self.expr(e))
                    .collect::<Result<_>>()?;
                self.code.functions.push(FunctionSite {
                    code: id,
                    captures,
                    defaults,
                });
                let r = self.alloc(1)?;
                self.emit(Op::Function, r, site, 0, s)?;
                let r = self.apply_decorators(r, &decorators)?;
                self.store(*name, r, s)?;
            }
            StmtKind::Try {
                body,
                handlers,
                otherwise,
                finalbody,
            } => {
                if finalbody.is_empty() {
                    self.lower_try_except(body, handlers, otherwise, s)?;
                } else {
                    let id = self.finally_bypasses.len();
                    self.finally_bypasses.push(Vec::new());
                    let cleanup = ControlCleanup::Finally {
                        id,
                        body: finalbody.clone(),
                        span: s,
                    };
                    let start = self.pc()?;
                    self.cleanups.push(cleanup);
                    if handlers.is_empty() {
                        self.block(body)?;
                    } else {
                        self.lower_try_except(body, handlers, otherwise, s)?;
                    }
                    self.cleanups.pop();
                    let end = self.pc()?;

                    self.block(finalbody)?;
                    let skip_exception_paths = self.emit(Op::Jump, 0, 0, 0, s)?;

                    let exception = self.alloc(1)?;
                    let target = self.pc()?;
                    if start != end {
                        self.code.exception_regions.push(ExceptionRegion {
                            start,
                            end,
                            target,
                            exception,
                        });
                    }
                    self.emit(Op::PushException, exception, 0, 0, s)?;
                    let final_start = self.pc()?;
                    let exception_cleanup = ExceptionCleanup {
                        name: None,
                        span: s,
                    };
                    self.cleanups
                        .push(ControlCleanup::Exception(exception_cleanup));
                    self.block(finalbody)?;
                    self.cleanups.pop();
                    let final_end = self.pc()?;
                    self.emit_exception_cleanup(exception_cleanup)?;
                    self.emit(Op::Raise, exception, 0, 0, s)?;
                    let cleanup_target = self.pc()?;
                    if final_start != final_end {
                        self.code.exception_regions.push(ExceptionRegion {
                            start: final_start,
                            end: final_end,
                            target: cleanup_target,
                            exception,
                        });
                    }
                    self.emit_exception_cleanup(exception_cleanup)?;
                    self.emit(Op::Raise, exception, 0, 0, s)?;

                    for bypass in std::mem::take(&mut self.finally_bypasses[id]) {
                        let target = self.pc()?;
                        self.code.exception_regions.push(ExceptionRegion {
                            start: bypass.start,
                            end: bypass.end,
                            target,
                            exception: bypass.exception,
                        });
                        self.emit(Op::Raise, bypass.exception, 0, 0, bypass.span)?;
                    }
                    self.patch(skip_exception_paths, self.pc()?);
                }
            }
            StmtKind::With { items, body } => self.lower_with(items, body, s)?,
            StmtKind::If(test, yes, no) => {
                let r = self.expr(test)?;
                let branch = self.emit(Op::JumpFalse, r, 0, 0, s)?;
                self.block(yes)?;
                let end = self.emit(Op::Jump, 0, 0, 0, s)?;
                self.patch(branch, self.pc()?);
                self.block(no)?;
                self.patch(end, self.pc()?);
            }
            StmtKind::While(test, body, otherwise) => {
                let start = self.pc()?;
                let r = self.expr(test)?;
                let branch = self.emit(Op::JumpFalse, r, 0, 0, s)?;
                self.loops.push(Loop {
                    start,
                    breaks: Vec::new(),
                    cleanup_depth: self.cleanups.len(),
                });
                self.block(body)?;
                self.emit(Op::Jump, start, 0, 0, s)?;
                self.patch(branch, self.pc()?);
                let lp = self.loops.pop().expect("active loop");
                self.block(otherwise)?;
                let end = self.pc()?;
                for b in lp.breaks {
                    self.patch(b, end);
                }
            }
            StmtKind::For(target, e, body, otherwise) => {
                let iterable = self.expr(e)?;
                let iter = self.alloc(1)?;
                self.emit(Op::Iter, iter, iterable, 0, s)?;
                let item = self.alloc(1)?;
                let start = self.pc()?;
                let next = self.emit(Op::Next, item, iter, 0, s)?;
                self.target(target, item, s)?;
                self.loops.push(Loop {
                    start,
                    breaks: Vec::new(),
                    cleanup_depth: self.cleanups.len(),
                });
                self.block(body)?;
                self.emit(Op::Jump, start, 0, 0, s)?;
                self.patch(next, self.pc()?);
                let lp = self.loops.pop().expect("active loop");
                self.block(otherwise)?;
                let end = self.pc()?;
                for b in lp.breaks {
                    self.patch(b, end);
                }
            }
            StmtKind::Break => {
                if self.loops.is_empty() {
                    return Err(Diagnostic::new("SyntaxError", "break outside loop").at(s));
                }
                let cleanup_depth = self.loops.last().expect("active loop").cleanup_depth;
                self.emit_cleanups_from(cleanup_depth)?;
                let pc = self.emit(Op::Jump, 0, 0, 0, s)?;
                self.loops.last_mut().expect("active loop").breaks.push(pc);
            }
            StmtKind::Continue => {
                let (start, cleanup_depth) = self
                    .loops
                    .last()
                    .map(|loop_| (loop_.start, loop_.cleanup_depth))
                    .ok_or_else(|| Diagnostic::new("SyntaxError", "continue outside loop").at(s))?;
                self.emit_cleanups_from(cleanup_depth)?;
                self.emit(Op::Jump, start, 0, 0, s)?;
            }
            StmtKind::Import(names) => {
                for (module, bound) in names {
                    let r = self.alloc(1)?;
                    self.emit(Op::Import, r, module.0, 0, s)?;
                    self.store(*bound, r, s)?;
                }
            }
            StmtKind::Pass | StmtKind::Global(_) | StmtKind::Nonlocal(_) => {}
        }
        Ok(())
    }
    fn window(&mut self, values: &[Expr]) -> Result<(u16, u16)> {
        let count = index(values.len())?;
        let base = self.alloc(count)?;
        for (i, e) in values.iter().enumerate() {
            let r = self.expr(e)?;
            self.emit(Op::Move, base + i as u16, r, 0, e.span)?;
        }
        Ok((base, count))
    }
    fn expr(&mut self, e: &Expr) -> Result<u16> {
        let s = e.span;
        match &e.kind {
            ExprKind::Constant(c) => self.constant(c.clone(), s),
            ExprKind::Name(n) => self.load(*n, s),
            ExprKind::Lambda { params, body } => {
                let child = self.scope.take_child(s.start);
                let captures = child
                    .free
                    .iter()
                    .map(|n| self.scope.cell(*n).expect("lambda capture"))
                    .collect();
                let statement = Stmt {
                    kind: StmtKind::Return(Some((**body).clone())),
                    span: body.span,
                };
                let id = build(
                    self.program,
                    self.qualified_name("<lambda>"),
                    params,
                    &[statement],
                    child,
                )?;
                let defaults = params
                    .defaults()
                    .map(|(_, e)| self.expr(e))
                    .collect::<Result<_>>()?;
                let site = index(self.code.functions.len())?;
                self.code.functions.push(FunctionSite {
                    code: id,
                    captures,
                    defaults,
                });
                let result = self.alloc(1)?;
                self.emit(Op::Function, result, site, 0, s)?;
                Ok(result)
            }
            ExprKind::Binary(a, op, b) => {
                let a = self.expr(a)?;
                let b = self.expr(b)?;
                let r = self.alloc(1)?;
                self.emit(binary(*op), r, a, b, s)?;
                Ok(r)
            }
            ExprKind::Unary(op, e) => {
                let v = self.expr(e)?;
                let r = self.alloc(1)?;
                self.emit(
                    match op {
                        UnaryOp::Negative => Op::Neg,
                        UnaryOp::Positive => Op::Pos,
                        UnaryOp::Not => Op::Not,
                    },
                    r,
                    v,
                    0,
                    s,
                )?;
                Ok(r)
            }
            ExprKind::Bool(and, values) => {
                let r = self.alloc(1)?;
                let mut ends = Vec::new();
                for (i, e) in values.iter().enumerate() {
                    let v = self.expr(e)?;
                    self.emit(Op::Move, r, v, 0, s)?;
                    if i + 1 < values.len() {
                        ends.push(self.emit(
                            if *and { Op::JumpFalse } else { Op::JumpTrue },
                            r,
                            0,
                            0,
                            s,
                        )?);
                    }
                }
                let end = self.pc()?;
                for pc in ends {
                    self.patch(pc, end);
                }
                Ok(r)
            }
            ExprKind::Compare(left, pairs) => {
                let mut left = self.expr(left)?;
                let r = self.alloc(1)?;
                let mut ends = Vec::new();
                for (i, (op, right)) in pairs.iter().enumerate() {
                    let right = self.expr(right)?;
                    self.emit(compare(*op), r, left, right, s)?;
                    if i + 1 < pairs.len() {
                        ends.push(self.emit(Op::JumpFalse, r, 0, 0, s)?);
                    }
                    left = right;
                }
                let end = self.pc()?;
                for pc in ends {
                    self.patch(pc, end);
                }
                Ok(r)
            }
            ExprKind::Call(callee, args) => {
                let callee = self.expr(callee)?;
                let expanded = args.positional.iter().any(|(star, _)| *star)
                    || args.keywords.iter().any(|(name, _)| name.is_none());
                if expanded {
                    self.emit(Op::BeginArgs, 0, 0, 0, s)?;
                    for (star, e) in &args.positional {
                        let r = self.expr(e)?;
                        self.emit(
                            if *star { Op::ArgStar } else { Op::ArgPos },
                            r,
                            0,
                            u16::from(*star && args.positional.len() == 1),
                            e.span,
                        )?;
                    }
                    let mut cursor = 0;
                    while cursor < args.keywords.len() {
                        let mut named = Vec::new();
                        // Evaluate each consecutive named group before merging it.
                        // Duplicate-key errors must not skip later expressions
                        // within that group (but do skip subsequent ** groups).
                        while let Some((Some(name), e)) = args.keywords.get(cursor) {
                            named.push((*name, self.expr(e)?, e.span));
                            cursor += 1;
                        }
                        for (name, r, span) in named {
                            self.emit(Op::ArgNamed, r, name.0, 0, span)?;
                        }
                        if let Some((_, e)) = args.keywords.get(cursor) {
                            let r = self.expr(e)?;
                            self.emit(Op::ArgMapping, r, 0, 0, e.span)?;
                            cursor += 1;
                        }
                    }
                    let r = self.alloc(1)?;
                    self.emit(Op::CallExpanded, r, callee, 0, s)?;
                    Ok(r)
                } else {
                    let total = index(args.positional.len() + args.keywords.len())?;
                    let first = self.alloc(total)?;
                    for (i, e) in args
                        .positional
                        .iter()
                        .map(|(_, e)| e)
                        .chain(args.keywords.iter().map(|(_, e)| e))
                        .enumerate()
                    {
                        let value = self.expr(e)?;
                        self.emit(Op::Move, first + i as u16, value, 0, e.span)?;
                    }
                    let site = index(self.code.calls.len())?;
                    self.code.calls.push(CallSite {
                        first,
                        count: index(args.positional.len())?,
                        keywords: args
                            .keywords
                            .iter()
                            .map(|(n, _)| n.expect("named keyword"))
                            .collect(),
                    });
                    let r = self.alloc(1)?;
                    self.emit(Op::Call, r, callee, site, s)?;
                    Ok(r)
                }
            }
            ExprKind::Dict(items) => {
                let r = self.alloc(1)?;
                self.emit(Op::Dict, r, 0, 0, s)?;
                let mut cursor = 0;
                while cursor < items.len() {
                    let mut pairs = Vec::new();
                    while let Some((Some(key), value)) = items.get(cursor) {
                        pairs.push((self.expr(key)?, self.expr(value)?));
                        cursor += 1;
                    }
                    for (key, value) in pairs {
                        self.emit(Op::SetItem, r, key, value, s)?;
                    }
                    if let Some((_, value)) = items.get(cursor) {
                        let value = self.expr(value)?;
                        self.emit(Op::DictMerge, r, value, 0, s)?;
                        cursor += 1;
                    }
                }
                Ok(r)
            }
            ExprKind::Tuple(values) | ExprKind::List(values) => {
                let (first, count) = self.window(values)?;
                let r = self.alloc(1)?;
                self.emit(
                    if matches!(e.kind, ExprKind::Tuple(_)) {
                        Op::Tuple
                    } else {
                        Op::List
                    },
                    r,
                    first,
                    count,
                    s,
                )?;
                Ok(r)
            }
            ExprKind::Attribute(value, name) => {
                let value = self.expr(value)?;
                let r = self.alloc(1)?;
                self.emit(Op::Attr, r, value, name.0, s)?;
                Ok(r)
            }
            ExprKind::Subscript(value, item) => {
                let value = self.expr(value)?;
                let item = self.expr(item)?;
                let r = self.alloc(1)?;
                self.emit(Op::Item, r, value, item, s)?;
                Ok(r)
            }
            ExprKind::Slice { start, stop, step } => {
                let first = self.alloc(3)?;
                for (offset, component) in [start, stop, step].into_iter().enumerate() {
                    let value = if let Some(component) = component {
                        self.expr(component)?
                    } else {
                        self.constant(Constant::None, s)?
                    };
                    self.emit(Op::Move, first + offset as u16, value, 0, s)?;
                }
                let result = self.alloc(1)?;
                self.emit(Op::Slice, result, first, 0, s)?;
                Ok(result)
            }
            ExprKind::Conditional(test, yes, no) => {
                let test = self.expr(test)?;
                let r = self.alloc(1)?;
                let branch = self.emit(Op::JumpFalse, test, 0, 0, s)?;
                let yes = self.expr(yes)?;
                self.emit(Op::Move, r, yes, 0, s)?;
                let end = self.emit(Op::Jump, 0, 0, 0, s)?;
                self.patch(branch, self.pc()?);
                let no = self.expr(no)?;
                self.emit(Op::Move, r, no, 0, s)?;
                self.patch(end, self.pc()?);
                Ok(r)
            }
        }
    }
}
fn binary(op: BinaryOp) -> Op {
    match op {
        BinaryOp::Add => Op::Add,
        BinaryOp::Subtract => Op::Sub,
        BinaryOp::Multiply => Op::Mul,
        BinaryOp::FloorDivide => Op::FloorDiv,
        BinaryOp::Modulo => Op::Mod,
        BinaryOp::Divide => Op::Div,
    }
}
fn compare(op: CompareOp) -> Op {
    match op {
        CompareOp::Equal => Op::Eq,
        CompareOp::NotEqual => Op::Ne,
        CompareOp::Less => Op::Lt,
        CompareOp::LessEqual => Op::Le,
        CompareOp::Greater => Op::Gt,
        CompareOp::GreaterEqual => Op::Ge,
    }
}
