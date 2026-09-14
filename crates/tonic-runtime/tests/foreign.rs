use std::{
    ffi::c_void,
    mem,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};
use tonic_compiler::compile;
use tonic_core::diagnostic::Result;
use tonic_runtime::{
    c_api::{negotiate_api, CAP_FOREIGN_OBJECT_V1},
    Context, Handle, PersistentHandle, TonicContext, TonicForeignVTable, TonicHandle, TonicStatus,
    TonicTraceVisitor, Vm, FOREIGN_OWNED, TONIC_ABI_VERSION,
};

struct Payload {
    reference: TonicHandle,
}

static DESTROYS: AtomicUsize = AtomicUsize::new(0);
static TRACE_CALLS: AtomicUsize = AtomicUsize::new(0);
static PANIC_DESTROYS: AtomicUsize = AtomicUsize::new(0);
static TEST_LOCK: Mutex<()> = Mutex::new(());
static ACTIVE_PAYLOAD: AtomicUsize = AtomicUsize::new(0);
static SAVED_WRAPPER: Mutex<Option<PersistentHandle>> = Mutex::new(None);

unsafe extern "C-unwind" fn trace_payload(
    payload: *mut c_void,
    visitor: *mut TonicTraceVisitor,
) -> TonicStatus {
    TRACE_CALLS.fetch_add(1, Ordering::SeqCst);
    // SAFETY: the wrapper owns a Payload and the runtime supplies a live visitor.
    unsafe {
        let payload = &*payload.cast::<Payload>();
        ((*visitor).visit)(visitor, payload.reference)
    }
}

unsafe extern "C-unwind" fn panic_trace(_: *mut c_void, _: *mut TonicTraceVisitor) -> TonicStatus {
    panic!("trace bug")
}

unsafe extern "C-unwind" fn destroy_payload(payload: *mut c_void) {
    DESTROYS.fetch_add(1, Ordering::SeqCst);
    // SAFETY: successful foreign_create transfers exactly one Box allocation.
    drop(unsafe { Box::from_raw(payload.cast::<Payload>()) });
}

unsafe extern "C-unwind" fn panic_destroy(payload: *mut c_void) {
    PANIC_DESTROYS.fetch_add(1, Ordering::SeqCst);
    // SAFETY: consume the allocation before simulating a faulty destructor.
    drop(unsafe { Box::from_raw(payload.cast::<Payload>()) });
    panic!("destroy bug")
}

static VTABLE: TonicForeignVTable = TonicForeignVTable {
    struct_size: mem::size_of::<TonicForeignVTable>() as u32,
    abi_version: TONIC_ABI_VERSION,
    adapter_id: 7,
    flags: FOREIGN_OWNED,
    trace: Some(trace_payload),
    destroy: Some(destroy_payload),
};

static PANIC_TRACE_VTABLE: TonicForeignVTable = TonicForeignVTable {
    trace: Some(panic_trace),
    ..VTABLE
};

static PANIC_DESTROY_VTABLE: TonicForeignVTable = TonicForeignVTable {
    trace: None,
    destroy: Some(panic_destroy),
    ..VTABLE
};

unsafe fn wrap_with(
    context: *mut TonicContext,
    argument: TonicHandle,
    output: *mut TonicHandle,
    vtable: &'static TonicForeignVTable,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_FOREIGN_OBJECT_V1).unwrap();
    let mut reference = TonicHandle::default();
    // SAFETY: the native call supplies a live context, argument, and outputs.
    let status = unsafe { (api.foreign_reference_create)(context, argument, &mut reference) };
    if status != TonicStatus::OK {
        return status;
    }
    let payload = Box::into_raw(Box::new(Payload { reference })).cast();
    // SAFETY: payload, vtable, and output remain valid for the synchronous call.
    let status = unsafe { (api.foreign_create)(context, payload, vtable, output) };
    if status != TonicStatus::OK {
        // Ownership transfers only after successful wrapper creation.
        // SAFETY: runtime rejected the wrapper and did not retain the payload.
        drop(unsafe { Box::from_raw(payload.cast::<Payload>()) });
        // SAFETY: the unassociated reference still belongs to this native call.
        let _ = unsafe { (api.foreign_reference_release)(context, reference) };
    } else {
        ACTIVE_PAYLOAD.store(payload as usize, Ordering::SeqCst);
    }
    status
}

unsafe extern "C-unwind" fn replace_reference(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_FOREIGN_OBJECT_V1).unwrap();
    let mut reference = TonicHandle::default();
    // SAFETY: VM supplies one argument and live output storage.
    let status =
        unsafe { (api.foreign_reference_create)(context, arguments.read(), &mut reference) };
    if status != TonicStatus::OK {
        return status;
    }
    let payload = ACTIVE_PAYLOAD.load(Ordering::SeqCst) as *mut Payload;
    if payload.is_null() {
        // SAFETY: reference belongs to this context and was not associated.
        let _ = unsafe { (api.foreign_reference_release)(context, reference) };
        return TonicStatus::INVALID_ARGUMENT;
    }
    // SAFETY: the persistent wrapper keeps this payload alive for the test.
    unsafe { (*payload).reference = reference };
    // SAFETY: output remains writable and None creates a scoped local result.
    unsafe { (api.none)(context, output) }
}

fn save_wrapper(context: &mut Context<'_>, arguments: &[Handle]) -> Result<Handle> {
    *SAVED_WRAPPER.lock().unwrap() = Some(context.persist(arguments[0])?);
    context.none()
}

unsafe extern "C-unwind" fn wrap(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: VM validates the one-argument native declaration.
    unsafe { wrap_with(context, arguments.read(), output, &VTABLE) }
}

unsafe extern "C-unwind" fn wrap_panicking_trace(
    context: *mut TonicContext,
    _: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_FOREIGN_OBJECT_V1).unwrap();
    let payload = Box::into_raw(Box::new(Payload {
        reference: TonicHandle::default(),
    }))
    .cast();
    // SAFETY: payload, static vtable, and output are valid for the call.
    let status = unsafe { (api.foreign_create)(context, payload, &PANIC_TRACE_VTABLE, output) };
    if status != TonicStatus::OK {
        // SAFETY: failed creation did not transfer payload ownership.
        drop(unsafe { Box::from_raw(payload.cast::<Payload>()) });
    }
    status
}

unsafe extern "C-unwind" fn wrap_panicking_destroy(
    context: *mut TonicContext,
    _: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_FOREIGN_OBJECT_V1).unwrap();
    let payload = Box::into_raw(Box::new(Payload {
        reference: TonicHandle::default(),
    }))
    .cast();
    // SAFETY: payload, static vtable, and output are valid for the call.
    unsafe { (api.foreign_create)(context, payload, &PANIC_DESTROY_VTABLE, output) }
}

fn vm_with_foreign() -> Vm {
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = None;
    vm.register_c_native(
        "foreign",
        "wrap",
        1,
        wrap,
        TONIC_ABI_VERSION,
        CAP_FOREIGN_OBJECT_V1,
    )
    .unwrap();
    vm
}

#[test]
fn foreign_trace_keeps_managed_cycle_alive_then_major_collection_reclaims_it() {
    let _guard = TEST_LOCK.lock().unwrap();
    DESTROYS.store(0, Ordering::SeqCst);
    TRACE_CALLS.store(0, Ordering::SeqCst);
    let mut vm = vm_with_foreign();
    let first = compile(
        "import foreign\nchild=[None]\nwrapper=foreign.wrap(child)\nchild[0]=wrapper",
        "foreign-cycle",
    )
    .unwrap();
    vm.run(&first, &mut Vec::new()).unwrap();
    let live = vm.collect_garbage().unwrap();
    assert_eq!(DESTROYS.load(Ordering::SeqCst), 0);
    assert!(live.survivors >= 2);
    assert_eq!(vm.active_handles(), 1, "one owned foreign reference");

    vm.run(&compile("pass", "drop-cycle").unwrap(), &mut Vec::new())
        .unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(DESTROYS.load(Ordering::SeqCst), 1);
    assert_eq!(vm.active_handles(), 0);
    assert_eq!(vm.stats.foreign_destructor_calls, 1);
    assert_eq!(vm.stats.foreign_destructor_panics, 0);
    vm.collect_garbage().unwrap();
    assert_eq!(
        DESTROYS.load(Ordering::SeqCst),
        1,
        "destructor is exactly once"
    );
    assert!(TRACE_CALLS.load(Ordering::SeqCst) >= 2);
}

#[test]
fn shutdown_drains_owned_payloads_before_invalidating_foreign_references() {
    let _guard = TEST_LOCK.lock().unwrap();
    DESTROYS.store(0, Ordering::SeqCst);
    let mut vm = vm_with_foreign();
    vm.run(
        &compile("import foreign\nvalue=foreign.wrap([1])", "shutdown").unwrap(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(vm.active_handles(), 1);
    vm.shutdown().unwrap();
    assert_eq!(DESTROYS.load(Ordering::SeqCst), 1);
    assert_eq!(vm.active_handles(), 0);
    assert_eq!(vm.stats.foreign_destructor_calls, 1);
}

#[test]
fn trace_refresh_releases_removed_edges_and_remembers_new_nursery_edges() {
    let _guard = TEST_LOCK.lock().unwrap();
    *SAVED_WRAPPER.lock().unwrap() = None;
    let mut vm = vm_with_foreign();
    vm.register_c_native(
        "foreign",
        "replace",
        1,
        replace_reference,
        TONIC_ABI_VERSION,
        CAP_FOREIGN_OBJECT_V1,
    )
    .unwrap();
    vm.register_native("root", "save", 1, save_wrapper).unwrap();
    vm.run(
        &compile(
            "import foreign\nimport root\nroot.save(foreign.wrap([1]))",
            "old-edge",
        )
        .unwrap(),
        &mut Vec::new(),
    )
    .unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.active_handles(), 2, "persistent wrapper + old edge");

    vm.run(
        &compile("import foreign\nforeign.replace([2])", "new-edge").unwrap(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(vm.active_handles(), 3, "new edge awaits trace refresh");
    let collection = vm.collect_garbage().unwrap();
    assert!(
        collection.reclaimed >= 1,
        "removed old child becomes collectible"
    );
    assert_eq!(vm.active_handles(), 2, "old edge handle was released");

    let wrapper = SAVED_WRAPPER.lock().unwrap().take().unwrap();
    vm.context().unwrap().release_persistent(&wrapper).unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn trace_panics_become_guest_errors_without_transferring_payload_ownership() {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = None;
    vm.register_c_native(
        "foreign",
        "bad",
        1,
        wrap_panicking_trace,
        TONIC_ABI_VERSION,
        CAP_FOREIGN_OBJECT_V1,
    )
    .unwrap();
    let error = vm
        .run(
            &compile("import foreign\nforeign.bad(1)", "bad-trace").unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "ForeignError");
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn destructor_panics_are_contained_and_never_retried() {
    let _guard = TEST_LOCK.lock().unwrap();
    PANIC_DESTROYS.store(0, Ordering::SeqCst);
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = None;
    vm.register_c_native(
        "foreign",
        "bad_drop",
        0,
        wrap_panicking_destroy,
        TONIC_ABI_VERSION,
        CAP_FOREIGN_OBJECT_V1,
    )
    .unwrap();
    vm.run(
        &compile("import foreign\nforeign.bad_drop()", "bad-drop").unwrap(),
        &mut Vec::new(),
    )
    .unwrap();
    vm.collect_garbage().unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(PANIC_DESTROYS.load(Ordering::SeqCst), 1);
    assert_eq!(vm.stats.foreign_destructor_calls, 1);
    assert_eq!(vm.stats.foreign_destructor_panics, 1);
}

#[test]
fn foreign_capability_is_negotiated_explicitly() {
    let _guard = TEST_LOCK.lock().unwrap();
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_FOREIGN_OBJECT_V1).unwrap();
    assert!(api.capabilities & CAP_FOREIGN_OBJECT_V1 != 0);
}
