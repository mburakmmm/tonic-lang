use crate::{
    heap::{Heap, Object, TypingFormKind},
    value::Value,
};
use std::{collections::HashSet, fmt};

pub const TYPE_PLAN_SCHEMA_VERSION: u16 = 6;
const MAX_TYPE_PLAN_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TypePlanBuiltins {
    pub none_type: Value,
    pub bool_: Value,
    pub int: Value,
    pub float: Value,
    pub str_: Value,
    pub list: Value,
    pub tuple: Value,
    pub dict: Value,
    pub set: Value,
    pub buffer: Value,
    pub literal: Value,
    pub callable: Value,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum ExactTypePlan {
    NoneType,
    Bool,
    Int,
    Float,
    Str,
    List,
    Tuple,
    Dict,
    Set,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum LiteralTypePlan {
    None,
    Bool(bool),
    Int(String),
    Str(String),
    Ellipsis,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum CallableParameters {
    Any,
    Positional(Vec<TypePlan>),
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum BufferDTypePlan {
    F64,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum BufferMutabilityPlan {
    Any,
    ReadOnly,
    Writable,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct BufferTypePlan {
    pub dtype: BufferDTypePlan,
    pub rank: Option<u32>,
    pub mutability: BufferMutabilityPlan,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum TypePlan {
    None,
    Exact(ExactTypePlan),
    List(Box<TypePlan>),
    Dict(Box<TypePlan>, Box<TypePlan>),
    Set(Box<TypePlan>),
    FixedTuple(Vec<TypePlan>),
    VariadicTuple(Box<TypePlan>),
    Literal(Vec<LiteralTypePlan>),
    Callable {
        parameters: CallableParameters,
        result: Box<TypePlan>,
    },
    Buffer(BufferTypePlan),
    Union(Vec<TypePlan>),
    Class {
        type_id: u32,
        version: u64,
    },
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TypePlanRejection {
    UnsupportedValue,
    UnsupportedGenericOrigin,
    InvalidGenericArity,
    TypeParameter,
    RecursiveAlias,
    RecursionLimit,
    NonStringAnnotationKey,
}

impl TypePlanRejection {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedValue => "unsupported-value",
            Self::UnsupportedGenericOrigin => "unsupported-generic-origin",
            Self::InvalidGenericArity => "invalid-generic-arity",
            Self::TypeParameter => "type-parameter",
            Self::RecursiveAlias => "recursive-alias",
            Self::RecursionLimit => "recursion-limit",
            Self::NonStringAnnotationKey => "non-string-annotation-key",
        }
    }
}

impl fmt::Display for TypePlanRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationTypePlan {
    pub name: String,
    pub plan: Result<TypePlan, TypePlanRejection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionTypePlan {
    pub schema_version: u16,
    pub annotation_version: u64,
    pub canonical_hash: u64,
    pub annotations: Vec<AnnotationTypePlan>,
    class_dependencies: Vec<ClassPlanDependency>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClassPlanDependency {
    class: Value,
    type_id: u32,
    version: u64,
}

impl FunctionTypePlan {
    fn new(
        annotation_version: u64,
        mut annotations: Vec<AnnotationTypePlan>,
        class_dependencies: Vec<ClassPlanDependency>,
    ) -> Self {
        annotations.sort_by(|left, right| left.name.cmp(&right.name));
        let mut hash = CanonicalHash::new();
        hash.u16(TYPE_PLAN_SCHEMA_VERSION);
        for annotation in &annotations {
            hash.string(&annotation.name);
            hash.result(&annotation.plan);
        }
        Self {
            schema_version: TYPE_PLAN_SCHEMA_VERSION,
            annotation_version,
            canonical_hash: hash.finish(),
            annotations,
            class_dependencies,
        }
    }

    pub(crate) fn dependencies_current(&self, heap: &Heap) -> bool {
        self.class_dependencies.iter().all(|dependency| {
            heap.class(dependency.class).is_ok_and(|class| {
                class.id.0 == dependency.type_id && class.version == dependency.version
            })
        })
    }

    pub(crate) fn class_handle(&self, type_id: u32, version: u64) -> Option<Value> {
        self.class_dependencies
            .iter()
            .find(|dependency| dependency.type_id == type_id && dependency.version == version)
            .map(|dependency| dependency.class)
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.annotations.capacity() * std::mem::size_of::<AnnotationTypePlan>()
            + self.class_dependencies.capacity() * std::mem::size_of::<ClassPlanDependency>()
            + self
                .annotations
                .iter()
                .map(|annotation| annotation.name.capacity() + plan_bytes(&annotation.plan))
                .sum::<usize>()
    }
}

fn plan_bytes(plan: &Result<TypePlan, TypePlanRejection>) -> usize {
    fn nested(plan: &TypePlan) -> usize {
        match plan {
            TypePlan::List(item) | TypePlan::Set(item) | TypePlan::VariadicTuple(item) => {
                std::mem::size_of::<TypePlan>() + nested(item)
            }
            TypePlan::Dict(key, value) => {
                2 * std::mem::size_of::<TypePlan>() + nested(key) + nested(value)
            }
            TypePlan::FixedTuple(items) | TypePlan::Union(items) => {
                items.capacity() * std::mem::size_of::<TypePlan>()
                    + items.iter().map(nested).sum::<usize>()
            }
            TypePlan::Literal(items) => {
                items.capacity() * std::mem::size_of::<LiteralTypePlan>()
                    + items
                        .iter()
                        .map(|item| match item {
                            LiteralTypePlan::Int(value) | LiteralTypePlan::Str(value) => {
                                value.capacity()
                            }
                            _ => 0,
                        })
                        .sum::<usize>()
            }
            TypePlan::Callable { parameters, result } => {
                let parameters = match parameters {
                    CallableParameters::Any => 0,
                    CallableParameters::Positional(items) => {
                        items.capacity() * std::mem::size_of::<TypePlan>()
                            + items.iter().map(nested).sum::<usize>()
                    }
                };
                parameters + std::mem::size_of::<TypePlan>() + nested(result)
            }
            _ => 0,
        }
    }
    plan.as_ref().map(nested).unwrap_or(0)
}

pub(crate) fn resolve_function_type_plan(
    heap: &Heap,
    builtins: TypePlanBuiltins,
    annotations: Value,
) -> Option<FunctionTypePlan> {
    let Object::Dict(dictionary) = heap.get(annotations).ok()? else {
        return None;
    };
    let mut plans = Vec::with_capacity(dictionary.entries.len());
    let mut class_dependencies = Vec::new();
    for (key, annotation) in &dictionary.entries {
        let name = match heap.get(*key) {
            Ok(Object::Str(name)) => name.clone(),
            _ => {
                plans.push(AnnotationTypePlan {
                    name: "<non-string>".into(),
                    plan: Err(TypePlanRejection::NonStringAnnotationKey),
                });
                continue;
            }
        };
        let mut visiting = HashSet::new();
        plans.push(AnnotationTypePlan {
            name,
            plan: resolve_type_plan(
                heap,
                builtins,
                *annotation,
                0,
                &mut visiting,
                &mut class_dependencies,
            ),
        });
    }
    Some(FunctionTypePlan::new(
        dictionary.mutation_version,
        plans,
        class_dependencies,
    ))
}

fn resolve_type_plan(
    heap: &Heap,
    builtins: TypePlanBuiltins,
    annotation: Value,
    depth: usize,
    visiting: &mut HashSet<Value>,
    class_dependencies: &mut Vec<ClassPlanDependency>,
) -> Result<TypePlan, TypePlanRejection> {
    if depth >= MAX_TYPE_PLAN_DEPTH {
        return Err(TypePlanRejection::RecursionLimit);
    }
    if annotation == Value::NONE {
        return Ok(TypePlan::None);
    }
    let exact = [
        (builtins.none_type, ExactTypePlan::NoneType),
        (builtins.bool_, ExactTypePlan::Bool),
        (builtins.int, ExactTypePlan::Int),
        (builtins.float, ExactTypePlan::Float),
        (builtins.str_, ExactTypePlan::Str),
        (builtins.list, ExactTypePlan::List),
        (builtins.tuple, ExactTypePlan::Tuple),
        (builtins.dict, ExactTypePlan::Dict),
        (builtins.set, ExactTypePlan::Set),
    ];
    if let Some(kind) = exact
        .into_iter()
        .find_map(|(candidate, kind)| (annotation == candidate).then_some(kind))
    {
        return Ok(TypePlan::Exact(kind));
    }
    if annotation == builtins.buffer {
        return Ok(TypePlan::Buffer(BufferTypePlan {
            dtype: BufferDTypePlan::F64,
            rank: None,
            mutability: BufferMutabilityPlan::Any,
        }));
    }
    if !visiting.insert(annotation) {
        return Err(TypePlanRejection::RecursiveAlias);
    }
    let result = match heap.get(annotation) {
        Ok(Object::Class(class)) => {
            if !class_dependencies
                .iter()
                .any(|dependency| dependency.class == annotation)
            {
                class_dependencies.push(ClassPlanDependency {
                    class: annotation,
                    type_id: class.id.0,
                    version: class.version,
                });
            }
            Ok(TypePlan::Class {
                type_id: class.id.0,
                version: class.version,
            })
        }
        Ok(Object::TypeParam { .. }) | Ok(Object::TypeUnpack(_)) => {
            Err(TypePlanRejection::TypeParameter)
        }
        Ok(Object::TypeAlias { value, .. }) => resolve_type_plan(
            heap,
            builtins,
            *value,
            depth + 1,
            visiting,
            class_dependencies,
        ),
        Ok(Object::GenericAlias { origin, args, .. }) => resolve_generic_alias(
            heap,
            builtins,
            *origin,
            *args,
            depth + 1,
            visiting,
            class_dependencies,
        ),
        Ok(Object::UnionType { members, .. }) => {
            let mut plans = Vec::with_capacity(members.len());
            for member in members {
                let plan = resolve_type_plan(
                    heap,
                    builtins,
                    *member,
                    depth + 1,
                    visiting,
                    class_dependencies,
                )?;
                match plan {
                    TypePlan::Union(nested) => plans.extend(nested),
                    TypePlan::Exact(ExactTypePlan::NoneType) => plans.push(TypePlan::None),
                    plan => plans.push(plan),
                }
            }
            plans.sort_unstable();
            plans.dedup();
            match plans.len() {
                0 => Err(TypePlanRejection::InvalidGenericArity),
                1 => Ok(plans.pop().expect("single union plan")),
                _ => Ok(TypePlan::Union(plans)),
            }
        }
        _ => Err(TypePlanRejection::UnsupportedValue),
    };
    visiting.remove(&annotation);
    result
}

fn resolve_generic_alias(
    heap: &Heap,
    builtins: TypePlanBuiltins,
    origin: Value,
    args: Value,
    depth: usize,
    visiting: &mut HashSet<Value>,
    class_dependencies: &mut Vec<ClassPlanDependency>,
) -> Result<TypePlan, TypePlanRejection> {
    let Ok(Object::Tuple(arguments)) = heap.get(args) else {
        return Err(TypePlanRejection::InvalidGenericArity);
    };
    if origin == builtins.literal {
        return canonical_literal_arguments(heap, args).map(TypePlan::Literal);
    }
    if origin == builtins.callable {
        if arguments.len() != 2 {
            return Err(TypePlanRejection::InvalidGenericArity);
        }
        let parameters = if arguments[0] == Value::ELLIPSIS {
            CallableParameters::Any
        } else {
            let parameter_values = match heap.get(arguments[0]) {
                Ok(Object::List(values)) | Ok(Object::Tuple(values)) => values.as_slice(),
                _ => return Err(TypePlanRejection::InvalidGenericArity),
            };
            let parameters = parameter_values
                .iter()
                .map(|parameter| {
                    resolve_type_plan(
                        heap,
                        builtins,
                        *parameter,
                        depth,
                        visiting,
                        class_dependencies,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            CallableParameters::Positional(parameters)
        };
        let result = resolve_type_plan(
            heap,
            builtins,
            arguments[1],
            depth,
            visiting,
            class_dependencies,
        )?;
        return Ok(TypePlan::Callable {
            parameters,
            result: Box::new(result),
        });
    }
    if origin == builtins.list || origin == builtins.set {
        if arguments.len() != 1 {
            return Err(TypePlanRejection::InvalidGenericArity);
        }
        let item = resolve_type_plan(
            heap,
            builtins,
            arguments[0],
            depth,
            visiting,
            class_dependencies,
        )?;
        return if origin == builtins.list {
            Ok(TypePlan::List(Box::new(item)))
        } else {
            Ok(TypePlan::Set(Box::new(item)))
        };
    }
    if origin == builtins.dict {
        if arguments.len() != 2 {
            return Err(TypePlanRejection::InvalidGenericArity);
        }
        let key = resolve_type_plan(
            heap,
            builtins,
            arguments[0],
            depth,
            visiting,
            class_dependencies,
        )?;
        let value = resolve_type_plan(
            heap,
            builtins,
            arguments[1],
            depth,
            visiting,
            class_dependencies,
        )?;
        return Ok(TypePlan::Dict(Box::new(key), Box::new(value)));
    }
    if origin == builtins.tuple {
        if arguments.len() == 2 && arguments[1] == Value::ELLIPSIS {
            let item = resolve_type_plan(
                heap,
                builtins,
                arguments[0],
                depth,
                visiting,
                class_dependencies,
            )?;
            return Ok(TypePlan::VariadicTuple(Box::new(item)));
        }
        if arguments.contains(&Value::ELLIPSIS) {
            return Err(TypePlanRejection::InvalidGenericArity);
        }
        let items = arguments
            .iter()
            .map(|argument| {
                resolve_type_plan(
                    heap,
                    builtins,
                    *argument,
                    depth,
                    visiting,
                    class_dependencies,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(TypePlan::FixedTuple(items));
    }
    Err(TypePlanRejection::UnsupportedGenericOrigin)
}

struct CanonicalHash(u64);

impl CanonicalHash {
    const fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u64(value.len() as u64);
        self.bytes(value.as_bytes());
    }

    fn result(&mut self, value: &Result<TypePlan, TypePlanRejection>) {
        match value {
            Ok(plan) => {
                self.u8(0);
                self.plan(plan);
            }
            Err(reason) => {
                self.u8(1);
                self.u8(*reason as u8);
            }
        }
    }

    fn plan(&mut self, plan: &TypePlan) {
        match plan {
            TypePlan::None => self.u8(0),
            TypePlan::Exact(kind) => {
                self.u8(1);
                self.u8(*kind as u8);
            }
            TypePlan::List(item) => {
                self.u8(2);
                self.plan(item);
            }
            TypePlan::Dict(key, value) => {
                self.u8(3);
                self.plan(key);
                self.plan(value);
            }
            TypePlan::Set(item) => {
                self.u8(4);
                self.plan(item);
            }
            TypePlan::FixedTuple(items) => {
                self.u8(5);
                self.u64(items.len() as u64);
                items.iter().for_each(|item| self.plan(item));
            }
            TypePlan::Class { type_id, version } => {
                self.u8(6);
                self.u32(*type_id);
                self.u64(*version);
            }
            TypePlan::Union(items) => {
                self.u8(7);
                self.u64(items.len() as u64);
                items.iter().for_each(|item| self.plan(item));
            }
            TypePlan::VariadicTuple(item) => {
                self.u8(8);
                self.plan(item);
            }
            TypePlan::Literal(items) => {
                self.u8(9);
                self.u64(items.len() as u64);
                for item in items {
                    match item {
                        LiteralTypePlan::None => self.u8(0),
                        LiteralTypePlan::Bool(value) => {
                            self.u8(1);
                            self.u8(u8::from(*value));
                        }
                        LiteralTypePlan::Int(value) => {
                            self.u8(2);
                            self.string(value);
                        }
                        LiteralTypePlan::Str(value) => {
                            self.u8(3);
                            self.string(value);
                        }
                        LiteralTypePlan::Ellipsis => self.u8(4),
                    }
                }
            }
            TypePlan::Callable { parameters, result } => {
                self.u8(10);
                match parameters {
                    CallableParameters::Any => self.u8(0),
                    CallableParameters::Positional(items) => {
                        self.u8(1);
                        self.u64(items.len() as u64);
                        items.iter().for_each(|item| self.plan(item));
                    }
                }
                self.plan(result);
            }
            TypePlan::Buffer(buffer) => {
                self.u8(11);
                self.u8(buffer.dtype as u8);
                match buffer.rank {
                    Some(rank) => {
                        self.u8(1);
                        self.u32(rank);
                    }
                    None => self.u8(0),
                }
                self.u8(buffer.mutability as u8);
            }
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

pub(crate) fn literal_type_plan(
    heap: &Heap,
    value: Value,
) -> Result<LiteralTypePlan, TypePlanRejection> {
    if value == Value::NONE {
        return Ok(LiteralTypePlan::None);
    }
    if let Some(value) = value.as_bool() {
        return Ok(LiteralTypePlan::Bool(value));
    }
    if let Some(value) = value.as_int() {
        return Ok(LiteralTypePlan::Int(value.to_string()));
    }
    if value == Value::ELLIPSIS {
        return Ok(LiteralTypePlan::Ellipsis);
    }
    match heap.get(value) {
        Ok(Object::Int(value)) => Ok(LiteralTypePlan::Int(value.to_string())),
        Ok(Object::Str(value)) => Ok(LiteralTypePlan::Str(value.clone())),
        _ => Err(TypePlanRejection::UnsupportedValue),
    }
}

pub(crate) fn canonical_literal_arguments(
    heap: &Heap,
    args: Value,
) -> Result<Vec<LiteralTypePlan>, TypePlanRejection> {
    fn append(
        heap: &Heap,
        args: Value,
        depth: usize,
        items: &mut Vec<LiteralTypePlan>,
    ) -> Result<(), TypePlanRejection> {
        if depth >= MAX_TYPE_PLAN_DEPTH {
            return Err(TypePlanRejection::RecursionLimit);
        }
        let Ok(Object::Tuple(arguments)) = heap.get(args) else {
            return Err(TypePlanRejection::InvalidGenericArity);
        };
        if arguments.is_empty() {
            return Err(TypePlanRejection::InvalidGenericArity);
        }
        for argument in arguments {
            let nested_args = match heap.get(*argument) {
                Ok(Object::GenericAlias { origin, args, .. })
                    if matches!(
                        heap.get(*origin),
                        Ok(Object::TypingForm {
                            kind: TypingFormKind::Literal,
                            ..
                        })
                    ) =>
                {
                    Some(*args)
                }
                _ => None,
            };
            if let Some(nested_args) = nested_args {
                append(heap, nested_args, depth + 1, items)?;
            } else {
                items.push(literal_type_plan(heap, *argument)?);
            }
        }
        Ok(())
    }

    let mut items = Vec::new();
    append(heap, args, 0, &mut items)?;
    items.sort_unstable();
    items.dedup();
    Ok(items)
}
