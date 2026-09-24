//! Call binding: ordinary calls read a register window; only actual expansion
//! uses scratch vectors. Guest tuple/dict objects exist only for *args/**kwargs.
use super::{
    ArgumentExpansion, DictConstruction, DictConstructionStart, DictPairConstruction, Frame,
    IterableCollection, IterableCollectionKind, NativeSubclassFinish, ReturnAction, Vm,
};
use crate::{
    dict::Dict,
    hashing::{
        hash_range, hash_string, hash_u64, normalize_bigint, sequence_finish, sequence_start,
        sequence_step,
    },
    heap::{Builtin, Object},
    native::Context,
    value::Value,
};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive};
use std::io::Write;
use tonic_core::{
    ast::SymbolId,
    bytecode::Program,
    diagnostic::{Diagnostic, Result},
};

const PROTOCOL_NESTING_LIMIT: usize = 32;

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
        positional: [Value; 3],
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
    pub(super) fn count(&self) -> usize {
        usize::from(self.receiver().is_some())
            + match self {
                Self::Direct { count, .. } => *count,
                Self::Inline { count, .. } => *count,
                Self::Expanded(args) => args.positional.len(),
            }
    }
    pub(super) fn positional(&self, registers: &[Value], i: usize) -> Value {
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
    pub(super) fn keyword_count(&self) -> usize {
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

fn native_new_kind(builtin: Builtin) -> Option<super::RuntimeTypeKind> {
    Some(match builtin {
        Builtin::IntNew => super::RuntimeTypeKind::Int,
        Builtin::BoolNew => super::RuntimeTypeKind::Bool,
        Builtin::FloatNew => super::RuntimeTypeKind::Float,
        Builtin::StrNew => super::RuntimeTypeKind::Str,
        Builtin::ListNew => super::RuntimeTypeKind::List,
        Builtin::TupleNew => super::RuntimeTypeKind::Tuple,
        Builtin::DictNew => super::RuntimeTypeKind::Dict,
        Builtin::RangeNew => super::RuntimeTypeKind::Range,
        _ => return None,
    })
}

fn native_hash_kind(builtin: Builtin) -> Option<super::RuntimeTypeKind> {
    Some(match builtin {
        Builtin::IntHash => super::RuntimeTypeKind::Int,
        Builtin::FloatHash => super::RuntimeTypeKind::Float,
        Builtin::StrHash => super::RuntimeTypeKind::Str,
        Builtin::TupleHash => super::RuntimeTypeKind::Tuple,
        Builtin::RangeHash => super::RuntimeTypeKind::Range,
        _ => return None,
    })
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
                object if object.instance_class().is_some() => {
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
                if let Some(kind) = native_new_kind(builtin) {
                    return self.invoke_native_new(p, kind, destination, &args, output);
                }
                if matches!(builtin, Builtin::ListInit | Builtin::DictInit) {
                    return self.invoke_native_initializer(
                        p,
                        builtin,
                        destination,
                        &args,
                        Value::NONE,
                        output,
                    );
                }
                if matches!(builtin, Builtin::TypeNew) {
                    return self.invoke_type_new(p, destination, &args, output);
                }
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
                                positional: [Value::UNBOUND; 3],
                                count: 0,
                            },
                            output,
                        )?;
                        if self.frames.len() > depth {
                            self.frames.last_mut().expect("__len__ frame").action =
                                super::ReturnAction::Length;
                        } else {
                            self.finish_length(
                                p,
                                destination,
                                self.registers[destination],
                                output,
                            )?;
                        }
                        return Ok(());
                    }
                }
                if matches!(builtin, Builtin::Hash | Builtin::ObjectHash)
                    || native_hash_kind(builtin).is_some()
                {
                    if args.keyword_count() != 0 || args.count() != 1 {
                        return Err(Diagnostic::new(
                            "TypeError",
                            "hash operation expects one argument",
                        ));
                    }
                    let value = args.positional(&self.registers, 0);
                    return match builtin {
                        Builtin::Hash => self.invoke_hash_protocol(
                            p,
                            value,
                            destination,
                            super::HashAction::Return,
                            output,
                        ),
                        Builtin::ObjectHash => self.complete_hash(
                            p,
                            destination,
                            hash_u64(value.raw()),
                            super::HashAction::Return,
                            output,
                        ),
                        _ => self.invoke_typed_native_hash(
                            p,
                            (builtin, value),
                            destination,
                            0,
                            super::HashAction::Return,
                            output,
                        ),
                    };
                }
                if matches!(builtin, Builtin::Abs) && args.keyword_count() == 0 && args.count() == 1
                {
                    let value = args.positional(&self.registers, 0);
                    return self.invoke_unary_protocol(
                        p,
                        value,
                        destination,
                        super::UnaryProtocolKind::Abs,
                        output,
                    );
                }
                if matches!(
                    builtin,
                    Builtin::GetAttr
                        | Builtin::SetAttr
                        | Builtin::DelAttr
                        | Builtin::HasAttr
                        | Builtin::ObjectGetAttribute
                        | Builtin::ObjectSetAttr
                        | Builtin::ObjectDelAttr
                        | Builtin::TypeGetAttribute
                        | Builtin::TypeSetAttr
                        | Builtin::TypeDelAttr
                ) {
                    if args.keyword_count() != 0 {
                        return Err(Diagnostic::new(
                            "TypeError",
                            "attribute builtin does not accept keyword arguments",
                        ));
                    }
                    let count = args.count();
                    let valid = match builtin {
                        Builtin::GetAttr => (2..=3).contains(&count),
                        Builtin::SetAttr | Builtin::ObjectSetAttr | Builtin::TypeSetAttr => {
                            count == 3
                        }
                        Builtin::DelAttr
                        | Builtin::HasAttr
                        | Builtin::ObjectGetAttribute
                        | Builtin::ObjectDelAttr
                        | Builtin::TypeGetAttribute
                        | Builtin::TypeDelAttr => count == 2,
                        _ => unreachable!(),
                    };
                    if !valid {
                        return Err(Diagnostic::new(
                            "TypeError",
                            "invalid attribute builtin arity",
                        ));
                    }
                    let owner = args.positional(&self.registers, 0);
                    let class_owner = matches!(self.heap.get(owner), Ok(Object::Class(_)));
                    if matches!(
                        builtin,
                        Builtin::TypeGetAttribute | Builtin::TypeSetAttr | Builtin::TypeDelAttr
                    ) && !class_owner
                    {
                        return Err(Diagnostic::new(
                            "TypeError",
                            "type attribute descriptor requires a class object",
                        ));
                    }
                    if matches!(builtin, Builtin::ObjectSetAttr | Builtin::ObjectDelAttr)
                        && class_owner
                    {
                        return Err(Diagnostic::new(
                            "TypeError",
                            "object attribute mutation does not accept a class object",
                        ));
                    }
                    let name_value = args.positional(&self.registers, 1);
                    let name = match self.heap.get(name_value) {
                        Ok(Object::Str(name)) => name.clone(),
                        _ => {
                            return Err(Diagnostic::new(
                                "TypeError",
                                "attribute name must be a string",
                            ))
                        }
                    };
                    return match builtin {
                        Builtin::GetAttr => {
                            let missing = if count == 3 {
                                super::AttributeMissing::Default(
                                    args.positional(&self.registers, 2),
                                )
                            } else {
                                super::AttributeMissing::Raise
                            };
                            self.invoke_attribute_get(p, owner, &name, destination, missing, output)
                        }
                        Builtin::HasAttr => self.invoke_attribute_get(
                            p,
                            owner,
                            &name,
                            destination,
                            super::AttributeMissing::HasAttr,
                            output,
                        ),
                        Builtin::SetAttr => self.invoke_attribute_set(
                            p,
                            owner,
                            &name,
                            args.positional(&self.registers, 2),
                            destination,
                            output,
                        ),
                        Builtin::DelAttr => {
                            self.invoke_attribute_delete(p, owner, &name, destination, output)
                        }
                        Builtin::ObjectGetAttribute | Builtin::TypeGetAttribute => self
                            .invoke_default_attribute_get(
                                p,
                                owner,
                                &name,
                                destination,
                                None,
                                output,
                            ),
                        Builtin::ObjectSetAttr | Builtin::TypeSetAttr => self
                            .invoke_default_attribute_set(
                                p,
                                owner,
                                &name,
                                args.positional(&self.registers, 2),
                                destination,
                                output,
                            ),
                        Builtin::ObjectDelAttr | Builtin::TypeDelAttr => self
                            .invoke_default_attribute_delete(p, owner, &name, destination, output),
                        _ => unreachable!(),
                    };
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
    fn finish_attribute_success(&mut self, destination: usize, state: &super::AttributeGet) {
        if matches!(state.missing, super::AttributeMissing::HasAttr) {
            self.registers[destination] = Value::bool(true);
        }
    }

    fn invoke_attribute_call(
        &mut self,
        p: &Program,
        call: crate::classes::DescriptorCall,
        destination: usize,
        arguments: ([Value; 3], usize),
        state: Option<super::AttributeGet>,
        output: &mut dyn Write,
    ) -> Result<()> {
        let (positional, count) = arguments;
        let depth = self.frames.len();
        self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Inline {
                receiver: call.receiver,
                positional,
                count,
            },
            output,
        )?;
        if let Some(state) = state {
            if self.frames.len() > depth {
                self.frames
                    .last_mut()
                    .expect("attribute protocol frame")
                    .action = super::ReturnAction::AttributeGet(state);
            } else {
                self.finish_attribute_success(destination, &state);
            }
        }
        Ok(())
    }

    fn invoke_default_attribute_get(
        &mut self,
        p: &Program,
        owner: Value,
        name: &str,
        destination: usize,
        state: Option<super::AttributeGet>,
        output: &mut dyn Write,
    ) -> Result<()> {
        let class_owner = matches!(self.heap.get(owner), Ok(Object::Class(_)));
        if let Some(access) = self.heap.super_getter(owner, name)? {
            match access {
                crate::classes::DescriptorAccess::Value(value) => {
                    self.registers[destination] = value;
                    if let Some(state) = state.as_ref() {
                        self.finish_attribute_success(destination, state);
                    }
                    return Ok(());
                }
                crate::classes::DescriptorAccess::Call {
                    callable,
                    receiver,
                    positional,
                    count,
                } => {
                    return self.invoke_attribute_call(
                        p,
                        crate::classes::DescriptorCall { callable, receiver },
                        destination,
                        (positional, count),
                        state,
                        output,
                    );
                }
            }
        }
        if class_owner {
            if let Some(access) = self.heap.metaclass_getter(owner, name, true)? {
                match access {
                    crate::classes::DescriptorAccess::Value(value) => {
                        self.registers[destination] = value;
                        if let Some(state) = state.as_ref() {
                            self.finish_attribute_success(destination, state);
                        }
                        return Ok(());
                    }
                    crate::classes::DescriptorAccess::Call {
                        callable,
                        receiver,
                        positional,
                        count,
                    } => {
                        return self.invoke_attribute_call(
                            p,
                            crate::classes::DescriptorCall { callable, receiver },
                            destination,
                            (positional, count),
                            state,
                            output,
                        );
                    }
                }
            }
        }
        if let Some(getter) = self.heap.property_getter(owner, name)? {
            return self.invoke_attribute_call(
                p,
                crate::classes::DescriptorCall {
                    callable: getter,
                    receiver: Some(owner),
                },
                destination,
                ([Value::UNBOUND; 3], 0),
                state,
                output,
            );
        }
        if let Some(access) = self.heap.descriptor_getter(owner, name)? {
            match access {
                crate::classes::DescriptorAccess::Value(value) => {
                    self.registers[destination] = value;
                    if let Some(state) = state.as_ref() {
                        self.finish_attribute_success(destination, state);
                    }
                    return Ok(());
                }
                crate::classes::DescriptorAccess::Call {
                    callable,
                    receiver,
                    positional,
                    count,
                } => {
                    return self.invoke_attribute_call(
                        p,
                        crate::classes::DescriptorCall { callable, receiver },
                        destination,
                        (positional, count),
                        state,
                        output,
                    );
                }
            }
        }
        match self.heap.attr(owner, name) {
            Ok(value) => self.registers[destination] = value,
            Err(error) if name == "__class__" && error.kind == "AttributeError" => {
                self.registers[destination] = self.runtime_class(owner)?;
            }
            Err(error) if class_owner && error.kind == "AttributeError" => {
                let Some(access) = self.heap.metaclass_getter(owner, name, false)? else {
                    return Err(error);
                };
                match access {
                    crate::classes::DescriptorAccess::Value(value) => {
                        self.registers[destination] = value;
                    }
                    crate::classes::DescriptorAccess::Call {
                        callable,
                        receiver,
                        positional,
                        count,
                    } => {
                        return self.invoke_attribute_call(
                            p,
                            crate::classes::DescriptorCall { callable, receiver },
                            destination,
                            (positional, count),
                            state,
                            output,
                        );
                    }
                }
            }
            Err(error) => return Err(error),
        }
        if let Some(state) = state.as_ref() {
            self.finish_attribute_success(destination, state);
        }
        Ok(())
    }

    pub(super) fn invoke_attribute_get(
        &mut self,
        p: &Program,
        owner: Value,
        name: &str,
        destination: usize,
        missing: super::AttributeMissing,
        output: &mut dyn Write,
    ) -> Result<()> {
        let state = super::AttributeGet {
            owner,
            name: name.to_owned(),
            missing,
            phase: super::AttributePhase::Primary,
        };
        let default = if matches!(self.heap.get(owner), Ok(Object::Class(_))) {
            Builtin::TypeGetAttribute
        } else {
            Builtin::ObjectGetAttribute
        };
        let result = if let Some(call) =
            self.heap
                .custom_attribute_method(owner, "__getattribute__", default)?
        {
            let name = self.heap.alloc(Object::Str(name.to_owned()))?;
            self.invoke_attribute_call(
                p,
                call,
                destination,
                ([name, Value::UNBOUND, Value::UNBOUND], 1),
                Some(state.clone()),
                output,
            )
        } else {
            self.invoke_default_attribute_get(
                p,
                owner,
                name,
                destination,
                Some(state.clone()),
                output,
            )
        };
        match result {
            Err(error) if error.kind == "AttributeError" => {
                self.continue_attribute_missing(p, destination, state, error, output)
            }
            result => result,
        }
    }

    pub(super) fn continue_attribute_missing(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: super::AttributeGet,
        error: Diagnostic,
        output: &mut dyn Write,
    ) -> Result<()> {
        if state.phase == super::AttributePhase::Primary {
            let call = if matches!(self.heap.get(state.owner), Ok(Object::Class(_))) {
                self.heap
                    .metaclass_method_call(state.owner, "__getattr__")?
            } else {
                self.heap.special_method_call(state.owner, "__getattr__")?
            };
            if let Some(call) = call {
                state.phase = super::AttributePhase::Fallback;
                let name = self.heap.alloc(Object::Str(state.name.clone()))?;
                let result = self.invoke_attribute_call(
                    p,
                    call,
                    destination,
                    ([name, Value::UNBOUND, Value::UNBOUND], 1),
                    Some(state.clone()),
                    output,
                );
                return match result {
                    Err(fallback_error) if fallback_error.kind == "AttributeError" => self
                        .continue_attribute_missing(p, destination, state, fallback_error, output),
                    result => result,
                };
            }
        }
        match state.missing {
            super::AttributeMissing::Raise => Err(error),
            super::AttributeMissing::Default(value) => {
                self.registers[destination] = value;
                Ok(())
            }
            super::AttributeMissing::HasAttr => {
                self.registers[destination] = Value::bool(false);
                Ok(())
            }
        }
    }

    fn invoke_setter_call(
        &mut self,
        p: &Program,
        call: crate::classes::DescriptorCall,
        destination: usize,
        positional: [Value; 3],
        count: usize,
        output: &mut dyn Write,
    ) -> Result<()> {
        let depth = self.frames.len();
        self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Inline {
                receiver: call.receiver,
                positional,
                count,
            },
            output,
        )?;
        if self.frames.len() > depth {
            self.frames
                .last_mut()
                .expect("attribute mutation frame")
                .action = super::ReturnAction::Setter;
        } else {
            self.registers[destination] = Value::NONE;
        }
        Ok(())
    }

    fn invoke_default_attribute_set(
        &mut self,
        p: &Program,
        owner: Value,
        name: &str,
        value: Value,
        destination: usize,
        output: &mut dyn Write,
    ) -> Result<()> {
        if matches!(self.heap.get(owner), Ok(Object::Class(_))) {
            if let Some(setter) = self.heap.metaclass_property_setter(owner, name)? {
                return self.invoke_setter_call(
                    p,
                    crate::classes::DescriptorCall {
                        callable: setter,
                        receiver: Some(owner),
                    },
                    destination,
                    [value, Value::UNBOUND, Value::UNBOUND],
                    1,
                    output,
                );
            }
            if let Some(setter) = self.heap.metaclass_descriptor_setter(owner, name)? {
                return self.invoke_setter_call(
                    p,
                    setter,
                    destination,
                    [owner, value, Value::UNBOUND],
                    2,
                    output,
                );
            }
            self.heap.set_attr(owner, name, value)?;
            self.registers[destination] = Value::NONE;
            return Ok(());
        }
        if let Some(setter) = self.heap.property_setter(owner, name)? {
            return self.invoke_setter_call(
                p,
                crate::classes::DescriptorCall {
                    callable: setter,
                    receiver: Some(owner),
                },
                destination,
                [value, Value::UNBOUND, Value::UNBOUND],
                1,
                output,
            );
        }
        if let Some(setter) = self.heap.descriptor_setter(owner, name)? {
            return self.invoke_setter_call(
                p,
                setter,
                destination,
                [owner, value, Value::UNBOUND],
                2,
                output,
            );
        }
        self.heap.set_attr(owner, name, value)?;
        self.registers[destination] = Value::NONE;
        Ok(())
    }

    pub(super) fn invoke_attribute_set(
        &mut self,
        p: &Program,
        owner: Value,
        name: &str,
        value: Value,
        destination: usize,
        output: &mut dyn Write,
    ) -> Result<()> {
        let default = if matches!(self.heap.get(owner), Ok(Object::Class(_))) {
            Builtin::TypeSetAttr
        } else {
            Builtin::ObjectSetAttr
        };
        if let Some(call) = self
            .heap
            .custom_attribute_method(owner, "__setattr__", default)?
        {
            let name = self.heap.alloc(Object::Str(name.to_owned()))?;
            return self.invoke_setter_call(
                p,
                call,
                destination,
                [name, value, Value::UNBOUND],
                2,
                output,
            );
        }
        self.invoke_default_attribute_set(p, owner, name, value, destination, output)
    }

    fn invoke_default_attribute_delete(
        &mut self,
        p: &Program,
        owner: Value,
        name: &str,
        destination: usize,
        output: &mut dyn Write,
    ) -> Result<()> {
        if matches!(self.heap.get(owner), Ok(Object::Class(_))) {
            if let Some(deleter) = self.heap.metaclass_property_deleter(owner, name)? {
                return self.invoke_setter_call(
                    p,
                    crate::classes::DescriptorCall {
                        callable: deleter,
                        receiver: Some(owner),
                    },
                    destination,
                    [Value::UNBOUND; 3],
                    0,
                    output,
                );
            }
            if let Some(deleter) = self.heap.metaclass_descriptor_deleter(owner, name)? {
                return self.invoke_setter_call(
                    p,
                    deleter,
                    destination,
                    [owner, Value::UNBOUND, Value::UNBOUND],
                    1,
                    output,
                );
            }
            self.heap.del_attr(owner, name)?;
            self.registers[destination] = Value::NONE;
            return Ok(());
        }
        if let Some(deleter) = self.heap.property_deleter(owner, name)? {
            return self.invoke_setter_call(
                p,
                crate::classes::DescriptorCall {
                    callable: deleter,
                    receiver: Some(owner),
                },
                destination,
                [Value::UNBOUND; 3],
                0,
                output,
            );
        }
        if let Some(deleter) = self.heap.descriptor_deleter(owner, name)? {
            return self.invoke_setter_call(
                p,
                deleter,
                destination,
                [owner, Value::UNBOUND, Value::UNBOUND],
                1,
                output,
            );
        }
        self.heap.del_attr(owner, name)?;
        self.registers[destination] = Value::NONE;
        Ok(())
    }

    pub(super) fn invoke_attribute_delete(
        &mut self,
        p: &Program,
        owner: Value,
        name: &str,
        destination: usize,
        output: &mut dyn Write,
    ) -> Result<()> {
        let default = if matches!(self.heap.get(owner), Ok(Object::Class(_))) {
            Builtin::TypeDelAttr
        } else {
            Builtin::ObjectDelAttr
        };
        if let Some(call) = self
            .heap
            .custom_attribute_method(owner, "__delattr__", default)?
        {
            let name = self.heap.alloc(Object::Str(name.to_owned()))?;
            return self.invoke_setter_call(
                p,
                call,
                destination,
                [name, Value::UNBOUND, Value::UNBOUND],
                1,
                output,
            );
        }
        self.invoke_default_attribute_delete(p, owner, name, destination, output)
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
            self.apply_truth(p, destination, truth, action, output)?;
            return Ok(());
        };
        let depth = self.frames.len();
        self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Inline {
                receiver: call.receiver,
                positional: [Value::UNBOUND; 3],
                count: 0,
            },
            output,
        )?;
        if self.frames.len() > depth {
            self.frames.last_mut().expect("truth protocol frame").action =
                super::ReturnAction::Truth { protocol, action };
        } else {
            self.finish_truth(
                p,
                destination,
                self.registers[destination],
                protocol,
                action,
                output,
            )?;
        }
        Ok(())
    }
    pub(super) fn finish_truth(
        &mut self,
        p: &Program,
        destination: usize,
        value: Value,
        protocol: super::TruthProtocol,
        action: super::TruthAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        let truth = match protocol {
            super::TruthProtocol::Bool => value
                .as_bool()
                .ok_or_else(|| Diagnostic::new("TypeError", "__bool__ should return bool"))?,
            super::TruthProtocol::Length if !self.heap.is_integer(value) => {
                return self.invoke_index_conversion(
                    p,
                    value,
                    destination,
                    super::IndexContinuation::Truth(action),
                    output,
                )
            }
            super::TruthProtocol::Length => self.checked_length(value)? != 0,
        };
        self.apply_truth(p, destination, truth, action, output)
    }
    fn apply_truth(
        &mut self,
        p: &Program,
        destination: usize,
        truth: bool,
        action: super::TruthAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        match action {
            super::TruthAction::Not => self.registers[destination] = Value::bool(!truth),
            super::TruthAction::Return => self.registers[destination] = Value::bool(truth),
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
            super::TruthAction::Equality(action) => {
                return self.complete_equality(p, destination, truth, action, output)
            }
        }
        Ok(())
    }
    fn operator_method_call(
        &self,
        value: Value,
        name: &str,
    ) -> Result<Option<crate::classes::DescriptorCall>> {
        if matches!(self.heap.get(value), Ok(Object::Class(_))) {
            self.heap.metaclass_method_call(value, name)
        } else {
            self.heap.special_method_call(value, name)
        }
    }
    fn operator_protocol_capable(&self, value: Value) -> bool {
        self.heap.get(value).is_ok_and(|object| {
            object.instance_class().is_some() || matches!(object, Object::Class(_))
        })
    }
    pub(super) fn binary_protocol(
        &self,
        op: tonic_core::bytecode::Op,
        left: Value,
        right: Value,
    ) -> Result<Option<super::BinaryProtocol>> {
        use tonic_core::bytecode::Op;
        if !self.operator_protocol_capable(left) && !self.operator_protocol_capable(right) {
            return Ok(None);
        }
        let (inplace, direct, reflected) = match op {
            Op::Add => (None, "__add__", "__radd__"),
            Op::InplaceAdd => (Some("__iadd__"), "__add__", "__radd__"),
            Op::Sub => (None, "__sub__", "__rsub__"),
            Op::InplaceSub => (Some("__isub__"), "__sub__", "__rsub__"),
            Op::Mul => (None, "__mul__", "__rmul__"),
            Op::InplaceMul => (Some("__imul__"), "__mul__", "__rmul__"),
            Op::Div => (None, "__truediv__", "__rtruediv__"),
            Op::InplaceDiv => (Some("__itruediv__"), "__truediv__", "__rtruediv__"),
            Op::FloorDiv => (None, "__floordiv__", "__rfloordiv__"),
            Op::InplaceFloorDiv => (Some("__ifloordiv__"), "__floordiv__", "__rfloordiv__"),
            Op::Mod => (None, "__mod__", "__rmod__"),
            Op::InplaceMod => (Some("__imod__"), "__mod__", "__rmod__"),
            Op::Pow => (None, "__pow__", "__rpow__"),
            Op::InplacePow => (Some("__ipow__"), "__pow__", "__rpow__"),
            Op::BitOr => (None, "__or__", "__ror__"),
            Op::InplaceBitOr => (Some("__ior__"), "__or__", "__ror__"),
            Op::BitXor => (None, "__xor__", "__rxor__"),
            Op::InplaceBitXor => (Some("__ixor__"), "__xor__", "__rxor__"),
            Op::BitAnd => (None, "__and__", "__rand__"),
            Op::InplaceBitAnd => (Some("__iand__"), "__and__", "__rand__"),
            Op::LeftShift => (None, "__lshift__", "__rlshift__"),
            Op::InplaceLeftShift => (Some("__ilshift__"), "__lshift__", "__rlshift__"),
            Op::RightShift => (None, "__rshift__", "__rrshift__"),
            Op::InplaceRightShift => (Some("__irshift__"), "__rshift__", "__rrshift__"),
            Op::Eq => (None, "__eq__", "__eq__"),
            Op::Ne => (None, "__ne__", "__ne__"),
            Op::Lt => (None, "__lt__", "__gt__"),
            Op::Le => (None, "__le__", "__ge__"),
            Op::Gt => (None, "__gt__", "__lt__"),
            Op::Ge => (None, "__ge__", "__le__"),
            _ => return Ok(None),
        };
        let mut candidates = Vec::with_capacity(3);
        if let Some(inplace) = inplace {
            if let Some(call) = self.operator_method_call(left, inplace)? {
                candidates.push(super::BinaryCandidate {
                    call,
                    argument: right,
                    negate: false,
                });
            }
        }
        let left_call = self.operator_method_call(left, direct)?;
        let left_class = self.runtime_class(left)?;
        let right_class = self.runtime_class(right)?;
        let right_call = if left_class == right_class {
            None
        } else {
            self.operator_method_call(right, reflected)?
        };
        let right_is_subclass = left_class != right_class
            && self
                .heap
                .class(right_class)
                .is_ok_and(|class| class.mro.contains(&left_class));
        let mut push = |call: Option<crate::classes::DescriptorCall>, argument, negate| {
            if let Some(call) = call {
                candidates.push(super::BinaryCandidate {
                    call,
                    argument,
                    negate,
                });
            }
        };
        if right_is_subclass {
            push(right_call, left, false);
            push(left_call, right, false);
        } else {
            push(left_call, right, false);
            push(right_call, left, false);
        }
        if op == Op::Ne {
            let left_eq = self.operator_method_call(left, "__eq__")?;
            let right_eq = if left_class == right_class {
                None
            } else {
                self.operator_method_call(right, "__eq__")?
            };
            if right_is_subclass {
                push(right_eq, left, true);
                push(left_eq, right, true);
            } else {
                push(left_eq, right, true);
                push(right_eq, left, true);
            }
        }
        Ok((!candidates.is_empty()).then_some(super::BinaryProtocol {
            op,
            left,
            right,
            candidates,
            next: 0,
            negate_result: false,
            completion: super::BinaryCompletion::Value,
        }))
    }
    pub(super) fn continue_binary_protocol(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: super::BinaryProtocol,
        output: &mut dyn Write,
    ) -> Result<()> {
        while let Some(candidate) = state.candidates.get(state.next).copied() {
            state.next += 1;
            state.negate_result = candidate.negate;
            let depth = self.frames.len();
            self.invoke_target(
                p,
                candidate.call.callable,
                destination,
                Arguments::Inline {
                    receiver: candidate.call.receiver,
                    positional: [candidate.argument, Value::UNBOUND, Value::UNBOUND],
                    count: 1,
                },
                output,
            )?;
            if self.frames.len() > depth {
                self.frames
                    .last_mut()
                    .expect("binary protocol frame")
                    .action = super::ReturnAction::BinaryProtocol(state);
                return Ok(());
            }
            if self.registers[destination] != Value::NOT_IMPLEMENTED {
                let value = self.registers[destination];
                return self.finish_binary_protocol_value(
                    p,
                    destination,
                    value,
                    state.negate_result,
                    state.completion,
                    output,
                );
            }
        }
        if let super::BinaryCompletion::Equality(action) = state.completion {
            return self.invoke_equality_fallback(
                p,
                state.left,
                state.right,
                destination,
                action,
                output,
            );
        }
        self.registers[destination] = match state.op {
            tonic_core::bytecode::Op::InplaceAdd => {
                self.heap.inplace_add(state.left, state.right)?
            }
            tonic_core::bytecode::Op::Eq
            | tonic_core::bytecode::Op::Ne
            | tonic_core::bytecode::Op::Lt
            | tonic_core::bytecode::Op::Le
            | tonic_core::bytecode::Op::Gt
            | tonic_core::bytecode::Op::Ge => {
                self.heap.compare(state.op, state.left, state.right)?
            }
            _ => self
                .heap
                .binary(super::base_binary_op(state.op), state.left, state.right)?,
        };
        Ok(())
    }
    pub(super) fn finish_binary_protocol_value(
        &mut self,
        p: &Program,
        destination: usize,
        value: Value,
        negate: bool,
        completion: super::BinaryCompletion,
        output: &mut dyn Write,
    ) -> Result<()> {
        match completion {
            super::BinaryCompletion::Value if negate => {
                self.invoke_truth(p, value, destination, super::TruthAction::Not, output)
            }
            super::BinaryCompletion::Value => Ok(()),
            super::BinaryCompletion::Equality(action) => {
                debug_assert!(!negate, "equality continuation uses the Eq protocol");
                self.invoke_truth(
                    p,
                    value,
                    destination,
                    super::TruthAction::Equality(action),
                    output,
                )
            }
        }
    }
    pub(super) fn invoke_unary_protocol(
        &mut self,
        p: &Program,
        value: Value,
        destination: usize,
        kind: super::UnaryProtocolKind,
        output: &mut dyn Write,
    ) -> Result<()> {
        let name = match kind {
            super::UnaryProtocolKind::Neg => "__neg__",
            super::UnaryProtocolKind::Pos => "__pos__",
            super::UnaryProtocolKind::Abs => "__abs__",
            super::UnaryProtocolKind::Invert => "__invert__",
        };
        if !self.operator_protocol_capable(value) {
            self.registers[destination] = self.unary_fallback(kind, value)?;
            return Ok(());
        }
        let Some(call) = self.operator_method_call(value, name)? else {
            self.registers[destination] = self.unary_fallback(kind, value)?;
            return Ok(());
        };
        let depth = self.frames.len();
        self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Inline {
                receiver: call.receiver,
                positional: [Value::UNBOUND; 3],
                count: 0,
            },
            output,
        )?;
        if self.frames.len() > depth {
            self.frames.last_mut().expect("unary protocol frame").action =
                super::ReturnAction::UnaryProtocol(super::UnaryProtocol { kind, value });
        } else if self.registers[destination] == Value::NOT_IMPLEMENTED {
            self.registers[destination] = self.unary_fallback(kind, value)?;
        }
        Ok(())
    }
    pub(super) fn unary_fallback(
        &mut self,
        kind: super::UnaryProtocolKind,
        value: Value,
    ) -> Result<Value> {
        match kind {
            super::UnaryProtocolKind::Neg => self.heap.unary(tonic_core::bytecode::Op::Neg, value),
            super::UnaryProtocolKind::Pos => self.heap.unary(tonic_core::bytecode::Op::Pos, value),
            super::UnaryProtocolKind::Abs => {
                if self.heap.is_float(value) {
                    let n = self.heap.float(value)?.abs();
                    self.heap.alloc(Object::Float(n))
                } else {
                    let zero = Value::int(0).expect("immediate");
                    let negative = self
                        .heap
                        .compare(tonic_core::bytecode::Op::Lt, value, zero)?
                        .as_bool()
                        .expect("numeric comparison returns bool");
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
            super::UnaryProtocolKind::Invert => {
                self.heap.unary(tonic_core::bytecode::Op::Invert, value)
            }
        }
    }
    fn checked_length(&self, value: Value) -> Result<i64> {
        let length = if let Some(value) = value.as_bool() {
            i64::from(value)
        } else if self.heap.is_integer(value) {
            self.heap.to_i64(value)?
        } else {
            return Err(Diagnostic::new(
                "TypeError",
                "__len__ returned a non-integer value",
            ));
        };
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
    pub(super) fn finish_length(
        &mut self,
        p: &Program,
        destination: usize,
        value: Value,
        output: &mut dyn Write,
    ) -> Result<()> {
        if self.heap.is_integer(value) || value.as_bool().is_some() {
            self.registers[destination] = self.validate_length(value)?;
            return Ok(());
        }
        self.invoke_index_conversion(
            p,
            value,
            destination,
            super::IndexContinuation::Length,
            output,
        )
    }
    pub(super) fn invoke_index_conversion(
        &mut self,
        p: &Program,
        source: Value,
        destination: usize,
        continuation: super::IndexContinuation,
        output: &mut dyn Write,
    ) -> Result<()> {
        if source.as_bool().is_some() || self.heap.is_integer(source) {
            let value = self.normalize_index_result(source)?;
            return self.finish_index_conversion(
                p,
                destination,
                value,
                super::IndexConversion { continuation },
                output,
            );
        }
        let call = self
            .heap
            .special_method_call(source, "__index__")?
            .ok_or_else(|| {
                Diagnostic::new("TypeError", "object cannot be interpreted as an integer")
            })?;
        let state = super::IndexConversion { continuation };
        let depth = self.frames.len();
        self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Inline {
                receiver: call.receiver,
                positional: [Value::UNBOUND; 3],
                count: 0,
            },
            output,
        )?;
        if self.frames.len() > depth {
            self.frames
                .last_mut()
                .expect("index conversion frame")
                .action = super::ReturnAction::IndexConversion(state);
            Ok(())
        } else {
            let value = self.normalize_index_result(self.registers[destination])?;
            self.finish_index_conversion(p, destination, value, state, output)
        }
    }
    fn normalize_index_result(&self, value: Value) -> Result<Value> {
        if let Some(value) = value.as_bool() {
            return Ok(Value::int(i64::from(value)).expect("bool is an immediate integer"));
        }
        if self.heap.is_integer(value) {
            return Ok(self.heap.native_value(value));
        }
        Err(Diagnostic::new(
            "TypeError",
            "__index__ returned a non-integer value",
        ))
    }
    pub(super) fn finish_index_conversion(
        &mut self,
        p: &Program,
        destination: usize,
        value: Value,
        state: super::IndexConversion,
        output: &mut dyn Write,
    ) -> Result<()> {
        let value = self.normalize_index_result(value)?;
        match state.continuation {
            super::IndexContinuation::Length => {
                self.registers[destination] = self.validate_length(value)?;
                Ok(())
            }
            super::IndexContinuation::Truth(action) => {
                let truth = self.checked_length(value)? != 0;
                self.apply_truth(p, destination, truth, action, output)
            }
            super::IndexContinuation::Range(mut state) => {
                state.values[state.next] = value;
                state.next += 1;
                self.continue_range_construction(p, destination, state, output)
            }
            super::IndexContinuation::IntBase(state) => {
                self.finish_int_base(p, destination, state, value, output)
            }
            super::IndexContinuation::GetItem { owner } => {
                self.registers[destination] = self.heap.item(owner, value)?;
                Ok(())
            }
            super::IndexContinuation::SetItem { owner, value: item } => {
                self.heap.set_item(owner, value, item)?;
                self.registers[destination] = Value::NONE;
                Ok(())
            }
            super::IndexContinuation::DelItem { owner } => {
                self.heap.delete_item(owner, value)?;
                self.registers[destination] = Value::NONE;
                Ok(())
            }
            super::IndexContinuation::Slice(mut state) => {
                state.components[state.next] = value;
                state.next += 1;
                self.continue_slice_conversion(p, destination, state, output)
            }
        }
    }
    pub(super) fn continue_range_construction(
        &mut self,
        p: &Program,
        destination: usize,
        state: super::RangeConstruction,
        output: &mut dyn Write,
    ) -> Result<()> {
        if state.next < state.count {
            let source = state.values[state.next];
            return self.invoke_index_conversion(
                p,
                source,
                destination,
                super::IndexContinuation::Range(state),
                output,
            );
        }
        let first = self.heap.to_i64(state.values[0])?;
        let (start, stop) = if state.count == 1 {
            (0, first)
        } else {
            (first, self.heap.to_i64(state.values[1])?)
        };
        let step = if state.count == 3 {
            self.heap.to_i64(state.values[2])?
        } else {
            1
        };
        if step == 0 {
            return Err(Diagnostic::new("ValueError", "range step must not be zero"));
        }
        self.registers[destination] = self.heap.alloc(Object::Range { start, stop, step })?;
        Ok(())
    }
    pub(super) fn continue_slice_conversion(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: super::SliceConversion,
        output: &mut dyn Write,
    ) -> Result<()> {
        while state.next < state.components.len() {
            if state.components[state.next] == Value::NONE {
                state.next += 1;
                continue;
            }
            let source = state.components[state.next];
            return self.invoke_index_conversion(
                p,
                source,
                destination,
                super::IndexContinuation::Slice(state),
                output,
            );
        }
        let slice = self.heap.alloc(Object::Slice(state.components))?;
        self.registers[destination] = self.heap.item(state.owner, slice)?;
        Ok(())
    }
    fn finish_int_base(
        &mut self,
        p: &Program,
        destination: usize,
        state: super::IntBaseConversion,
        base: Value,
        output: &mut dyn Write,
    ) -> Result<()> {
        let base = self
            .heap
            .integer(base)?
            .to_i64()
            .ok_or_else(invalid_int_base)
            .and_then(validate_int_base)?;
        let argument = self.heap.native_value(state.argument);
        let Object::Str(value) = self.heap.get(argument)? else {
            return Err(Diagnostic::new(
                "TypeError",
                "int with explicit base requires a string",
            ));
        };
        self.registers[destination] = self.heap.int(
            parse_int_literal(value, base)
                .ok_or_else(|| Diagnostic::new("ValueError", "invalid literal for int"))?,
        )?;
        self.finish_builtin_value(p, destination, state.finish, output)
    }
    fn runtime_type_for_kind(&self, kind: super::RuntimeTypeKind) -> Value {
        match kind {
            super::RuntimeTypeKind::Int => self.runtime_types.int,
            super::RuntimeTypeKind::Bool => self.runtime_types.bool_,
            super::RuntimeTypeKind::Float => self.runtime_types.float,
            super::RuntimeTypeKind::Str => self.runtime_types.str_,
            super::RuntimeTypeKind::List => self.runtime_types.list,
            super::RuntimeTypeKind::Tuple => self.runtime_types.tuple,
            super::RuntimeTypeKind::Dict => self.runtime_types.dict,
            super::RuntimeTypeKind::Range => self.runtime_types.range,
            _ => unreachable!("kind has no native subclass storage"),
        }
    }
    fn validate_native_new_class(
        &self,
        class: Value,
        kind: super::RuntimeTypeKind,
    ) -> Result<bool> {
        self.heap.class(class)?;
        let exact = self.runtime_type_for_kind(kind);
        if class == exact {
            return Ok(true);
        }
        if self.builtin_type_kind(class).is_some()
            || self.builtin_subclass_kind(class) != Some(kind)
        {
            return Err(Diagnostic::new(
                "TypeError",
                "builtin __new__ received an incompatible class",
            ));
        }
        Ok(false)
    }
    fn invoke_native_new(
        &mut self,
        p: &Program,
        kind: super::RuntimeTypeKind,
        destination: usize,
        args: &Arguments<'_>,
        output: &mut dyn Write,
    ) -> Result<()> {
        if args.count() == 0 {
            return Err(Diagnostic::new(
                "TypeError",
                "builtin __new__ expects a class argument",
            ));
        }
        let class = args.positional(&self.registers, 0);
        let exact = self.validate_native_new_class(class, kind)?;
        let mut arguments = args.owned(p, &self.registers);
        if arguments.receiver.is_some() || arguments.positional.is_empty() {
            return Err(Diagnostic::new("TypeError", "invalid builtin __new__ call"));
        }
        arguments.positional.remove(0);
        if matches!(
            kind,
            super::RuntimeTypeKind::List | super::RuntimeTypeKind::Dict
        ) {
            let native = self.heap.alloc(if kind == super::RuntimeTypeKind::List {
                Object::List(Vec::new())
            } else {
                Object::Dict(Dict::default())
            })?;
            self.registers[destination] = if exact {
                native
            } else {
                self.heap.native_instance(class, native)?
            };
            return Ok(());
        }
        let finish = (!exact).then_some(NativeSubclassFinish {
            class,
            initialize: None,
        });
        self.invoke_builtin_type(
            p,
            (self.runtime_type_for_kind(kind), kind),
            destination,
            Arguments::Expanded(arguments),
            finish,
            output,
        )
    }
    fn invoke_native_initializer(
        &mut self,
        p: &Program,
        builtin: Builtin,
        destination: usize,
        args: &Arguments<'_>,
        result: Value,
        output: &mut dyn Write,
    ) -> Result<()> {
        if args.count() == 0 {
            return Err(Diagnostic::new(
                "TypeError",
                "builtin __init__ expects an instance",
            ));
        }
        let owner = args.positional(&self.registers, 0);
        let native = self.heap.native_value(owner);
        match builtin {
            Builtin::ListInit => {
                if args.keyword_count() != 0 || args.count() > 2 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "list.__init__ expects at most one argument",
                    ));
                }
                if !matches!(self.heap.get(native)?, Object::List(_)) {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "list.__init__ requires a list instance",
                    ));
                }
                self.heap.replace_list(owner, Vec::new())?;
                if args.count() == 1 {
                    self.registers[destination] = result;
                    return Ok(());
                }
                self.invoke_iterable_collection(
                    p,
                    destination,
                    IterableCollectionKind::ListInit { owner, result },
                    args.positional(&self.registers, 1),
                    output,
                )
            }
            Builtin::DictInit => {
                if args.count() > 2 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "dict.__init__ expects at most one positional argument",
                    ));
                }
                if !matches!(self.heap.get(native)?, Object::Dict(_)) {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "dict.__init__ requires a dict instance",
                    ));
                }
                let source = (args.count() == 2).then(|| args.positional(&self.registers, 1));
                let self_source =
                    source.is_some_and(|source| self.heap.native_value(source) == native);
                if !self_source {
                    self.heap.dict_clear(owner)?;
                }
                let mut keywords = Vec::with_capacity(args.keyword_count());
                for index in 0..args.keyword_count() {
                    let (name, value) = args.keyword(p, &self.registers, index);
                    let key = self.heap.alloc(Object::Str(name.to_owned()))?;
                    keywords.push((key, value));
                }
                let start = DictConstructionStart {
                    result: native,
                    keywords,
                    native: None,
                    return_value: Some(result),
                };
                if let Some(source) = source {
                    if matches!(
                        self.heap.get(self.heap.native_value(source))?,
                        Object::Dict(_)
                    ) {
                        if !self_source {
                            return self.invoke_dict_merge(
                                p,
                                destination,
                                native,
                                source,
                                super::DictMergeFinish::Construction(start),
                                output,
                            );
                        }
                    } else {
                        return self.invoke_dict_construction(
                            p,
                            destination,
                            start,
                            source,
                            output,
                        );
                    }
                }
                self.finish_dict_construction(p, destination, start, output)
            }
            _ => unreachable!("not a native initializer"),
        }
    }
    fn has_canonical_native_initializer(
        &self,
        class: Value,
        kind: super::RuntimeTypeKind,
    ) -> Result<bool> {
        let Some(initializer) = self.heap.class_lookup(class, "__init__")? else {
            return Ok(false);
        };
        let expected = match kind {
            super::RuntimeTypeKind::List => Builtin::ListInit,
            super::RuntimeTypeKind::Dict => Builtin::DictInit,
            _ => Builtin::ObjectInit,
        };
        Ok(matches!(self.heap.get(initializer), Ok(Object::Builtin(actual)) if *actual == expected))
    }
    fn invoke_numeric_conversion(
        &mut self,
        p: &Program,
        source: Value,
        destination: usize,
        target: super::RuntimeTypeKind,
        finish: Option<NativeSubclassFinish>,
        output: &mut dyn Write,
    ) -> Result<()> {
        let (call, kind) = match target {
            super::RuntimeTypeKind::Int => {
                if let Some(call) = self.heap.special_method_call(source, "__int__")? {
                    (call, super::NumericConversionKind::Int)
                } else if let Some(call) = self.heap.special_method_call(source, "__index__")? {
                    (call, super::NumericConversionKind::IndexToInt)
                } else if self.heap.is_integer(source) {
                    self.registers[destination] = self.heap.native_value(source);
                    return self.finish_builtin_value(p, destination, finish, output);
                } else {
                    return Err(Diagnostic::new("TypeError", "cannot convert value to int"));
                }
            }
            super::RuntimeTypeKind::Float => {
                if let Some(call) = self.heap.special_method_call(source, "__float__")? {
                    (call, super::NumericConversionKind::Float)
                } else if let Some(call) = self.heap.special_method_call(source, "__index__")? {
                    (call, super::NumericConversionKind::IndexToFloat)
                } else if self.heap.is_float(source) {
                    self.registers[destination] = self.heap.native_value(source);
                    return self.finish_builtin_value(p, destination, finish, output);
                } else {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "cannot convert value to float",
                    ));
                }
            }
            _ => unreachable!("not a numeric conversion target"),
        };
        let state = super::NumericConversion { kind, finish };
        let depth = self.frames.len();
        self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Inline {
                receiver: call.receiver,
                positional: [Value::UNBOUND; 3],
                count: 0,
            },
            output,
        )?;
        if self.frames.len() > depth {
            self.frames
                .last_mut()
                .expect("numeric conversion frame")
                .action = super::ReturnAction::NumericConversion(state);
            Ok(())
        } else {
            let value = self.registers[destination];
            self.finish_numeric_conversion(p, destination, value, state, output)
        }
    }
    pub(super) fn finish_numeric_conversion(
        &mut self,
        p: &Program,
        destination: usize,
        value: Value,
        state: super::NumericConversion,
        output: &mut dyn Write,
    ) -> Result<()> {
        let converted = match state.kind {
            super::NumericConversionKind::Int | super::NumericConversionKind::IndexToInt => {
                if let Some(value) = value.as_bool() {
                    Value::int(i64::from(value)).expect("bool always fits an immediate integer")
                } else if self.heap.is_integer(value) {
                    self.heap.native_value(value)
                } else {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "integer conversion method must return int",
                    ));
                }
            }
            super::NumericConversionKind::Float => {
                if !self.heap.is_float(value) {
                    return Err(Diagnostic::new("TypeError", "__float__ must return float"));
                }
                self.heap.native_value(value)
            }
            super::NumericConversionKind::IndexToFloat => {
                let value = if let Some(value) = value.as_bool() {
                    Value::int(i64::from(value)).expect("bool always fits an immediate integer")
                } else if self.heap.is_integer(value) {
                    self.heap.native_value(value)
                } else {
                    return Err(Diagnostic::new("TypeError", "__index__ must return int"));
                };
                let value = self.heap.float(value)?;
                self.heap.alloc(Object::Float(value))?
            }
        };
        self.registers[destination] = converted;
        self.finish_builtin_value(p, destination, state.finish, output)
    }
    fn invoke_class(
        &mut self,
        p: &Program,
        class: Value,
        destination: usize,
        args: Arguments<'_>,
        output: &mut dyn Write,
    ) -> Result<()> {
        if class == self.type_class {
            if args.keyword_count() == 0 && args.count() == 3 {
                let name = args.positional(&self.registers, 0);
                let bases = args.positional(&self.registers, 1);
                let namespace = args.positional(&self.registers, 2);
                return self.invoke_dynamic_type(p, destination, name, bases, namespace, output);
            }
            if args.keyword_count() != 0 || args.count() != 1 {
                return Err(Diagnostic::new(
                    "TypeError",
                    "type expects one or three positional arguments",
                ));
            }
            let value = args.positional(&self.registers, 0);
            self.registers[destination] = self.runtime_class(value)?;
            return Ok(());
        }
        if let Some(kind) = self.builtin_type_kind(class) {
            return self.invoke_builtin_type(p, (class, kind), destination, args, None, output);
        }
        if self.instance_check(class, self.runtime_types.base_exception, true, 0)? {
            let mut arguments = args.owned(p, &self.registers);
            if arguments.receiver.take().is_some() {
                return Err(Diagnostic::new(
                    "BytecodeError",
                    "bound exception class call",
                ));
            }
            let message = self.heap.exception_message(&arguments.positional)?;
            let instance = self.heap.alloc(Object::Exception {
                class,
                message,
                arguments: arguments.positional.clone(),
                attributes: Default::default(),
                cause: None,
                context: None,
                suppress_context: false,
                traceback: None,
            })?;
            let initializer = self.heap.class_lookup(class, "__init__")?;
            if initializer.is_none_or(|initializer| {
                matches!(
                    self.heap.get(initializer),
                    Ok(Object::Builtin(Builtin::ObjectInit))
                )
            }) {
                if !arguments.keywords.is_empty() {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "exception classes do not accept keyword arguments without __init__",
                    ));
                }
                self.registers[destination] = instance;
                return Ok(());
            }
            return self.finish_new(p, destination, class, instance, arguments, output);
        }
        if self.instance_check(class, self.type_class, true, 0)? {
            return Err(Diagnostic::new(
                "UnsupportedFeature",
                "direct custom metaclass calls require metaclass __new__/__init__ support",
            ));
        }
        let mut arguments = args.owned(p, &self.registers);
        if arguments.receiver.take().is_some() {
            return Err(Diagnostic::new("BytecodeError", "bound class call"));
        }
        if let Some(kind) = self.builtin_subclass_kind(class) {
            let constructor = self.heap.class_lookup(class, "__new__")?;
            if let Some(constructor_kind) =
                constructor.and_then(|constructor| match self.heap.get(constructor) {
                    Ok(Object::Builtin(builtin)) => native_new_kind(*builtin),
                    _ => None,
                })
            {
                if constructor_kind != kind {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "native subclass inherited an incompatible __new__",
                    ));
                }
                let initialize = (!self.has_canonical_native_initializer(class, kind)?)
                    .then_some(arguments.clone());
                if matches!(
                    kind,
                    super::RuntimeTypeKind::List | super::RuntimeTypeKind::Dict
                ) && initialize.is_some()
                {
                    let native = self.heap.alloc(if kind == super::RuntimeTypeKind::List {
                        Object::List(Vec::new())
                    } else {
                        Object::Dict(Dict::default())
                    })?;
                    let instance = self.heap.native_instance(class, native)?;
                    return self.finish_new(p, destination, class, instance, arguments, output);
                }
                let finish = NativeSubclassFinish { class, initialize };
                return self.invoke_builtin_type(
                    p,
                    (class, kind),
                    destination,
                    Arguments::Expanded(arguments),
                    Some(finish),
                    output,
                );
            }
            if constructor.is_some_and(|constructor| {
                matches!(
                    self.heap.get(constructor),
                    Ok(Object::Builtin(Builtin::ObjectNew))
                )
            }) {
                let finish = NativeSubclassFinish {
                    class,
                    initialize: Some(arguments.clone()),
                };
                return self.invoke_builtin_type(
                    p,
                    (class, kind),
                    destination,
                    Arguments::Expanded(arguments),
                    Some(finish),
                    output,
                );
            }
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
    fn invoke_builtin_type(
        &mut self,
        p: &Program,
        target: (Value, super::RuntimeTypeKind),
        destination: usize,
        args: Arguments<'_>,
        finish: Option<NativeSubclassFinish>,
        output: &mut dyn Write,
    ) -> Result<()> {
        let (class, kind) = target;
        if kind == super::RuntimeTypeKind::Range {
            if args.keyword_count() != 0 || !(1..=3).contains(&args.count()) {
                return Err(Diagnostic::new(
                    "TypeError",
                    "range expects 1 to 3 positional arguments",
                ));
            }
            let mut values = [Value::NONE; 3];
            for (index, slot) in values.iter_mut().take(args.count()).enumerate() {
                *slot = args.positional(&self.registers, index);
            }
            return self.continue_range_construction(
                p,
                destination,
                super::RangeConstruction {
                    values,
                    count: args.count(),
                    next: 0,
                },
                output,
            );
        }
        if kind == super::RuntimeTypeKind::Int {
            if args.count() > 2 || args.keyword_count() > 1 {
                return Err(Diagnostic::new(
                    "TypeError",
                    "int expects at most two arguments",
                ));
            }
            let mut base = (args.count() == 2).then(|| args.positional(&self.registers, 1));
            if args.keyword_count() == 1 {
                let (name, value) = args.keyword(p, &self.registers, 0);
                if name != "base" {
                    return Err(Diagnostic::new(
                        "TypeError",
                        format!("int got unexpected keyword '{name}'"),
                    ));
                }
                if base.is_some() {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "int got multiple values for base",
                    ));
                }
                base = Some(value);
            }
            let argument = (args.count() >= 1).then(|| args.positional(&self.registers, 0));
            if let Some(base) = base {
                let argument = argument.ok_or_else(|| {
                    Diagnostic::new(
                        "TypeError",
                        "int missing string argument when base is given",
                    )
                })?;
                return self.invoke_index_conversion(
                    p,
                    base,
                    destination,
                    super::IndexContinuation::IntBase(super::IntBaseConversion {
                        argument,
                        finish,
                    }),
                    output,
                );
            }
            if let Some(value) = argument {
                if self.heap.special_method_call(value, "__int__")?.is_some()
                    || self.heap.special_method_call(value, "__index__")?.is_some()
                {
                    return self.invoke_numeric_conversion(
                        p,
                        value,
                        destination,
                        super::RuntimeTypeKind::Int,
                        finish,
                        output,
                    );
                }
            }
            self.registers[destination] = match argument {
                None => Value::int(0).expect("zero is immediate"),
                Some(value) if value.as_bool().is_some() => {
                    Value::int(i64::from(value.as_bool().expect("checked bool")))
                        .expect("bool integer is immediate")
                }
                Some(value) if self.heap.is_integer(value) => self.heap.native_value(value),
                Some(value) => match self.heap.get(self.heap.native_value(value))? {
                    Object::Float(value) => {
                        self.heap
                            .int(BigInt::from_f64(value.trunc()).ok_or_else(|| {
                                Diagnostic::new("OverflowError", "cannot convert float to integer")
                            })?)?
                    }
                    Object::Str(value) => {
                        self.heap.int(parse_int_literal(value, 10).ok_or_else(|| {
                            Diagnostic::new("ValueError", "invalid literal for int")
                        })?)?
                    }
                    _ => return Err(Diagnostic::new("TypeError", "cannot convert value to int")),
                },
            };
            return self.finish_builtin_value(p, destination, finish, output);
        }
        if kind == super::RuntimeTypeKind::Dict {
            if args.count() > 1 {
                return Err(Diagnostic::new(
                    "TypeError",
                    "dict expects at most one positional argument",
                ));
            }
            let result = self.heap.alloc(Object::Dict(Dict::default()))?;
            let mut keywords = Vec::with_capacity(args.keyword_count());
            for index in 0..args.keyword_count() {
                let (name, value) = args.keyword(p, &self.registers, index);
                let key = self.heap.alloc(Object::Str(name.to_owned()))?;
                keywords.push((key, value));
            }
            let start = DictConstructionStart {
                result,
                keywords,
                native: finish,
                return_value: None,
            };
            if args.count() == 1 {
                let source = args.positional(&self.registers, 0);
                if matches!(
                    self.heap.get(self.heap.native_value(source))?,
                    Object::Dict(_)
                ) {
                    return self.invoke_dict_merge(
                        p,
                        destination,
                        result,
                        source,
                        super::DictMergeFinish::Construction(start),
                        output,
                    );
                } else {
                    return self.invoke_dict_construction(p, destination, start, source, output);
                }
            }
            return self.finish_dict_construction(p, destination, start, output);
        }
        if kind == super::RuntimeTypeKind::Exception {
            if args.keyword_count() != 0 {
                return Err(Diagnostic::new(
                    "TypeError",
                    "exception types do not accept keyword arguments",
                ));
            }
            let arguments = (0..args.count())
                .map(|index| args.positional(&self.registers, index))
                .collect::<Vec<_>>();
            let message = self.heap.exception_message(&arguments)?;
            self.registers[destination] = self.heap.alloc(Object::Exception {
                class,
                message,
                arguments,
                attributes: Default::default(),
                cause: None,
                context: None,
                suppress_context: false,
                traceback: None,
            })?;
            return Ok(());
        }
        if args.keyword_count() != 0 || args.count() > 1 {
            return Err(Diagnostic::new(
                "TypeError",
                "builtin type accepts at most one positional argument",
            ));
        }
        let argument = (args.count() == 1).then(|| args.positional(&self.registers, 0));
        use super::RuntimeTypeKind;
        if matches!(kind, RuntimeTypeKind::List | RuntimeTypeKind::Tuple) {
            let collection_kind = if kind == RuntimeTypeKind::List {
                finish.map_or(
                    IterableCollectionKind::List,
                    IterableCollectionKind::NativeList,
                )
            } else {
                finish.map_or(
                    IterableCollectionKind::Tuple,
                    IterableCollectionKind::NativeTuple,
                )
            };
            let Some(source) = argument else {
                return self.finish_iterable_collection(
                    p,
                    destination,
                    collection_kind,
                    Vec::new(),
                    output,
                );
            };
            return self.invoke_iterable_collection(
                p,
                destination,
                collection_kind,
                source,
                output,
            );
        }
        self.registers[destination] = match kind {
            RuntimeTypeKind::Bool => {
                let Some(value) = argument else {
                    return {
                        self.registers[destination] = Value::bool(false);
                        Ok(())
                    };
                };
                return self.invoke_truth(
                    p,
                    value,
                    destination,
                    super::TruthAction::Return,
                    output,
                );
            }
            RuntimeTypeKind::Int => unreachable!("int handled before unary constructors"),
            RuntimeTypeKind::Float => {
                if let Some(value) = argument {
                    if self.heap.special_method_call(value, "__float__")?.is_some()
                        || self.heap.special_method_call(value, "__index__")?.is_some()
                    {
                        return self.invoke_numeric_conversion(
                            p,
                            value,
                            destination,
                            RuntimeTypeKind::Float,
                            finish,
                            output,
                        );
                    }
                }
                let value = match argument {
                    None => 0.0,
                    Some(value) => match self.heap.get(self.heap.native_value(value)) {
                        Ok(Object::Str(value)) => value.parse::<f64>().map_err(|_| {
                            Diagnostic::new("ValueError", "could not convert string to float")
                        })?,
                        _ => self.heap.float(value)?,
                    },
                };
                self.heap.alloc(Object::Float(value))?
            }
            RuntimeTypeKind::Str => {
                let value = argument
                    .map(|value| self.heap.format(value, false))
                    .transpose()?
                    .unwrap_or_default();
                self.heap.alloc(Object::Str(value))?
            }
            RuntimeTypeKind::List | RuntimeTypeKind::Tuple => {
                unreachable!("list and tuple handled before scalar constructors")
            }
            RuntimeTypeKind::Dict => unreachable!("dict handled before unary constructors"),
            RuntimeTypeKind::Range => unreachable!("range handled before unary constructors"),
            RuntimeTypeKind::Exception => unreachable!("exceptions handled before unary types"),
        };
        self.finish_builtin_value(p, destination, finish, output)
    }

    fn finish_builtin_value(
        &mut self,
        p: &Program,
        destination: usize,
        finish: Option<NativeSubclassFinish>,
        output: &mut dyn Write,
    ) -> Result<()> {
        let Some(finish) = finish else {
            return Ok(());
        };
        let native = self.registers[destination];
        let instance = self.heap.native_instance(finish.class, native)?;
        if let Some(arguments) = finish.initialize {
            self.finish_new(p, destination, finish.class, instance, arguments, output)
        } else {
            self.registers[destination] = instance;
            Ok(())
        }
    }
    pub(super) fn invoke_dict_construction(
        &mut self,
        p: &Program,
        destination: usize,
        start: DictConstructionStart,
        source: Value,
        output: &mut dyn Write,
    ) -> Result<()> {
        match self.heap.iterator(source) {
            Ok(iterator) => self.continue_dict_construction(
                p,
                destination,
                DictConstruction {
                    start,
                    iterator,
                    index: 0,
                },
                output,
            ),
            Err(error) if error.kind == "TypeError" => {
                let Some(call) = self.heap.special_method_call(source, "__iter__")? else {
                    return Err(Diagnostic::new("TypeError", "value is not iterable"));
                };
                let depth = self.frames.len();
                self.invoke_target(
                    p,
                    call.callable,
                    destination,
                    Arguments::Inline {
                        receiver: call.receiver,
                        positional: [Value::UNBOUND; 3],
                        count: 0,
                    },
                    output,
                )?;
                if self.frames.len() > depth {
                    self.frames.last_mut().expect("dict __iter__ frame").action =
                        ReturnAction::DictIterableStart(start);
                    Ok(())
                } else {
                    let iterator = self.registers[destination];
                    self.validate_iterator(iterator)?;
                    self.continue_dict_construction(
                        p,
                        destination,
                        DictConstruction {
                            start,
                            iterator,
                            index: 0,
                        },
                        output,
                    )
                }
            }
            Err(error) => Err(error),
        }
    }
    pub(super) fn continue_dict_construction(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: DictConstruction,
        output: &mut dyn Write,
    ) -> Result<()> {
        loop {
            let item = if self.heap.is_iterator(state.iterator) {
                let Some(item) = self.heap.next(state.iterator)? else {
                    return self.finish_dict_construction(p, destination, state.start, output);
                };
                item
            } else {
                let Some(call) = self.heap.special_method_call(state.iterator, "__next__")? else {
                    return Err(Diagnostic::new("TypeError", "object is not an iterator"));
                };
                let depth = self.frames.len();
                match self.invoke_target(
                    p,
                    call.callable,
                    destination,
                    Arguments::Inline {
                        receiver: call.receiver,
                        positional: [Value::UNBOUND; 3],
                        count: 0,
                    },
                    output,
                ) {
                    Ok(()) => {}
                    Err(error) if error.kind == "StopIteration" => {
                        return self.finish_dict_construction(p, destination, state.start, output)
                    }
                    Err(error) => return Err(error),
                }
                if self.frames.len() > depth {
                    self.frames.last_mut().expect("dict __next__ frame").action =
                        ReturnAction::DictIterableNext(state);
                    return Ok(());
                }
                self.registers[destination]
            };
            match self.invoke_dict_pair(p, destination, state, item, output)? {
                Some(next) => state = next,
                None => return Ok(()),
            }
        }
    }
    pub(super) fn invoke_dict_pair(
        &mut self,
        p: &Program,
        destination: usize,
        outer: DictConstruction,
        item: Value,
        output: &mut dyn Write,
    ) -> Result<Option<DictConstruction>> {
        match self.heap.iterator(item) {
            Ok(iterator) => self.invoke_dict_pair_iterator(p, destination, outer, iterator, output),
            Err(error) if error.kind == "TypeError" => {
                let Some(call) = self.heap.special_method_call(item, "__iter__")? else {
                    return Err(Diagnostic::new(
                        "TypeError",
                        format!(
                            "cannot convert dictionary update sequence element #{} to a sequence",
                            outer.index
                        ),
                    ));
                };
                let depth = self.frames.len();
                self.invoke_target(
                    p,
                    call.callable,
                    destination,
                    Arguments::Inline {
                        receiver: call.receiver,
                        positional: [Value::UNBOUND; 3],
                        count: 0,
                    },
                    output,
                )?;
                if self.frames.len() > depth {
                    self.frames
                        .last_mut()
                        .expect("dict pair __iter__ frame")
                        .action = ReturnAction::DictPairStart(outer);
                    Ok(None)
                } else {
                    let iterator = self.registers[destination];
                    self.validate_iterator(iterator)?;
                    self.invoke_dict_pair_iterator(p, destination, outer, iterator, output)
                }
            }
            Err(error) => Err(error),
        }
    }
    pub(super) fn invoke_dict_pair_iterator(
        &mut self,
        p: &Program,
        destination: usize,
        outer: DictConstruction,
        iterator: Value,
        output: &mut dyn Write,
    ) -> Result<Option<DictConstruction>> {
        match self.heap.iterator(iterator) {
            Ok(iterator) => self.continue_dict_pair(
                p,
                destination,
                DictPairConstruction {
                    outer,
                    iterator,
                    items: Vec::new(),
                },
                output,
            ),
            Err(error) if error.kind == "TypeError" => {
                let Some(call) = self.heap.special_method_call(iterator, "__iter__")? else {
                    return Err(Diagnostic::new("TypeError", "object is not an iterator"));
                };
                let depth = self.frames.len();
                self.invoke_target(
                    p,
                    call.callable,
                    destination,
                    Arguments::Inline {
                        receiver: call.receiver,
                        positional: [Value::UNBOUND; 3],
                        count: 0,
                    },
                    output,
                )?;
                if self.frames.len() > depth {
                    self.frames
                        .last_mut()
                        .expect("dict pair iterator __iter__ frame")
                        .action = ReturnAction::DictPairIteratorStart(outer);
                    Ok(None)
                } else {
                    let iterator = self.registers[destination];
                    self.validate_iterator(iterator)?;
                    self.continue_dict_pair(
                        p,
                        destination,
                        DictPairConstruction {
                            outer,
                            iterator,
                            items: Vec::new(),
                        },
                        output,
                    )
                }
            }
            Err(error) => Err(error),
        }
    }
    pub(super) fn continue_dict_pair(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: DictPairConstruction,
        output: &mut dyn Write,
    ) -> Result<Option<DictConstruction>> {
        if self.heap.is_iterator(state.iterator) {
            while let Some(item) = self.heap.next(state.iterator)? {
                state.items.push(item);
            }
            return self.finish_dict_pair(p, destination, state, output);
        }
        loop {
            let Some(call) = self.heap.special_method_call(state.iterator, "__next__")? else {
                return Err(Diagnostic::new("TypeError", "object is not an iterator"));
            };
            let depth = self.frames.len();
            match self.invoke_target(
                p,
                call.callable,
                destination,
                Arguments::Inline {
                    receiver: call.receiver,
                    positional: [Value::UNBOUND; 3],
                    count: 0,
                },
                output,
            ) {
                Ok(()) => {}
                Err(error) if error.kind == "StopIteration" => {
                    return self.finish_dict_pair(p, destination, state, output)
                }
                Err(error) => return Err(error),
            }
            if self.frames.len() > depth {
                self.frames
                    .last_mut()
                    .expect("dict pair __next__ frame")
                    .action = ReturnAction::DictPairNext(state);
                return Ok(None);
            }
            state.items.push(self.registers[destination]);
        }
    }
    pub(super) fn finish_dict_pair(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: DictPairConstruction,
        output: &mut dyn Write,
    ) -> Result<Option<DictConstruction>> {
        if state.items.len() != 2 {
            return Err(Diagnostic::new(
                "ValueError",
                format!(
                    "dictionary update sequence element #{} has length {}; 2 is required",
                    state.outer.index,
                    state.items.len()
                ),
            ));
        }
        state.outer.index += 1;
        self.invoke_dict_operation(
            p,
            state.outer.start.result,
            state.items[0],
            destination,
            super::DictOperationKind::SetAndContinue {
                value: state.items[1],
                state: Box::new(state.outer),
            },
            output,
        )?;
        Ok(None)
    }
    pub(super) fn finish_dict_construction(
        &mut self,
        p: &Program,
        destination: usize,
        state: DictConstructionStart,
        output: &mut dyn Write,
    ) -> Result<()> {
        let DictConstructionStart {
            result,
            keywords,
            native,
            return_value,
        } = state;
        for (key, value) in keywords {
            self.heap.dict_set(result, key, value)?;
        }
        self.registers[destination] = result;
        if let Some(return_value) = return_value {
            self.registers[destination] = return_value;
            Ok(())
        } else {
            self.finish_builtin_value(p, destination, native, output)
        }
    }
    pub(super) fn invoke_iterable_collection(
        &mut self,
        p: &Program,
        destination: usize,
        kind: IterableCollectionKind,
        source: Value,
        output: &mut dyn Write,
    ) -> Result<()> {
        match self.heap.iterator(source) {
            Ok(iterator) => {
                let mut items = Vec::new();
                while let Some(item) = self.heap.next(iterator)? {
                    if let IterableCollectionKind::ListInit { owner, .. } = &kind {
                        self.heap.append_list(*owner, item)?;
                    } else {
                        items.push(item);
                    }
                }
                self.finish_iterable_collection(p, destination, kind, items, output)
            }
            Err(error) if error.kind == "TypeError" => {
                let Some(call) = self.heap.special_method_call(source, "__iter__")? else {
                    return Err(Diagnostic::new("TypeError", "value is not iterable"));
                };
                let depth = self.frames.len();
                self.invoke_target(
                    p,
                    call.callable,
                    destination,
                    Arguments::Inline {
                        receiver: call.receiver,
                        positional: [Value::UNBOUND; 3],
                        count: 0,
                    },
                    output,
                )?;
                if self.frames.len() > depth {
                    self.frames.last_mut().expect("__iter__ frame").action =
                        ReturnAction::CollectIterableStart(kind);
                    Ok(())
                } else {
                    let iterator = self.registers[destination];
                    self.validate_iterator(iterator)?;
                    self.continue_iterable_collection(
                        p,
                        destination,
                        IterableCollection {
                            kind,
                            iterator,
                            items: Vec::new(),
                        },
                        output,
                    )
                }
            }
            Err(error) => Err(error),
        }
    }
    pub(super) fn continue_iterable_collection(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: IterableCollection,
        output: &mut dyn Write,
    ) -> Result<()> {
        if self.heap.is_iterator(state.iterator) {
            while let Some(item) = self.heap.next(state.iterator)? {
                if let IterableCollectionKind::ListInit { owner, .. } = &state.kind {
                    self.heap.append_list(*owner, item)?;
                } else {
                    state.items.push(item);
                }
            }
            return self.finish_iterable_collection(
                p,
                destination,
                state.kind,
                state.items,
                output,
            );
        }
        loop {
            let Some(call) = self.heap.special_method_call(state.iterator, "__next__")? else {
                return Err(Diagnostic::new("TypeError", "object is not an iterator"));
            };
            let depth = self.frames.len();
            match self.invoke_target(
                p,
                call.callable,
                destination,
                Arguments::Inline {
                    receiver: call.receiver,
                    positional: [Value::UNBOUND; 3],
                    count: 0,
                },
                output,
            ) {
                Ok(()) => {}
                Err(error) if error.kind == "StopIteration" => {
                    return self.finish_iterable_collection(
                        p,
                        destination,
                        state.kind,
                        state.items,
                        output,
                    )
                }
                Err(error) => return Err(error),
            }
            if self.frames.len() > depth {
                self.frames.last_mut().expect("__next__ frame").action =
                    ReturnAction::CollectIterableNext(state);
                return Ok(());
            }
            let item = self.registers[destination];
            if let IterableCollectionKind::ListInit { owner, .. } = &state.kind {
                self.heap.append_list(*owner, item)?;
            } else {
                state.items.push(item);
            }
        }
    }
    pub(super) fn finish_iterable_collection(
        &mut self,
        p: &Program,
        destination: usize,
        kind: IterableCollectionKind,
        items: Vec<Value>,
        output: &mut dyn Write,
    ) -> Result<()> {
        let (object, finish) = match kind {
            IterableCollectionKind::List => (Object::List(items), None),
            IterableCollectionKind::Tuple => (Object::Tuple(items), None),
            IterableCollectionKind::NativeList(finish) => (Object::List(items), Some(finish)),
            IterableCollectionKind::NativeTuple(finish) => (Object::Tuple(items), Some(finish)),
            IterableCollectionKind::ListInit { result, .. } => {
                self.registers[destination] = result;
                return Ok(());
            }
            IterableCollectionKind::Unpack { first, count } => {
                if items.len() < count {
                    return Err(Diagnostic::new(
                        "ValueError",
                        format!(
                            "not enough values to unpack (expected {count}, got {})",
                            items.len()
                        ),
                    ));
                }
                if items.len() > count {
                    return Err(Diagnostic::new(
                        "ValueError",
                        format!("too many values to unpack (expected {count})"),
                    ));
                }
                self.registers[first..first + count].copy_from_slice(&items);
                return Ok(());
            }
        };
        self.registers[destination] = self.heap.alloc(object)?;
        self.finish_builtin_value(p, destination, finish, output)
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
        if !self.instance_check(instance, class, false, 0)? {
            self.registers[destination] = instance;
            return Ok(());
        }
        if self.heap.class_lookup(class, "__init__")?.is_none() {
            let native_instance = matches!(
                self.heap.get(instance),
                Ok(Object::Instance {
                    native: Some(_),
                    ..
                })
            );
            if !native_instance && (arguments.count() != 0 || !arguments.keywords.is_empty()) {
                return Err(Diagnostic::new(
                    "TypeError",
                    "class without __init__ accepts no arguments",
                ));
            }
            self.registers[destination] = instance;
            return Ok(());
        }
        let initializer = self.heap.attr(instance, "__init__")?;
        let builtin = match self.heap.get(initializer)? {
            Object::Builtin(builtin) => Some(*builtin),
            Object::BoundMethod { function, .. } => match self.heap.get(*function)? {
                Object::Builtin(builtin) => Some(*builtin),
                _ => None,
            },
            _ => None,
        };
        if builtin == Some(Builtin::ObjectInit) {
            if arguments.count() != 0 {
                let constructor = self.heap.class_lookup(class, "__new__")?;
                if constructor.is_some_and(|value| {
                    matches!(
                        self.heap.get(value),
                        Ok(Object::Builtin(Builtin::ObjectNew))
                    )
                }) {
                    return Err(Diagnostic::new("TypeError", "class accepts no arguments"));
                }
            }
            self.registers[destination] = instance;
            return Ok(());
        }
        if builtin.is_some_and(|builtin| matches!(builtin, Builtin::ListInit | Builtin::DictInit)) {
            let mut initializer_arguments = arguments.clone();
            initializer_arguments.receiver = Some(instance);
            return self.invoke_native_initializer(
                p,
                builtin.expect("checked builtin initializer"),
                destination,
                &Arguments::Expanded(initializer_arguments),
                instance,
                output,
            );
        }
        if !matches!(
            self.heap.get(initializer)?,
            Object::Function { .. } | Object::BoundMethod { .. }
        ) {
            return Err(Diagnostic::new("TypeError", "__init__ must be callable"));
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
            argument_base: self.arguments.len(),
            pending_class_base: self.pending_classes.len(),
            callable,
            exception_stack: Vec::new(),
            namespace: None,
            action: super::ReturnAction::Value,
            jit_attempted: matches!(self.jit_cache.get(code), Some(super::JitEntry::Unsupported)),
            jit_resume: false,
            jit_expanded_resume_depth: None,
        });
        self.stats.peak_registers = self.stats.peak_registers.max(end);
        Ok(())
    }
    pub(super) fn invoke_equality(
        &mut self,
        p: &Program,
        left: Value,
        right: Value,
        destination: usize,
        action: super::EqualityAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        if left == right {
            return self.complete_equality(p, destination, true, action, output);
        }
        if let Some(mut protocol) =
            self.binary_protocol(tonic_core::bytecode::Op::Eq, left, right)?
        {
            protocol.completion = super::BinaryCompletion::Equality(action);
            return self.continue_binary_protocol(p, destination, protocol, output);
        }
        self.invoke_equality_fallback(p, left, right, destination, action, output)
    }

    pub(super) fn invoke_equality_fallback(
        &mut self,
        p: &Program,
        left: Value,
        right: Value,
        destination: usize,
        action: super::EqualityAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        let nested_depth = action.comparison_depth() + 1;
        if left == right {
            return self.complete_equality(p, destination, true, action, output);
        }
        if self.heap.is_integer(left)
            || self.heap.is_float(left)
            || self.heap.is_integer(right)
            || self.heap.is_float(right)
        {
            let equal = self
                .heap
                .compare(tonic_core::bytecode::Op::Eq, left, right)?
                .as_bool()
                .expect("comparison returns bool");
            return self.complete_equality(p, destination, equal, action, output);
        }
        let native_left = self.heap.native_value(left);
        let native_right = self.heap.native_value(right);
        enum EqualityFallback {
            Ready(bool),
            Sequence(Vec<Value>, Vec<Value>),
        }
        let fallback = match (self.heap.get(native_left), self.heap.get(native_right)) {
            (Ok(Object::Str(left)), Ok(Object::Str(right))) => {
                EqualityFallback::Ready(left == right)
            }
            (Ok(Object::Range { .. }), Ok(Object::Range { .. })) => {
                let equal = self
                    .heap
                    .compare(tonic_core::bytecode::Op::Eq, native_left, native_right)?
                    .as_bool()
                    .expect("range comparison returns bool");
                EqualityFallback::Ready(equal)
            }
            (Ok(Object::Tuple(left)), Ok(Object::Tuple(right)))
            | (Ok(Object::List(left)), Ok(Object::List(right))) => {
                EqualityFallback::Sequence(left.clone(), right.clone())
            }
            (Ok(Object::Slice(left)), Ok(Object::Slice(right))) => {
                EqualityFallback::Sequence(left.to_vec(), right.to_vec())
            }
            (
                Ok(Object::BoundMethod {
                    function: left_function,
                    receiver: left_receiver,
                }),
                Ok(Object::BoundMethod {
                    function: right_function,
                    receiver: right_receiver,
                }),
            ) => EqualityFallback::Sequence(
                vec![*left_function, *left_receiver],
                vec![*right_function, *right_receiver],
            ),
            (Ok(Object::Dict(_)), Ok(Object::Dict(_))) => {
                return self.begin_dict_equality(
                    p,
                    destination,
                    native_left,
                    native_right,
                    action,
                    output,
                );
            }
            _ => EqualityFallback::Ready(false),
        };
        match fallback {
            EqualityFallback::Ready(equal) => {
                self.complete_equality(p, destination, equal, action, output)
            }
            EqualityFallback::Sequence(left, right) => self.begin_equality_sequence(
                p,
                destination,
                (left, right),
                nested_depth,
                action,
                output,
            ),
        }
    }

    pub(super) fn invoke_order_comparison(
        &mut self,
        p: &Program,
        left: Value,
        right: Value,
        destination: usize,
        order: (tonic_core::bytecode::Op, usize),
        output: &mut dyn Write,
    ) -> Result<()> {
        let (op, depth) = order;
        if let Some(protocol) = self.binary_protocol(op, left, right)? {
            return self.continue_binary_protocol(p, destination, protocol, output);
        }
        let native_left = self.heap.native_value(left);
        let native_right = self.heap.native_value(right);
        let sequences = match (self.heap.get(native_left), self.heap.get(native_right)) {
            (Ok(Object::Tuple(left)), Ok(Object::Tuple(right)))
            | (Ok(Object::List(left)), Ok(Object::List(right))) => {
                Some((left.clone(), right.clone()))
            }
            _ => None,
        };
        if let Some((left, right)) = sequences {
            if depth > PROTOCOL_NESTING_LIMIT {
                return Err(Diagnostic::new(
                    "RecursionError",
                    "comparison nesting limit exceeded",
                ));
            }
            self.continue_sequence_order(
                p,
                destination,
                super::SequenceOrder {
                    left,
                    right,
                    index: 0,
                    op,
                    depth,
                },
                output,
            )
        } else {
            self.registers[destination] = self.heap.compare(op, left, right)?;
            Ok(())
        }
    }

    fn continue_sequence_order(
        &mut self,
        p: &Program,
        destination: usize,
        state: super::SequenceOrder,
        output: &mut dyn Write,
    ) -> Result<()> {
        let Some((&left, &right)) = state
            .left
            .get(state.index)
            .zip(state.right.get(state.index))
        else {
            let ordering = state.left.len().cmp(&state.right.len());
            let result = match state.op {
                tonic_core::bytecode::Op::Lt => ordering.is_lt(),
                tonic_core::bytecode::Op::Le => ordering.is_le(),
                tonic_core::bytecode::Op::Gt => ordering.is_gt(),
                tonic_core::bytecode::Op::Ge => ordering.is_ge(),
                _ => unreachable!("sequence ordering requires an ordering opcode"),
            };
            self.registers[destination] = Value::bool(result);
            return Ok(());
        };
        self.invoke_equality(
            p,
            left,
            right,
            destination,
            super::EqualityAction::SequenceOrder(state),
            output,
        )
    }

    fn begin_equality_sequence(
        &mut self,
        p: &Program,
        destination: usize,
        values: (Vec<Value>, Vec<Value>),
        depth: usize,
        action: super::EqualityAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        if depth > PROTOCOL_NESTING_LIMIT {
            return Err(Diagnostic::new(
                "RecursionError",
                "comparison nesting limit exceeded",
            ));
        }
        let (left, right) = values;
        if left.len() != right.len() {
            return self.complete_equality(p, destination, false, action, output);
        }
        let Some((&first_left, &first_right)) = left.first().zip(right.first()) else {
            return self.complete_equality(p, destination, true, action, output);
        };
        self.invoke_equality(
            p,
            first_left,
            first_right,
            destination,
            super::EqualityAction::Sequence {
                left,
                right,
                next: 1,
                depth,
                outer: Box::new(action),
            },
            output,
        )
    }

    pub(super) fn complete_equality(
        &mut self,
        p: &Program,
        destination: usize,
        equal: bool,
        action: super::EqualityAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        match action {
            super::EqualityAction::Return { negate } => {
                self.registers[destination] = Value::bool(if negate { !equal } else { equal });
                Ok(())
            }
            super::EqualityAction::Sequence {
                left,
                right,
                next,
                depth,
                outer,
            } => {
                if !equal {
                    return self.complete_equality(p, destination, false, *outer, output);
                }
                if let Some((&next_left, &next_right)) = left.get(next).zip(right.get(next)) {
                    self.invoke_equality(
                        p,
                        next_left,
                        next_right,
                        destination,
                        super::EqualityAction::Sequence {
                            left,
                            right,
                            next: next + 1,
                            depth,
                            outer,
                        },
                        output,
                    )
                } else {
                    self.complete_equality(p, destination, true, *outer, output)
                }
            }
            super::EqualityAction::DictCandidate { state, index } => {
                if self.heap.dict_version(state.start.owner)? != state.version {
                    return Err(Diagnostic::new(
                        "RuntimeError",
                        "dictionary changed during key comparison",
                    ));
                }
                if equal {
                    self.finish_dict_operation(p, destination, state, Some(index), output)
                } else {
                    self.continue_dict_operation(p, destination, state, output)
                }
            }
            super::EqualityAction::DictEntries {
                left,
                right,
                entries,
                next,
                left_version,
                right_version,
                depth,
                outer,
            } => {
                if self.heap.dict_version(left)? != left_version
                    || self.heap.dict_version(right)? != right_version
                {
                    return Err(Diagnostic::new(
                        "RuntimeError",
                        "dictionary changed during comparison",
                    ));
                }
                if !equal {
                    return self.complete_equality(p, destination, false, *outer, output);
                }
                if let Some((key, expected)) = entries.get(next).copied() {
                    self.invoke_dict_operation(
                        p,
                        right,
                        key,
                        destination,
                        super::DictOperationKind::CompareValue {
                            expected,
                            action: Box::new(super::EqualityAction::DictEntries {
                                left,
                                right,
                                entries,
                                next: next + 1,
                                left_version,
                                right_version,
                                depth,
                                outer,
                            }),
                        },
                        output,
                    )
                } else {
                    self.complete_equality(p, destination, true, *outer, output)
                }
            }
            super::EqualityAction::SequenceOrder(mut state) => {
                let left = state.left[state.index];
                let right = state.right[state.index];
                if equal {
                    state.index += 1;
                    self.continue_sequence_order(p, destination, state, output)
                } else {
                    self.invoke_order_comparison(
                        p,
                        left,
                        right,
                        destination,
                        (state.op, state.depth + 1),
                        output,
                    )
                }
            }
        }
    }

    fn begin_dict_equality(
        &mut self,
        p: &Program,
        destination: usize,
        left: Value,
        right: Value,
        action: super::EqualityAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        let depth = action.comparison_depth() + 1;
        if depth > PROTOCOL_NESTING_LIMIT {
            return Err(Diagnostic::new(
                "RecursionError",
                "comparison nesting limit exceeded",
            ));
        }
        if self.heap.dict_len(left)? != self.heap.dict_len(right)? {
            return self.complete_equality(p, destination, false, action, output);
        }
        let entries = self.heap.dict_entries(left)?;
        let left_version = self.heap.dict_version(left)?;
        let right_version = self.heap.dict_version(right)?;
        let Some((key, expected)) = entries.first().copied() else {
            return self.complete_equality(p, destination, true, action, output);
        };
        self.invoke_dict_operation(
            p,
            right,
            key,
            destination,
            super::DictOperationKind::CompareValue {
                expected,
                action: Box::new(super::EqualityAction::DictEntries {
                    left,
                    right,
                    entries,
                    next: 1,
                    left_version,
                    right_version,
                    depth,
                    outer: Box::new(action),
                }),
            },
            output,
        )
    }

    pub(super) fn invoke_dict_operation(
        &mut self,
        p: &Program,
        owner: Value,
        key: Value,
        destination: usize,
        kind: super::DictOperationKind,
        output: &mut dyn Write,
    ) -> Result<()> {
        self.invoke_hash_protocol(
            p,
            key,
            destination,
            super::HashAction::Dict(super::DictOperationStart { owner, key, kind }),
            output,
        )
    }

    pub(super) fn invoke_dict_merge(
        &mut self,
        p: &Program,
        destination: usize,
        target: Value,
        source: Value,
        finish: super::DictMergeFinish,
        output: &mut dyn Write,
    ) -> Result<()> {
        let entries = self.heap.dict_entries(source)?;
        self.continue_dict_merge(
            p,
            destination,
            super::DictMergeState {
                target,
                entries,
                next: 0,
                finish,
            },
            output,
        )
    }

    fn continue_dict_merge(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: super::DictMergeState,
        output: &mut dyn Write,
    ) -> Result<()> {
        if let Some((key, value)) = state.entries.get(state.next).copied() {
            state.next += 1;
            return self.invoke_dict_operation(
                p,
                state.target,
                key,
                destination,
                super::DictOperationKind::SetAndMerge {
                    value,
                    state: Box::new(state),
                },
                output,
            );
        }
        match state.finish {
            super::DictMergeFinish::Preserve => {
                self.registers[destination] = state.target;
                Ok(())
            }
            super::DictMergeFinish::Construction(start) => {
                self.finish_dict_construction(p, destination, start, output)
            }
        }
    }

    fn begin_dict_operation(
        &mut self,
        p: &Program,
        destination: usize,
        start: super::DictOperationStart,
        hash: i64,
        output: &mut dyn Write,
    ) -> Result<()> {
        let version = self.heap.dict_version(start.owner)?;
        let candidates = self.heap.dict_candidates(start.owner, hash)?;
        let comparison_depth = match &start.kind {
            super::DictOperationKind::CompareValue { action, .. } => action.comparison_depth(),
            _ => 0,
        };
        self.continue_dict_operation(
            p,
            destination,
            super::DictOperation {
                start,
                hash,
                version,
                candidates,
                next: 0,
                comparison_depth,
            },
            output,
        )
    }

    fn continue_dict_operation(
        &mut self,
        p: &Program,
        destination: usize,
        mut state: super::DictOperation,
        output: &mut dyn Write,
    ) -> Result<()> {
        if self.heap.dict_version(state.start.owner)? != state.version {
            return Err(Diagnostic::new(
                "RuntimeError",
                "dictionary changed during key comparison",
            ));
        }
        if let Some(index) = state.candidates.get(state.next).copied() {
            state.next += 1;
            let (candidate, _) = self.heap.dict_entry_at(state.start.owner, index)?;
            if candidate == state.start.key {
                return self.finish_dict_operation(p, destination, state, Some(index), output);
            }
            return self.invoke_equality(
                p,
                candidate,
                state.start.key,
                destination,
                super::EqualityAction::DictCandidate { state, index },
                output,
            );
        }
        self.finish_dict_operation(p, destination, state, None, output)
    }

    fn finish_dict_operation(
        &mut self,
        p: &Program,
        destination: usize,
        state: super::DictOperation,
        matched: Option<usize>,
        output: &mut dyn Write,
    ) -> Result<()> {
        match state.start.kind {
            super::DictOperationKind::Get => {
                let index = matched.ok_or_else(|| {
                    Diagnostic::new(
                        "KeyError",
                        self.heap
                            .format(state.start.key, true)
                            .unwrap_or_else(|_| "missing key".into()),
                    )
                })?;
                self.registers[destination] = self.heap.dict_entry_at(state.start.owner, index)?.1;
            }
            super::DictOperationKind::Set(value) => {
                self.heap.dict_set_hashed(
                    state.start.owner,
                    state.start.key,
                    value,
                    state.hash,
                    matched,
                )?;
                self.registers[destination] = state.start.owner;
            }
            super::DictOperationKind::SetAndContinue {
                value,
                state: continuation,
            } => {
                self.heap.dict_set_hashed(
                    state.start.owner,
                    state.start.key,
                    value,
                    state.hash,
                    matched,
                )?;
                return self.continue_dict_construction(p, destination, *continuation, output);
            }
            super::DictOperationKind::SetAndMerge {
                value,
                state: continuation,
            } => {
                self.heap.dict_set_hashed(
                    state.start.owner,
                    state.start.key,
                    value,
                    state.hash,
                    matched,
                )?;
                return self.continue_dict_merge(p, destination, *continuation, output);
            }
            super::DictOperationKind::Delete => {
                let index = matched.ok_or_else(|| {
                    Diagnostic::new(
                        "KeyError",
                        self.heap
                            .format(state.start.key, true)
                            .unwrap_or_else(|_| "missing key".into()),
                    )
                })?;
                self.heap.dict_delete_at(state.start.owner, index)?;
                self.registers[destination] = state.start.owner;
            }
            super::DictOperationKind::CompareValue { expected, action } => {
                let Some(index) = matched else {
                    return self.complete_equality(p, destination, false, *action, output);
                };
                let actual = self.heap.dict_entry_at(state.start.owner, index)?.1;
                return self.invoke_equality(p, actual, expected, destination, *action, output);
            }
        }
        Ok(())
    }

    pub(super) fn invoke_hash_protocol(
        &mut self,
        p: &Program,
        value: Value,
        destination: usize,
        action: super::HashAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        self.invoke_hash_protocol_at(p, value, destination, 0, action, output)
    }

    fn invoke_hash_protocol_at(
        &mut self,
        p: &Program,
        value: Value,
        destination: usize,
        depth: usize,
        action: super::HashAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        if depth > PROTOCOL_NESTING_LIMIT {
            return Err(Diagnostic::new(
                "RecursionError",
                "hash nesting limit exceeded",
            ));
        }
        if let Some(call) = self.operator_method_call(value, "__hash__")? {
            if call.callable == Value::NONE {
                return Err(Diagnostic::new("TypeError", "unhashable type"));
            }
            match self.heap.get(call.callable) {
                Ok(Object::Builtin(Builtin::ObjectHash)) => {
                    return self.complete_hash(
                        p,
                        destination,
                        hash_u64(value.raw()),
                        action,
                        output,
                    );
                }
                Ok(Object::Builtin(builtin)) if native_hash_kind(*builtin).is_some() => {
                    return self.invoke_typed_native_hash(
                        p,
                        (*builtin, value),
                        destination,
                        depth,
                        action,
                        output,
                    );
                }
                _ => {}
            }
            let depth = self.frames.len();
            self.invoke_target(
                p,
                call.callable,
                destination,
                Arguments::Inline {
                    receiver: call.receiver,
                    positional: [Value::UNBOUND; 3],
                    count: 0,
                },
                output,
            )?;
            if self.frames.len() > depth {
                self.frames.last_mut().expect("__hash__ frame").action =
                    super::ReturnAction::Hash(action);
            } else {
                let result = self.registers[destination];
                self.finish_hash_result(p, destination, result, action, output)?;
            }
            return Ok(());
        }
        self.invoke_native_hash(p, value, destination, depth, action, output)
    }

    fn invoke_typed_native_hash(
        &mut self,
        p: &Program,
        target: (Builtin, Value),
        destination: usize,
        depth: usize,
        action: super::HashAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        let (builtin, value) = target;
        let kind = native_hash_kind(builtin).expect("typed native hash builtin");
        let native = self.heap.native_value(value);
        let valid = match kind {
            super::RuntimeTypeKind::Int => self.heap.is_integer(value),
            super::RuntimeTypeKind::Float => {
                matches!(self.heap.get(native), Ok(Object::Float(_)))
            }
            super::RuntimeTypeKind::Str => matches!(self.heap.get(native), Ok(Object::Str(_))),
            super::RuntimeTypeKind::Tuple => {
                matches!(self.heap.get(native), Ok(Object::Tuple(_)))
            }
            super::RuntimeTypeKind::Range => {
                matches!(self.heap.get(native), Ok(Object::Range { .. }))
            }
            _ => unreachable!("only immutable native types expose typed hash builtins"),
        };
        if !valid {
            return Err(Diagnostic::new(
                "TypeError",
                "hash descriptor received an incompatible object",
            ));
        }
        self.invoke_native_hash(p, native, destination, depth, action, output)
    }

    fn invoke_native_hash(
        &mut self,
        p: &Program,
        value: Value,
        destination: usize,
        depth: usize,
        action: super::HashAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        if depth > PROTOCOL_NESTING_LIMIT {
            return Err(Diagnostic::new(
                "RecursionError",
                "hash nesting limit exceeded",
            ));
        }
        let value = self.heap.native_value(value);
        let ready = if value == Value::NONE {
            Some(0x421)
        } else if value == Value::NOT_IMPLEMENTED {
            Some(0x422)
        } else if value == Value::UNBOUND {
            return Err(Diagnostic::new(
                "TypeError",
                "internal value is not hashable",
            ));
        } else if let Some(integer) = value.integer() {
            Some(crate::hashing::normalize_i64(integer))
        } else {
            match self.heap.get(value)? {
                Object::Int(integer) => Some(normalize_bigint(integer)),
                Object::Float(number) if number.is_nan() => Some(hash_u64(value.raw())),
                Object::Float(number) if number.is_finite() && number.fract() == 0.0 => Some(
                    normalize_bigint(&BigInt::from_f64(*number).expect("finite integral float")),
                ),
                Object::Float(number) => Some(hash_u64(number.to_bits())),
                Object::Str(string) => Some(hash_string(string)),
                Object::Range { start, stop, step } => Some(hash_range(*start, *stop, *step)),
                Object::Tuple(values) => {
                    return self.begin_hash_sequence(
                        p,
                        destination,
                        values.clone(),
                        depth,
                        action,
                        output,
                    );
                }
                Object::Slice(values) => {
                    return self.begin_hash_sequence(
                        p,
                        destination,
                        values.to_vec(),
                        depth,
                        action,
                        output,
                    );
                }
                Object::BoundMethod { function, receiver } => {
                    return self.begin_hash_sequence(
                        p,
                        destination,
                        vec![*function, *receiver],
                        depth,
                        action,
                        output,
                    );
                }
                Object::List(_)
                | Object::Dict(_)
                | Object::Buffer(_)
                | Object::MappingProxy { .. } => {
                    return Err(Diagnostic::new("TypeError", "unhashable type"));
                }
                _ => Some(hash_u64(value.raw())),
            }
        };
        self.complete_hash(
            p,
            destination,
            ready.expect("native hash branch produces a value"),
            action,
            output,
        )
    }

    fn begin_hash_sequence(
        &mut self,
        p: &Program,
        destination: usize,
        values: Vec<Value>,
        depth: usize,
        action: super::HashAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        let Some(first) = values.first().copied() else {
            return self.complete_hash(
                p,
                destination,
                sequence_finish(sequence_start(), 0),
                action,
                output,
            );
        };
        self.invoke_hash_protocol_at(
            p,
            first,
            destination,
            depth + 1,
            super::HashAction::Sequence {
                values,
                next: 1,
                accumulator: sequence_start(),
                depth: depth + 1,
                outer: Box::new(action),
            },
            output,
        )
    }

    pub(super) fn finish_hash_result(
        &mut self,
        p: &Program,
        destination: usize,
        result: Value,
        action: super::HashAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        if !self.heap.is_integer(result) {
            return Err(Diagnostic::new(
                "TypeError",
                "__hash__ method must return an integer",
            ));
        }
        let hash = normalize_bigint(&self.heap.integer(result)?);
        self.complete_hash(p, destination, hash, action, output)
    }

    fn complete_hash(
        &mut self,
        p: &Program,
        destination: usize,
        hash: i64,
        action: super::HashAction,
        output: &mut dyn Write,
    ) -> Result<()> {
        match action {
            super::HashAction::Return => {
                self.registers[destination] = self.heap.i64(hash)?;
                Ok(())
            }
            super::HashAction::Dict(start) => {
                self.begin_dict_operation(p, destination, start, hash, output)
            }
            super::HashAction::Sequence {
                values,
                next,
                accumulator,
                depth,
                outer,
            } => {
                let accumulator = sequence_step(accumulator, hash);
                if let Some(value) = values.get(next).copied() {
                    self.invoke_hash_protocol_at(
                        p,
                        value,
                        destination,
                        depth,
                        super::HashAction::Sequence {
                            values,
                            next: next + 1,
                            accumulator,
                            depth,
                            outer,
                        },
                        output,
                    )
                } else {
                    let hash = sequence_finish(accumulator, values.len());
                    self.complete_hash(p, destination, hash, *outer, output)
                }
            }
        }
    }

    fn call_builtin(
        &mut self,
        builtin: Builtin,
        p: &Program,
        args: &Arguments<'_>,
        output: &mut dyn Write,
    ) -> Result<Value> {
        if !matches!(builtin, Builtin::Print | Builtin::ObjectInit) && args.keyword_count() != 0 {
            return Err(Diagnostic::new(
                "TypeError",
                "builtin does not accept keyword arguments",
            ));
        }
        let count = args.count();
        match builtin {
            Builtin::TypeNew
            | Builtin::IntNew
            | Builtin::BoolNew
            | Builtin::FloatNew
            | Builtin::StrNew
            | Builtin::ListNew
            | Builtin::ListInit
            | Builtin::TupleNew
            | Builtin::DictNew
            | Builtin::DictInit => unreachable!("builtin has a suspending call path"),
            Builtin::RangeNew => unreachable!("builtin has a suspending call path"),
            Builtin::Hash
            | Builtin::ObjectHash
            | Builtin::IntHash
            | Builtin::FloatHash
            | Builtin::StrHash
            | Builtin::TupleHash
            | Builtin::RangeHash => {
                unreachable!("hash builtin has a suspending call path")
            }
            Builtin::ObjectNew => {
                if count != 1 || args.keyword_count() != 0 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "object.__new__ expects one class argument",
                    ));
                }
                let class = args.positional(&self.registers, 0);
                if self.builtin_type_kind(class).is_some()
                    || self.builtin_subclass_kind(class).is_some()
                {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "object.__new__ cannot allocate native builtin storage",
                    ));
                }
                self.heap.instance(class)
            }
            Builtin::ObjectInit => {
                if count == 0 {
                    return Err(Diagnostic::new(
                        "TypeError",
                        "object.__init__ expects an instance",
                    ));
                }
                if count != 1 || args.keyword_count() != 0 {
                    let instance = args.positional(&self.registers, 0);
                    let class = self.runtime_class(instance)?;
                    let constructor = self.heap.class_lookup(class, "__new__")?;
                    let initializer = self.heap.class_lookup(class, "__init__")?;
                    let custom_new = !constructor.is_some_and(|value| {
                        matches!(
                            self.heap.get(value),
                            Ok(Object::Builtin(Builtin::ObjectNew))
                        )
                    });
                    let object_init = initializer.is_some_and(|value| {
                        matches!(
                            self.heap.get(value),
                            Ok(Object::Builtin(Builtin::ObjectInit))
                        )
                    });
                    if !(custom_new && object_init) {
                        return Err(Diagnostic::new(
                            "TypeError",
                            "object.__init__ expects only the instance",
                        ));
                    }
                }
                Ok(Value::NONE)
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
                let result = self.instance_check(
                    args.positional(&self.registers, 0),
                    args.positional(&self.registers, 1),
                    matches!(builtin, Builtin::IsSubclass),
                    0,
                )?;
                Ok(Value::bool(result))
            }
            Builtin::GetAttr
            | Builtin::SetAttr
            | Builtin::DelAttr
            | Builtin::HasAttr
            | Builtin::ObjectGetAttribute
            | Builtin::ObjectSetAttr
            | Builtin::ObjectDelAttr
            | Builtin::TypeGetAttribute
            | Builtin::TypeSetAttr
            | Builtin::TypeDelAttr => {
                unreachable!("attribute builtins use the suspending call path")
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
                self.push_expanded_positional(value)?;
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
                    self.push_expanded_positional(value)?;
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
    pub(super) fn push_expanded_positional(&mut self, value: Value) -> Result<()> {
        let args = self.arguments.last_mut().expect("verified argument stack");
        if args.count() >= self.limits.arguments {
            return Err(Diagnostic::new(
                "ResourceError",
                "expanded argument budget exceeded",
            ));
        }
        args.positional.push(value);
        Ok(())
    }
    pub(super) fn expand_star_argument(
        &mut self,
        p: &Program,
        destination: usize,
        source: Value,
        resume_pc: Option<usize>,
        output: &mut dyn Write,
    ) -> Result<bool> {
        match self.heap.iterator(source) {
            Ok(iterator) => {
                while let Some(item) = self.heap.next(iterator)? {
                    self.push_expanded_positional(item)?;
                }
                Ok(false)
            }
            Err(error) if error.kind == "TypeError" => {
                let Some(call) = self.heap.special_method_call(source, "__iter__")? else {
                    return Err(Diagnostic::new("TypeError", "value is not iterable"));
                };
                let depth = self.frames.len();
                self.invoke_target(
                    p,
                    call.callable,
                    destination,
                    Arguments::Inline {
                        receiver: call.receiver,
                        positional: [Value::UNBOUND; 3],
                        count: 0,
                    },
                    output,
                )?;
                if self.frames.len() > depth {
                    self.frames.last_mut().expect("__iter__ frame").action =
                        ReturnAction::ExpandIterableStart(resume_pc);
                    Ok(true)
                } else {
                    let iterator = self.registers[destination];
                    self.validate_iterator(iterator)?;
                    self.continue_argument_expansion(
                        p,
                        destination,
                        ArgumentExpansion {
                            iterator,
                            resume_pc: None,
                        },
                        output,
                    )?;
                    Ok(self.frames.len() > depth)
                }
            }
            Err(error) => Err(error),
        }
    }
    pub(super) fn continue_argument_expansion(
        &mut self,
        p: &Program,
        destination: usize,
        state: ArgumentExpansion,
        output: &mut dyn Write,
    ) -> Result<()> {
        if self.heap.is_iterator(state.iterator) {
            while let Some(item) = self.heap.next(state.iterator)? {
                self.push_expanded_positional(item)?;
            }
            if let Some(resume_pc) = state.resume_pc {
                self.frames
                    .last_mut()
                    .expect("argument expansion caller")
                    .ip = resume_pc;
            }
            return Ok(());
        }
        let Some(call) = self.heap.special_method_call(state.iterator, "__next__")? else {
            return Err(Diagnostic::new("TypeError", "object is not an iterator"));
        };
        let depth = self.frames.len();
        match self.invoke_target(
            p,
            call.callable,
            destination,
            Arguments::Inline {
                receiver: call.receiver,
                positional: [Value::UNBOUND; 3],
                count: 0,
            },
            output,
        ) {
            Ok(()) => {}
            Err(error) if error.kind == "StopIteration" => {
                if let Some(resume_pc) = state.resume_pc {
                    self.frames
                        .last_mut()
                        .expect("argument expansion caller")
                        .ip = resume_pc;
                }
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        if self.frames.len() > depth {
            self.frames.last_mut().expect("__next__ frame").action =
                ReturnAction::ExpandIterableNext(state);
        } else {
            self.push_expanded_positional(self.registers[destination])?;
            self.continue_argument_expansion(p, destination, state, output)?;
        }
        Ok(())
    }
}
fn io_error(error: std::io::Error) -> Diagnostic {
    Diagnostic::new("OSError", error.to_string())
}

fn validate_int_base(base: i64) -> Result<u32> {
    if base == 0 || (2..=36).contains(&base) {
        Ok(base as u32)
    } else {
        Err(invalid_int_base())
    }
}
fn invalid_int_base() -> Diagnostic {
    Diagnostic::new("ValueError", "int base must be 0 or between 2 and 36")
}

fn parse_int_literal(text: &str, requested_base: u32) -> Option<BigInt> {
    let text = text.trim();
    let (negative, mut digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() {
        return None;
    }
    let prefixed = digits.len() >= 2 && digits.as_bytes()[0] == b'0';
    let prefix_base = if prefixed {
        match digits.as_bytes()[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        }
    } else {
        0
    };
    let mut base = requested_base;
    if prefix_base != 0 && (base == 0 || base == prefix_base) {
        base = prefix_base;
        digits = &digits[2..];
        if let Some(rest) = digits.strip_prefix('_') {
            digits = rest;
        }
    } else if base == 0 {
        base = 10;
    }
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
    {
        return None;
    }
    let normalized = digits.replace('_', "");
    if requested_base == 0
        && prefix_base == 0
        && normalized.len() > 1
        && normalized.starts_with('0')
        && normalized.bytes().any(|byte| byte != b'0')
    {
        return None;
    }
    let value = BigInt::parse_bytes(normalized.as_bytes(), base)?;
    Some(if negative { -value } else { value })
}
