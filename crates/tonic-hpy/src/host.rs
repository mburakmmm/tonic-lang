#![allow(unsafe_code)]

use crate::{LoadError, PinnedUniversalModule, UNIVERSAL_ABI};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    error::Error,
    ffi::{c_char, c_int, c_void, CStr},
    fmt,
    path::Path,
    ptr::{self, NonNull},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tonic_core::diagnostic::{Diagnostic, Result as TonicResult};
use tonic_runtime::{Context, Handle, StatefulKeywordNativeFn, StatefulNativeSignature, Vm};

const MAX_DEFINITIONS: usize = 4_096;
const MAX_CONTAINER_ITEMS: usize = 1_048_576;
const HPY_DEF_KIND_METHOD: c_int = 2;
const HPY_FUNC_VARARGS: c_int = 1;
const HPY_FUNC_KEYWORDS: c_int = 2;
const HPY_FUNC_NOARGS: c_int = 3;
const HPY_FUNC_O: c_int = 4;
const CONTEXT_NAME: &[u8] = b"Tonic HPy 0.9 Universal\0";
type VectorArguments = (Vec<Handle>, Vec<(String, Handle)>);

const SLOT_DUP: usize = 77;
const SLOT_CLOSE: usize = 78;
const SLOT_LONG_FROM_I64: usize = 81;
const SLOT_LONG_AS_I64: usize = 88;
const SLOT_ERR_SET_STRING: usize = 137;
const SLOT_ERR_OCCURRED: usize = 141;
const SLOT_ERR_CLEAR: usize = 144;
const SLOT_UNICODE_FROM_STRING: usize = 185;
const SLOT_UNICODE_AS_UTF8_AND_SIZE: usize = 190;
const SLOT_LENGTH: usize = 98;
const SLOT_CALLABLE_CHECK: usize = 134;
const SLOT_CALL_TUPLE_DICT: usize = 135;
const SLOT_GET_ATTR: usize = 152;
const SLOT_GET_ATTR_S: usize = 153;
const SLOT_HAS_ATTR: usize = 154;
const SLOT_HAS_ATTR_S: usize = 155;
const SLOT_SET_ATTR: usize = 156;
const SLOT_SET_ATTR_S: usize = 157;
const SLOT_GET_ITEM: usize = 158;
const SLOT_GET_ITEM_I: usize = 159;
const SLOT_GET_ITEM_S: usize = 160;
const SLOT_CONTAINS: usize = 161;
const SLOT_SET_ITEM: usize = 162;
const SLOT_SET_ITEM_I: usize = 163;
const SLOT_SET_ITEM_S: usize = 164;
const SLOT_REPR: usize = 171;
const SLOT_LIST_CHECK: usize = 198;
const SLOT_LIST_NEW: usize = 199;
const SLOT_LIST_APPEND: usize = 200;
const SLOT_DICT_CHECK: usize = 201;
const SLOT_DICT_NEW: usize = 202;
const SLOT_TUPLE_CHECK: usize = 203;
const SLOT_TUPLE_FROM_ARRAY: usize = 204;
const SLOT_LIST_BUILDER_NEW: usize = 209;
const SLOT_LIST_BUILDER_SET: usize = 210;
const SLOT_LIST_BUILDER_BUILD: usize = 211;
const SLOT_LIST_BUILDER_CANCEL: usize = 212;
const SLOT_TUPLE_BUILDER_NEW: usize = 213;
const SLOT_TUPLE_BUILDER_SET: usize = 214;
const SLOT_TUPLE_BUILDER_BUILD: usize = 215;
const SLOT_TUPLE_BUILDER_CANCEL: usize = 216;
const SLOT_DEL_ITEM: usize = 235;
const SLOT_DEL_ITEM_I: usize = 236;
const SLOT_DEL_ITEM_S: usize = 237;
const SLOT_CALL: usize = 261;
const SLOT_CALL_METHOD: usize = 262;

const HANDLE_NONE: usize = 0;
const HANDLE_TRUE: usize = 1;
const HANDLE_FALSE: usize = 2;
const FIRST_EXCEPTION: usize = 5;
const LAST_EXCEPTION: usize = 68;
const LAST_CORE_TYPE: usize = 76;
const FIRST_LATE_HANDLE: usize = 238;
const LAST_LATE_HANDLE: usize = 243;

static NEXT_LOCAL_HANDLE: AtomicUsize = AtomicUsize::new(1);
static NEXT_BUILDER: AtomicUsize = AtomicUsize::new(1);

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Hpy {
    bits: isize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HpyBuilder {
    bits: isize,
}

impl Hpy {
    const NULL: Self = Self { bits: 0 };

    const fn special(slot: usize) -> Self {
        Self {
            bits: -(slot as isize) - 1,
        }
    }

    fn special_slot(self) -> Option<usize> {
        (self.bits < 0).then(|| (-self.bits - 1) as usize)
    }
}

#[repr(C)]
struct HpyContext {
    name: *const c_char,
    private: *mut c_void,
    abi_version: c_int,
    slots: [usize; 263],
}

// SAFETY: after construction the context and all slot values are immutable.
// Call-local mutable state is selected through thread-local storage and native
// calls are serialized by `ModuleState::call_lock`.
unsafe impl Send for HpyContext {}
// SAFETY: same immutable-layout and serialized-call invariant as above.
unsafe impl Sync for HpyContext {}

impl HpyContext {
    fn new() -> Box<Self> {
        let mut context = Box::new(Self {
            name: CONTEXT_NAME.as_ptr().cast(),
            private: ptr::null_mut(),
            abi_version: UNIVERSAL_ABI.major as c_int,
            slots: [0; 263],
        });
        for slot in 0..=LAST_CORE_TYPE {
            context.slots[slot] = Hpy::special(slot).bits as usize;
        }
        for slot in FIRST_LATE_HANDLE..=LAST_LATE_HANDLE {
            context.slots[slot] = Hpy::special(slot).bits as usize;
        }
        context.slots[SLOT_DUP] = hpy_dup as usize;
        context.slots[SLOT_CLOSE] = hpy_close as usize;
        context.slots[SLOT_LONG_FROM_I64] = hpy_long_from_i64 as usize;
        context.slots[SLOT_LONG_AS_I64] = hpy_long_as_i64 as usize;
        context.slots[SLOT_ERR_SET_STRING] = hpy_err_set_string as usize;
        context.slots[SLOT_ERR_OCCURRED] = hpy_err_occurred as usize;
        context.slots[SLOT_ERR_CLEAR] = hpy_err_clear as usize;
        context.slots[SLOT_UNICODE_FROM_STRING] = hpy_unicode_from_string as usize;
        context.slots[SLOT_UNICODE_AS_UTF8_AND_SIZE] = hpy_unicode_as_utf8_and_size as usize;
        context.slots[SLOT_LENGTH] = hpy_length as usize;
        context.slots[SLOT_CALLABLE_CHECK] = hpy_callable_check as usize;
        context.slots[SLOT_CALL_TUPLE_DICT] = hpy_call_tuple_dict as usize;
        context.slots[SLOT_GET_ATTR] = hpy_get_attr as usize;
        context.slots[SLOT_GET_ATTR_S] = hpy_get_attr_s as usize;
        context.slots[SLOT_HAS_ATTR] = hpy_has_attr as usize;
        context.slots[SLOT_HAS_ATTR_S] = hpy_has_attr_s as usize;
        context.slots[SLOT_SET_ATTR] = hpy_set_attr as usize;
        context.slots[SLOT_SET_ATTR_S] = hpy_set_attr_s as usize;
        context.slots[SLOT_GET_ITEM] = hpy_get_item as usize;
        context.slots[SLOT_GET_ITEM_I] = hpy_get_item_i as usize;
        context.slots[SLOT_GET_ITEM_S] = hpy_get_item_s as usize;
        context.slots[SLOT_CONTAINS] = hpy_contains as usize;
        context.slots[SLOT_SET_ITEM] = hpy_set_item as usize;
        context.slots[SLOT_SET_ITEM_I] = hpy_set_item_i as usize;
        context.slots[SLOT_SET_ITEM_S] = hpy_set_item_s as usize;
        context.slots[SLOT_REPR] = hpy_repr as usize;
        context.slots[SLOT_LIST_CHECK] = hpy_list_check as usize;
        context.slots[SLOT_LIST_NEW] = hpy_list_new as usize;
        context.slots[SLOT_LIST_APPEND] = hpy_list_append as usize;
        context.slots[SLOT_DICT_CHECK] = hpy_dict_check as usize;
        context.slots[SLOT_DICT_NEW] = hpy_dict_new as usize;
        context.slots[SLOT_TUPLE_CHECK] = hpy_tuple_check as usize;
        context.slots[SLOT_TUPLE_FROM_ARRAY] = hpy_tuple_from_array as usize;
        context.slots[SLOT_LIST_BUILDER_NEW] = hpy_list_builder_new as usize;
        context.slots[SLOT_LIST_BUILDER_SET] = hpy_list_builder_set as usize;
        context.slots[SLOT_LIST_BUILDER_BUILD] = hpy_list_builder_build as usize;
        context.slots[SLOT_LIST_BUILDER_CANCEL] = hpy_list_builder_cancel as usize;
        context.slots[SLOT_TUPLE_BUILDER_NEW] = hpy_tuple_builder_new as usize;
        context.slots[SLOT_TUPLE_BUILDER_SET] = hpy_tuple_builder_set as usize;
        context.slots[SLOT_TUPLE_BUILDER_BUILD] = hpy_tuple_builder_build as usize;
        context.slots[SLOT_TUPLE_BUILDER_CANCEL] = hpy_tuple_builder_cancel as usize;
        context.slots[SLOT_DEL_ITEM] = hpy_del_item as usize;
        context.slots[SLOT_DEL_ITEM_I] = hpy_del_item_i as usize;
        context.slots[SLOT_DEL_ITEM_S] = hpy_del_item_s as usize;
        context.slots[SLOT_CALL] = hpy_call as usize;
        context.slots[SLOT_CALL_METHOD] = hpy_call_method as usize;
        context
    }
}

#[repr(C)]
struct HpyModuleDef {
    _doc: *const c_char,
    size: isize,
    legacy_methods: *mut c_void,
    defines: *mut *mut c_void,
    globals: *mut *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HpyMethod {
    name: *const c_char,
    implementation: *const c_void,
    _cpython_trampoline: *const c_void,
    signature: c_int,
    _doc: *const c_char,
}

#[repr(C)]
struct HpyMethodDefinition {
    kind: c_int,
    method: HpyMethod,
}

#[derive(Clone, Debug)]
struct Method {
    name: String,
    implementation: usize,
    signature: MethodSignature,
}

#[derive(Clone, Copy, Debug)]
enum MethodSignature {
    NoArgs,
    OneArg,
    VarArgs,
    Keywords,
}

impl MethodSignature {
    const fn native_signature(self) -> StatefulNativeSignature {
        match self {
            Self::NoArgs => StatefulNativeSignature::Exact(0),
            Self::OneArg => StatefulNativeSignature::Exact(1),
            Self::VarArgs => StatefulNativeSignature::VarArgs,
            Self::Keywords => StatefulNativeSignature::Keywords,
        }
    }
}

struct ModuleState {
    _library: PinnedUniversalModule,
    context: Box<HpyContext>,
    module_name: String,
    methods: Vec<Method>,
    call_lock: Mutex<()>,
}

/// A validated HPy Universal module whose supported methods can be registered
/// in one Tonic VM.
pub struct UniversalModule {
    state: Arc<ModuleState>,
}

impl fmt::Debug for UniversalModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UniversalModule")
            .field("module_name", &self.state.module_name)
            .field("methods", &self.method_names())
            .finish_non_exhaustive()
    }
}

impl UniversalModule {
    /// Load, validate and materialize the supported HPy module surface.
    ///
    /// # Safety
    ///
    /// `path` must name a trusted HPy 0.9 Universal library. Its static module
    /// definition, definition pointer array, strings and method signatures must
    /// remain valid for the process lifetime.
    pub unsafe fn load(module_name: &str, path: impl AsRef<Path>) -> Result<Self, HostError> {
        // SAFETY: forwarded from this function's native-library precondition.
        let library = unsafe { PinnedUniversalModule::load(module_name, path) }?;
        // SAFETY: the loader validated HPy's module initializer signature.
        let definition = unsafe { library.module_definition() }?;
        let mut context = HpyContext::new();
        // SAFETY: `context` is a stable boxed HPy 0.9 layout and remains owned
        // by the returned module for every possible extension call.
        unsafe {
            library.initialize_context(ptr::from_mut(context.as_mut()).cast());
        }
        // SAFETY: trusted HPy module definitions are process-static and follow
        // the exact 0.9 C layouts declared above.
        let methods = unsafe { parse_module_definition(definition) }?;
        Ok(Self {
            state: Arc::new(ModuleState {
                _library: library,
                context,
                module_name: module_name.to_owned(),
                methods,
                call_lock: Mutex::new(()),
            }),
        })
    }

    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.state.module_name
    }

    #[must_use]
    pub fn method_names(&self) -> Vec<&str> {
        self.state
            .methods
            .iter()
            .map(|method| method.name.as_str())
            .collect()
    }

    /// Register every materialized HPy method as a VM-owned native function.
    pub fn register(&self, vm: &mut Vm) -> Result<(), HostError> {
        let mut methods = Vec::with_capacity(self.state.methods.len());
        for index in 0..self.state.methods.len() {
            let state = Arc::clone(&self.state);
            let method = &state.methods[index];
            let method_name = method.name.clone();
            let signature = method.signature.native_signature();
            let callback: Arc<StatefulKeywordNativeFn> =
                Arc::new(move |context, arguments, keywords| {
                    invoke_method(&state, index, context, arguments, keywords)
                });
            methods.push((method_name, signature, callback));
        }
        vm.register_stateful_keyword_module(&self.state.module_name, methods)?;
        Ok(())
    }
}

unsafe fn parse_module_definition(definition: NonNull<c_void>) -> Result<Vec<Method>, HostError> {
    // SAFETY: the caller guarantees the pointer names a process-static
    // `HPyModuleDef` from the validated HPy 0.9 library.
    let definition = unsafe { &*definition.cast::<HpyModuleDef>().as_ptr() };
    if definition.size != 0 {
        return Err(HostError::Module(
            "HPy module C state is unavailable before milestone H3".into(),
        ));
    }
    if !definition.legacy_methods.is_null() {
        return Err(HostError::Module(
            "HPy Universal modules cannot contain CPython legacy methods".into(),
        ));
    }
    if !definition.globals.is_null() {
        return Err(HostError::Module(
            "HPyGlobal definitions are unavailable until milestone H3".into(),
        ));
    }
    if definition.defines.is_null() {
        return Ok(Vec::new());
    }

    let mut methods = Vec::new();
    let mut names = HashSet::new();
    for index in 0..MAX_DEFINITIONS {
        // SAFETY: HPy requires a null-terminated, process-static definition
        // array. The trusted-extension precondition covers each readable slot.
        let raw = unsafe { *definition.defines.add(index) };
        if raw.is_null() {
            return Ok(methods);
        }
        // SAFETY: only the common kind and method-union prefix are read.
        let method_definition = unsafe { &*raw.cast::<HpyMethodDefinition>() };
        if method_definition.kind != HPY_DEF_KIND_METHOD {
            return Err(HostError::Module(format!(
                "definition {index} has unsupported HPyDef kind {}",
                method_definition.kind
            )));
        }
        let method = method_definition.method;
        let name = read_required_utf8(method.name, "method name")?;
        if !names.insert(name.clone()) {
            return Err(HostError::Module(format!("duplicate HPy method '{name}'")));
        }
        if method.implementation.is_null() {
            return Err(HostError::Module(format!(
                "HPy method '{name}' has a null implementation"
            )));
        }
        let signature = match method.signature {
            HPY_FUNC_NOARGS => MethodSignature::NoArgs,
            HPY_FUNC_O => MethodSignature::OneArg,
            HPY_FUNC_VARARGS => MethodSignature::VarArgs,
            HPY_FUNC_KEYWORDS => MethodSignature::Keywords,
            signature => {
                return Err(HostError::Module(format!(
                    "HPy method '{name}' uses unsupported signature {signature}"
                )));
            }
        };
        methods.push(Method {
            name,
            implementation: method.implementation as usize,
            signature,
        });
    }
    Err(HostError::Module(format!(
        "HPy definition list exceeds {MAX_DEFINITIONS} entries or is not null-terminated"
    )))
}

fn read_required_utf8(pointer: *const c_char, field: &str) -> Result<String, HostError> {
    if pointer.is_null() {
        return Err(HostError::Module(format!("HPy {field} is null")));
    }
    // SAFETY: the trusted module-definition contract requires a terminated
    // process-static string for this field.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    let text = std::str::from_utf8(bytes)
        .map_err(|_| HostError::Module(format!("HPy {field} is not UTF-8")))?;
    if text.is_empty() || text.as_bytes().contains(&0) {
        return Err(HostError::Module(format!("HPy {field} is invalid")));
    }
    Ok(text.to_owned())
}

fn invoke_method(
    state: &ModuleState,
    method_index: usize,
    context: &mut Context<'_>,
    arguments: &[Handle],
    keywords: &[(String, Handle)],
) -> TonicResult<Handle> {
    let _lock = state
        .call_lock
        .lock()
        .map_err(|_| Diagnostic::new("RuntimeError", "HPy module call lock is poisoned"))?;
    let method = state
        .methods
        .get(method_index)
        .ok_or_else(|| Diagnostic::new("RuntimeError", "HPy method index is invalid"))?;
    if ACTIVE_CALL.with(|active| !active.get().is_null()) {
        return Err(Diagnostic::new(
            "RuntimeError",
            "nested HPy calls are unavailable before the H5 execution-state boundary",
        ));
    }

    let module = context.native_module(&state.module_name)?;
    let mut call = CallState::new(context, ptr::from_ref(state.context.as_ref()).cast_mut());
    let h_self = call.insert(module)?;
    let h_arguments = arguments
        .iter()
        .copied()
        .map(|argument| call.insert(argument))
        .collect::<TonicResult<Vec<_>>>()?;
    let mut h_arguments = h_arguments;
    h_arguments.extend(
        keywords
            .iter()
            .map(|(_, argument)| call.insert(*argument))
            .collect::<TonicResult<Vec<_>>>()?,
    );
    let h_keyword_names = if keywords.is_empty() {
        Hpy::NULL
    } else {
        let names = keywords
            .iter()
            .map(|(name, _)| {
                // SAFETY: call lifetime keeps the erased context valid.
                unsafe { &mut *call.context }.from_str(name)
            })
            .collect::<TonicResult<Vec<_>>>()?;
        // SAFETY: call lifetime keeps the erased context valid.
        let tuple = unsafe { &mut *call.context }.new_tuple(&names)?;
        call.insert(tuple)?
    };
    let _active = ActiveCallGuard::enter(&mut call)?;
    let result = match method.signature {
        MethodSignature::NoArgs => {
            type Function = unsafe extern "C" fn(*mut HpyContext, Hpy) -> Hpy;
            // SAFETY: module parsing validated the HPy signature tag and the
            // trusted library keeps this method pointer mapped for the process.
            let function: Function = unsafe { std::mem::transmute(method.implementation) };
            // SAFETY: context and self handle remain live for the whole call.
            unsafe { function(ptr::from_ref(state.context.as_ref()).cast_mut(), h_self) }
        }
        MethodSignature::OneArg => {
            type Function = unsafe extern "C" fn(*mut HpyContext, Hpy, Hpy) -> Hpy;
            // SAFETY: same validated signature and pinned-library invariant.
            let function: Function = unsafe { std::mem::transmute(method.implementation) };
            // SAFETY: arity validation guarantees one live argument handle.
            unsafe {
                function(
                    ptr::from_ref(state.context.as_ref()).cast_mut(),
                    h_self,
                    h_arguments[0],
                )
            }
        }
        MethodSignature::VarArgs => {
            type Function = unsafe extern "C" fn(*mut HpyContext, Hpy, *const Hpy, usize) -> Hpy;
            // SAFETY: module parsing validated HPyFunc_VARARGS.
            let function: Function = unsafe { std::mem::transmute(method.implementation) };
            // SAFETY: the argument array remains live and keywords were rejected by the VM.
            unsafe {
                function(
                    ptr::from_ref(state.context.as_ref()).cast_mut(),
                    h_self,
                    h_arguments.as_ptr(),
                    arguments.len(),
                )
            }
        }
        MethodSignature::Keywords => {
            type Function =
                unsafe extern "C" fn(*mut HpyContext, Hpy, *const Hpy, usize, Hpy) -> Hpy;
            // SAFETY: module parsing validated HPyFunc_KEYWORDS.
            let function: Function = unsafe { std::mem::transmute(method.implementation) };
            // SAFETY: positional and keyword arrays remain live for the native call.
            unsafe {
                function(
                    ptr::from_ref(state.context.as_ref()).cast_mut(),
                    h_self,
                    h_arguments.as_ptr(),
                    arguments.len(),
                    h_keyword_names,
                )
            }
        }
    };
    call.finish(result)
}

struct CallState {
    context: *mut Context<'static>,
    hpy_context: *mut HpyContext,
    handles: HashMap<isize, Handle>,
    utf8: HashMap<isize, Box<[u8]>>,
    builders: HashMap<isize, BuilderState>,
    exception: Option<Diagnostic>,
}

impl CallState {
    fn new(context: &mut Context<'_>, hpy_context: *mut HpyContext) -> Self {
        Self {
            context: ptr::from_mut(context).cast::<Context<'static>>(),
            hpy_context,
            handles: HashMap::new(),
            utf8: HashMap::new(),
            builders: HashMap::new(),
            exception: None,
        }
    }

    fn insert(&mut self, handle: Handle) -> TonicResult<Hpy> {
        let token = NEXT_LOCAL_HANDLE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current
                    .checked_add(1)
                    .filter(|next| *next <= isize::MAX as usize)
            })
            .map_err(|_| Diagnostic::new("HandleError", "HPy local token space exhausted"))?;
        let token = token as isize;
        self.handles.insert(token, handle);
        Ok(Hpy { bits: token })
    }

    fn duplicate(&mut self, handle: Hpy) -> TonicResult<Hpy> {
        let handle = self.resolve(handle)?;
        self.insert(handle)
    }

    fn resolve(&mut self, handle: Hpy) -> TonicResult<Handle> {
        if handle == Hpy::NULL {
            return Err(Diagnostic::new("HandleError", "HPy_NULL is not an object"));
        }
        if let Some(slot) = handle.special_slot() {
            // SAFETY: the erased context pointer is valid exactly while the
            // active-call guard is installed.
            let context = unsafe { &mut *self.context };
            return match slot {
                HANDLE_NONE => context.none(),
                HANDLE_TRUE => context.from_bool(true),
                HANDLE_FALSE => context.from_bool(false),
                _ => Err(Diagnostic::new(
                    "TypeError",
                    "HPy context type/exception handle is not a guest value on this surface",
                )),
            };
        }
        self.handles.get(&handle.bits).copied().ok_or_else(|| {
            Diagnostic::new("HandleError", "stale or cross-runtime HPy local handle")
        })
    }

    fn close(&mut self, handle: Hpy) -> TonicResult<()> {
        if handle == Hpy::NULL || handle.special_slot().is_some() {
            return Ok(());
        }
        self.utf8.remove(&handle.bits);
        self.handles
            .remove(&handle.bits)
            .map(|_| ())
            .ok_or_else(|| {
                Diagnostic::new("HandleError", "stale or cross-runtime HPy local handle")
            })
    }

    fn fail<T>(&mut self, error: Diagnostic, fallback: T) -> T {
        if self.exception.is_none() {
            self.exception = Some(error);
        }
        fallback
    }

    fn new_builder(&mut self, size: isize, kind: BuilderKind) -> TonicResult<HpyBuilder> {
        let size = usize::try_from(size)
            .map_err(|_| Diagnostic::new("ValueError", "HPy builder size cannot be negative"))?;
        if size > MAX_CONTAINER_ITEMS {
            return Err(Diagnostic::new(
                "OverflowError",
                "HPy builder exceeds the container item limit",
            ));
        }
        let token = NEXT_BUILDER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current
                    .checked_add(1)
                    .filter(|next| *next <= isize::MAX as usize)
            })
            .map_err(|_| Diagnostic::new("HandleError", "HPy builder token space exhausted"))?
            as isize;
        self.builders.insert(
            token,
            BuilderState {
                kind,
                items: vec![None; size],
            },
        );
        Ok(HpyBuilder { bits: token })
    }

    fn set_builder_item(
        &mut self,
        builder: HpyBuilder,
        index: isize,
        item: Hpy,
        kind: BuilderKind,
    ) -> TonicResult<()> {
        let item = self.resolve(item)?;
        let index = usize::try_from(index)
            .map_err(|_| Diagnostic::new("IndexError", "HPy builder index is negative"))?;
        let state = self
            .builders
            .get_mut(&builder.bits)
            .ok_or_else(|| Diagnostic::new("HandleError", "stale HPy builder"))?;
        if state.kind != kind {
            return Err(Diagnostic::new("HandleError", "HPy builder kind mismatch"));
        }
        let slot = state
            .items
            .get_mut(index)
            .ok_or_else(|| Diagnostic::new("IndexError", "HPy builder index out of range"))?;
        *slot = Some(item);
        Ok(())
    }

    fn take_builder(&mut self, builder: HpyBuilder, kind: BuilderKind) -> TonicResult<Vec<Handle>> {
        let state = self
            .builders
            .remove(&builder.bits)
            .ok_or_else(|| Diagnostic::new("HandleError", "stale HPy builder"))?;
        if state.kind != kind {
            return Err(Diagnostic::new("HandleError", "HPy builder kind mismatch"));
        }
        state
            .items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                item.ok_or_else(|| {
                    Diagnostic::new(
                        "SystemError",
                        format!("HPy builder item {index} was not initialized"),
                    )
                })
            })
            .collect()
    }

    fn finish(&mut self, result: Hpy) -> TonicResult<Handle> {
        if !self.builders.is_empty() && self.exception.is_none() {
            return Err(Diagnostic::new(
                "HandleError",
                "HPy method leaked an unfinished builder",
            ));
        }
        match (result == Hpy::NULL, self.exception.take()) {
            (true, Some(error)) => Err(error),
            (true, None) => Err(Diagnostic::new(
                "SystemError",
                "HPy method returned HPy_NULL without setting an exception",
            )),
            (false, Some(error)) => Err(Diagnostic::new(
                "SystemError",
                format!(
                    "HPy method returned a result with {} set: {}",
                    error.kind, error.message
                ),
            )),
            (false, None) => self.resolve(result),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BuilderKind {
    List,
    Tuple,
}

struct BuilderState {
    kind: BuilderKind,
    items: Vec<Option<Handle>>,
}

thread_local! {
    static ACTIVE_CALL: Cell<*mut CallState> = const { Cell::new(ptr::null_mut()) };
}

struct ActiveCallGuard;

impl ActiveCallGuard {
    fn enter(call: &mut CallState) -> TonicResult<Self> {
        ACTIVE_CALL.with(|active| {
            if !active.get().is_null() {
                return Err(Diagnostic::new(
                    "RuntimeError",
                    "nested HPy call state is unavailable",
                ));
            }
            active.set(ptr::from_mut(call));
            Ok(Self)
        })
    }
}

impl Drop for ActiveCallGuard {
    fn drop(&mut self) {
        ACTIVE_CALL.with(|active| active.set(ptr::null_mut()));
    }
}

unsafe fn active_call(context: *mut HpyContext) -> Option<&'static mut CallState> {
    ACTIVE_CALL.with(|active| {
        let call = active.get();
        if call.is_null() {
            return None;
        }
        // SAFETY: `ActiveCallGuard` bounds this pointer to the native call and
        // serialized C execution prevents aliasing mutable access.
        let call = unsafe { &mut *call };
        if call.hpy_context != context {
            call.fail(
                Diagnostic::new("RuntimeError", "HPy callback used the wrong context"),
                (),
            );
            return None;
        }
        Some(call)
    })
}

unsafe extern "C" fn hpy_dup(context: *mut HpyContext, handle: Hpy) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    match call.duplicate(handle) {
        Ok(handle) => handle,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_close(context: *mut HpyContext, handle: Hpy) {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return;
    };
    if let Err(error) = call.close(handle) {
        call.fail(error, ());
    }
}

unsafe extern "C" fn hpy_long_from_i64(context: *mut HpyContext, value: i64) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }.from_i64(value);
    match result.and_then(|handle| call.insert(handle)) {
        Ok(handle) => handle,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_long_as_i64(context: *mut HpyContext, handle: Hpy) -> i64 {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    let result = call.resolve(handle).and_then(|handle| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &*call.context }.to_i64(handle)
    });
    match result {
        Ok(value) => value,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_unicode_from_string(
    context: *mut HpyContext,
    value: *const c_char,
) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    if value.is_null() {
        return call.fail(
            Diagnostic::new("SystemError", "HPyUnicode_FromString received null"),
            Hpy::NULL,
        );
    }
    // SAFETY: HPy requires a readable NUL-terminated UTF-8 input.
    let value = unsafe { CStr::from_ptr(value) };
    let Ok(value) = value.to_str() else {
        return call.fail(
            Diagnostic::new("UnicodeDecodeError", "HPy string input is not UTF-8"),
            Hpy::NULL,
        );
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }.from_str(value);
    match result.and_then(|handle| call.insert(handle)) {
        Ok(handle) => handle,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_unicode_as_utf8_and_size(
    context: *mut HpyContext,
    handle: Hpy,
    size: *mut isize,
) -> *const c_char {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return ptr::null();
    };
    let result = call.resolve(handle).and_then(|resolved| {
        // SAFETY: active-call lifetime keeps the erased context valid. Copying
        // the bytes ends the Rust string borrow before mutating the cache.
        let bytes = unsafe { &*call.context }
            .as_str(resolved)?
            .as_bytes()
            .to_vec();
        isize::try_from(bytes.len())
            .map(|length| (bytes, length))
            .map_err(|_| Diagnostic::new("OverflowError", "HPy UTF-8 length exceeds isize"))
    });
    let (mut bytes, length) = match result {
        Ok(result) => result,
        Err(error) => return call.fail(error, ptr::null()),
    };
    bytes.push(0);
    let bytes = bytes.into_boxed_slice();
    let pointer = bytes.as_ptr().cast();
    call.utf8.insert(handle.bits, bytes);
    if !size.is_null() {
        // SAFETY: HPy declares `size` as an optional writable output pointer.
        unsafe { size.write(length) };
    }
    pointer
}

fn resolve_name(call: &mut CallState, name: Hpy) -> TonicResult<String> {
    let name = call.resolve(name)?;
    // SAFETY: active-call lifetime keeps the erased context valid.
    Ok(unsafe { &*call.context }.as_str(name)?.to_owned())
}

fn read_call_string(pointer: *const c_char, field: &str) -> TonicResult<String> {
    if pointer.is_null() {
        return Err(Diagnostic::new(
            "SystemError",
            format!("HPy {field} received null text"),
        ));
    }
    // SAFETY: HPy requires a readable NUL-terminated string.
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| Diagnostic::new("UnicodeDecodeError", format!("HPy {field} is not UTF-8")))
}

unsafe extern "C" fn hpy_length(context: *mut HpyContext, owner: Hpy) -> isize {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    let result = call.resolve(owner).and_then(|owner| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &mut *call.context }.length(owner)
    });
    match result {
        Ok(length) => length,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_callable_check(context: *mut HpyContext, value: Hpy) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return 0;
    };
    let result = call.resolve(value).and_then(|value| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &*call.context }.is_callable(value)
    });
    match result {
        Ok(value) => c_int::from(value),
        Err(error) => call.fail(error, 0),
    }
}

fn get_attr_impl(call: &mut CallState, owner: Hpy, name: String) -> TonicResult<Hpy> {
    let owner = call.resolve(owner)?;
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }.get_attr(owner, &name)?;
    call.insert(result)
}

unsafe extern "C" fn hpy_get_attr(context: *mut HpyContext, owner: Hpy, name: Hpy) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let result = resolve_name(call, name).and_then(|name| get_attr_impl(call, owner, name));
    match result {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_get_attr_s(
    context: *mut HpyContext,
    owner: Hpy,
    name: *const c_char,
) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let result =
        read_call_string(name, "GetAttr_s name").and_then(|name| get_attr_impl(call, owner, name));
    match result {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

fn has_attr_result(call: &mut CallState, result: TonicResult<Hpy>) -> c_int {
    match result {
        Ok(_) => 1,
        Err(error) if error.kind == "AttributeError" => 0,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_has_attr(context: *mut HpyContext, owner: Hpy, name: Hpy) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return 0;
    };
    let result = resolve_name(call, name).and_then(|name| get_attr_impl(call, owner, name));
    has_attr_result(call, result)
}

unsafe extern "C" fn hpy_has_attr_s(
    context: *mut HpyContext,
    owner: Hpy,
    name: *const c_char,
) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return 0;
    };
    let result =
        read_call_string(name, "HasAttr_s name").and_then(|name| get_attr_impl(call, owner, name));
    has_attr_result(call, result)
}

fn set_attr_impl(call: &mut CallState, owner: Hpy, name: String, value: Hpy) -> TonicResult<()> {
    let owner = call.resolve(owner)?;
    let value = call.resolve(value)?;
    // SAFETY: active-call lifetime keeps the erased context valid.
    unsafe { &mut *call.context }.set_attr(owner, &name, value)
}

unsafe extern "C" fn hpy_set_attr(
    context: *mut HpyContext,
    owner: Hpy,
    name: Hpy,
    value: Hpy,
) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    let result = resolve_name(call, name).and_then(|name| set_attr_impl(call, owner, name, value));
    match result {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_set_attr_s(
    context: *mut HpyContext,
    owner: Hpy,
    name: *const c_char,
    value: Hpy,
) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    let result = read_call_string(name, "SetAttr_s name")
        .and_then(|name| set_attr_impl(call, owner, name, value));
    match result {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

fn get_item_impl(call: &mut CallState, owner: Hpy, key: Hpy) -> TonicResult<Hpy> {
    let owner = call.resolve(owner)?;
    let key = call.resolve(key)?;
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }.get_item(owner, key)?;
    call.insert(result)
}

unsafe extern "C" fn hpy_get_item(context: *mut HpyContext, owner: Hpy, key: Hpy) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    match get_item_impl(call, owner, key) {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_get_item_i(context: *mut HpyContext, owner: Hpy, index: isize) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }
        .from_i64(index as i64)
        .and_then(|key| call.insert(key))
        .and_then(|key| get_item_impl(call, owner, key));
    match result {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_get_item_s(
    context: *mut HpyContext,
    owner: Hpy,
    key: *const c_char,
) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let result = read_call_string(key, "GetItem_s key").and_then(|key| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &mut *call.context }
            .from_str(&key)
            .and_then(|key| call.insert(key))
            .and_then(|key| get_item_impl(call, owner, key))
    });
    match result {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_contains(context: *mut HpyContext, container: Hpy, key: Hpy) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    let result = call
        .resolve(container)
        .and_then(|container| call.resolve(key).map(|key| (container, key)))
        .and_then(|(container, key)| {
            // SAFETY: active-call lifetime keeps the erased context valid.
            unsafe { &mut *call.context }.contains(container, key)
        });
    match result {
        Ok(value) => c_int::from(value),
        Err(error) => call.fail(error, -1),
    }
}

fn set_item_impl(call: &mut CallState, owner: Hpy, key: Hpy, value: Hpy) -> TonicResult<()> {
    let owner = call.resolve(owner)?;
    let key = call.resolve(key)?;
    let value = call.resolve(value)?;
    // SAFETY: active-call lifetime keeps the erased context valid.
    unsafe { &mut *call.context }.set_item(owner, key, value)
}

unsafe extern "C" fn hpy_set_item(
    context: *mut HpyContext,
    owner: Hpy,
    key: Hpy,
    value: Hpy,
) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    match set_item_impl(call, owner, key, value) {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_set_item_i(
    context: *mut HpyContext,
    owner: Hpy,
    index: isize,
    value: Hpy,
) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }
        .from_i64(index as i64)
        .and_then(|key| call.insert(key))
        .and_then(|key| set_item_impl(call, owner, key, value));
    match result {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_set_item_s(
    context: *mut HpyContext,
    owner: Hpy,
    key: *const c_char,
    value: Hpy,
) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    let result = read_call_string(key, "SetItem_s key").and_then(|key| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &mut *call.context }
            .from_str(&key)
            .and_then(|key| call.insert(key))
            .and_then(|key| set_item_impl(call, owner, key, value))
    });
    match result {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

fn del_item_impl(call: &mut CallState, owner: Hpy, key: Hpy) -> TonicResult<()> {
    let owner = call.resolve(owner)?;
    let key = call.resolve(key)?;
    // SAFETY: active-call lifetime keeps the erased context valid.
    unsafe { &mut *call.context }.delete_item(owner, key)
}

unsafe extern "C" fn hpy_del_item(context: *mut HpyContext, owner: Hpy, key: Hpy) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    match del_item_impl(call, owner, key) {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_del_item_i(context: *mut HpyContext, owner: Hpy, index: isize) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }
        .from_i64(index as i64)
        .and_then(|key| call.insert(key))
        .and_then(|key| del_item_impl(call, owner, key));
    match result {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_del_item_s(
    context: *mut HpyContext,
    owner: Hpy,
    key: *const c_char,
) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    let result = read_call_string(key, "DelItem_s key").and_then(|key| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &mut *call.context }
            .from_str(&key)
            .and_then(|key| call.insert(key))
            .and_then(|key| del_item_impl(call, owner, key))
    });
    match result {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_repr(context: *mut HpyContext, value: Hpy) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let result = call.resolve(value).and_then(|value| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &mut *call.context }.repr(value)
    });
    match result.and_then(|value| call.insert(value)) {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

fn sequence_handles(call: &mut CallState, sequence: Hpy) -> TonicResult<Vec<Handle>> {
    let sequence = call.resolve(sequence)?;
    // SAFETY: active-call lifetime keeps the erased context valid.
    let context = unsafe { &mut *call.context };
    if !context.is_tuple(sequence)? {
        return Err(Diagnostic::new(
            "TypeError",
            "HPy call arguments must be a tuple",
        ));
    }
    let length = context.sequence_len(sequence)?;
    if length > MAX_CONTAINER_ITEMS {
        return Err(Diagnostic::new(
            "OverflowError",
            "HPy argument tuple exceeds the item limit",
        ));
    }
    (0..length)
        .map(|index| context.sequence_get(sequence, index))
        .collect()
}

unsafe extern "C" fn hpy_call_tuple_dict(
    context: *mut HpyContext,
    callable: Hpy,
    arguments: Hpy,
    keywords: Hpy,
) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let result = (|| {
        let callable = call.resolve(callable)?;
        let arguments = sequence_handles(call, arguments)?;
        // SAFETY: active-call lifetime keeps the erased context valid.
        let context = unsafe { &mut *call.context };
        let result = if keywords == Hpy::NULL {
            context.call(callable, &arguments)?
        } else {
            let keywords = call.resolve(keywords)?;
            context.call_with_keywords(callable, &arguments, keywords)?
        };
        call.insert(result)
    })();
    match result {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

fn vector_arguments(
    call: &mut CallState,
    arguments: *const Hpy,
    positional_count: usize,
    keyword_names: Hpy,
) -> TonicResult<VectorArguments> {
    let keyword_count = if keyword_names == Hpy::NULL {
        0
    } else {
        let names = call.resolve(keyword_names)?;
        // SAFETY: active-call lifetime keeps the erased context valid.
        let context = unsafe { &mut *call.context };
        if !context.is_tuple(names)? {
            return Err(Diagnostic::new("TypeError", "HPy kwnames must be a tuple"));
        }
        context.sequence_len(names)?
    };
    let total = positional_count
        .checked_add(keyword_count)
        .filter(|total| *total <= MAX_CONTAINER_ITEMS)
        .ok_or_else(|| Diagnostic::new("OverflowError", "HPy call argument count is too large"))?;
    if arguments.is_null() && total != 0 {
        return Err(Diagnostic::new(
            "SystemError",
            "HPy call argument array is null",
        ));
    }
    let raw = if total == 0 {
        &[]
    } else {
        // SAFETY: HPy requires `arguments` to span positional plus keyword values.
        unsafe { std::slice::from_raw_parts(arguments, total) }
    };
    let positional = raw[..positional_count]
        .iter()
        .copied()
        .map(|value| call.resolve(value))
        .collect::<TonicResult<Vec<_>>>()?;
    let keywords = if keyword_count == 0 {
        Vec::new()
    } else {
        let names = call.resolve(keyword_names)?;
        let mut keywords = Vec::with_capacity(keyword_count);
        for index in 0..keyword_count {
            // SAFETY: active-call lifetime keeps the erased context valid.
            let name = unsafe { &mut *call.context }.sequence_get(names, index)?;
            // SAFETY: active-call lifetime keeps the erased context valid.
            let name = unsafe { &*call.context }.as_str(name)?.to_owned();
            let value = call.resolve(raw[positional_count + index])?;
            keywords.push((name, value));
        }
        keywords
    };
    Ok((positional, keywords))
}

unsafe extern "C" fn hpy_call(
    context: *mut HpyContext,
    callable: Hpy,
    arguments: *const Hpy,
    positional_count: usize,
    keyword_names: Hpy,
) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let result = (|| {
        let callable = call.resolve(callable)?;
        let (positional, keywords) =
            vector_arguments(call, arguments, positional_count, keyword_names)?;
        // SAFETY: active-call lifetime keeps the erased context valid.
        let result =
            unsafe { &mut *call.context }.call_with_named(callable, &positional, &keywords)?;
        call.insert(result)
    })();
    match result {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_call_method(
    context: *mut HpyContext,
    name: Hpy,
    arguments: *const Hpy,
    positional_count: usize,
    keyword_names: Hpy,
) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let result = (|| {
        let name = resolve_name(call, name)?;
        let (mut positional, keywords) =
            vector_arguments(call, arguments, positional_count, keyword_names)?;
        if positional.is_empty() {
            return Err(Diagnostic::new(
                "TypeError",
                "HPy_CallMethod requires the receiver as argument zero",
            ));
        }
        let receiver = positional.remove(0);
        // SAFETY: active-call lifetime keeps the erased context valid.
        let method = unsafe { &mut *call.context }.get_attr(receiver, &name)?;
        // SAFETY: active-call lifetime keeps the erased context valid.
        let result =
            unsafe { &mut *call.context }.call_with_named(method, &positional, &keywords)?;
        call.insert(result)
    })();
    match result {
        Ok(value) => value,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_err_set_string(
    context: *mut HpyContext,
    exception_type: Hpy,
    message: *const c_char,
) {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return;
    };
    let Some(slot) = exception_type.special_slot() else {
        call.fail(
            Diagnostic::new("SystemError", "HPyErr_SetString requires an exception type"),
            (),
        );
        return;
    };
    if !(FIRST_EXCEPTION..=LAST_EXCEPTION).contains(&slot) {
        call.fail(
            Diagnostic::new("SystemError", "HPyErr_SetString requires an exception type"),
            (),
        );
        return;
    }
    if message.is_null() {
        call.fail(
            Diagnostic::new("SystemError", "HPyErr_SetString received null text"),
            (),
        );
        return;
    }
    // SAFETY: HPy requires a readable NUL-terminated message.
    let message = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    call.exception = Some(Diagnostic::new(exception_name(slot), message));
}

unsafe extern "C" fn hpy_err_occurred(context: *mut HpyContext) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    unsafe { active_call(context) }
        .is_some_and(|call| call.exception.is_some())
        .into()
}

unsafe extern "C" fn hpy_err_clear(context: *mut HpyContext) {
    // SAFETY: the extension must pass the active context it received.
    if let Some(call) = unsafe { active_call(context) } {
        call.exception = None;
    }
}

unsafe extern "C" fn hpy_list_check(context: *mut HpyContext, handle: Hpy) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return 0;
    };
    let result = call.resolve(handle).and_then(|handle| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &*call.context }.is_list(handle)
    });
    match result {
        Ok(value) => c_int::from(value),
        Err(error) => call.fail(error, 0),
    }
}

unsafe extern "C" fn hpy_list_new(context: *mut HpyContext, size: isize) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    if size != 0 {
        return call.fail(
            Diagnostic::new(
                "NotImplementedError",
                "HPyList_New currently requires size 0; use HPyListBuilder",
            ),
            Hpy::NULL,
        );
    }
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }.new_list();
    match result.and_then(|handle| call.insert(handle)) {
        Ok(handle) => handle,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_list_append(context: *mut HpyContext, list: Hpy, item: Hpy) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return -1;
    };
    let result = call
        .resolve(list)
        .and_then(|list| call.resolve(item).map(|item| (list, item)))
        .and_then(|(list, item)| {
            // SAFETY: active-call lifetime keeps the erased context valid.
            unsafe { &mut *call.context }.list_append(list, item)
        });
    match result {
        Ok(()) => 0,
        Err(error) => call.fail(error, -1),
    }
}

unsafe extern "C" fn hpy_dict_check(context: *mut HpyContext, handle: Hpy) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return 0;
    };
    let result = call.resolve(handle).and_then(|handle| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &*call.context }.is_dict(handle)
    });
    match result {
        Ok(value) => c_int::from(value),
        Err(error) => call.fail(error, 0),
    }
}

unsafe extern "C" fn hpy_dict_new(context: *mut HpyContext) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }.new_dict();
    match result.and_then(|handle| call.insert(handle)) {
        Ok(handle) => handle,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_tuple_check(context: *mut HpyContext, handle: Hpy) -> c_int {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return 0;
    };
    let result = call.resolve(handle).and_then(|handle| {
        // SAFETY: active-call lifetime keeps the erased context valid.
        unsafe { &*call.context }.is_tuple(handle)
    });
    match result {
        Ok(value) => c_int::from(value),
        Err(error) => call.fail(error, 0),
    }
}

unsafe extern "C" fn hpy_tuple_from_array(
    context: *mut HpyContext,
    items: *const Hpy,
    count: isize,
) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let count = match usize::try_from(count) {
        Ok(count) if count <= MAX_CONTAINER_ITEMS => count,
        _ => {
            return call.fail(
                Diagnostic::new("ValueError", "invalid HPy tuple item count"),
                Hpy::NULL,
            )
        }
    };
    if items.is_null() && count != 0 {
        return call.fail(
            Diagnostic::new("SystemError", "HPy tuple item array is null"),
            Hpy::NULL,
        );
    }
    let items = if count == 0 {
        &[]
    } else {
        // SAFETY: HPy requires `items` to reference `count` readable handles.
        unsafe { std::slice::from_raw_parts(items, count) }
    };
    let items = match items
        .iter()
        .copied()
        .map(|item| call.resolve(item))
        .collect::<TonicResult<Vec<_>>>()
    {
        Ok(items) => items,
        Err(error) => return call.fail(error, Hpy::NULL),
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }.new_tuple(&items);
    match result.and_then(|handle| call.insert(handle)) {
        Ok(handle) => handle,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_list_builder_new(context: *mut HpyContext, size: isize) -> HpyBuilder {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return HpyBuilder { bits: 0 };
    };
    match call.new_builder(size, BuilderKind::List) {
        Ok(builder) => builder,
        Err(error) => call.fail(error, HpyBuilder { bits: 0 }),
    }
}

unsafe extern "C" fn hpy_list_builder_set(
    context: *mut HpyContext,
    builder: HpyBuilder,
    index: isize,
    item: Hpy,
) {
    // SAFETY: the extension must pass the active context it received.
    if let Some(call) = unsafe { active_call(context) } {
        if let Err(error) = call.set_builder_item(builder, index, item, BuilderKind::List) {
            call.fail(error, ());
        }
    }
}

unsafe extern "C" fn hpy_list_builder_build(context: *mut HpyContext, builder: HpyBuilder) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let items = match call.take_builder(builder, BuilderKind::List) {
        Ok(items) => items,
        Err(error) => return call.fail(error, Hpy::NULL),
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let context = unsafe { &mut *call.context };
    let list = match context.new_list() {
        Ok(list) => list,
        Err(error) => return call.fail(error, Hpy::NULL),
    };
    for item in items {
        if let Err(error) = context.list_append(list, item) {
            return call.fail(error, Hpy::NULL);
        }
    }
    match call.insert(list) {
        Ok(handle) => handle,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_list_builder_cancel(context: *mut HpyContext, builder: HpyBuilder) {
    // SAFETY: the extension must pass the active context it received.
    if let Some(call) = unsafe { active_call(context) } {
        if call.builders.remove(&builder.bits).is_none() {
            call.fail(Diagnostic::new("HandleError", "stale HPy builder"), ());
        }
    }
}

unsafe extern "C" fn hpy_tuple_builder_new(context: *mut HpyContext, size: isize) -> HpyBuilder {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return HpyBuilder { bits: 0 };
    };
    match call.new_builder(size, BuilderKind::Tuple) {
        Ok(builder) => builder,
        Err(error) => call.fail(error, HpyBuilder { bits: 0 }),
    }
}

unsafe extern "C" fn hpy_tuple_builder_set(
    context: *mut HpyContext,
    builder: HpyBuilder,
    index: isize,
    item: Hpy,
) {
    // SAFETY: the extension must pass the active context it received.
    if let Some(call) = unsafe { active_call(context) } {
        if let Err(error) = call.set_builder_item(builder, index, item, BuilderKind::Tuple) {
            call.fail(error, ());
        }
    }
}

unsafe extern "C" fn hpy_tuple_builder_build(context: *mut HpyContext, builder: HpyBuilder) -> Hpy {
    // SAFETY: the extension must pass the active context it received.
    let Some(call) = (unsafe { active_call(context) }) else {
        return Hpy::NULL;
    };
    let items = match call.take_builder(builder, BuilderKind::Tuple) {
        Ok(items) => items,
        Err(error) => return call.fail(error, Hpy::NULL),
    };
    // SAFETY: active-call lifetime keeps the erased context valid.
    let result = unsafe { &mut *call.context }.new_tuple(&items);
    match result.and_then(|handle| call.insert(handle)) {
        Ok(handle) => handle,
        Err(error) => call.fail(error, Hpy::NULL),
    }
}

unsafe extern "C" fn hpy_tuple_builder_cancel(context: *mut HpyContext, builder: HpyBuilder) {
    // SAFETY: the extension must pass the active context it received.
    if let Some(call) = unsafe { active_call(context) } {
        if call.builders.remove(&builder.bits).is_none() {
            call.fail(Diagnostic::new("HandleError", "stale HPy builder"), ());
        }
    }
}

fn exception_name(slot: usize) -> &'static str {
    match slot {
        5 => "BaseException",
        6 => "Exception",
        13 => "AttributeError",
        14 => "BufferError",
        18 => "ImportError",
        19 => "ModuleNotFoundError",
        20 => "IndexError",
        21 => "KeyError",
        23 => "MemoryError",
        24 => "NameError",
        25 => "OverflowError",
        26 => "RuntimeError",
        27 => "RecursionError",
        28 => "NotImplementedError",
        29 => "SyntaxError",
        33 => "SystemError",
        35 => "TypeError",
        37 => "UnicodeError",
        38 => "UnicodeEncodeError",
        39 => "UnicodeDecodeError",
        40 => "UnicodeTranslateError",
        41 => "ValueError",
        42 => "ZeroDivisionError",
        _ => "Exception",
    }
}

#[derive(Debug)]
pub enum HostError {
    Load(LoadError),
    Module(String),
    Runtime(Diagnostic),
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(error) => error.fmt(formatter),
            Self::Module(message) => write!(formatter, "invalid HPy module: {message}"),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl Error for HostError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::Module(_) => None,
        }
    }
}

impl From<LoadError> for HostError {
    fn from(error: LoadError) -> Self {
        Self::Load(error)
    }
}

impl From<Diagnostic> for HostError {
    fn from(error: Diagnostic) -> Self {
        Self::Runtime(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_context_layout_matches_the_pinned_word_table() {
        assert_eq!(
            std::mem::offset_of!(HpyContext, slots),
            std::mem::size_of::<*const c_char>()
                + std::mem::size_of::<*mut c_void>()
                + std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::size_of::<HpyContext>(),
            std::mem::offset_of!(HpyContext, slots)
                + usize::from(UNIVERSAL_ABI.context_slot_count) * std::mem::size_of::<usize>()
        );
        assert_eq!(Hpy::special(35).special_slot(), Some(35));
    }
}
