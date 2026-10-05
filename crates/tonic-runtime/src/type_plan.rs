use crate::{
    heap::{Heap, Object},
    value::Value,
};
use std::{collections::HashSet, fmt};

pub const TYPE_PLAN_SCHEMA_VERSION: u16 = 1;
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
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TypePlan {
    None,
    Exact(ExactTypePlan),
    List(Box<TypePlan>),
    Dict(Box<TypePlan>, Box<TypePlan>),
    Set(Box<TypePlan>),
    FixedTuple(Vec<TypePlan>),
    Class { type_id: u32, version: u64 },
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
            TypePlan::List(item) | TypePlan::Set(item) => {
                std::mem::size_of::<TypePlan>() + nested(item)
            }
            TypePlan::Dict(key, value) => {
                2 * std::mem::size_of::<TypePlan>() + nested(key) + nested(value)
            }
            TypePlan::FixedTuple(items) => {
                items.capacity() * std::mem::size_of::<TypePlan>()
                    + items.iter().map(nested).sum::<usize>()
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
        Ok(Object::GenericAlias { origin, args }) => resolve_generic_alias(
            heap,
            builtins,
            *origin,
            *args,
            depth + 1,
            visiting,
            class_dependencies,
        ),
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
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}
