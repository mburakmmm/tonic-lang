#![deny(unsafe_op_in_unsafe_fn)]

#[allow(unsafe_code)]
mod ffi;

use std::{
    cell::Cell,
    collections::{HashMap, HashSet, VecDeque},
    ffi::{c_void, CString},
    mem, ptr,
    sync::{Mutex, OnceLock},
};
use tonic_core::diagnostic::{Diagnostic, Result};
use tonic_runtime::{
    c_api::{
        negotiate_api, CAP_CONTAINER_ACCESS_V1, CAP_CROSS_COLLECTOR_V1, CAP_FOREIGN_OBJECT_V1,
        CAP_PERSISTENT_HANDLES_V1, CAP_PROTOCOL_ACCESS_V1,
    },
    TonicContext, TonicExceptionKind, TonicForeignVTable, TonicHandle, TonicPersistentHandle,
    TonicRuntimeOwner, TonicStatus, TonicValueKind, Vm, FOREIGN_OWNED, TONIC_ABI_VERSION,
};

/// Stable identity for CPython-owned objects wrapped by Tonic.
pub const CPYTHON_ADAPTER_ID: u64 = 0x4350_5954_484f_4e01;
/// Stable adapter identity for callable Tonic proxies stored as Python objects.
pub const CPYTHON_PROXY_ADAPTER_ID: u64 = 0x4350_5954_5052_5801;
static INITIALIZE: Mutex<()> = Mutex::new(());
static API: OnceLock<&'static tonic_runtime::TonicApi> = OnceLock::new();
static PROXY_TYPE: OnceLock<usize> = OnceLock::new();
static PROXY_CACHE: OnceLock<Mutex<Vec<usize>>> = OnceLock::new();
static FOREIGN_ROOTS: OnceLock<Mutex<Vec<usize>>> = OnceLock::new();
const PROXY_TYPE_NAME: &[u8] = b"tonic.PyTonicProxy\0";
const GRAPH_NODE_LIMIT: usize = 4_096;
const GRAPH_EDGE_LIMIT: usize = 16_384;

thread_local! {
    static ACTIVE_CONTEXT: Cell<*mut TonicContext> = const { Cell::new(ptr::null_mut()) };
}

struct ProxyPayload {
    handle: TonicPersistentHandle,
    foreign_reference: Option<TonicHandle>,
    owner: *mut TonicRuntimeOwner,
    execution_id: u64,
    runtime_id: u64,
    strong: bool,
    tonic_wrappers: usize,
    closed: bool,
}

struct ForeignPyPayload {
    object: *mut ffi::PyObject,
    runtime_id: u64,
}

struct ActiveContextGuard(*mut TonicContext);

impl ActiveContextGuard {
    fn enter(context: *mut TonicContext) -> Self {
        let previous = ACTIVE_CONTEXT.replace(context);
        Self(previous)
    }
}

impl Drop for ActiveContextGuard {
    fn drop(&mut self) {
        ACTIVE_CONTEXT.set(self.0);
    }
}

struct GilGuard(ffi::PyGilState);

impl Drop for GilGuard {
    fn drop(&mut self) {
        // SAFETY: the state token came from PyGILState_Ensure on this thread.
        unsafe { ffi::PyGILState_Release(self.0) }
    }
}

fn enter_python() -> GilGuard {
    let lock = INITIALIZE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: initialization is serialized and CPython documents repeated state
    // inspection. Tonic deliberately keeps the embedded interpreter alive.
    unsafe {
        if ffi::Py_IsInitialized() == 0 {
            ffi::Py_Initialize();
            // Py_Initialize leaves this thread holding CPython execution state.
            // SaveThread releases it so later bridge calls and test threads can
            // acquire it independently through PyGILState_Ensure.
            let _main_state = ffi::PyEval_SaveThread();
        }
    }
    drop(lock);
    // SAFETY: CPython is initialized; the returned state is released by guard.
    GilGuard(unsafe { ffi::PyGILState_Ensure() })
}

fn python_error(operation: &'static str) -> Diagnostic {
    let mut detail = None;
    // SAFETY: caller holds CPython execution state. GetRaisedException transfers
    // the active exception reference and clears CPython's indicator.
    let exception = unsafe { ffi::PyErr_GetRaisedException() };
    if !exception.is_null() {
        let traceback_module = CString::new("traceback").expect("literal has no NUL");
        let format_name = CString::new("format_exception").expect("literal has no NUL");
        let module = unsafe { ffi::PyImport_ImportModule(traceback_module.as_ptr()) };
        let formatter = if module.is_null() {
            ptr::null_mut()
        } else {
            unsafe { ffi::PyObject_GetAttrString(module, format_name.as_ptr()) }
        };
        let lines = if formatter.is_null() {
            ptr::null_mut()
        } else {
            unsafe { ffi::PyObject_CallOneArg(formatter, exception) }
        };
        let separator = unsafe { ffi::PyUnicode_FromStringAndSize(ptr::null(), 0) };
        let formatted = if lines.is_null() || separator.is_null() {
            ptr::null_mut()
        } else {
            unsafe { ffi::PyUnicode_Join(separator, lines) }
        };
        if !separator.is_null() {
            unsafe { ffi::Py_DecRef(separator) };
        }
        if !lines.is_null() {
            unsafe { ffi::Py_DecRef(lines) };
        }
        if !formatter.is_null() {
            unsafe { ffi::Py_DecRef(formatter) };
        }
        if !module.is_null() {
            unsafe { ffi::Py_DecRef(module) };
        }
        // Formatting failure must not hide the original exception.
        unsafe { ffi::PyErr_Clear() };
        let display = if formatted.is_null() {
            unsafe { ffi::PyObject_Str(exception) }
        } else {
            formatted
        };
        if !display.is_null() {
            let mut size = 0;
            // SAFETY: display is Unicode for ordinary exception formatting.
            let bytes = unsafe { ffi::PyUnicode_AsUTF8AndSize(display, &mut size) };
            if !bytes.is_null() && size >= 0 {
                // SAFETY: CPython owns size readable UTF-8 bytes until DECREF.
                let slice = unsafe { std::slice::from_raw_parts(bytes.cast(), size as usize) };
                detail = Some(String::from_utf8_lossy(slice).into_owned());
            }
            unsafe { ffi::Py_DecRef(display) };
        }
        unsafe { ffi::Py_DecRef(exception) };
    }
    // Formatting itself can fail; never leak that secondary error.
    unsafe { ffi::PyErr_Clear() };
    let message = detail.map_or_else(
        || format!("CPython operation failed: {operation}"),
        |detail| format!("CPython operation failed: {operation}: {detail}"),
    );
    Diagnostic::new("PythonError", message)
}

unsafe fn release_proxy_payload(payload: *mut ProxyPayload) {
    // SAFETY: every caller transfers the sole Box allocation exactly once.
    let payload = unsafe { Box::from_raw(payload) };
    // The stable owner routes release to the correct VM without retaining a VM
    // address. A dead runtime rejects the queue operation; its handle table is
    // already gone, so there is nothing left to release.
    if payload.strong {
        let _ = unsafe { (api().persistent_release_deferred)(payload.owner, payload.handle) };
    }
    if let Some(reference) = payload.foreign_reference {
        let _ = unsafe { (api().foreign_reference_release_deferred)(payload.owner, reference) };
    }
    let _ = unsafe { (api().runtime_owner_release)(payload.owner) };
}

fn foreign_roots() -> &'static Mutex<Vec<usize>> {
    FOREIGN_ROOTS.get_or_init(|| Mutex::new(Vec::new()))
}

unsafe fn foreign_pyobject(payload: *mut c_void) -> *mut ffi::PyObject {
    if payload.is_null() {
        ptr::null_mut()
    } else {
        // SAFETY: CPYTHON_ADAPTER_ID payloads are always ForeignPyPayload boxes.
        unsafe { (*payload.cast::<ForeignPyPayload>()).object }
    }
}

fn proxy_cache() -> &'static Mutex<Vec<usize>> {
    PROXY_CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

unsafe fn proxy_payload(object: *mut ffi::PyObject) -> *mut ProxyPayload {
    let Some(proxy_type) = PROXY_TYPE.get().copied() else {
        return ptr::null_mut();
    };
    // SAFETY: every callback is installed only on this heap type. CPython 3.12's
    // negative basicsize API returns its private trailing storage without relying
    // on PyObject_HEAD layout.
    let data = unsafe { ffi::PyObject_GetTypeData(object, proxy_type as *mut ffi::PyObject) };
    if data.is_null() {
        ptr::null_mut()
    } else {
        unsafe { data.cast::<*mut ProxyPayload>().read() }
    }
}

unsafe fn prune_proxy_cache(removing: *mut ffi::PyObject) {
    let mut cache = proxy_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|object| *object != removing as usize);
}

unsafe extern "C" fn proxy_dealloc(object: *mut ffi::PyObject) {
    unsafe { prune_proxy_cache(object) };
    let proxy_type = *PROXY_TYPE.get().expect("proxy type initialized") as *mut ffi::PyObject;
    let payload = unsafe { proxy_payload(object) };
    if !payload.is_null() {
        let data = unsafe { ffi::PyObject_GetTypeData(object, proxy_type) };
        unsafe { data.cast::<*mut ProxyPayload>().write(ptr::null_mut()) };
        unsafe { release_proxy_payload(payload) };
    }
    // Use the allocator paired with this dynamically created type.
    let free = unsafe { ffi::PyType_GetSlot(proxy_type, ffi::PY_TP_FREE) };
    if !free.is_null() {
        let free: unsafe extern "C" fn(*mut c_void) = unsafe { mem::transmute(free) };
        unsafe { free(object.cast()) };
    }
}

unsafe fn proxy_value_handle(
    context: *mut TonicContext,
    payload: &ProxyPayload,
    output: *mut TonicHandle,
) -> TonicStatus {
    if payload.strong {
        unsafe { (api().persistent_borrow)(context, payload.handle, output) }
    } else if let Some(reference) = payload.foreign_reference {
        unsafe { (api().foreign_reference_borrow)(context, reference, output) }
    } else {
        unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::RUNTIME_ERROR,
                "PyTonicProxy has no live Tonic reference",
            )
        }
    }
}

unsafe fn promote_proxy(context: *mut TonicContext, payload: &mut ProxyPayload) -> TonicStatus {
    if payload.closed {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::RUNTIME_ERROR,
                "PyTonicProxy is closed",
            )
        };
    }
    if payload.strong {
        return TonicStatus::OK;
    }
    let Some(reference) = payload.foreign_reference else {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::RUNTIME_ERROR,
                "PyTonicProxy has no promotable Tonic reference",
            )
        };
    };
    let mut local = TonicHandle::default();
    let status = unsafe { (api().foreign_reference_borrow)(context, reference, &mut local) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut persistent = TonicPersistentHandle::default();
    let status = unsafe { (api().persistent_create)(context, local, &mut persistent) };
    if status == TonicStatus::OK {
        payload.handle = persistent;
        payload.strong = true;
    }
    status
}

unsafe fn cached_proxy(
    context: *mut TonicContext,
    value: TonicHandle,
) -> std::result::Result<Option<*mut ffi::PyObject>, TonicStatus> {
    unsafe { prune_proxy_cache(ptr::null_mut()) };
    let mut execution_id = 0;
    let status = unsafe { (api().runtime_execution_id)(context, &mut execution_id) };
    if status != TonicStatus::OK {
        return Err(status);
    }
    let cache = proxy_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for object in cache.iter().copied() {
        let object = object as *mut ffi::PyObject;
        let payload = unsafe { proxy_payload(object) };
        if payload.is_null() {
            continue;
        }
        let payload = unsafe { &mut *payload };
        if payload.closed || payload.execution_id != execution_id {
            continue;
        }
        let mut matches = 0;
        let status = unsafe { (api().runtime_owner_matches)(context, payload.owner, &mut matches) };
        if status != TonicStatus::OK {
            return Err(status);
        }
        if matches == 0 {
            continue;
        }
        let mut candidate = TonicHandle::default();
        let status = unsafe { proxy_value_handle(context, payload, &mut candidate) };
        if status != TonicStatus::OK {
            return Err(status);
        }
        let mut identical = 0;
        let status = unsafe { (api().is_identical)(context, value, candidate, &mut identical) };
        if status != TonicStatus::OK {
            return Err(status);
        }
        if identical != 0 {
            let status = unsafe { promote_proxy(context, payload) };
            if status != TonicStatus::OK {
                return Err(status);
            }
            unsafe { ffi::Py_IncRef(object) };
            return Ok(Some(object));
        }
    }
    Ok(None)
}

unsafe fn cache_proxy(object: *mut ffi::PyObject) {
    proxy_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(object as usize);
}

unsafe fn active_proxy_context(payload: &ProxyPayload) -> Option<*mut TonicContext> {
    if payload.closed {
        unsafe { set_python_runtime_error("PyTonicProxy is closed") };
        return None;
    }
    ACTIVE_CONTEXT.with(|active| {
        let context = active.get();
        if context.is_null() {
            unsafe {
                set_python_runtime_error("PyTonicProxy used outside an active Tonic bridge scope")
            };
            return None;
        }
        let mut matches = 0;
        let status = unsafe { (api().runtime_owner_matches)(context, payload.owner, &mut matches) };
        if status != TonicStatus::OK || matches == 0 {
            unsafe {
                set_python_runtime_error(
                    "PyTonicProxy belongs to another or inactive Tonic runtime",
                )
            };
            return None;
        }
        let mut execution_id = 0;
        let status = unsafe { (api().runtime_execution_id)(context, &mut execution_id) };
        if status != TonicStatus::OK || execution_id != payload.execution_id {
            unsafe {
                set_python_runtime_error("PyTonicProxy belongs to an inactive Tonic execution")
            };
            return None;
        }
        Some(context)
    })
}

unsafe fn set_python_error(exception: *mut ffi::PyObject, message: &str) {
    let message = CString::new(message)
        .unwrap_or_else(|_| CString::new("Tonic callback failed").expect("literal has no NUL"));
    // SAFETY: CPython execution state is held and both pointers are live.
    unsafe { ffi::PyErr_SetString(exception, message.as_ptr()) };
}

unsafe fn set_python_runtime_error(message: &str) {
    unsafe { set_python_error(ffi::PyExc_RuntimeError, message) };
}

unsafe fn tonic_exception_message(context: *mut TonicContext) -> String {
    let mut required = 0;
    let status = unsafe { (api().exception_message)(context, ptr::null_mut(), 0, &mut required) };
    if status != TonicStatus::OK {
        return "Tonic callback failed".into();
    }
    let mut bytes = vec![0; required];
    let status = unsafe {
        (api().exception_message)(context, bytes.as_mut_ptr(), bytes.len(), &mut required)
    };
    if status != TonicStatus::OK {
        return "Tonic callback failed".into();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

unsafe fn callback_failed(context: *mut TonicContext) -> *mut ffi::PyObject {
    let mut kind_required = 0;
    let kind_status =
        unsafe { (api().exception_kind)(context, ptr::null_mut(), 0, &mut kind_required) };
    let kind = if kind_status == TonicStatus::OK {
        let mut bytes = vec![0; kind_required];
        if unsafe {
            (api().exception_kind)(context, bytes.as_mut_ptr(), bytes.len(), &mut kind_required)
        } == TonicStatus::OK
        {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let message = unsafe { tonic_exception_message(context) };
    let _ = unsafe { (api().status_clear)(context) };
    let exception = unsafe {
        match kind.as_str() {
            "AttributeError" => ffi::PyExc_AttributeError,
            "TypeError" => ffi::PyExc_TypeError,
            "ValueError" => ffi::PyExc_ValueError,
            "OverflowError" => ffi::PyExc_OverflowError,
            _ => ffi::PyExc_RuntimeError,
        }
    };
    unsafe { set_python_error(exception, &message) };
    ptr::null_mut()
}

unsafe extern "C" fn proxy_call(
    self_object: *mut ffi::PyObject,
    arguments: *mut ffi::PyObject,
    keywords: *mut ffi::PyObject,
) -> *mut ffi::PyObject {
    let payload = unsafe { proxy_payload(self_object) };
    if payload.is_null() {
        unsafe { set_python_runtime_error("invalid PyTonicProxy payload") };
        return ptr::null_mut();
    }
    let payload = unsafe { &*payload.cast::<ProxyPayload>() };
    let Some(context) = (unsafe { active_proxy_context(payload) }) else {
        return ptr::null_mut();
    };
    (|| {
        let count = unsafe { ffi::PyTuple_Size(arguments) };
        if count < 0 {
            return ptr::null_mut();
        }
        let Ok(count) = usize::try_from(count) else {
            unsafe { set_python_runtime_error("PyTonicProxy argument count overflow") };
            return ptr::null_mut();
        };
        let mut callable = TonicHandle::default();
        if unsafe { proxy_value_handle(context, payload, &mut callable) } != TonicStatus::OK {
            return unsafe { callback_failed(context) };
        }
        let mut state = ToTonicState {
            memo: HashMap::new(),
            tuple_stack: HashSet::new(),
            transferred: HashSet::new(),
            depth: 0,
        };
        let mut tonic_arguments = Vec::with_capacity(count);
        for index in 0..count {
            let argument = unsafe { ffi::PyTuple_GetItem(arguments, index as isize) };
            if argument.is_null() {
                return ptr::null_mut();
            }
            // Tuple items are borrowed; conversion consumes one owned reference.
            unsafe { ffi::Py_IncRef(argument) };
            let converted = match unsafe { python_to_tonic_inner(context, argument, &mut state) } {
                Ok(value) => value,
                Err(_) => return unsafe { callback_failed(context) },
            };
            tonic_arguments.push(converted);
        }
        let mut result = TonicHandle::default();
        let status = if keywords.is_null() {
            unsafe {
                (api().call)(
                    context,
                    callable,
                    tonic_arguments.as_ptr(),
                    tonic_arguments.len(),
                    &mut result,
                )
            }
        } else {
            unsafe { ffi::Py_IncRef(keywords) };
            let tonic_keywords =
                match unsafe { python_to_tonic_inner(context, keywords, &mut state) } {
                    Ok(value) => value,
                    Err(_) => return unsafe { callback_failed(context) },
                };
            unsafe {
                (api().call_kw)(
                    context,
                    callable,
                    tonic_arguments.as_ptr(),
                    tonic_arguments.len(),
                    tonic_keywords,
                    &mut result,
                )
            }
        };
        if status != TonicStatus::OK {
            return unsafe { callback_failed(context) };
        }
        match unsafe { tonic_to_python(context, result) } {
            Ok(object) => object,
            Err(_) => unsafe { callback_failed(context) },
        }
    })()
}

unsafe extern "C" fn proxy_get_attr(
    self_object: *mut ffi::PyObject,
    name: *mut ffi::PyObject,
) -> *mut ffi::PyObject {
    let payload = unsafe { proxy_payload(self_object) };
    if payload.is_null() {
        unsafe { set_python_runtime_error("invalid PyTonicProxy payload") };
        return ptr::null_mut();
    }
    let Some(context) = (unsafe { active_proxy_context(&*payload) }) else {
        return ptr::null_mut();
    };
    let mut length = 0;
    let name = unsafe { ffi::PyUnicode_AsUTF8AndSize(name, &mut length) };
    if name.is_null() || length < 0 {
        return ptr::null_mut();
    }
    let mut owner = TonicHandle::default();
    if unsafe { proxy_value_handle(context, &*payload, &mut owner) } != TonicStatus::OK {
        return unsafe { callback_failed(context) };
    }
    let mut result = TonicHandle::default();
    if unsafe { (api().get_attr)(context, owner, name.cast(), length as usize, &mut result) }
        != TonicStatus::OK
    {
        return unsafe { callback_failed(context) };
    }
    match unsafe { tonic_to_python(context, result) } {
        Ok(object) => object,
        Err(_) => unsafe { callback_failed(context) },
    }
}

unsafe extern "C" fn proxy_set_attr(
    self_object: *mut ffi::PyObject,
    name: *mut ffi::PyObject,
    value: *mut ffi::PyObject,
) -> i32 {
    let payload = unsafe { proxy_payload(self_object) };
    if payload.is_null() {
        unsafe { set_python_runtime_error("invalid PyTonicProxy payload") };
        return -1;
    }
    let Some(context) = (unsafe { active_proxy_context(&*payload) }) else {
        return -1;
    };
    if value.is_null() {
        unsafe { set_python_runtime_error("PyTonicProxy attribute deletion is not supported") };
        return -1;
    }
    let mut length = 0;
    let name = unsafe { ffi::PyUnicode_AsUTF8AndSize(name, &mut length) };
    if name.is_null() || length < 0 {
        return -1;
    }
    let mut owner = TonicHandle::default();
    if unsafe { proxy_value_handle(context, &*payload, &mut owner) } != TonicStatus::OK {
        let _ = unsafe { callback_failed(context) };
        return -1;
    }
    unsafe { ffi::Py_IncRef(value) };
    let mut converted = TonicHandle::default();
    if unsafe { python_to_tonic(context, value, &mut converted) } != TonicStatus::OK {
        let _ = unsafe { callback_failed(context) };
        return -1;
    }
    if unsafe { (api().set_attr)(context, owner, name.cast(), length as usize, converted) }
        != TonicStatus::OK
    {
        let _ = unsafe { callback_failed(context) };
        return -1;
    }
    0
}

unsafe extern "C" fn proxy_repr(self_object: *mut ffi::PyObject) -> *mut ffi::PyObject {
    let payload = unsafe { proxy_payload(self_object) };
    if payload.is_null() {
        unsafe { set_python_runtime_error("invalid PyTonicProxy payload") };
        return ptr::null_mut();
    }
    let Some(context) = (unsafe { active_proxy_context(&*payload) }) else {
        return ptr::null_mut();
    };
    let mut owner = TonicHandle::default();
    if unsafe { proxy_value_handle(context, &*payload, &mut owner) } != TonicStatus::OK {
        return unsafe { callback_failed(context) };
    }
    let mut result = TonicHandle::default();
    if unsafe { (api().repr_value)(context, owner, &mut result) } != TonicStatus::OK {
        return unsafe { callback_failed(context) };
    }
    match unsafe { tonic_to_python(context, result) } {
        Ok(object) => object,
        Err(_) => unsafe { callback_failed(context) },
    }
}

unsafe fn proxy_type() -> *mut ffi::PyObject {
    if let Some(proxy_type) = PROXY_TYPE.get().copied() {
        return proxy_type as *mut ffi::PyObject;
    }
    let mut slots = [
        ffi::PyTypeSlot {
            slot: ffi::PY_TP_CALL,
            function: proxy_call as *const () as *mut c_void,
        },
        ffi::PyTypeSlot {
            slot: ffi::PY_TP_DEALLOC,
            function: proxy_dealloc as *const () as *mut c_void,
        },
        ffi::PyTypeSlot {
            slot: ffi::PY_TP_GETATTRO,
            function: proxy_get_attr as *const () as *mut c_void,
        },
        ffi::PyTypeSlot {
            slot: ffi::PY_TP_REPR,
            function: proxy_repr as *const () as *mut c_void,
        },
        ffi::PyTypeSlot {
            slot: ffi::PY_TP_SETATTRO,
            function: proxy_set_attr as *const () as *mut c_void,
        },
        ffi::PyTypeSlot {
            slot: 0,
            function: ptr::null_mut(),
        },
    ];
    let mut spec = ffi::PyTypeSpec {
        name: PROXY_TYPE_NAME.as_ptr().cast(),
        basic_size: -(mem::size_of::<*mut ProxyPayload>() as i32),
        item_size: 0,
        flags: 0,
        slots: slots.as_mut_ptr(),
    };
    let proxy_type = unsafe { ffi::PyType_FromSpec(&mut spec) };
    if proxy_type.is_null() {
        return ptr::null_mut();
    }
    let stored = PROXY_TYPE.get_or_init(|| proxy_type as usize);
    if *stored != proxy_type as usize {
        unsafe { ffi::Py_DecRef(proxy_type) };
    }
    *stored as *mut ffi::PyObject
}

unsafe fn create_proxy(
    context: *mut TonicContext,
    value: TonicHandle,
    use_cache: bool,
) -> std::result::Result<*mut ffi::PyObject, TonicStatus> {
    if use_cache {
        if let Some(proxy) = unsafe { cached_proxy(context, value) }? {
            return Ok(proxy);
        }
    }
    let mut persistent = TonicPersistentHandle::default();
    let status = unsafe { (api().persistent_create)(context, value, &mut persistent) };
    if status != TonicStatus::OK {
        return Err(status);
    }
    let mut foreign_reference = TonicHandle::default();
    let status =
        unsafe { (api().foreign_reference_create)(context, value, &mut foreign_reference) };
    if status != TonicStatus::OK {
        let _ = unsafe { (api().persistent_release)(context, persistent) };
        return Err(status);
    }
    let mut owner = ptr::null_mut();
    let status = unsafe { (api().runtime_owner_acquire)(context, &mut owner) };
    if status != TonicStatus::OK {
        let _ = unsafe { (api().foreign_reference_release)(context, foreign_reference) };
        let _ = unsafe { (api().persistent_release)(context, persistent) };
        return Err(status);
    }
    let mut execution_id = 0;
    let status = unsafe { (api().runtime_execution_id)(context, &mut execution_id) };
    if status != TonicStatus::OK {
        let _ = unsafe { (api().runtime_owner_release)(owner) };
        let _ = unsafe { (api().foreign_reference_release)(context, foreign_reference) };
        let _ = unsafe { (api().persistent_release)(context, persistent) };
        return Err(status);
    }
    let mut runtime_id = 0;
    let status = unsafe { (api().runtime_identity)(context, &mut runtime_id) };
    if status != TonicStatus::OK {
        let _ = unsafe { (api().runtime_owner_release)(owner) };
        let _ = unsafe { (api().foreign_reference_release)(context, foreign_reference) };
        let _ = unsafe { (api().persistent_release)(context, persistent) };
        return Err(status);
    }
    let payload = Box::into_raw(Box::new(ProxyPayload {
        handle: persistent,
        foreign_reference: Some(foreign_reference),
        owner,
        execution_id,
        runtime_id,
        strong: true,
        tonic_wrappers: 0,
        closed: false,
    }));
    let proxy_type = unsafe { proxy_type() };
    if proxy_type.is_null() {
        unsafe { release_proxy_payload(payload) };
        return Err(unsafe { raise_python_error(context, "PyType_FromSpec") });
    }
    let proxy = unsafe { ffi::PyType_GenericAlloc(proxy_type, 0) };
    if proxy.is_null() {
        unsafe { release_proxy_payload(payload) };
        return Err(unsafe { raise_python_error(context, "PyType_GenericAlloc") });
    }
    let data = unsafe { ffi::PyObject_GetTypeData(proxy, proxy_type) };
    if data.is_null() {
        unsafe {
            ffi::Py_DecRef(proxy);
            release_proxy_payload(payload);
        }
        return Err(unsafe { raise_python_error(context, "PyObject_GetTypeData") });
    }
    unsafe { data.cast::<*mut ProxyPayload>().write(payload) };
    if use_cache {
        unsafe { cache_proxy(proxy) };
    }
    Ok(proxy)
}

unsafe extern "C-unwind" fn destroy_pyobject(payload: *mut c_void) {
    let _gil = enter_python();
    if payload.is_null() {
        return;
    }
    // SAFETY: successful wrapper creation transfers exactly one payload box.
    let payload = unsafe { Box::from_raw(payload.cast::<ForeignPyPayload>()) };
    foreign_roots()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .retain(|candidate| *candidate != ptr::from_ref(&*payload) as usize);
    // The CPython reference is released only after the registry no longer exposes
    // this payload to concurrent bridge scans.
    unsafe { ffi::Py_DecRef(payload.object) };
}

unsafe fn create_foreign_pyobject(
    context: *mut TonicContext,
    object: *mut ffi::PyObject,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut runtime_id = 0;
    let status = unsafe { (api().runtime_identity)(context, &mut runtime_id) };
    if status != TonicStatus::OK {
        return status;
    }
    let payload = Box::into_raw(Box::new(ForeignPyPayload { object, runtime_id }));
    foreign_roots()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(payload as usize);
    let status = unsafe {
        (api().foreign_create)(context, payload.cast(), &FOREIGN_PYOBJECT_VTABLE, output)
    };
    if status != TonicStatus::OK {
        foreign_roots()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|candidate| *candidate != payload as usize);
        // Ownership of the PyObject remains with the caller on failure.
        drop(unsafe { Box::from_raw(payload) });
    }
    status
}

unsafe extern "C" fn collect_referent(object: *mut ffi::PyObject, state: *mut c_void) -> i32 {
    if object.is_null() || state.is_null() {
        return 0;
    }
    // SAFETY: direct_referents passes a live Vec for the synchronous traversal.
    unsafe { &mut *state.cast::<Vec<usize>>() }.push(object as usize);
    0
}

unsafe fn direct_referents(object: *mut ffi::PyObject) -> Option<Vec<usize>> {
    if unsafe { ffi::PyObject_GC_IsTracked(object) } == 0 {
        return Some(Vec::new());
    }
    let object_type = unsafe { ffi::PyObject_Type(object) };
    if object_type.is_null() {
        unsafe { ffi::PyErr_Clear() };
        return None;
    }
    let slot = unsafe { ffi::PyType_GetSlot(object_type, ffi::PY_TP_TRAVERSE) };
    if slot.is_null() {
        unsafe { ffi::Py_DecRef(object_type) };
        return Some(Vec::new());
    }
    // SAFETY: Py_tp_traverse is documented with the PyTraverseProc signature;
    // object_type remains owned until the call returns.
    let traverse: ffi::PyTraverseProc = unsafe { mem::transmute(slot) };
    let mut referents = Vec::new();
    let status = unsafe {
        traverse(
            object,
            collect_referent,
            ptr::from_mut(&mut referents).cast(),
        )
    };
    unsafe { ffi::Py_DecRef(object_type) };
    if status != 0 {
        unsafe { ffi::PyErr_Clear() };
        None
    } else {
        Some(referents)
    }
}

unsafe fn is_instance_of(object: *mut ffi::PyObject, class: *mut ffi::PyObject) -> bool {
    let result = unsafe { ffi::PyObject_IsInstance(object, class) };
    if result < 0 {
        unsafe { ffi::PyErr_Clear() };
        false
    } else {
        result != 0
    }
}

unsafe fn is_graph_boundary(object: *mut ffi::PyObject) -> bool {
    unsafe {
        is_instance_of(object, ptr::addr_of_mut!(ffi::PyType_Type))
            || is_instance_of(object, ptr::addr_of_mut!(ffi::PyModule_Type))
            || is_instance_of(object, ptr::addr_of_mut!(ffi::PyFunction_Type))
    }
}

unsafe fn graph_proxy_payload(object: *mut ffi::PyObject) -> Option<*mut ProxyPayload> {
    let proxy_type = PROXY_TYPE.get().copied()? as *mut ffi::PyObject;
    let object_type = unsafe { ffi::PyObject_Type(object) };
    if object_type.is_null() {
        unsafe { ffi::PyErr_Clear() };
        return None;
    }
    let exact_proxy = object_type == proxy_type;
    unsafe { ffi::Py_DecRef(object_type) };
    if !exact_proxy {
        return None;
    }
    let payload = unsafe { proxy_payload(object) };
    (!payload.is_null()).then_some(payload)
}

#[derive(Default)]
struct PythonGraph {
    nodes: Vec<usize>,
    incoming: HashMap<usize, usize>,
    proxies: Vec<usize>,
    complete: bool,
}

unsafe fn scan_python_graph(roots: &[usize]) -> PythonGraph {
    let mut graph = PythonGraph {
        complete: true,
        ..PythonGraph::default()
    };
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    let mut edge_count = 0usize;
    for root in roots.iter().copied() {
        if seen.insert(root) {
            queue.push_back(root);
        }
    }
    while let Some(raw) = queue.pop_front() {
        if graph.nodes.len() >= GRAPH_NODE_LIMIT {
            graph.complete = false;
            break;
        }
        graph.nodes.push(raw);
        let object = raw as *mut ffi::PyObject;
        if unsafe { graph_proxy_payload(object) }.is_some() {
            graph.proxies.push(raw);
            continue;
        }
        let Some(referents) = (unsafe { direct_referents(object) }) else {
            graph.complete = false;
            break;
        };
        for referent in referents {
            if edge_count >= GRAPH_EDGE_LIMIT {
                graph.complete = false;
                break;
            }
            edge_count += 1;
            let object = referent as *mut ffi::PyObject;
            let proxy = unsafe { graph_proxy_payload(object) }.is_some();
            if !proxy
                && (unsafe { ffi::PyObject_GC_IsTracked(object) } == 0
                    || unsafe { is_graph_boundary(object) })
            {
                continue;
            }
            *graph.incoming.entry(referent).or_default() += 1;
            if seen.insert(referent) {
                queue.push_back(referent);
            }
        }
        if !graph.complete {
            break;
        }
    }
    graph
}

unsafe fn python_reference_count(object: *mut ffi::PyObject) -> Option<isize> {
    if object.is_null() {
        return None;
    }
    // SAFETY: the bridge-local C shim receives a live PyObject while the GIL is
    // held and evaluates CPython's version-correct Py_REFCNT accessor.
    let count = unsafe { ffi::tonic_cpython_refcount(object) };
    (count >= 0).then_some(count)
}

unsafe fn graph_has_external_root(
    graph: &PythonGraph,
    root_counts: &HashMap<usize, usize>,
    runtime_id: u64,
) -> bool {
    if !graph.complete {
        return true;
    }
    for raw in &graph.nodes {
        let object = *raw as *mut ffi::PyObject;
        let mut expected = graph.incoming.get(raw).copied().unwrap_or(0)
            + root_counts.get(raw).copied().unwrap_or(0);
        if let Some(payload) = unsafe { graph_proxy_payload(object) } {
            // A proxy from another runtime cannot be represented in this trace
            // visitor. Keep every involved persistent root conservatively.
            if unsafe { (*payload).runtime_id } != runtime_id {
                return true;
            }
            expected += unsafe { (*payload).tonic_wrappers };
        }
        let Some(reference_count) = (unsafe { python_reference_count(object) }) else {
            return true;
        };
        if reference_count > expected as isize {
            return true;
        }
    }
    false
}

unsafe fn set_proxy_rooted(
    visitor: *mut tonic_runtime::TonicTraceVisitor,
    payload: &mut ProxyPayload,
    rooted: bool,
) -> TonicStatus {
    if payload.closed || payload.strong == rooted {
        return TonicStatus::OK;
    }
    let Some(reference) = payload.foreign_reference else {
        return TonicStatus::INVALID_ARGUMENT;
    };
    if rooted {
        let mut persistent = TonicPersistentHandle::default();
        let status = unsafe { ((*visitor).promote)(visitor, reference, &mut persistent) };
        if status == TonicStatus::OK {
            payload.handle = persistent;
            payload.strong = true;
        }
        status
    } else {
        let status = unsafe { (api().persistent_release_deferred)(payload.owner, payload.handle) };
        if status == TonicStatus::OK {
            payload.strong = false;
        }
        status
    }
}

unsafe extern "C-unwind" fn trace_pyobject_graph(
    payload: *mut c_void,
    visitor: *mut tonic_runtime::TonicTraceVisitor,
) -> TonicStatus {
    let _gil = enter_python();
    if payload.is_null() || visitor.is_null() {
        return TonicStatus::INVALID_ARGUMENT;
    }
    // SAFETY: CPYTHON_ADAPTER_ID stores this exact owned payload type.
    let current = unsafe { &*payload.cast::<ForeignPyPayload>() };
    let registered = foreign_roots()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let mut roots = Vec::new();
    let mut root_counts = HashMap::new();
    for raw in registered {
        // SAFETY: the registry is pruned under the GIL before payload destruction.
        let root = unsafe { &*(raw as *const ForeignPyPayload) };
        if root.runtime_id == current.runtime_id {
            roots.push(root.object as usize);
            *root_counts.entry(root.object as usize).or_default() += 1;
        }
    }
    let mut graph_proxies = HashMap::<usize, bool>::new();
    for root in roots {
        let object = root as *mut ffi::PyObject;
        if unsafe { is_graph_boundary(object) } {
            continue;
        }
        let graph = unsafe { scan_python_graph(&[root]) };
        let externally_rooted =
            unsafe { graph_has_external_root(&graph, &root_counts, current.runtime_id) };
        for proxy in graph.proxies {
            graph_proxies
                .entry(proxy)
                .and_modify(|external| *external |= externally_rooted)
                .or_insert(externally_rooted);
        }
    }
    for (raw, externally_rooted) in &graph_proxies {
        let Some(proxy) = (unsafe { graph_proxy_payload(*raw as *mut ffi::PyObject) }) else {
            continue;
        };
        // SAFETY: graph traversal and proxy destruction are serialized by GIL.
        let proxy = unsafe { &mut *proxy };
        if proxy.runtime_id != current.runtime_id || proxy.closed {
            continue;
        }
        let Some(reference) = proxy.foreign_reference else {
            return TonicStatus::INVALID_ARGUMENT;
        };
        let status = unsafe { ((*visitor).visit_borrowed)(visitor, reference) };
        if status != TonicStatus::OK {
            return status;
        }
        let status = unsafe { set_proxy_rooted(visitor, proxy, *externally_rooted) };
        if status != TonicStatus::OK {
            return status;
        }
    }

    // A proxy can be moved out of a Tonic-owned graph by opaque Python code.
    // Reconcile cached proxies absent from the current union so such a move
    // promotes its target before this wrapper can be swept.
    let proxies = proxy_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    for raw in proxies {
        if graph_proxies.contains_key(&raw) {
            continue;
        }
        let Some(proxy) = (unsafe { graph_proxy_payload(raw as *mut ffi::PyObject) }) else {
            continue;
        };
        let proxy = unsafe { &mut *proxy };
        if proxy.runtime_id != current.runtime_id || proxy.closed || proxy.strong {
            continue;
        }
        let isolated = unsafe { scan_python_graph(&[raw]) };
        let external =
            unsafe { graph_has_external_root(&isolated, &HashMap::new(), current.runtime_id) };
        if external {
            let status = unsafe { set_proxy_rooted(visitor, proxy, true) };
            if status != TonicStatus::OK {
                return status;
            }
        }
    }
    TonicStatus::OK
}

unsafe extern "C-unwind" fn trace_proxy(
    object: *mut c_void,
    visitor: *mut tonic_runtime::TonicTraceVisitor,
) -> TonicStatus {
    let _gil = enter_python();
    let object = object.cast::<ffi::PyObject>();
    let payload = unsafe { proxy_payload(object) };
    if payload.is_null() {
        return TonicStatus::INVALID_ARGUMENT;
    }
    let payload = unsafe { &mut *payload };
    if payload.closed {
        return TonicStatus::OK;
    }
    let Some(reference) = payload.foreign_reference else {
        return TonicStatus::OK;
    };
    if visitor.is_null() {
        return TonicStatus::INVALID_ARGUMENT;
    }
    let status = unsafe { ((*visitor).visit_borrowed)(visitor, reference) };
    if status != TonicStatus::OK {
        return status;
    }
    // foreign_create invokes trace before python.proxy has transferred the
    // CPython object into its managed wrapper. Do not demote that construction
    // root until the wrapper count records the transfer.
    if payload.tonic_wrappers == 0 {
        return TonicStatus::OK;
    }
    let Some(reference_count) = (unsafe { python_reference_count(object) }) else {
        return TonicStatus::INVALID_ARGUMENT;
    };
    let external = reference_count > payload.tonic_wrappers as isize;
    let status = unsafe { set_proxy_rooted(visitor, payload, external) };
    if status != TonicStatus::OK {
        return status;
    }
    TonicStatus::OK
}

unsafe extern "C-unwind" fn destroy_proxy_wrapper(object: *mut c_void) {
    let _gil = enter_python();
    let object = object.cast::<ffi::PyObject>();
    let payload = unsafe { proxy_payload(object) };
    if !payload.is_null() {
        let payload = unsafe { &mut *payload };
        payload.tonic_wrappers = payload.tonic_wrappers.saturating_sub(1);
    }
    unsafe { ffi::Py_DecRef(object) };
}

static FOREIGN_PYOBJECT_VTABLE: TonicForeignVTable = TonicForeignVTable {
    struct_size: mem::size_of::<TonicForeignVTable>() as u32,
    abi_version: TONIC_ABI_VERSION,
    adapter_id: CPYTHON_ADAPTER_ID,
    flags: FOREIGN_OWNED,
    trace: Some(trace_pyobject_graph),
    destroy: Some(destroy_pyobject),
};

static TONIC_PROXY_VTABLE: TonicForeignVTable = TonicForeignVTable {
    struct_size: mem::size_of::<TonicForeignVTable>() as u32,
    abi_version: TONIC_ABI_VERSION,
    adapter_id: CPYTHON_PROXY_ADAPTER_ID,
    flags: FOREIGN_OWNED,
    trace: Some(trace_proxy),
    destroy: Some(destroy_proxy_wrapper),
};

fn api() -> &'static tonic_runtime::TonicApi {
    API.get_or_init(|| {
        negotiate_api(
            TONIC_ABI_VERSION,
            mem::size_of::<tonic_runtime::TonicApi>() as u32,
            CAP_FOREIGN_OBJECT_V1
                | CAP_PERSISTENT_HANDLES_V1
                | CAP_CONTAINER_ACCESS_V1
                | CAP_PROTOCOL_ACCESS_V1
                | CAP_CROSS_COLLECTOR_V1
                | tonic_runtime::c_api::CAP_RUNTIME_OWNER_V1,
        )
        .expect("runtime and bridge ABI versions are built together")
    })
}

unsafe fn raise_bridge_error(
    context: *mut TonicContext,
    kind: TonicExceptionKind,
    message: &str,
) -> TonicStatus {
    unsafe { (api().raise_exception)(context, kind, message.as_ptr(), message.len()) }
}

struct ToPythonState {
    memo: Vec<(TonicHandle, *mut ffi::PyObject)>,
    depth: usize,
}

struct ToTonicState {
    memo: HashMap<usize, TonicHandle>,
    tuple_stack: HashSet<usize>,
    transferred: HashSet<usize>,
    depth: usize,
}

unsafe fn tonic_to_python(
    context: *mut TonicContext,
    value: TonicHandle,
) -> std::result::Result<*mut ffi::PyObject, TonicStatus> {
    let mut state = ToPythonState {
        memo: Vec::new(),
        depth: 0,
    };
    unsafe { tonic_to_python_inner(context, value, &mut state) }
}

unsafe fn tonic_to_python_inner(
    context: *mut TonicContext,
    value: TonicHandle,
    state: &mut ToPythonState,
) -> std::result::Result<*mut ffi::PyObject, TonicStatus> {
    if state.depth >= 256 {
        return Err(unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::VALUE_ERROR,
                "container conversion nesting limit exceeded",
            )
        });
    }
    state.depth += 1;
    let result = (|| {
        let mut kind = TonicValueKind::OTHER;
        let status = unsafe { (api().value_kind)(context, value, &mut kind) };
        if status != TonicStatus::OK {
            return Err(status);
        }
        if kind == TonicValueKind::LIST
            || kind == TonicValueKind::TUPLE
            || kind == TonicValueKind::DICT
            || kind == TonicValueKind::OTHER
        {
            for (seen, object) in &state.memo {
                let mut identical = 0;
                let status = unsafe { (api().is_identical)(context, value, *seen, &mut identical) };
                if status != TonicStatus::OK {
                    return Err(status);
                }
                if identical != 0 {
                    unsafe { ffi::Py_IncRef(*object) };
                    return Ok(*object);
                }
            }
        }
        let object = if kind == TonicValueKind::NONE {
            let object = ptr::addr_of_mut!(ffi::_Py_NoneStruct);
            unsafe { ffi::Py_IncRef(object) };
            object
        } else if kind == TonicValueKind::BOOL {
            let mut boolean = 0;
            let status = unsafe { (api().bool_as)(context, value, &mut boolean) };
            if status != TonicStatus::OK {
                return Err(status);
            }
            unsafe { ffi::PyBool_FromLong(boolean.into()) }
        } else if kind == TonicValueKind::INT {
            let mut integer = 0;
            if unsafe { (api().int_as_i64)(context, value, &mut integer) } == TonicStatus::OK {
                unsafe { ffi::PyLong_FromLongLong(integer) }
            } else {
                let mut required = 0;
                let status = unsafe {
                    (api().int_decimal)(context, value, ptr::null_mut(), 0, &mut required)
                };
                if status != TonicStatus::OK {
                    return Err(status);
                }
                let mut decimal = vec![0; required + 1];
                let status = unsafe {
                    (api().int_decimal)(
                        context,
                        value,
                        decimal.as_mut_ptr(),
                        required,
                        &mut required,
                    )
                };
                if status != TonicStatus::OK {
                    return Err(status);
                }
                unsafe { ffi::PyLong_FromString(decimal.as_ptr().cast(), ptr::null_mut(), 10) }
            }
        } else if kind == TonicValueKind::FLOAT {
            let mut float = 0.0;
            let status = unsafe { (api().float_as_f64)(context, value, &mut float) };
            if status != TonicStatus::OK {
                return Err(status);
            }
            unsafe { ffi::PyFloat_FromDouble(float) }
        } else if kind == TonicValueKind::STR {
            let string = match unsafe { tonic_string(context, value) } {
                Ok(string) => string,
                Err(error) => {
                    return Err(unsafe {
                        raise_bridge_error(context, TonicExceptionKind::TYPE_ERROR, &error.message)
                    })
                }
            };
            let Ok(size) = isize::try_from(string.len()) else {
                return Err(unsafe {
                    raise_bridge_error(
                        context,
                        TonicExceptionKind::OVERFLOW_ERROR,
                        "string is too large for CPython",
                    )
                });
            };
            unsafe { ffi::PyUnicode_FromStringAndSize(string.as_ptr().cast(), size) }
        } else if kind == TonicValueKind::FOREIGN {
            let mut payload = ptr::null_mut();
            let mut proxy = false;
            let mut status = unsafe {
                (api().foreign_borrow_payload)(context, value, CPYTHON_ADAPTER_ID, &mut payload)
            };
            if status != TonicStatus::OK {
                status = unsafe {
                    (api().foreign_borrow_payload)(
                        context,
                        value,
                        CPYTHON_PROXY_ADAPTER_ID,
                        &mut payload,
                    )
                };
                proxy = status == TonicStatus::OK;
            }
            if status != TonicStatus::OK {
                return Err(status);
            }
            let object = if proxy {
                payload.cast()
            } else {
                unsafe { foreign_pyobject(payload) }
            };
            if object.is_null() {
                return Err(TonicStatus::INVALID_ARGUMENT);
            }
            if proxy {
                let proxy_payload = unsafe { proxy_payload(object) };
                if proxy_payload.is_null() {
                    return Err(unsafe {
                        raise_bridge_error(
                            context,
                            TonicExceptionKind::RUNTIME_ERROR,
                            "invalid PyTonicProxy payload",
                        )
                    });
                }
                let status = unsafe { promote_proxy(context, &mut *proxy_payload) };
                if status != TonicStatus::OK {
                    return Err(status);
                }
            }
            unsafe { ffi::Py_IncRef(object) };
            object
        } else if kind == TonicValueKind::LIST || kind == TonicValueKind::TUPLE {
            let mut length = 0;
            let status = unsafe { (api().sequence_len)(context, value, &mut length) };
            if status != TonicStatus::OK {
                return Err(status);
            }
            let Ok(length) = isize::try_from(length) else {
                return Err(unsafe {
                    raise_bridge_error(
                        context,
                        TonicExceptionKind::OVERFLOW_ERROR,
                        "sequence is too large for CPython",
                    )
                });
            };
            let object = if kind == TonicValueKind::LIST {
                unsafe { ffi::PyList_New(length) }
            } else {
                unsafe { ffi::PyTuple_New(length) }
            };
            if object.is_null() {
                return Err(unsafe { raise_python_error(context, "sequence allocation") });
            }
            state.memo.push((value, object));
            for index in 0..length as usize {
                let mut item = TonicHandle::default();
                let status = unsafe { (api().sequence_get)(context, value, index, &mut item) };
                if status != TonicStatus::OK {
                    unsafe { ffi::Py_DecRef(object) };
                    return Err(status);
                }
                let item = match unsafe { tonic_to_python_inner(context, item, state) } {
                    Ok(item) => item,
                    Err(status) => {
                        unsafe { ffi::Py_DecRef(object) };
                        return Err(status);
                    }
                };
                let status = if kind == TonicValueKind::LIST {
                    unsafe { ffi::PyList_SetItem(object, index as isize, item) }
                } else {
                    unsafe { ffi::PyTuple_SetItem(object, index as isize, item) }
                };
                if status != 0 {
                    unsafe { ffi::Py_DecRef(object) };
                    return Err(unsafe { raise_python_error(context, "sequence item conversion") });
                }
            }
            object
        } else if kind == TonicValueKind::DICT {
            let object = unsafe { ffi::PyDict_New() };
            if object.is_null() {
                return Err(unsafe { raise_python_error(context, "PyDict_New") });
            }
            state.memo.push((value, object));
            let mut length = 0;
            let status = unsafe { (api().dict_len)(context, value, &mut length) };
            if status != TonicStatus::OK {
                unsafe { ffi::Py_DecRef(object) };
                return Err(status);
            }
            for index in 0..length {
                let mut key = TonicHandle::default();
                let mut item = TonicHandle::default();
                let status =
                    unsafe { (api().dict_entry)(context, value, index, &mut key, &mut item) };
                if status != TonicStatus::OK {
                    unsafe { ffi::Py_DecRef(object) };
                    return Err(status);
                }
                let key = match unsafe { tonic_to_python_inner(context, key, state) } {
                    Ok(key) => key,
                    Err(status) => {
                        unsafe { ffi::Py_DecRef(object) };
                        return Err(status);
                    }
                };
                let item = match unsafe { tonic_to_python_inner(context, item, state) } {
                    Ok(item) => item,
                    Err(status) => {
                        unsafe { ffi::Py_DecRef(key) };
                        unsafe { ffi::Py_DecRef(object) };
                        return Err(status);
                    }
                };
                let status = unsafe { ffi::PyDict_SetItem(object, key, item) };
                unsafe {
                    ffi::Py_DecRef(key);
                    ffi::Py_DecRef(item);
                }
                if status != 0 {
                    unsafe { ffi::Py_DecRef(object) };
                    return Err(unsafe { raise_python_error(context, "PyDict_SetItem") });
                }
            }
            object
        } else {
            let object = unsafe { create_proxy(context, value, true) }?;
            state.memo.push((value, object));
            object
        };
        if object.is_null() {
            Err(unsafe { raise_python_error(context, "Tonic to CPython conversion") })
        } else {
            Ok(object)
        }
    })();
    state.depth -= 1;
    result
}

unsafe fn python_to_tonic(
    context: *mut TonicContext,
    object: *mut ffi::PyObject,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut state = ToTonicState {
        memo: HashMap::new(),
        tuple_stack: HashSet::new(),
        transferred: HashSet::new(),
        depth: 0,
    };
    match unsafe { python_to_tonic_inner(context, object, &mut state) } {
        Ok(value) => {
            unsafe { output.write(value) };
            TonicStatus::OK
        }
        Err(status) => status,
    }
}

unsafe fn python_to_tonic_inner(
    context: *mut TonicContext,
    object: *mut ffi::PyObject,
    state: &mut ToTonicState,
) -> std::result::Result<TonicHandle, TonicStatus> {
    if state.depth >= 256 {
        unsafe { ffi::Py_DecRef(object) };
        return Err(unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::VALUE_ERROR,
                "container conversion nesting limit exceeded",
            )
        });
    }
    if let Some(value) = state.memo.get(&(object as usize)).copied() {
        unsafe { ffi::Py_DecRef(object) };
        return Ok(value);
    }
    state.depth += 1;
    let result = (|| {
        if object == ptr::addr_of_mut!(ffi::_Py_NoneStruct) {
            let mut output = TonicHandle::default();
            let status = unsafe { (api().none)(context, &mut output) };
            return (status == TonicStatus::OK).then_some(output).ok_or(status);
        }
        if object == ptr::addr_of_mut!(ffi::_Py_TrueStruct)
            || object == ptr::addr_of_mut!(ffi::_Py_FalseStruct)
        {
            let value = u32::from(object == ptr::addr_of_mut!(ffi::_Py_TrueStruct));
            let mut output = TonicHandle::default();
            let status = unsafe { (api().bool_from)(context, value, &mut output) };
            return (status == TonicStatus::OK).then_some(output).ok_or(status);
        }
        let is_long =
            unsafe { ffi::PyObject_IsInstance(object, ptr::addr_of_mut!(ffi::PyLong_Type)) };
        if is_long < 0 {
            return Err(unsafe { raise_python_error(context, "integer type check") });
        }
        if is_long != 0 {
            let mut overflow = 0;
            let value = unsafe { ffi::PyLong_AsLongLongAndOverflow(object, &mut overflow) };
            let mut output = TonicHandle::default();
            if overflow == 0 && unsafe { ffi::PyErr_Occurred().is_null() } {
                let status = unsafe { (api().int_from_i64)(context, value, &mut output) };
                return (status == TonicStatus::OK).then_some(output).ok_or(status);
            }
            unsafe { ffi::PyErr_Clear() };
            let decimal = unsafe { ffi::PyObject_Str(object) };
            if decimal.is_null() {
                return Err(unsafe { raise_python_error(context, "bigint formatting") });
            }
            let mut size = 0;
            let bytes = unsafe { ffi::PyUnicode_AsUTF8AndSize(decimal, &mut size) };
            if bytes.is_null() || size < 0 {
                unsafe { ffi::Py_DecRef(decimal) };
                return Err(unsafe { raise_python_error(context, "bigint UTF-8 conversion") });
            }
            let status = unsafe {
                (api().int_from_decimal)(context, bytes.cast(), size as usize, &mut output)
            };
            unsafe { ffi::Py_DecRef(decimal) };
            return (status == TonicStatus::OK).then_some(output).ok_or(status);
        }
        let is_float =
            unsafe { ffi::PyObject_IsInstance(object, ptr::addr_of_mut!(ffi::PyFloat_Type)) };
        if is_float < 0 {
            return Err(unsafe { raise_python_error(context, "float type check") });
        }
        if is_float != 0 {
            let value = unsafe { ffi::PyFloat_AsDouble(object) };
            if unsafe { !ffi::PyErr_Occurred().is_null() } {
                return Err(unsafe { raise_python_error(context, "float result conversion") });
            }
            let mut output = TonicHandle::default();
            let status = unsafe { (api().float_from_f64)(context, value, &mut output) };
            return (status == TonicStatus::OK).then_some(output).ok_or(status);
        }
        let is_string =
            unsafe { ffi::PyObject_IsInstance(object, ptr::addr_of_mut!(ffi::PyUnicode_Type)) };
        if is_string < 0 {
            return Err(unsafe { raise_python_error(context, "string type check") });
        }
        if is_string != 0 {
            let mut size = 0;
            let bytes = unsafe { ffi::PyUnicode_AsUTF8AndSize(object, &mut size) };
            if bytes.is_null() || size < 0 {
                return Err(unsafe { raise_python_error(context, "string result conversion") });
            }
            let mut output = TonicHandle::default();
            let status =
                unsafe { (api().str_from_utf8)(context, bytes.cast(), size as usize, &mut output) };
            return (status == TonicStatus::OK).then_some(output).ok_or(status);
        }
        if let Some(proxy_type) = PROXY_TYPE.get().copied() {
            let is_proxy =
                unsafe { ffi::PyObject_IsInstance(object, proxy_type as *mut ffi::PyObject) };
            if is_proxy < 0 {
                return Err(unsafe { raise_python_error(context, "proxy type check") });
            }
            if is_proxy != 0 {
                let payload = unsafe { proxy_payload(object) };
                if payload.is_null() {
                    return Err(unsafe {
                        raise_bridge_error(
                            context,
                            TonicExceptionKind::RUNTIME_ERROR,
                            "invalid PyTonicProxy payload",
                        )
                    });
                }
                let payload = unsafe { &*payload };
                if payload.closed {
                    return Err(unsafe {
                        raise_bridge_error(
                            context,
                            TonicExceptionKind::RUNTIME_ERROR,
                            "PyTonicProxy is closed",
                        )
                    });
                }
                let mut matches = 0;
                let status =
                    unsafe { (api().runtime_owner_matches)(context, payload.owner, &mut matches) };
                if status != TonicStatus::OK {
                    return Err(status);
                }
                let mut execution_id = 0;
                let status = unsafe { (api().runtime_execution_id)(context, &mut execution_id) };
                if status != TonicStatus::OK {
                    return Err(status);
                }
                if matches == 0 || execution_id != payload.execution_id {
                    return Err(unsafe {
                        raise_bridge_error(
                            context,
                            TonicExceptionKind::RUNTIME_ERROR,
                            "PyTonicProxy belongs to another or inactive Tonic execution",
                        )
                    });
                }
                let mut output = TonicHandle::default();
                let status = unsafe { proxy_value_handle(context, payload, &mut output) };
                if status != TonicStatus::OK {
                    return Err(status);
                }
                state.memo.insert(object as usize, output);
                return Ok(output);
            }
        }
        let is_list =
            unsafe { ffi::PyObject_IsInstance(object, ptr::addr_of_mut!(ffi::PyList_Type)) };
        if is_list < 0 {
            return Err(unsafe { raise_python_error(context, "list type check") });
        }
        if is_list != 0 {
            let mut list = TonicHandle::default();
            let status = unsafe { (api().list_new)(context, &mut list) };
            if status != TonicStatus::OK {
                return Err(status);
            }
            state.memo.insert(object as usize, list);
            let length = unsafe { ffi::PyList_Size(object) };
            if length < 0 {
                return Err(unsafe { raise_python_error(context, "PyList_Size") });
            }
            for index in 0..length {
                let item = unsafe { ffi::PyList_GetItem(object, index) };
                if item.is_null() {
                    return Err(unsafe { raise_python_error(context, "PyList_GetItem") });
                }
                unsafe { ffi::Py_IncRef(item) };
                let item = unsafe { python_to_tonic_inner(context, item, state) }?;
                let status = unsafe { (api().list_append)(context, list, item) };
                if status != TonicStatus::OK {
                    return Err(status);
                }
            }
            return Ok(list);
        }
        let is_tuple =
            unsafe { ffi::PyObject_IsInstance(object, ptr::addr_of_mut!(ffi::PyTuple_Type)) };
        if is_tuple < 0 {
            return Err(unsafe { raise_python_error(context, "tuple type check") });
        }
        if is_tuple != 0 {
            if !state.tuple_stack.insert(object as usize) {
                return Err(unsafe {
                    raise_bridge_error(
                        context,
                        TonicExceptionKind::VALUE_ERROR,
                        "cycle through a Python tuple cannot be materialized",
                    )
                });
            }
            let length = unsafe { ffi::PyTuple_Size(object) };
            if length < 0 {
                return Err(unsafe { raise_python_error(context, "PyTuple_Size") });
            }
            let mut items = Vec::with_capacity(length as usize);
            for index in 0..length {
                let item = unsafe { ffi::PyTuple_GetItem(object, index) };
                if item.is_null() {
                    return Err(unsafe { raise_python_error(context, "PyTuple_GetItem") });
                }
                unsafe { ffi::Py_IncRef(item) };
                items.push(unsafe { python_to_tonic_inner(context, item, state) }?);
            }
            state.tuple_stack.remove(&(object as usize));
            let mut tuple = TonicHandle::default();
            let status =
                unsafe { (api().tuple_new)(context, items.as_ptr(), items.len(), &mut tuple) };
            if status != TonicStatus::OK {
                return Err(status);
            }
            state.memo.insert(object as usize, tuple);
            return Ok(tuple);
        }
        let is_dict =
            unsafe { ffi::PyObject_IsInstance(object, ptr::addr_of_mut!(ffi::PyDict_Type)) };
        if is_dict < 0 {
            return Err(unsafe { raise_python_error(context, "dict type check") });
        }
        if is_dict != 0 {
            let mut dict = TonicHandle::default();
            let status = unsafe { (api().dict_new)(context, &mut dict) };
            if status != TonicStatus::OK {
                return Err(status);
            }
            state.memo.insert(object as usize, dict);
            let mut position = 0;
            let mut key = ptr::null_mut();
            let mut value = ptr::null_mut();
            while unsafe { ffi::PyDict_Next(object, &mut position, &mut key, &mut value) } != 0 {
                unsafe { ffi::Py_IncRef(key) };
                let key = unsafe { python_to_tonic_inner(context, key, state) }?;
                unsafe { ffi::Py_IncRef(value) };
                let value = unsafe { python_to_tonic_inner(context, value, state) }?;
                let status = unsafe { (api().dict_set)(context, dict, key, value) };
                if status != TonicStatus::OK {
                    return Err(status);
                }
            }
            return Ok(dict);
        }
        let mut output = TonicHandle::default();
        let status = unsafe { create_foreign_pyobject(context, object, &mut output) };
        if status == TonicStatus::OK {
            state.memo.insert(object as usize, output);
            state.transferred.insert(object as usize);
            Ok(output)
        } else {
            Err(status)
        }
    })();
    state.depth -= 1;
    let transferred = state.transferred.remove(&(object as usize));
    if !transferred {
        unsafe { ffi::Py_DecRef(object) };
    }
    result
}

unsafe fn tonic_string(context: *mut TonicContext, value: TonicHandle) -> Result<String> {
    let mut required = 0;
    let status = unsafe { (api().str_utf8)(context, value, ptr::null_mut(), 0, &mut required) };
    if status != TonicStatus::OK {
        return Err(Diagnostic::new("TypeError", "expected Tonic string"));
    }
    let mut bytes = vec![0; required];
    let status = unsafe {
        (api().str_utf8)(
            context,
            value,
            bytes.as_mut_ptr(),
            bytes.len(),
            &mut required,
        )
    };
    if status != TonicStatus::OK {
        return Err(Diagnostic::new("TypeError", "failed to read Tonic string"));
    }
    String::from_utf8(bytes).map_err(|_| Diagnostic::new("ValueError", "invalid UTF-8 string"))
}

unsafe fn raise_python_error(context: *mut TonicContext, operation: &'static str) -> TonicStatus {
    let error = python_error(operation);
    // SAFETY: message bytes live for this synchronous API call.
    unsafe {
        (api().raise_exception)(
            context,
            TonicExceptionKind::PYTHON_ERROR,
            error.message.as_ptr(),
            error.message.len(),
        )
    }
}

unsafe extern "C-unwind" fn python_abs(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut value = 0i64;
    // SAFETY: VM supplies one argument and live output storage.
    let status = unsafe { (api().int_as_i64)(context, arguments.read(), &mut value) };
    if status != TonicStatus::OK {
        return status;
    }
    let _gil = enter_python();
    // SAFETY: CPython execution state is held for all object operations.
    let input = unsafe { ffi::PyLong_FromLongLong(value) };
    if input.is_null() {
        return unsafe { raise_python_error(context, "PyLong_FromLongLong") };
    }
    // SAFETY: input is a live owned PyObject reference.
    let result = unsafe { ffi::PyNumber_Absolute(input) };
    // SAFETY: input ownership is local to this call.
    unsafe { ffi::Py_DecRef(input) };
    if result.is_null() {
        return unsafe { raise_python_error(context, "PyNumber_Absolute") };
    }
    let mut overflow = 0;
    // SAFETY: result remains live until the following DECREF.
    let result_value = unsafe { ffi::PyLong_AsLongLongAndOverflow(result, &mut overflow) };
    // SAFETY: result ownership is local to this call.
    unsafe { ffi::Py_DecRef(result) };
    if overflow != 0 || unsafe { !ffi::PyErr_Occurred().is_null() } {
        return unsafe { raise_python_error(context, "PyLong_AsLongLongAndOverflow") };
    }
    // SAFETY: output remains writable for the native scope.
    unsafe { (api().int_from_i64)(context, result_value, output) }
}

unsafe extern "C-unwind" fn python_make_list(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut value = 0i64;
    // SAFETY: VM supplies one argument and live output storage.
    let status = unsafe { (api().int_as_i64)(context, arguments.read(), &mut value) };
    if status != TonicStatus::OK {
        return status;
    }
    let _gil = enter_python();
    // SAFETY: CPython execution state is held.
    let list = unsafe { ffi::PyList_New(0) };
    let item = unsafe { ffi::PyLong_FromLongLong(value) };
    if list.is_null() || item.is_null() {
        if !list.is_null() {
            unsafe { ffi::Py_DecRef(list) };
        }
        if !item.is_null() {
            unsafe { ffi::Py_DecRef(item) };
        }
        return unsafe { raise_python_error(context, "PyList_New/PyLong_FromLongLong") };
    }
    if unsafe { ffi::PyList_Append(list, item) } != 0 {
        unsafe {
            ffi::Py_DecRef(item);
            ffi::Py_DecRef(list);
        }
        return unsafe { raise_python_error(context, "PyList_Append") };
    }
    // PyList_Append increments rather than steals the item reference.
    unsafe { ffi::Py_DecRef(item) };
    // SAFETY: on success the foreign wrapper owns the list reference.
    let status = unsafe { create_foreign_pyobject(context, list, output) };
    if status != TonicStatus::OK {
        // SAFETY: failed creation leaves ownership with this bridge call.
        unsafe { ffi::Py_DecRef(list) };
    }
    status
}

unsafe extern "C-unwind" fn python_echo_float(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut value = 0.0;
    // SAFETY: VM supplies one argument and a live conversion output.
    let status = unsafe { (api().float_as_f64)(context, arguments.read(), &mut value) };
    if status != TonicStatus::OK {
        return status;
    }
    let _gil = enter_python();
    // SAFETY: CPython execution state is held.
    let object = unsafe { ffi::PyFloat_FromDouble(value) };
    if object.is_null() {
        return unsafe { raise_python_error(context, "PyFloat_FromDouble") };
    }
    // SAFETY: object remains live until DECREF below.
    let value = unsafe { ffi::PyFloat_AsDouble(object) };
    let failed = unsafe { !ffi::PyErr_Occurred().is_null() };
    unsafe { ffi::Py_DecRef(object) };
    if failed {
        return unsafe { raise_python_error(context, "PyFloat_AsDouble") };
    }
    // SAFETY: output remains writable for the native call.
    unsafe { (api().float_from_f64)(context, value, output) }
}

unsafe extern "C-unwind" fn python_echo_str(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let argument = unsafe { arguments.read() };
    let mut required = 0;
    // SAFETY: NULL/zero is the documented two-pass length query.
    let status = unsafe { (api().str_utf8)(context, argument, ptr::null_mut(), 0, &mut required) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut bytes = vec![0; required];
    // SAFETY: the Vec exposes exactly `required` writable bytes.
    let status = unsafe {
        (api().str_utf8)(
            context,
            argument,
            bytes.as_mut_ptr(),
            bytes.len(),
            &mut required,
        )
    };
    if status != TonicStatus::OK {
        return status;
    }
    let Ok(size) = isize::try_from(bytes.len()) else {
        return TonicStatus::INVALID_ARGUMENT;
    };
    let _gil = enter_python();
    // SAFETY: byte storage remains live and Tonic strings are valid UTF-8.
    let object = unsafe { ffi::PyUnicode_FromStringAndSize(bytes.as_ptr().cast(), size) };
    if object.is_null() {
        return unsafe { raise_python_error(context, "PyUnicode_FromStringAndSize") };
    }
    let mut result_size = 0;
    // SAFETY: object is a live Unicode instance.
    let result = unsafe { ffi::PyUnicode_AsUTF8AndSize(object, &mut result_size) };
    if result.is_null() || result_size < 0 {
        unsafe { ffi::Py_DecRef(object) };
        return unsafe { raise_python_error(context, "PyUnicode_AsUTF8AndSize") };
    }
    // SAFETY: CPython promises result_size readable bytes until object DECREF;
    // str_from_utf8 copies them into Tonic-owned storage synchronously.
    let status =
        unsafe { (api().str_from_utf8)(context, result.cast(), result_size as usize, output) };
    unsafe { ffi::Py_DecRef(object) };
    status
}

unsafe extern "C-unwind" fn python_length(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut payload = ptr::null_mut();
    // SAFETY: VM supplies one argument and the payload output is local.
    let status = unsafe {
        (api().foreign_borrow_payload)(context, arguments.read(), CPYTHON_ADAPTER_ID, &mut payload)
    };
    if status != TonicStatus::OK {
        return status;
    }
    let _gil = enter_python();
    // SAFETY: wrapper owns the PyObject and native scopes prohibit GC/finalize.
    let object = unsafe { foreign_pyobject(payload) };
    if object.is_null() {
        return TonicStatus::INVALID_ARGUMENT;
    }
    let length = unsafe { ffi::PyObject_Length(object) };
    if length < 0 {
        return unsafe { raise_python_error(context, "PyObject_Length") };
    }
    let Ok(length) = i64::try_from(length) else {
        return unsafe { raise_python_error(context, "length conversion") };
    };
    // SAFETY: output remains writable for the call.
    unsafe { (api().int_from_i64)(context, length, output) }
}

unsafe extern "C-unwind" fn python_length_of_int(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut value = 0;
    let status = unsafe { (api().int_as_i64)(context, arguments.read(), &mut value) };
    if status != TonicStatus::OK {
        return status;
    }
    let _gil = enter_python();
    let object = unsafe { ffi::PyLong_FromLongLong(value) };
    if object.is_null() {
        return unsafe { raise_python_error(context, "PyLong_FromLongLong") };
    }
    let length = unsafe { ffi::PyObject_Length(object) };
    unsafe { ffi::Py_DecRef(object) };
    if length < 0 {
        return unsafe { raise_python_error(context, "PyObject_Length") };
    }
    unsafe { (api().int_from_i64)(context, length as i64, output) }
}

unsafe extern "C-unwind" fn python_proxy_roundtrip(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let _gil = enter_python();
    let proxy = match unsafe { create_proxy(context, arguments.read(), false) } {
        Ok(proxy) => proxy,
        Err(status) => return status,
    };
    let stored = unsafe { proxy_payload(proxy) };
    if stored.is_null() {
        unsafe { ffi::Py_DecRef(proxy) };
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::RUNTIME_ERROR,
                "invalid PyTonicProxy payload",
            )
        };
    }
    let stored = unsafe { &*stored.cast::<ProxyPayload>() };
    let status = unsafe { (api().persistent_borrow)(context, stored.handle, output) };
    unsafe { ffi::Py_DecRef(proxy) };
    status
}

unsafe extern "C-unwind" fn python_proxy(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let _gil = enter_python();
    let proxy = match unsafe { create_proxy(context, arguments.read(), false) } {
        Ok(proxy) => proxy,
        Err(status) => return status,
    };
    let payload = unsafe { proxy_payload(proxy) };
    if payload.is_null() {
        unsafe { ffi::Py_DecRef(proxy) };
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::RUNTIME_ERROR,
                "invalid PyTonicProxy payload",
            )
        };
    }
    let status =
        unsafe { (api().foreign_create)(context, proxy.cast(), &TONIC_PROXY_VTABLE, output) };
    if status != TonicStatus::OK {
        unsafe { ffi::Py_DecRef(proxy) };
        return status;
    }
    unsafe { (*payload).tonic_wrappers += 1 };
    let status = unsafe { (api().persistent_release)(context, (*payload).handle) };
    if status == TonicStatus::OK {
        unsafe { (*payload).strong = false };
    }
    status
}

unsafe extern "C-unwind" fn python_invoke_proxy_int(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut proxy = ptr::null_mut();
    let status = unsafe {
        (api().foreign_borrow_payload)(
            context,
            arguments.read(),
            CPYTHON_PROXY_ADAPTER_ID,
            &mut proxy,
        )
    };
    if status != TonicStatus::OK {
        return status;
    }
    let mut argument = 0;
    let status = unsafe { (api().int_as_i64)(context, arguments.add(1).read(), &mut argument) };
    if status != TonicStatus::OK {
        return status;
    }
    let _gil = enter_python();
    let argument = unsafe { ffi::PyLong_FromLongLong(argument) };
    if argument.is_null() {
        return unsafe { raise_python_error(context, "PyLong_FromLongLong") };
    }
    let _active = ActiveContextGuard::enter(context);
    let result = unsafe { ffi::PyObject_CallOneArg(proxy.cast(), argument) };
    unsafe { ffi::Py_DecRef(argument) };
    if result.is_null() {
        return unsafe { raise_python_error(context, "PyTonicProxy callback") };
    }
    let mut overflow = 0;
    let value = unsafe { ffi::PyLong_AsLongLongAndOverflow(result, &mut overflow) };
    unsafe { ffi::Py_DecRef(result) };
    if overflow != 0 || unsafe { !ffi::PyErr_Occurred().is_null() } {
        return unsafe { raise_python_error(context, "PyTonicProxy result conversion") };
    }
    unsafe { (api().int_from_i64)(context, value, output) }
}

unsafe extern "C-unwind" fn python_invoke_proxy2(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut proxy = ptr::null_mut();
    let status = unsafe {
        (api().foreign_borrow_payload)(
            context,
            arguments.read(),
            CPYTHON_PROXY_ADAPTER_ID,
            &mut proxy,
        )
    };
    if status != TonicStatus::OK {
        return status;
    }
    let _gil = enter_python();
    let left = match unsafe { tonic_to_python(context, arguments.add(1).read()) } {
        Ok(object) => object,
        Err(status) => return status,
    };
    let right = match unsafe { tonic_to_python(context, arguments.add(2).read()) } {
        Ok(object) => object,
        Err(status) => {
            unsafe { ffi::Py_DecRef(left) };
            return status;
        }
    };
    let python_arguments = [left, right];
    let _active = ActiveContextGuard::enter(context);
    let result = unsafe {
        ffi::PyObject_Vectorcall(
            proxy.cast(),
            python_arguments.as_ptr(),
            python_arguments.len(),
            ptr::null_mut(),
        )
    };
    unsafe {
        ffi::Py_DecRef(left);
        ffi::Py_DecRef(right);
    }
    if result.is_null() {
        return unsafe { raise_python_error(context, "PyTonicProxy callback") };
    }
    unsafe { python_to_tonic(context, result, output) }
}

unsafe extern "C-unwind" fn python_close_proxy(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut proxy = ptr::null_mut();
    let status = unsafe {
        (api().foreign_borrow_payload)(
            context,
            arguments.read(),
            CPYTHON_PROXY_ADAPTER_ID,
            &mut proxy,
        )
    };
    if status != TonicStatus::OK {
        return status;
    }
    let _gil = enter_python();
    let payload = unsafe { proxy_payload(proxy.cast()) };
    if payload.is_null() {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::RUNTIME_ERROR,
                "invalid PyTonicProxy payload",
            )
        };
    }
    let payload = unsafe { &mut *payload.cast::<ProxyPayload>() };
    if !payload.closed {
        let mut matches = 0;
        let status = unsafe { (api().runtime_owner_matches)(context, payload.owner, &mut matches) };
        if status != TonicStatus::OK {
            return status;
        }
        if matches == 0 {
            return unsafe {
                raise_bridge_error(
                    context,
                    TonicExceptionKind::RUNTIME_ERROR,
                    "PyTonicProxy belongs to another Tonic runtime",
                )
            };
        }
        if payload.strong {
            let status =
                unsafe { (api().persistent_release_deferred)(payload.owner, payload.handle) };
            if status != TonicStatus::OK {
                return status;
            }
            payload.strong = false;
        }
        payload.closed = true;
    }
    unsafe { (api().none)(context, output) }
}

unsafe extern "C-unwind" fn python_call_int1(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: VM validates the three-argument declaration and contiguous slice.
    let module = match unsafe { tonic_string(context, arguments.read()) } {
        Ok(value) => value,
        Err(error) => {
            return unsafe {
                (api().raise_exception)(
                    context,
                    TonicExceptionKind::TYPE_ERROR,
                    error.message.as_ptr(),
                    error.message.len(),
                )
            }
        }
    };
    let function = match unsafe { tonic_string(context, arguments.add(1).read()) } {
        Ok(value) => value,
        Err(error) => {
            return unsafe {
                (api().raise_exception)(
                    context,
                    TonicExceptionKind::TYPE_ERROR,
                    error.message.as_ptr(),
                    error.message.len(),
                )
            }
        }
    };
    let mut argument = 0;
    let status = unsafe { (api().int_as_i64)(context, arguments.add(2).read(), &mut argument) };
    if status != TonicStatus::OK {
        return status;
    }
    let Ok(module) = CString::new(module) else {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::VALUE_ERROR,
                "Python module name contains NUL",
            )
        };
    };
    let Ok(function) = CString::new(function) else {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::VALUE_ERROR,
                "Python function name contains NUL",
            )
        };
    };
    let _gil = enter_python();
    let module_object = unsafe { ffi::PyImport_ImportModule(module.as_ptr()) };
    if module_object.is_null() {
        return unsafe { raise_python_error(context, "PyImport_ImportModule") };
    }
    let callable = unsafe { ffi::PyObject_GetAttrString(module_object, function.as_ptr()) };
    unsafe { ffi::Py_DecRef(module_object) };
    if callable.is_null() {
        return unsafe { raise_python_error(context, "PyObject_GetAttrString") };
    }
    let argument_object = unsafe { ffi::PyLong_FromLongLong(argument) };
    if argument_object.is_null() {
        unsafe { ffi::Py_DecRef(callable) };
        return unsafe { raise_python_error(context, "PyLong_FromLongLong") };
    }
    let result = unsafe { ffi::PyObject_CallOneArg(callable, argument_object) };
    unsafe {
        ffi::Py_DecRef(argument_object);
        ffi::Py_DecRef(callable);
    }
    if result.is_null() {
        return unsafe { raise_python_error(context, "PyObject_CallOneArg") };
    }
    let mut overflow = 0;
    let value = unsafe { ffi::PyLong_AsLongLongAndOverflow(result, &mut overflow) };
    unsafe { ffi::Py_DecRef(result) };
    if overflow != 0 || unsafe { !ffi::PyErr_Occurred().is_null() } {
        return unsafe { raise_python_error(context, "integer result conversion") };
    }
    unsafe { (api().int_from_i64)(context, value, output) }
}

unsafe extern "C-unwind" fn python_call1(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let module = match unsafe { tonic_string(context, arguments.read()) } {
        Ok(value) => value,
        Err(error) => {
            return unsafe {
                raise_bridge_error(context, TonicExceptionKind::TYPE_ERROR, &error.message)
            }
        }
    };
    let function = match unsafe { tonic_string(context, arguments.add(1).read()) } {
        Ok(value) => value,
        Err(error) => {
            return unsafe {
                raise_bridge_error(context, TonicExceptionKind::TYPE_ERROR, &error.message)
            }
        }
    };
    let Ok(module) = CString::new(module) else {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::VALUE_ERROR,
                "Python module name contains NUL",
            )
        };
    };
    let Ok(function) = CString::new(function) else {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::VALUE_ERROR,
                "Python function name contains NUL",
            )
        };
    };
    let _gil = enter_python();
    let argument = match unsafe { tonic_to_python(context, arguments.add(2).read()) } {
        Ok(object) => object,
        Err(status) => return status,
    };
    let module_object = unsafe { ffi::PyImport_ImportModule(module.as_ptr()) };
    if module_object.is_null() {
        unsafe { ffi::Py_DecRef(argument) };
        return unsafe { raise_python_error(context, "PyImport_ImportModule") };
    }
    let callable = unsafe { ffi::PyObject_GetAttrString(module_object, function.as_ptr()) };
    unsafe { ffi::Py_DecRef(module_object) };
    if callable.is_null() {
        unsafe { ffi::Py_DecRef(argument) };
        return unsafe { raise_python_error(context, "PyObject_GetAttrString") };
    }
    let _active = ActiveContextGuard::enter(context);
    let result = unsafe { ffi::PyObject_CallOneArg(callable, argument) };
    unsafe {
        ffi::Py_DecRef(argument);
        ffi::Py_DecRef(callable);
    }
    if result.is_null() {
        return unsafe { raise_python_error(context, "PyObject_CallOneArg") };
    }
    unsafe { python_to_tonic(context, result, output) }
}

unsafe extern "C-unwind" fn python_call(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let module = match unsafe { tonic_string(context, arguments.read()) } {
        Ok(value) => value,
        Err(error) => {
            return unsafe {
                raise_bridge_error(context, TonicExceptionKind::TYPE_ERROR, &error.message)
            };
        }
    };
    let function = match unsafe { tonic_string(context, arguments.add(1).read()) } {
        Ok(value) => value,
        Err(error) => {
            return unsafe {
                raise_bridge_error(context, TonicExceptionKind::TYPE_ERROR, &error.message)
            };
        }
    };
    let positional = unsafe { arguments.add(2).read() };
    let keywords = unsafe { arguments.add(3).read() };
    let mut positional_kind = TonicValueKind::OTHER;
    let status = unsafe { (api().value_kind)(context, positional, &mut positional_kind) };
    if status != TonicStatus::OK {
        return status;
    }
    if positional_kind != TonicValueKind::LIST && positional_kind != TonicValueKind::TUPLE {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::TYPE_ERROR,
                "python.call positional arguments must be a list or tuple",
            )
        };
    }
    let mut keyword_kind = TonicValueKind::OTHER;
    let status = unsafe { (api().value_kind)(context, keywords, &mut keyword_kind) };
    if status != TonicStatus::OK {
        return status;
    }
    if keyword_kind != TonicValueKind::DICT {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::TYPE_ERROR,
                "python.call keyword arguments must be a dict",
            )
        };
    }
    let Ok(module) = CString::new(module) else {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::VALUE_ERROR,
                "Python module name contains NUL",
            )
        };
    };
    let Ok(function) = CString::new(function) else {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::VALUE_ERROR,
                "Python function name contains NUL",
            )
        };
    };

    let _gil = enter_python();
    let module_object = unsafe { ffi::PyImport_ImportModule(module.as_ptr()) };
    if module_object.is_null() {
        return unsafe { raise_python_error(context, "PyImport_ImportModule") };
    }
    let callable = unsafe { ffi::PyObject_GetAttrString(module_object, function.as_ptr()) };
    unsafe { ffi::Py_DecRef(module_object) };
    if callable.is_null() {
        return unsafe { raise_python_error(context, "PyObject_GetAttrString") };
    }

    let mut state = ToPythonState {
        memo: Vec::new(),
        depth: 0,
    };
    let positional_sequence =
        match unsafe { tonic_to_python_inner(context, positional, &mut state) } {
            Ok(object) => object,
            Err(status) => {
                unsafe { ffi::Py_DecRef(callable) };
                return status;
            }
        };
    let positional_tuple = if positional_kind == TonicValueKind::TUPLE {
        unsafe { ffi::Py_IncRef(positional_sequence) };
        positional_sequence
    } else {
        unsafe { ffi::PySequence_Tuple(positional_sequence) }
    };
    if positional_tuple.is_null() {
        unsafe {
            ffi::Py_DecRef(positional_sequence);
            ffi::Py_DecRef(callable);
        }
        return unsafe { raise_python_error(context, "PySequence_Tuple") };
    }
    let keyword_dict = match unsafe { tonic_to_python_inner(context, keywords, &mut state) } {
        Ok(object) => object,
        Err(status) => {
            unsafe {
                ffi::Py_DecRef(positional_tuple);
                ffi::Py_DecRef(positional_sequence);
                ffi::Py_DecRef(callable);
            }
            return status;
        }
    };
    let _active = ActiveContextGuard::enter(context);
    let result = unsafe { ffi::PyObject_Call(callable, positional_tuple, keyword_dict) };
    unsafe {
        ffi::Py_DecRef(keyword_dict);
        ffi::Py_DecRef(positional_tuple);
        ffi::Py_DecRef(positional_sequence);
        ffi::Py_DecRef(callable);
    }
    if result.is_null() {
        return unsafe { raise_python_error(context, "PyObject_Call") };
    }
    unsafe { python_to_tonic(context, result, output) }
}

unsafe extern "C-unwind" fn python_invoke(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let mut callable = ptr::null_mut();
    let mut callable_is_proxy = true;
    let mut status = unsafe {
        (api().foreign_borrow_payload)(
            context,
            arguments.read(),
            CPYTHON_PROXY_ADAPTER_ID,
            &mut callable,
        )
    };
    if status != TonicStatus::OK {
        callable_is_proxy = false;
        status = unsafe {
            (api().foreign_borrow_payload)(
                context,
                arguments.read(),
                CPYTHON_ADAPTER_ID,
                &mut callable,
            )
        };
    }
    if status != TonicStatus::OK {
        return status;
    }
    if !callable_is_proxy {
        callable = unsafe { foreign_pyobject(callable) }.cast();
        if callable.is_null() {
            return TonicStatus::INVALID_ARGUMENT;
        }
    }
    let positional = unsafe { arguments.add(1).read() };
    let keywords = unsafe { arguments.add(2).read() };
    let mut positional_kind = TonicValueKind::OTHER;
    status = unsafe { (api().value_kind)(context, positional, &mut positional_kind) };
    if status != TonicStatus::OK {
        return status;
    }
    if positional_kind != TonicValueKind::LIST && positional_kind != TonicValueKind::TUPLE {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::TYPE_ERROR,
                "python.invoke positional arguments must be a list or tuple",
            )
        };
    }
    let mut keyword_kind = TonicValueKind::OTHER;
    status = unsafe { (api().value_kind)(context, keywords, &mut keyword_kind) };
    if status != TonicStatus::OK {
        return status;
    }
    if keyword_kind != TonicValueKind::DICT {
        return unsafe {
            raise_bridge_error(
                context,
                TonicExceptionKind::TYPE_ERROR,
                "python.invoke keyword arguments must be a dict",
            )
        };
    }

    let _gil = enter_python();
    let mut state = ToPythonState {
        memo: Vec::new(),
        depth: 0,
    };
    let positional_sequence =
        match unsafe { tonic_to_python_inner(context, positional, &mut state) } {
            Ok(object) => object,
            Err(status) => return status,
        };
    let positional_tuple = if positional_kind == TonicValueKind::TUPLE {
        unsafe { ffi::Py_IncRef(positional_sequence) };
        positional_sequence
    } else {
        unsafe { ffi::PySequence_Tuple(positional_sequence) }
    };
    if positional_tuple.is_null() {
        unsafe { ffi::Py_DecRef(positional_sequence) };
        return unsafe { raise_python_error(context, "PySequence_Tuple") };
    }
    let keyword_dict = match unsafe { tonic_to_python_inner(context, keywords, &mut state) } {
        Ok(object) => object,
        Err(status) => {
            unsafe {
                ffi::Py_DecRef(positional_tuple);
                ffi::Py_DecRef(positional_sequence);
            }
            return status;
        }
    };
    let _active = ActiveContextGuard::enter(context);
    let result = unsafe { ffi::PyObject_Call(callable.cast(), positional_tuple, keyword_dict) };
    unsafe {
        ffi::Py_DecRef(keyword_dict);
        ffi::Py_DecRef(positional_tuple);
        ffi::Py_DecRef(positional_sequence);
    }
    if result.is_null() {
        return unsafe { raise_python_error(context, "PyObject_Call") };
    }
    unsafe { python_to_tonic(context, result, output) }
}

/// Installs the first isolated CPython compatibility module. CPython is lazily
/// initialized on first call and deliberately remains loaded until process exit.
pub fn register(vm: &mut Vm) -> Result<()> {
    for (name, arity, function) in [
        ("abs", 1, python_abs as tonic_runtime::CNativeFn),
        (
            "echo_float",
            1,
            python_echo_float as tonic_runtime::CNativeFn,
        ),
        ("echo_str", 1, python_echo_str as tonic_runtime::CNativeFn),
        ("make_list", 1, python_make_list as tonic_runtime::CNativeFn),
        ("len", 1, python_length as tonic_runtime::CNativeFn),
        (
            "length_of_int",
            1,
            python_length_of_int as tonic_runtime::CNativeFn,
        ),
        (
            "proxy_roundtrip",
            1,
            python_proxy_roundtrip as tonic_runtime::CNativeFn,
        ),
        ("call_int1", 3, python_call_int1 as tonic_runtime::CNativeFn),
        ("call1", 3, python_call1 as tonic_runtime::CNativeFn),
        ("call", 4, python_call as tonic_runtime::CNativeFn),
        ("invoke", 3, python_invoke as tonic_runtime::CNativeFn),
        ("proxy", 1, python_proxy as tonic_runtime::CNativeFn),
        (
            "invoke_proxy_int",
            2,
            python_invoke_proxy_int as tonic_runtime::CNativeFn,
        ),
        (
            "invoke_proxy2",
            3,
            python_invoke_proxy2 as tonic_runtime::CNativeFn,
        ),
        (
            "close_proxy",
            1,
            python_close_proxy as tonic_runtime::CNativeFn,
        ),
    ] {
        vm.register_c_native(
            "python",
            name,
            arity,
            function,
            TONIC_ABI_VERSION,
            CAP_FOREIGN_OBJECT_V1
                | CAP_PERSISTENT_HANDLES_V1
                | CAP_CONTAINER_ACCESS_V1
                | CAP_PROTOCOL_ACCESS_V1
                | CAP_CROSS_COLLECTOR_V1
                | tonic_runtime::c_api::CAP_RUNTIME_OWNER_V1,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C-unwind" fn cached_proxy_identity(
        context: *mut TonicContext,
        arguments: *const TonicHandle,
        _: usize,
        output: *mut TonicHandle,
    ) -> TonicStatus {
        let _gil = enter_python();
        let first = match unsafe { create_proxy(context, arguments.read(), true) } {
            Ok(proxy) => proxy,
            Err(status) => return status,
        };
        let second = match unsafe { create_proxy(context, arguments.read(), true) } {
            Ok(proxy) => proxy,
            Err(status) => {
                unsafe { ffi::Py_DecRef(first) };
                return status;
            }
        };
        let identical = first == second;
        unsafe {
            ffi::Py_DecRef(first);
            ffi::Py_DecRef(second);
            (api().bool_from)(context, u32::from(identical), output)
        }
    }

    #[test]
    fn primitive_call_and_foreign_pyobject_roundtrip() {
        let program = tonic_compiler::compile(
            "import python\nprint(python.abs(-42))\nprint(python.echo_float(1.25))\nprint(python.echo_str('é字'))\nprint(python.proxy_roundtrip(99))\nprint(python.call_int1('math','isqrt',81))\nprint(python.call1('builtins','abs',-7))\nprint(python.call1('math','sqrt',81))\nprint(python.call1('builtins','str',41))\nprint(python.call1('builtins','bool',None))\nobj=python.make_list(7)\nprint(python.len(obj))\nprint(python.call1('builtins','len',obj))\niterator=python.call1('builtins','iter',obj)\nprint(python.call1('builtins','next',iterator))",
            "cpython",
        )
        .unwrap();
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        register(&mut vm).unwrap();
        let mut output = Vec::new();
        vm.run(&program, &mut output).unwrap();
        assert_eq!(
            output,
            "42\n1.25\né字\n99\n9\n7\n9.0\n41\nFalse\n1\n1\n7\n".as_bytes()
        );
        assert_eq!(vm.active_handles(), 0);
        assert_eq!(vm.stats.foreign_wrapper_creations, 2);
        vm.run(
            &tonic_compiler::compile("pass", "release-python").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.stats.foreign_destructor_calls, 2);
    }

    #[test]
    fn bigint_containers_aliases_cycles_and_keywords_cross_the_bridge() {
        let source = "import python\nprint(python.call1('builtins','abs',-1267650600228229401496703205376))\nprint(python.call1('builtins','sorted',[3,1,2]))\nprint(python.call1('builtins','tuple',[1,2]))\nprint(python.call1('builtins','dict',{'x':1,'y':2}))\nprint(python.call('builtins','round',[1.25],{'ndigits':1}))\ncycle=[None]\ncycle[0]=cycle\nprint(python.call1('builtins','repr',cycle))\nprint(python.call1('builtins','list',cycle))";
        let program = tonic_compiler::compile(source, "cpython-containers").unwrap();
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = Some(1);
        register(&mut vm).unwrap();
        let mut output = Vec::new();
        vm.run(&program, &mut output).unwrap();
        assert_eq!(
            output,
            b"1267650600228229401496703205376\n[1, 2, 3]\n(1, 2)\n{'x': 1, 'y': 2}\n1.2\n[[...]]\n[[[...]]]\n"
        );
        assert_eq!(vm.active_handles(), 0);
    }

    #[test]
    fn general_call_validates_argument_container_shapes() {
        let program = tonic_compiler::compile(
            "import python\npython.call('builtins','round',1,{})",
            "cpython-call-shape",
        )
        .unwrap();
        let mut vm = Vm::new().unwrap();
        register(&mut vm).unwrap();
        let error = vm.run(&program, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind, "TypeError");
        assert!(error.message.contains("list or tuple"));
    }

    #[test]
    fn proxy_forwards_attributes_properties_repr_and_keyword_calls() {
        let source = "import python\nclass Box:\n    def __init__(self,value):\n        self.value=value\n    @property\n    def doubled(self):\n        return self.value*2\nbox=Box(4)\nproxy=python.proxy(box)\nprint(python.call('builtins','getattr',[proxy,'value'],{}))\nprint(python.call('builtins','getattr',[proxy,'doubled'],{}))\npython.call('builtins','setattr',[proxy,'value',9],{})\nprint(box.value)\nprint(python.call1('builtins','repr',proxy))\nreturned=python.call1('builtins','list',[proxy])[0]\nreturned.value=10\nprint(box.value)\nother=Box(3)\nother_proxy=python.proxy(other)\npython.call('builtins','setattr',[proxy,'child',other_proxy],{})\nbox.child.value=6\nprint(other.value)\ndef add(left,right=0):\n    return left+right\ncallable_proxy=python.proxy(add)\nprint(python.invoke(callable_proxy,[20],{'right':22}))";
        let program = tonic_compiler::compile(source, "cpython-proxy-protocols").unwrap();
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = Some(1);
        register(&mut vm).unwrap();
        let mut output = Vec::new();
        vm.run(&program, &mut output).unwrap();
        assert_eq!(output, b"4\n8\n9\n<Box instance>\n10\n6\n42\n");
        assert_eq!(vm.active_handles(), 3);

        vm.run(
            &tonic_compiler::compile("pass", "proxy-protocol-release").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
    }

    #[test]
    fn proxy_attribute_errors_keep_their_python_exception_class() {
        let source = "import python\nclass Box:\n    pass\nproxy=python.proxy(Box())\npython.call('builtins','getattr',[proxy,'missing'],{})";
        let program = tonic_compiler::compile(source, "cpython-proxy-attribute-error").unwrap();
        let mut vm = Vm::new().unwrap();
        register(&mut vm).unwrap();
        let error = vm.run(&program, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind, "PythonError");
        assert!(
            error.message.contains("AttributeError"),
            "{}",
            error.message
        );

        vm.run(
            &tonic_compiler::compile("pass", "proxy-attribute-error-release").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
    }

    #[test]
    fn weak_proxy_cache_reuses_only_live_identity() {
        let program = tonic_compiler::compile(
            "import testbridge\nclass Box:\n    pass\nprint(testbridge.cached_identity(Box()))",
            "cpython-proxy-identity",
        )
        .unwrap();
        let mut vm = Vm::new().unwrap();
        vm.register_c_native(
            "testbridge",
            "cached_identity",
            1,
            cached_proxy_identity,
            TONIC_ABI_VERSION,
            CAP_FOREIGN_OBJECT_V1
                | CAP_PERSISTENT_HANDLES_V1
                | CAP_CONTAINER_ACCESS_V1
                | CAP_PROTOCOL_ACCESS_V1
                | tonic_runtime::c_api::CAP_RUNTIME_OWNER_V1,
        )
        .unwrap();
        let mut output = Vec::new();
        vm.run(&program, &mut output).unwrap();
        assert_eq!(output, b"True\n");
        assert_eq!(vm.active_handles(), 0);
    }

    #[test]
    fn bridge_cycle_is_collected_without_explicit_close() {
        let source = "import python\nholder=[None]\ndef identity(value):\n    len(holder)\n    return value\nproxy=python.proxy(identity)\nholder[0]=proxy";
        let mut vm = Vm::new().unwrap();
        register(&mut vm).unwrap();
        vm.run(
            &tonic_compiler::compile(source, "proxy-cycle-automatic").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(vm.active_handles(), 1);
        vm.run(
            &tonic_compiler::compile("pass", "proxy-cycle-unroot").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
        assert_eq!(vm.stats.foreign_destructor_calls, 1);
    }

    #[test]
    fn foreign_pyobject_graph_cycle_is_collected_without_explicit_close() {
        let source = "import python\ntypes=python.call1('builtins','__import__','types')\nnamespace_type=python.call('builtins','getattr',[types,'SimpleNamespace'],{})\nholder=python.invoke(namespace_type,[],{})\nclass Box:\n    pass\nbox=Box()\npython.call('builtins','setattr',[holder,'proxy',box],{})\nbox.holder=holder";
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        register(&mut vm).unwrap();
        vm.run(
            &tonic_compiler::compile(source, "foreign-pyobject-cycle").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(
            vm.active_handles(),
            2,
            "proxy starts with one strong root and one non-rooting trace token"
        );

        vm.run(
            &tonic_compiler::compile("pass", "foreign-pyobject-unroot").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
    }

    #[test]
    fn external_reference_to_foreign_graph_promotes_then_releases_proxy_target() {
        let source = "import python\ntypes=python.call1('builtins','__import__','types')\nnamespace_type=python.call('builtins','getattr',[types,'SimpleNamespace'],{})\nholder=python.invoke(namespace_type,[],{})\nclass Box:\n    pass\nbox=Box()\npython.call('builtins','setattr',[holder,'proxy',box],{})\nbox.holder=holder\nsysmod=python.call1('builtins','__import__','sys')\npython.call('builtins','setattr',[sysmod,'_tonic_graph_holder',holder],{})";
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = None;
        register(&mut vm).unwrap();
        vm.run(
            &tonic_compiler::compile(source, "foreign-graph-external").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.run(
            &tonic_compiler::compile("pass", "foreign-graph-drop-tonic-roots").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.active_handles(),
            2,
            "external CPython graph keeps strong and trace handles"
        );

        let release = "import python\nsysmod=python.call1('builtins','__import__','sys')\npython.call('builtins','delattr',[sysmod,'_tonic_graph_holder'],{})";
        vm.run(
            &tonic_compiler::compile(release, "foreign-graph-release-external").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.run(
            &tonic_compiler::compile("pass", "foreign-graph-release-globals").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
    }

    #[test]
    fn external_python_reference_keeps_proxy_target_until_release() {
        let source = "import python\nclass Box:\n    pass\nbox=Box()\nproxy=python.proxy(box)\nsysmod=python.call1('builtins','__import__','sys')\npython.call('builtins','setattr',[sysmod,'_tonic_test_proxy',proxy],{})";
        let mut vm = Vm::new().unwrap();
        register(&mut vm).unwrap();
        vm.run(
            &tonic_compiler::compile(source, "proxy-external-root").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.run(
            &tonic_compiler::compile("pass", "proxy-external-unroot-tonic").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.active_handles(),
            2,
            "external proxy keeps one strong root and one trace token"
        );

        let release = "import python\nsysmod=python.call1('builtins','__import__','sys')\npython.call('builtins','delattr',[sysmod,'_tonic_test_proxy'],{})";
        vm.run(
            &tonic_compiler::compile(release, "proxy-external-release").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.run(
            &tonic_compiler::compile("pass", "proxy-external-drain").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
    }

    #[test]
    fn adapter_guard_rejects_non_python_values() {
        let program =
            tonic_compiler::compile("import python\npython.len([1])", "wrong-adapter").unwrap();
        let mut vm = Vm::new().unwrap();
        register(&mut vm).unwrap();
        assert_eq!(
            vm.run(&program, &mut Vec::new()).unwrap_err().kind,
            "TypeError"
        );
    }

    #[test]
    fn python_exception_message_is_captured_and_indicator_is_cleared() {
        let program =
            tonic_compiler::compile("import python\npython.length_of_int(7)", "python-error")
                .unwrap();
        let mut vm = Vm::new().unwrap();
        register(&mut vm).unwrap();
        let error = vm.run(&program, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind, "PythonError");
        assert!(error.message.contains("has no len()"), "{}", error.message);

        let mut output = Vec::new();
        vm.run(
            &tonic_compiler::compile("import python\nprint(python.abs(-2))", "after-error")
                .unwrap(),
            &mut output,
        )
        .unwrap();
        assert_eq!(output, b"2\n");
    }

    #[test]
    fn long_lived_proxy_forwards_callback_and_releases_to_owning_runtime() {
        let source = "import python\ndef add(left,right):\n    kept=[left]\n    return kept[0]+right\nproxy=python.proxy(add)\nprint(python.invoke_proxy2(proxy,20,22))";
        let program = tonic_compiler::compile(source, "proxy-callback").unwrap();
        let mut vm = Vm::new().unwrap();
        vm.gc_interval = Some(1);
        register(&mut vm).unwrap();
        let mut output = Vec::new();
        vm.run(&program, &mut output).unwrap();
        assert_eq!(output, b"42\n");
        assert_eq!(vm.stats.callback_calls, 1);
        assert_eq!(vm.active_handles(), 1);
        vm.run(
            &tonic_compiler::compile("pass", "proxy-release").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
        assert_eq!(vm.stats.foreign_destructor_calls, 1);
    }

    #[test]
    fn tonic_callback_exception_crosses_python_and_returns_as_python_error() {
        let source = "import python\ndef fail(value):\n    return 1//value\nproxy=python.proxy(fail)\npython.invoke_proxy_int(proxy,0)";
        let program = tonic_compiler::compile(source, "proxy-error").unwrap();
        let mut vm = Vm::new().unwrap();
        register(&mut vm).unwrap();
        let error = vm.run(&program, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind, "PythonError");
        assert!(
            error.message.contains("division or modulo by zero"),
            "{}",
            error.message
        );
        assert!(error.message.contains("RuntimeError"), "{}", error.message);

        vm.run(
            &tonic_compiler::compile("pass", "proxy-cleanup").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
    }

    #[test]
    fn explicit_close_breaks_cross_runtime_cycle_and_closed_proxy_is_diagnostic() {
        let source = "import python\nholder=[None]\ndef identity(value):\n    len(holder)\n    return value\nproxy=python.proxy(identity)\nholder[0]=proxy\npython.close_proxy(proxy)\npython.close_proxy(proxy)\npython.invoke_proxy_int(proxy,1)";
        let program = tonic_compiler::compile(source, "proxy-cycle-close").unwrap();
        let mut vm = Vm::new().unwrap();
        register(&mut vm).unwrap();
        let error = vm.run(&program, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind, "PythonError");
        assert!(error.message.contains("PyTonicProxy is closed"));
        // Explicit close drops the persistent root immediately; the non-rooting
        // foreign trace token is released at the following collection refresh.
        assert_eq!(vm.active_handles(), 1);

        vm.run(
            &tonic_compiler::compile("pass", "proxy-cycle-release").unwrap(),
            &mut Vec::new(),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(vm.active_handles(), 0);
        assert_eq!(vm.stats.foreign_destructor_calls, 1);
    }
}
