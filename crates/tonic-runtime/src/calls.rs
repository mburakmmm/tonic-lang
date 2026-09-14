//! Call binding: ordinary calls read a register window; only actual expansion
//! uses scratch vectors. Guest tuple/dict objects exist only for *args/**kwargs.
use super::{Frame, Vm};
use crate::{
    dict::Dict,
    heap::{Builtin, Object},
    native::Context,
    value::Value,
};
use std::io::Write;
use tonic_core::{
    ast::SymbolId,
    bytecode::Program,
    diagnostic::{Diagnostic, Result},
};

#[derive(Clone, Default)]
pub(crate) struct ExpandedArgs {
    pub receiver: Option<Value>,
    pub positional: Vec<Value>,
    pub keywords: Vec<(String, Value)>,
    pub deferred_star: Option<Value>,
    // Non-string ** keys are diagnosed after argument expression evaluation.
    // Keep identities/values rooted meanwhile; duplicate merge errors are eager.
    pub invalid_keywords: Vec<(Value, Value)>,
}
impl ExpandedArgs {
    pub fn count(&self) -> usize {
        self.positional.len() + self.keywords.len() + self.invalid_keywords.len()
    }
    pub fn keyword(&mut self, name: String, value: Value) -> Result<()> {
        if self.keywords.iter().any(|(n, _)| *n == name) {
            return Err(Diagnostic::new(
                "TypeError",
                format!("multiple values for keyword argument '{name}'"),
            ));
        }
        self.keywords.push((name, value));
        Ok(())
    }
    pub fn trace(&self, mut visit: impl FnMut(Value)) {
        self.receiver.iter().copied().for_each(&mut visit);
        self.positional.iter().copied().for_each(&mut visit);
        self.keywords.iter().for_each(|(_, value)| visit(*value));
        self.deferred_star.iter().copied().for_each(&mut visit);
        self.invalid_keywords.iter().for_each(|(key, value)| {
            visit(*key);
            visit(*value);
        });
    }
}
pub(crate) enum Arguments<'a> {
    Direct {
        receiver: Option<Value>,
        first: usize,
        count: usize,
        keywords: &'a [SymbolId],
    },
    Inline {
        receiver: Option<Value>,
        positional: [Value; 2],
        count: usize,
    },
    Expanded(ExpandedArgs),
}
impl Arguments<'_> {
    fn owned(&self, p: &Program, registers: &[Value]) -> ExpandedArgs {
        let mut owned = ExpandedArgs {
            receiver: self.receiver(),
            ..ExpandedArgs::default()
        };
        let receiver = usize::from(self.receiver().is_some());
        owned.positional = (receiver..self.count())
            .map(|i| self.positional(registers, i))
            .collect();
        owned.keywords = (0..self.keyword_count())
            .map(|i| {
                let (name, value) = self.keyword(p, registers, i);
                (name.to_owned(), value)
            })
            .collect();
        if let Self::Expanded(args) = self {
            owned.deferred_star = args.deferred_star;
            owned.invalid_keywords = args.invalid_keywords.clone();
        }
        owned
    }
    fn bind(&mut self, receiver: Value) -> Result<()> {
        let slot = match self {
            Self::Direct { receiver, .. } => receiver,
            Self::Inline { receiver, .. } => receiver,
            Self::Expanded(args) => &mut args.receiver,
        };
        if slot.replace(receiver).is_some() {
            return Err(Diagnostic::new("BytecodeError", "duplicate bound receiver"));
        }
        Ok(())
    }
    fn receiver(&self) -> Option<Value> {
        match self {
            Self::Direct { receiver, .. } => *receiver,
            Self::Inline { receiver, .. } => *receiver,
            Self::Expanded(args) => args.receiver,
        }
    }
    fn count(&self) -> usize {
        usize::from(self.receiver().is_some())
            + match self {
                Self::Direct { count, .. } => *count,
                Self::Inline { count, .. } => *count,
                Self::Expanded(args) => args.positional.len(),
            }
    }
    fn positional(&self, registers: &[Value], i: usize) -> Value {
        if i == 0 {
            if let Some(receiver) = self.receiver() {
                return receiver;
            }
        }
        let i = i - usize::from(self.receiver().is_some());
        match self {
            Self::Direct { first, .. } => registers[*first + i],
            Self::Inline { positional, .. } => positional[i],
            Self::Expanded(args) => args.positional[i],
        }
    }
    fn keyword_count(&self) -> usize {
        match self {
            Self::Direct { keywords, .. } => keywords.len(),
            Self::Inline { .. } => 0,
            Self::Expanded(args) => args.keywords.len(),
        }
    }
    fn keyword<'a>(&'a self, p: &'a Program, registers: &[Value], i: usize) -> (&'a str, Value) {
        match self {
            Self::Direct {
                first,
                count,
                keywords,
                ..
            } => (
                &p.symbols[keywords[i].0 as usize],
                registers[*first + *count + i],
            ),
            Self::Inline { .. } => unreachable!("inline arguments have no keywords"),
            Self::Expanded(args) => (&args.keywords[i].0, args.keywords[i].1),
        }
    }
}
impl Vm {
    pub(super) fn invoke(
        &mut self,
        p: &Program,
        callee: Value,
        destination: usize,
        args: Arguments<'_>,
        output: &mut dyn Write,
    ) -> Result<()> {
        self.stats.calls += 1;
        self.invoke_target(p, callee, destination, args, output)
    }
    pub(crate) fn invoke_target(
        &mut self,
        p: &Program,
        callee: Value,
        destination: usize,
        mut args: Arguments<'_>,
        output: &mut dyn Write,
    ) -> Result<()> {
        if matches!(&args, Arguments::Expanded(args) if !args.invalid_keywords.is_empty()) {
            return Err(Diagnostic::new("TypeError", "keywords must be strings"));
        }
        let mut callee = callee;
        let mut redirects = 0;
        loop {
            let call = match self.heap.get(callee)? {
                Object::StaticMethod(function) => Some(crate::classes::DescriptorCall {
                    callable: *function,
                    receiver: None,
                }),
                Object::BoundMethod { function, receiver } => {
                    Some(crate::classes::DescriptorCall {
                        callable: *function,
                        receiver: Some(*receiver),
                    })
                }
                Object::Instance { .. } => {
                    let Some(call) = self.heap.special_method_call(callee, "__call__")? else {
                        return Err(Diagnostic::new("TypeError", "object is not callable"));
                    };
                    Some(call)
                }
                _ => None,
            };
            let Some(call) = call else { break };
            redirects += 1;
            if redirects > 100 {
                return Err(Diagnostic::new(
                    "RecursionError",
                    "callable protocol chain is too deep",
                ));
            }
            if let Some(receiver) = call.receiver {
                args.bind(receiver)?;
            }
            callee = call.callable;
        }
        match self.heap.get(callee)? {
            Object::PropertyDeleter(property) => {
                if args.count() != 1 || args.keyword_count() != 0 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "property deleter decorator expects one argument",
                    ));
                }
                let (getter, setter) = match self.heap.get(*property)? {
                    Object::Property { getter, setter, .. } => (*getter, *setter),
                    _ => return Err(Diagnostic::new("TypeError", "invalid property deleter")),
                };
                let deleter = args.positional(&self.registers, 0);
                self.registers[destination] = self.heap.alloc(Object::Property {
                    getter,
                    setter,
                    deleter: Some(deleter),
                })?;
            }
            Object::PropertySetter(property) => {
                if args.count() != 1 || args.keyword_count() != 0 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "property setter decorator expects one argument",
                    ));
                }
                let (getter, deleter) = match self.heap.get(*property)? {
                    Object::Property {
                        getter, deleter, ..
                    } => (*getter, *deleter),
                    _ => return Err(Diagnostic::new("TypeError", "invalid property setter")),
                };
                let setter = args.positional(&self.registers, 0);
                self.registers[destination] = self.heap.alloc(Object::Property {
                    getter,
                    setter: Some(setter),
                    deleter,
                })?;
            }
            Object::Class(_) => {
                self.invoke_class(p, callee, destination, args, output)?;
            }
            Object::Function {
                code, execution, ..
            } => {
                if *execution != self.execution {
                    return Err(Diagnostic::new(
                        "RuntimeError",
                        "callable belongs to a previous module execution",
                    ));
                }
                if p.code[*code as usize].class_body {
                    return Err(Diagnostic::new(
                        "BytecodeError",
                        "class body cannot be called directly",
                    ));
                }
                self.enter_frame(p, *code as usize, Some(destination), args, Some(callee))?;
            }
            Object::Builtin(builtin) => {
                let builtin = *builtin;
                if matches!(builtin, Builtin::Len) && args.keyword_count() == 0 && args.count() == 1
                {
                    let owner = args.positional(&self.registers, 0);
                    if let Some(call) = self.heap.special_method_call(owner, "__len__")? {
                        let depth = self.frames.len();
                        self.invoke_target(
                            p,
                            call.callable,
                            destination,
                            Arguments::Inline {
                                receiver: call.receiver,
                                positional: [Value::UNBOUND; 2],
                                count: 0,
                            },
                            output,
                        )?;
                        if self.frames.len() > depth {
                            self.frames.last_mut().expect("__len__ frame").action =
                                super::ReturnAction::Length;
                        } else {
                            self.registers[destination] =
                                self.validate_length(self.registers[destination])?;
                        }
                        return Ok(());
                    }
                }
                if matches!(
                    builtin,
                    Builtin::GetAttr | Builtin::SetAttr | Builtin::HasAttr
                ) && args.count() >= 2
                {
                    let owner = args.positional(&self.registers, 0);
                    let name = args.positional(&self.registers, 1);
                    if let Ok(Object::Str(name)) = self.heap.get(name) {
                        let name = name.clone();
                        if !matches!(builtin, Builtin::SetAttr) {
                            match self.heap.super_getter(owner, &name) {
                                Ok(Some(access)) => {
                                    if !matches!(builtin, Builtin::GetAttr) || args.count() != 2 {
                                        return Err(Diagnostic::new(
                                            "UnsupportedFeature",
                                            "hasattr/getattr(default) with descriptor is not implemented",
                                        ));
                                    }
                                    match access {
                                        crate::classes::DescriptorAccess::Value(value) => {
                                            self.registers[destination] = value;
                                            return Ok(());
                                        }
                                        crate::classes::DescriptorAccess::Call {
                                            callable,
                                            receiver,
                                            positional,
                                            count,
                                        } => {
                                            return self.invoke_target(
                                                p,
                                                callable,
                                                destination,
                                                Arguments::Inline {
                                                    receiver,
                                                    positional,
                                                    count,
                                                },
                                                output,
                                            );
                                        }
                                    }
                                }
                                Err(error)
                                    if error.kind == "AttributeError"
                                        && matches!(builtin, Builtin::HasAttr) =>
                                {
                                    self.registers[destination] = Value::bool(false);
                                    return Ok(());
                                }
                                Err(error)
                                    if error.kind == "AttributeError" && args.count() == 3 =>
                                {
                                    self.registers[destination] =
                                        args.positional(&self.registers, 2);
                                    return Ok(());
                                }
                                Err(error) => return Err(error),
                                Ok(None) => {}
                            }
                            if let Some(getter) = self.heap.property_getter(owner, &name)? {
                                if !matches!(builtin, Builtin::GetAttr) || args.count() != 2 {
                                    return Err(Diagnostic::new(
                                        "UnsupportedFeature",
                                        "hasattr/getattr(default) with property is not implemented",
                                    ));
                                }
                                return self.invoke_target(
                                    p,
                                    getter,
                                    destination,
                                    Arguments::Direct {
                                        receiver: Some(owner),
                                        first: 0,
                                        count: 0,
                                        keywords: &[],
                                    },
                                    output,
                                );
                            }
                            if let Some(access) = self.heap.descriptor_getter(owner, &name)? {
                                if !matches!(builtin, Builtin::GetAttr) || args.count() != 2 {
                                    return Err(Diagnostic::new(
                                        "UnsupportedFeature",
                                        "hasattr/getattr(default) with descriptor is not implemented",
                                    ));
                                }
                                match access {
                                    crate::classes::DescriptorAccess::Value(value) => {
                                        self.registers[destination] = value;
                                        return Ok(());
                                    }
                                    crate::classes::DescriptorAccess::Call {
                                        callable,
                                        receiver,
                                        positional,
                                        count,
                                    } => {
                                        return self.invoke_target(
                                            p,
                                            callable,
                                            destination,
                                            Arguments::Inline {
                                                receiver,
                                                positional,
                                                count,
                                            },
                                            output,
                                        );
                                    }
                                }
                            }
                        }
                        if matches!(builtin, Builtin::SetAttr) && args.count() == 3 {
                            if let Some(setter) = self.heap.property_setter(owner, &name)? {
                                let value = args.positional(&self.registers, 2);
                                let depth = self.frames.len();
                                self.invoke_target(
                                    p,
                                    setter,
                                    destination,
                                    Arguments::Expanded(ExpandedArgs {
                                        receiver: Some(owner),
                                        positional: vec![value],
                                        ..ExpandedArgs::default()
                                    }),
                                    output,
                                )?;
                                if self.frames.len() > depth {
                                    self.frames
                                        .last_mut()
                                        .expect("property setter frame")
                                        .action = super::ReturnAction::Setter;
                                } else {
                                    self.registers[destination] = Value::NONE;
                                }
                                return Ok(());
                            }
                            if let Some(setter) = self.heap.descriptor_setter(owner, &name)? {
                                let value = args.positional(&self.registers, 2);
                                let depth = self.frames.len();
                                self.invoke_target(
                                    p,
                                    setter.callable,
                                    destination,
                                    Arguments::Inline {
                                        receiver: setter.receiver,
                                        positional: [owner, value],
                                        count: 2,
                                    },
                                    output,
                                )?;
                                if self.frames.len() > depth {
                                    self.frames
                                        .last_mut()
                                        .expect("descriptor setter frame")
                                        .action = super::ReturnAction::Setter;
                                } else {
                                    self.registers[destination] = Value::NONE;
                                }
                                return Ok(());
                            }
                        }
                    }
                }
                self.registers[destination] = self.call_builtin(builtin, p, &args, output)?
            }
            Object::Native(id) => {
                let def = &self.natives[*id];
                if args.keyword_count() != 0 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "registered native function does not accept keyword arguments",
                    ));
                }
                if args.count() != def.arity {
                    return Err(Diagnostic::new(
                        "TypeError",
                        format!(
                            "native function expects {} arguments, got {}",
                            def.arity,
                            args.count()
                        ),
                    ));
                }
                let function = def.function;
                let values = (0..args.count())
                    .map(|index| args.positional(&self.registers, index))
                    .collect::<Vec<_>>();
                self.stats.native_calls += 1;
                let mut ctx = Context::for_native_call(self, p, output);
                let handles = values
                    .into_iter()
                    .map(|value| ctx.local(value))
                    .collect::<Result<Vec<_>>>()?;
                let result = match function {
                    crate::native::NativeCallable::Rust(function) => function(&mut ctx, &handles),
                    crate::native::NativeCallable::C(function) => {
                        crate::c_api::invoke_native(&mut ctx, function, &handles)
                    }
                }?;
                let result = ctx.resolve(result)?;
                drop(ctx);
                self.registers[destination] = result;
            }
            _ => return Err(Diagnostic::new("TypeError", "object is not callable")),
        }
        Ok(())
    }
    pub(super) fn invoke_truth(
        &mut self,
        p: &Program,
        value: Value,
        destination: usize,
        action: super::TruthAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        let protocol_call = if let Some(call) = self.heap.special_method_call(value, "__bool__")? {
            Some((call, super::TruthProtocol::Bool))
        } else {
            self.heap
                .special_method_call(value, "__len__")?
                .map(|call| (call, super::TruthProtocol::Length))
        };
        let Some((call, protocol)) = protocol_call else {
            let truth = self.heap.truth(value)?;
            self.apply_truth(destination, truth, action);
            return Ok(());
        };
        let depth = self.frames.len();
        self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Inline {
                receiver: call.receiver,
                positional: [Value::UNBOUND; 2],
                count: 0,
            },
            output,
        )?;
        if self.frames.len() > depth {
            self.frames.last_mut().expect("truth protocol frame").action =
                super::ReturnAction::Truth { protocol, action };
        } else {
            self.finish_truth(destination, self.registers[destination], protocol, action)?;
        }
        Ok(())
    }
    pub(super) fn finish_truth(
        &mut self,
        destination: usize,
        value: Value,
        protocol: super::TruthProtocol,
        action: super::TruthAction,
    ) -> Result<()> {
        let truth = match protocol {
            super::TruthProtocol::Bool => value
                .as_bool()
                .ok_or_else(|| Diagnostic::new("TypeError", "__bool__ should return bool"))?,
            super::TruthProtocol::Length => self.checked_length(value)? != 0,
        };
        self.apply_truth(destination, truth, action);
        Ok(())
    }
    fn apply_truth(&mut self, destination: usize, truth: bool, action: super::TruthAction) {
        match action {
            super::TruthAction::Not => self.registers[destination] = Value::bool(!truth),
            super::TruthAction::Jump {
                when,
                target,
                pc,
                original,
            } => {
                self.registers[destination] = original;
                if truth == when {
                    self.jump(target, pc);
                }
            }
        }
    }
    fn checked_length(&self, value: Value) -> Result<i64> {
        if !self.heap.is_integer(value) {
            return Err(Diagnostic::new(
                "TypeError",
                "__len__ returned a non-integer value",
            ));
        }
        let length = self.heap.to_i64(value)?;
        if length < 0 {
            return Err(Diagnostic::new(
                "ValueError",
                "__len__() should return >= 0",
            ));
        }
        Ok(length)
    }
    pub(super) fn validate_length(&mut self, value: Value) -> Result<Value> {
        let length = self.checked_length(value)?;
        self.heap.i64(length)
    }
    fn invoke_class(
        &mut self,
        p: &Program,
        class: Value,
        destination: usize,
        args: Arguments<'_>,
        output: &mut dyn Write,
    ) -> Result<()> {
        let mut arguments = args.owned(p, &self.registers);
        if arguments.receiver.take().is_some() {
            return Err(Diagnostic::new("BytecodeError", "bound class call"));
        }
        let constructor = self
            .heap
            .class_lookup(class, "__new__")?
            .ok_or_else(|| Diagnostic::new("TypeError", "class has no __new__"))?;
        if matches!(
            self.heap.get(constructor),
            Ok(Object::Builtin(Builtin::ObjectNew))
        ) {
            let instance = self.heap.instance(class)?;
            return self.finish_new(p, destination, class, instance, arguments, output);
        }
        let mut constructor_args = arguments.clone();
        constructor_args.receiver = Some(class);
        let depth = self.frames.len();
        self.invoke_target(
            p,
            constructor,
            destination,
            Arguments::Expanded(constructor_args),
            output,
        )?;
        if self.frames.len() > depth {
            self.frames.last_mut().expect("__new__ frame").action =
                super::ReturnAction::New { class, arguments };
        } else {
            let value = self.registers[destination];
            self.finish_new(p, destination, class, value, arguments, output)?;
        }
        Ok(())
    }
    pub(super) fn finish_new(
        &mut self,
        p: &Program,
        destination: usize,
        class: Value,
        instance: Value,
        arguments: ExpandedArgs,
        output: &mut dyn Write,
    ) -> Result<()> {
        if !self.heap.instance_check(instance, class, false, 0)? {
            self.registers[destination] = instance;
            return Ok(());
        }
        if self.heap.class_lookup(class, "__init__")?.is_none() {
            if arguments.count() != 0 || !arguments.keywords.is_empty() {
                return Err(Diagnostic::new(
                    "TypeError",
                    "class without __init__ accepts no arguments",
                ));
            }
            self.registers[destination] = instance;
            return Ok(());
        }
        let initializer = self.heap.attr(instance, "__init__")?;
        if !matches!(
            self.heap.get(initializer)?,
            Object::Function { .. } | Object::BoundMethod { .. }
        ) {
            return Err(Diagnostic::new(
                "TypeError",
                "__init__ must be a Tonic function",
            ));
        }
        let depth = self.frames.len();
        self.invoke_target(
            p,
            initializer,
            destination,
            Arguments::Expanded(arguments),
            output,
        )?;
        if self.frames.len() > depth {
            self.frames.last_mut().expect("initializer frame").action =
                super::ReturnAction::Initializer(instance);
        } else {
            self.registers[destination] = instance;
        }
        Ok(())
    }
    pub(super) fn enter_frame(
        &mut self,
        p: &Program,
        code: usize,
        destination: Option<usize>,
        args: Arguments<'_>,
        callable: Option<Value>,
    ) -> Result<()> {
        if self.frames.len() >= self.limits.frames {
            return Err(Diagnostic::new(
                "RecursionError",
                "maximum call depth exceeded",
            ));
        }
        let base = self.registers.len();
        let metadata = &p.code[code];
        let sig = &metadata.signature;
        let end = base
            .checked_add(metadata.registers as usize)
            .ok_or_else(|| Diagnostic::new("MemoryError", "register size overflow"))?;
        if end > self.limits.registers {
            return Err(Diagnostic::new("MemoryError", "register budget exceeded"));
        }
        let positional = sig.positional as usize;
        let named = positional + sig.keyword_only as usize;
        if args.count() > positional && sig.vararg.is_none() {
            return Err(Diagnostic::new(
                "TypeError",
                format!(
                    "{}() accepts at most {positional} positional arguments, got {}",
                    metadata.name,
                    args.count()
                ),
            ));
        }
        self.registers.resize(end, Value::UNBOUND);
        for i in 0..args.count().min(positional) {
            self.registers[base + i] = args.positional(&self.registers, i);
        }
        if let Some(slot) = sig.vararg {
            let values = (positional..args.count())
                .map(|i| args.positional(&self.registers, i))
                .collect();
            self.registers[base + slot as usize] = self.heap.alloc(Object::Tuple(values))?;
        }
        let kwdict = if let Some(slot) = sig.kwarg {
            let value = self.heap.alloc(Object::Dict(Dict::default()))?;
            self.registers[base + slot as usize] = value;
            Some(value)
        } else {
            None
        };
        for i in 0..args.keyword_count() {
            let (name, value) = args.keyword(p, &self.registers, i);
            if let Some(slot) = (sig.posonly as usize..named)
                .find(|slot| p.symbols[metadata.locals[*slot].0 as usize] == name)
            {
                if self.registers[base + slot] != Value::UNBOUND {
                    return Err(Diagnostic::new(
                        "TypeError",
                        format!(
                            "{}() got multiple values for argument '{name}'",
                            metadata.name
                        ),
                    ));
                }
                self.registers[base + slot] = value;
            } else if let Some(dict) = kwdict {
                let key = self.heap.alloc(Object::Str(name.to_owned()))?;
                self.heap.dict_set(dict, key, value)?;
            } else {
                return Err(Diagnostic::new(
                    "TypeError",
                    format!(
                        "{}() got unexpected or positional-only keyword '{name}'",
                        metadata.name
                    ),
                ));
            }
        }
        if let Some(callable) = callable {
            let Object::Function { defaults, .. } = self.heap.get(callable)? else {
                unreachable!()
            };
            for (slot, value) in sig.defaults.iter().zip(defaults) {
                let reg = base + *slot as usize;
                if self.registers[reg] == Value::UNBOUND {
                    self.registers[reg] = *value;
                }
            }
        }
        for slot in 0..named {
            if self.registers[base + slot] == Value::UNBOUND {
                return Err(Diagnostic::new(
                    "TypeError",
                    format!(
                        "{}() missing required argument '{}'",
                        metadata.name, p.symbols[metadata.locals[slot].0 as usize]
                    ),
                ));
            }
        }
        let cell_base = self.cells.len();
        for local in &metadata.cell_locals {
            let reg = base + *local as usize;
            self.cells
                .push(self.heap.alloc(Object::Cell(self.registers[reg]))?);
            self.registers[reg] = Value::UNBOUND;
        }
        if let Some(callable) = callable {
            if let Object::Function { captures, .. } = self.heap.get(callable)? {
                self.cells.extend_from_slice(captures);
            }
        }
        self.frames.push(Frame {
            code,
            ip: 0,
            base,
            destination,
            cell_base,
            callable,
            namespace: None,
            action: super::ReturnAction::Value,
            jit_attempted: matches!(self.jit_cache.get(code), Some(super::JitEntry::Unsupported)),
            jit_resume: false,
            jit_expanded_resume_depth: None,
        });
        self.stats.peak_registers = self.stats.peak_registers.max(end);
        Ok(())
    }
    fn call_builtin(
        &mut self,
        builtin: Builtin,
        p: &Program,
        args: &Arguments<'_>,
        output: &mut dyn Write,
    ) -> Result<Value> {
        if !matches!(builtin, Builtin::Print) && args.keyword_count() != 0 {
            return Err(Diagnostic::new(
                "TypeError",
                "builtin does not accept keyword arguments",
            ));
        }
        let count = args.count();
        match builtin {
            Builtin::ObjectNew => {
                if count != 1 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "object.__new__ expects one class argument",
                    ));
                }
                self.heap.instance(args.positional(&self.registers, 0))
            }
            Builtin::Super => {
                let (start_class, receiver) = match count {
                    0 => {
                        let frame = self.frames.last().ok_or_else(|| {
                            Diagnostic::new("RuntimeError", "super(): no current frame")
                        })?;
                        let metadata = &p.code[frame.code];
                        let free = metadata
                            .free_vars
                            .iter()
                            .position(|name| p.symbols[name.0 as usize] == "__class__")
                            .ok_or_else(|| {
                                Diagnostic::new("RuntimeError", "super(): __class__ cell not found")
                            })?;
                        if metadata.params == 0 {
                            return Err(Diagnostic::new("RuntimeError", "super(): no arguments"));
                        }
                        let class_cell =
                            self.cells[frame.cell_base + metadata.cell_locals.len() + free];
                        let start_class = self.heap.cell(class_cell)?;
                        if start_class == Value::UNBOUND {
                            return Err(Diagnostic::new(
                                "RuntimeError",
                                "super(): empty __class__ cell",
                            ));
                        }
                        let receiver = self.registers[frame.base];
                        (start_class, receiver)
                    }
                    2 => (
                        args.positional(&self.registers, 0),
                        args.positional(&self.registers, 1),
                    ),
                    _ => {
                        return Err(Diagnostic::new(
                            "TypeError",
                            "super() expects zero or two arguments",
                        ))
                    }
                };
                self.heap.super_value(start_class, receiver)
            }
            Builtin::Property => {
                if count > 3 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "property expects at most three arguments",
                    ));
                }
                let getter = (count > 0)
                    .then(|| args.positional(&self.registers, 0))
                    .filter(|v| *v != Value::NONE);
                let setter = (count > 1)
                    .then(|| args.positional(&self.registers, 1))
                    .filter(|v| *v != Value::NONE);
                let deleter = (count > 2)
                    .then(|| args.positional(&self.registers, 2))
                    .filter(|v| *v != Value::NONE);
                Ok(self.heap.alloc(Object::Property {
                    getter,
                    setter,
                    deleter,
                })?)
            }
            Builtin::StaticMethod | Builtin::ClassMethod => {
                if count != 1 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "method descriptor expects one argument",
                    ));
                }
                let function = args.positional(&self.registers, 0);
                Ok(self
                    .heap
                    .alloc(if matches!(builtin, Builtin::StaticMethod) {
                        Object::StaticMethod(function)
                    } else {
                        Object::ClassMethod(function)
                    })?)
            }
            Builtin::IsInstance | Builtin::IsSubclass => {
                if count != 2 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "type check expects two arguments",
                    ));
                }
                let result = self.heap.instance_check(
                    args.positional(&self.registers, 0),
                    args.positional(&self.registers, 1),
                    matches!(builtin, Builtin::IsSubclass),
                    0,
                )?;
                Ok(Value::bool(result))
            }
            Builtin::GetAttr | Builtin::SetAttr | Builtin::HasAttr => {
                let valid = match builtin {
                    Builtin::GetAttr => (2..=3).contains(&count),
                    Builtin::SetAttr => count == 3,
                    _ => count == 2,
                };
                if !valid {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "invalid attribute builtin arity",
                    ));
                }
                let owner = args.positional(&self.registers, 0);
                let name = args.positional(&self.registers, 1);
                let Ok(Object::Str(name)) = self.heap.get(name) else {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "attribute name must be a string",
                    ));
                };
                let name = name.clone();
                if matches!(builtin, Builtin::SetAttr) {
                    self.heap
                        .set_attr(owner, &name, args.positional(&self.registers, 2))?;
                    return Ok(Value::NONE);
                }
                match self.heap.attr(owner, &name) {
                    Ok(value) => Ok(if matches!(builtin, Builtin::HasAttr) {
                        Value::bool(true)
                    } else {
                        value
                    }),
                    Err(e) if e.kind == "AttributeError" && matches!(builtin, Builtin::HasAttr) => {
                        Ok(Value::bool(false))
                    }
                    Err(e) if e.kind == "AttributeError" && count == 3 => {
                        Ok(args.positional(&self.registers, 2))
                    }
                    Err(e) => Err(e),
                }
            }
            Builtin::Print => {
                let (mut sep, mut end) = (" ".to_owned(), "\n".to_owned());
                for i in 0..args.keyword_count() {
                    let (name, value) = args.keyword(p, &self.registers, i);
                    let default = match name {
                        "sep" => " ",
                        "end" => "\n",
                        _ => {
                            return Err(Diagnostic::new(
                                "TypeError",
                                format!("invalid print keyword '{name}'"),
                            ))
                        }
                    };
                    let text = if value == Value::NONE {
                        default.to_owned()
                    } else {
                        let Object::Str(s) = self.heap.get(value)? else {
                            return Err(Diagnostic::new(
                                "TypeError",
                                "print sep/end must be str or None",
                            ));
                        };
                        s.clone()
                    };
                    if name == "sep" {
                        sep = text;
                    } else {
                        end = text;
                    }
                }
                for i in 0..count {
                    if i > 0 {
                        output.write_all(sep.as_bytes()).map_err(io_error)?;
                    }
                    let text = self
                        .heap
                        .format(args.positional(&self.registers, i), false)?;
                    output.write_all(text.as_bytes()).map_err(io_error)?;
                }
                output.write_all(end.as_bytes()).map_err(io_error)?;
                Ok(Value::NONE)
            }
            Builtin::Range => {
                if !(1..=3).contains(&count) {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "range expects 1 to 3 arguments",
                    ));
                }
                let x = self.heap.to_i64(args.positional(&self.registers, 0))?;
                let (start, stop) = if count == 1 {
                    (0, x)
                } else {
                    (x, self.heap.to_i64(args.positional(&self.registers, 1))?)
                };
                let step = if count == 3 {
                    self.heap.to_i64(args.positional(&self.registers, 2))?
                } else {
                    1
                };
                if step == 0 {
                    return Err(Diagnostic::new("ValueError", "range step must not be zero"));
                }
                self.heap.alloc(Object::Range { start, stop, step })
            }
            Builtin::Len | Builtin::Abs => {
                if count != 1 {
                    return Err(Diagnostic::new("TypeError", "builtin expects one argument"));
                }
                let value = args.positional(&self.registers, 0);
                if matches!(builtin, Builtin::Len) {
                    return self.heap.length(value);
                }
                if self.heap.is_float(value) {
                    let n = self.heap.float(value)?.abs();
                    return self.heap.alloc(Object::Float(n));
                }
                let zero = Value::int(0).expect("immediate");
                let negative = self
                    .heap
                    .compare(tonic_core::bytecode::Op::Lt, value, zero)?
                    .as_bool()
                    .expect("comparison");
                self.heap.unary(
                    if negative {
                        tonic_core::bytecode::Op::Neg
                    } else {
                        tonic_core::bytecode::Op::Pos
                    },
                    value,
                )
            }
        }
    }
    pub(super) fn append_argument(
        &mut self,
        p: &Program,
        op: tonic_core::bytecode::Op,
        value: Value,
        name: u16,
        flags: u16,
    ) -> Result<()> {
        use tonic_core::bytecode::Op;
        let limit = self.limits.arguments;
        let room = |args: &ExpandedArgs| -> Result<()> {
            if args.count() >= limit {
                Err(Diagnostic::new(
                    "ResourceError",
                    "expanded argument budget exceeded",
                ))
            } else {
                Ok(())
            }
        };
        match op {
            Op::ArgPos => {
                let args = self.arguments.last_mut().expect("verified argument stack");
                room(args)?;
                args.positional.push(value);
            }
            Op::ArgStar if flags == 1 => {
                // A lone star expression is evaluated now, but materialized at
                // the call boundary, after keyword expressions (Python order).
                let args = self.arguments.last_mut().expect("verified argument stack");
                if args.deferred_star.is_some() || !args.positional.is_empty() {
                    return Err(Diagnostic::new(
                        "BytecodeError",
                        "invalid deferred argument state",
                    ));
                }
                args.deferred_star = Some(value);
            }
            Op::ArgStar => {
                let iter = self.heap.iterator(value)?;
                while let Some(value) = self.heap.next(iter)? {
                    let args = self.arguments.last_mut().expect("verified argument stack");
                    room(args)?;
                    args.positional.push(value);
                }
            }
            Op::ArgNamed => {
                let args = self.arguments.last_mut().expect("verified argument stack");
                room(args)?;
                args.keyword(p.symbols[name as usize].clone(), value)?;
            }
            Op::ArgMapping => {
                let Object::Dict(dict) = self.heap.get(value)? else {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "** argument must be a mapping",
                    ));
                };
                for (key, value) in &dict.entries {
                    let args = self.arguments.last_mut().expect("verified argument stack");
                    room(args)?;
                    if let Ok(Object::Str(name)) = self.heap.get(*key) {
                        args.keyword(name.clone(), *value)?;
                    } else {
                        for (previous, _) in &args.invalid_keywords {
                            if self.heap.dict_keys_equal(*previous, *key)? {
                                return Err(Diagnostic::new(
                                    "TypeError",
                                    "multiple values for keyword argument",
                                ));
                            }
                        }
                        args.invalid_keywords.push((*key, *value));
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
fn io_error(error: std::io::Error) -> Diagnostic {
    Diagnostic::new("OSError", error.to_string())
}
