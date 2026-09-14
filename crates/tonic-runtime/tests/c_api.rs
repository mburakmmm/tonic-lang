use std::{ptr, sync::Mutex};
use tonic_compiler::compile;
use tonic_core::diagnostic::Result;
use tonic_runtime::{
    c_api::{
        negotiate_api, CAP_BUFFER_V1, CAP_CONTAINER_ACCESS_V1, CAP_CORE, CAP_FOREIGN_OBJECT_V1,
        CAP_PROTOCOL_ACCESS_V1,
    },
    Context, DType, Handle, TonicBuffer, TonicContext, TonicExceptionKind, TonicHandle,
    TonicPersistentHandle, TonicStatus, Vm, BUFFER_C_CONTIGUOUS, BUFFER_WRITABLE,
    TONIC_ABI_VERSION,
};

unsafe extern "C-unwind" fn build_container(
    context: *mut TonicContext,
    _: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_CONTAINER_ACCESS_V1).unwrap();
    let decimal = b"1267650600228229401496703205376";
    let mut integer = TonicHandle::default();
    let status =
        unsafe { (api.int_from_decimal)(context, decimal.as_ptr(), decimal.len(), &mut integer) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut required = 0;
    let status = unsafe { (api.int_decimal)(context, integer, ptr::null_mut(), 0, &mut required) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut rendered = vec![0; required];
    let status = unsafe {
        (api.int_decimal)(
            context,
            integer,
            rendered.as_mut_ptr(),
            rendered.len(),
            &mut required,
        )
    };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(rendered, decimal);

    let mut boolean = TonicHandle::default();
    let status = unsafe { (api.bool_from)(context, 1, &mut boolean) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut truth = 0;
    let status = unsafe { (api.bool_as)(context, boolean, &mut truth) };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(truth, 1);
    let tuple_items = [integer, boolean];
    let mut tuple = TonicHandle::default();
    let status =
        unsafe { (api.tuple_new)(context, tuple_items.as_ptr(), tuple_items.len(), &mut tuple) };
    if status != TonicStatus::OK {
        return status;
    }

    let mut list = TonicHandle::default();
    let status = unsafe { (api.list_new)(context, &mut list) };
    if status != TonicStatus::OK {
        return status;
    }
    let status = unsafe { (api.list_append)(context, list, tuple) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut length = 0;
    let status = unsafe { (api.sequence_len)(context, list, &mut length) };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(length, 1);
    let mut item = TonicHandle::default();
    let status = unsafe { (api.sequence_get)(context, list, 0, &mut item) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut identical = 0;
    let status = unsafe { (api.is_identical)(context, tuple, item, &mut identical) };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(identical, 1);

    let mut dict = TonicHandle::default();
    let status = unsafe { (api.dict_new)(context, &mut dict) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut key = TonicHandle::default();
    let status = unsafe { (api.str_from_utf8)(context, b"value".as_ptr(), 5, &mut key) };
    if status != TonicStatus::OK {
        return status;
    }
    let status = unsafe { (api.dict_set)(context, dict, key, list) };
    if status != TonicStatus::OK {
        return status;
    }
    let status = unsafe { (api.dict_len)(context, dict, &mut length) };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(length, 1);
    let mut entry_key = TonicHandle::default();
    let mut entry_value = TonicHandle::default();
    let status = unsafe { (api.dict_entry)(context, dict, 0, &mut entry_key, &mut entry_value) };
    if status != TonicStatus::OK {
        return status;
    }
    let status = unsafe { (api.is_identical)(context, list, entry_value, &mut identical) };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(identical, 1);
    unsafe { output.write(dict) };
    TonicStatus::OK
}

unsafe extern "C-unwind" fn use_protocols(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(
        TONIC_ABI_VERSION,
        0,
        CAP_CONTAINER_ACCESS_V1 | CAP_FOREIGN_OBJECT_V1 | CAP_PROTOCOL_ACCESS_V1,
    )
    .unwrap();
    let owner = unsafe { arguments.read() };
    let callable = unsafe { arguments.add(1).read() };
    let mut reference = TonicHandle::default();
    let status = unsafe { (api.foreign_reference_create)(context, owner, &mut reference) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut borrowed = TonicHandle::default();
    let status = unsafe { (api.foreign_reference_borrow)(context, reference, &mut borrowed) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut identical = 0;
    let status = unsafe { (api.is_identical)(context, owner, borrowed, &mut identical) };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(identical, 1);
    let status = unsafe { (api.foreign_reference_release)(context, reference) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut value = TonicHandle::default();
    let status = unsafe { (api.get_attr)(context, owner, b"value".as_ptr(), 5, &mut value) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut integer = 0;
    let status = unsafe { (api.int_as_i64)(context, value, &mut integer) };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(integer, 4);
    let status = unsafe { (api.int_from_i64)(context, 9, &mut value) };
    if status != TonicStatus::OK {
        return status;
    }
    let status = unsafe { (api.set_attr)(context, owner, b"value".as_ptr(), 5, value) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut representation = TonicHandle::default();
    let status = unsafe { (api.repr_value)(context, owner, &mut representation) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut required = 0;
    let status =
        unsafe { (api.str_utf8)(context, representation, ptr::null_mut(), 0, &mut required) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut text = vec![0; required];
    let status = unsafe {
        (api.str_utf8)(
            context,
            representation,
            text.as_mut_ptr(),
            text.len(),
            &mut required,
        )
    };
    if status != TonicStatus::OK {
        return status;
    }
    assert_eq!(text, b"<Box instance>");

    let mut positional = TonicHandle::default();
    let status = unsafe { (api.int_from_i64)(context, 20, &mut positional) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut keyword_value = TonicHandle::default();
    let status = unsafe { (api.int_from_i64)(context, 22, &mut keyword_value) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut keyword_name = TonicHandle::default();
    let status = unsafe { (api.str_from_utf8)(context, b"right".as_ptr(), 5, &mut keyword_name) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut keywords = TonicHandle::default();
    let status = unsafe { (api.dict_new)(context, &mut keywords) };
    if status != TonicStatus::OK {
        return status;
    }
    let status = unsafe { (api.dict_set)(context, keywords, keyword_name, keyword_value) };
    if status != TonicStatus::OK {
        return status;
    }
    unsafe { (api.call_kw)(context, callable, &positional, 1, keywords, output) }
}

unsafe extern "C-unwind" fn double(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    argument_count: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    assert_eq!(argument_count, 1);
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_CORE).unwrap();
    // SAFETY: VM validates arity and supplies one live argument handle.
    let argument = unsafe { arguments.read() };
    // SAFETY: context and output remain valid for this native call.
    unsafe { (api.add)(context, argument, argument, output) }
}

unsafe extern "C-unwind" fn fail(
    context: *mut TonicContext,
    _: *const TonicHandle,
    _: usize,
    _: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_CORE).unwrap();
    let message = b"failure from extension";
    // SAFETY: context and message bytes remain valid for the call.
    unsafe {
        (api.raise_exception)(
            context,
            TonicExceptionKind::VALUE_ERROR,
            message.as_ptr(),
            message.len(),
        )
    }
}

unsafe extern "C-unwind" fn panic_in_extension(
    _: *mut TonicContext,
    _: *const TonicHandle,
    _: usize,
    _: *mut TonicHandle,
) -> TonicStatus {
    panic!("native extension bug")
}

unsafe extern "C-unwind" fn initialize_extension(
    api: *const tonic_runtime::TonicApi,
    context: *mut TonicContext,
) -> TonicStatus {
    // SAFETY: VM supplies its static immutable function table.
    let api = unsafe { &*api };
    assert_eq!(api.abi_version, TONIC_ABI_VERSION);
    let mut version = 0;
    // SAFETY: context and version output live for the init call.
    unsafe {
        (api.query_capability)(
            context,
            tonic_runtime::TonicCapability::CORE,
            1,
            &mut version,
        )
    }
}

unsafe extern "C-unwind" fn panic_in_init(
    _: *const tonic_runtime::TonicApi,
    _: *mut TonicContext,
) -> TonicStatus {
    panic!("extension init bug")
}

unsafe extern "C-unwind" fn clear_previous_error(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_CORE).unwrap();
    let mut integer = 0;
    // SAFETY: VM supplies one valid handle, but it deliberately has float type.
    assert_eq!(
        unsafe { (api.int_as_i64)(context, arguments.read(), &mut integer) },
        TonicStatus::EXCEPTION
    );
    // A successful operation begins by clearing the preceding operation's error.
    // SAFETY: context and output remain valid for this native call.
    unsafe { (api.int_from_i64)(context, 7, output) }
}

static SAVED: Mutex<Option<TonicHandle>> = Mutex::new(None);

unsafe extern "C-unwind" fn save_local(
    _: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: VM supplies one readable argument and one writable output slot.
    let argument = unsafe { arguments.read() };
    *SAVED.lock().unwrap() = Some(argument);
    // SAFETY: output is writable for the call.
    unsafe { output.write(argument) };
    TonicStatus::OK
}

unsafe extern "C-unwind" fn use_stale_local(
    context: *mut TonicContext,
    _: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_CORE).unwrap();
    let saved = SAVED.lock().unwrap().expect("saved handle");
    let mut integer = 0;
    // SAFETY: the stale logical handle is data; the runtime validates it.
    let status = unsafe { (api.int_as_i64)(context, saved, &mut integer) };
    if status != TonicStatus::OK {
        return status;
    }
    // SAFETY: context and output remain valid for this native call.
    unsafe { (api.int_from_i64)(context, integer, output) }
}

unsafe extern "C-unwind" fn c_sum(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_BUFFER_V1).unwrap();
    let mut descriptor = std::mem::MaybeUninit::<TonicBuffer>::uninit();
    // SAFETY: VM supplies one live buffer handle and descriptor output storage.
    let status = unsafe {
        (api.buffer_export)(
            context,
            arguments.read(),
            BUFFER_C_CONTIGUOUS,
            descriptor.as_mut_ptr(),
        )
    };
    if status != TonicStatus::OK {
        return status;
    }
    // SAFETY: successful export initialized the complete descriptor.
    let mut descriptor = unsafe { descriptor.assume_init() };
    assert_eq!(descriptor.dtype, DType::F64);
    assert_eq!(descriptor.item_size, std::mem::size_of::<f64>());
    assert_eq!(descriptor.ndim, 1);
    let total: f64 = {
        // SAFETY: export owns a live, aligned f64 allocation for byte_len bytes.
        let values = unsafe {
            std::slice::from_raw_parts(
                descriptor.data.cast::<f64>(),
                descriptor.byte_len / descriptor.item_size,
            )
        };
        values.iter().sum()
    };
    // SAFETY: descriptor and its dedicated owner belong to this context.
    let status = unsafe { (api.buffer_release)(context, &mut descriptor) };
    if status != TonicStatus::OK {
        return status;
    }
    assert!(descriptor.data.is_null());
    // The dedicated owner cannot be released twice. A following successful API
    // operation clears this deliberately induced exception.
    // SAFETY: the cleared descriptor remains initialized storage.
    assert_eq!(
        unsafe { (api.buffer_release)(context, &mut descriptor) },
        TonicStatus::EXCEPTION
    );
    // SAFETY: result output remains writable through the callback.
    unsafe { (api.float_from_f64)(context, total, output) }
}

unsafe extern "C-unwind" fn require_writable(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    _: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_BUFFER_V1).unwrap();
    let mut descriptor = std::mem::MaybeUninit::<TonicBuffer>::uninit();
    // SAFETY: VM supplies one live handle and descriptor storage.
    unsafe {
        (api.buffer_export)(
            context,
            arguments.read(),
            BUFFER_WRITABLE,
            descriptor.as_mut_ptr(),
        )
    }
}

unsafe extern "C-unwind" fn invoke_tonic(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    argument_count: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    assert_eq!(argument_count, 2);
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_CORE).unwrap();
    // SAFETY: VM validates two argument handles for this native definition.
    let callable = unsafe { arguments.read() };
    // SAFETY: the second argument is contiguous with the first and output is live.
    unsafe { (api.call)(context, callable, arguments.add(1), 1, output) }
}

unsafe extern "C-unwind" fn persistent_roundtrip(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_CORE).unwrap();
    let mut persistent = TonicPersistentHandle::default();
    // SAFETY: VM supplies one live argument and persistent output storage.
    let status = unsafe { (api.persistent_create)(context, arguments.read(), &mut persistent) };
    if status != TonicStatus::OK {
        return status;
    }
    // SAFETY: persistent belongs to this runtime and output is writable.
    let status = unsafe { (api.persistent_borrow)(context, persistent, output) };
    if status != TonicStatus::OK {
        return status;
    }
    // SAFETY: token remains live and is consumed once.
    unsafe { (api.persistent_release)(context, persistent) }
}

unsafe extern "C-unwind" fn deferred_persistent_release(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(
        TONIC_ABI_VERSION,
        0,
        tonic_runtime::c_api::CAP_RUNTIME_OWNER_V1,
    )
    .unwrap();
    let mut persistent = TonicPersistentHandle::default();
    let status = unsafe { (api.persistent_create)(context, arguments.read(), &mut persistent) };
    if status != TonicStatus::OK {
        return status;
    }
    let mut owner = ptr::null_mut();
    let status = unsafe { (api.runtime_owner_acquire)(context, &mut owner) };
    if status != TonicStatus::OK {
        let _ = unsafe { (api.persistent_release)(context, persistent) };
        return status;
    }
    let mut matches = 0;
    assert_eq!(
        unsafe { (api.runtime_owner_matches)(context, owner, &mut matches) },
        TonicStatus::OK
    );
    assert_eq!(matches, 1);
    unsafe { output.write(arguments.read()) };
    let status = unsafe { (api.persistent_release_deferred)(owner, persistent) };
    assert_eq!(
        unsafe { (api.runtime_owner_release)(owner) },
        TonicStatus::OK
    );
    status
}

fn make_writable(context: &mut Context<'_>, _: &[Handle]) -> Result<Handle> {
    context.from_f64_buffer(&[1.0, 2.0, 3.0], &[3], true)
}

unsafe extern "C-unwind" fn increment_buffer(
    context: *mut TonicContext,
    arguments: *const TonicHandle,
    _: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    let api = negotiate_api(TONIC_ABI_VERSION, 0, CAP_BUFFER_V1).unwrap();
    let mut descriptor = std::mem::MaybeUninit::<TonicBuffer>::uninit();
    // SAFETY: VM supplies one live buffer handle and descriptor output storage.
    let status = unsafe {
        (api.buffer_export)(
            context,
            arguments.read(),
            BUFFER_WRITABLE | BUFFER_C_CONTIGUOUS,
            descriptor.as_mut_ptr(),
        )
    };
    if status != TonicStatus::OK {
        return status;
    }
    // SAFETY: successful export initialized a writable f64 descriptor.
    let mut descriptor = unsafe { descriptor.assume_init() };
    // SAFETY: writable export grants exclusive native access for this call.
    for value in unsafe {
        std::slice::from_raw_parts_mut(
            descriptor.data.cast::<f64>(),
            descriptor.byte_len / descriptor.item_size,
        )
    } {
        *value += 1.0;
    }
    // Preserve the original argument as the return before releasing only the
    // descriptor's dedicated owner handle.
    // SAFETY: one argument and the output slot are valid.
    unsafe { output.write(arguments.read()) };
    // SAFETY: descriptor belongs to this context.
    unsafe { (api.buffer_release)(context, &mut descriptor) }
}

#[test]
fn c_function_table_native_executes_without_guest_argument_containers() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native("demo", "double", 1, double, TONIC_ABI_VERSION, CAP_CORE)
        .unwrap();
    let mut output = Vec::new();
    vm.run(
        &compile("import demo\nprint(demo.double(21))", "c-api").unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, b"42\n");
    assert_eq!(vm.stats.native_calls, 1);
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn container_api_preserves_bigints_identity_and_nested_values() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native(
        "demo",
        "container",
        0,
        build_container,
        TONIC_ABI_VERSION,
        CAP_CONTAINER_ACCESS_V1,
    )
    .unwrap();
    let mut output = Vec::new();
    vm.run(
        &compile("import demo\nprint(demo.container())", "c-api-container").unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(
        output,
        b"{'value': [(1267650600228229401496703205376, True)]}\n"
    );
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn protocol_api_forwards_attributes_repr_and_keyword_calls() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native(
        "demo",
        "protocols",
        2,
        use_protocols,
        TONIC_ABI_VERSION,
        CAP_CONTAINER_ACCESS_V1 | CAP_FOREIGN_OBJECT_V1 | CAP_PROTOCOL_ACCESS_V1,
    )
    .unwrap();
    let source = "import demo\nclass Box:\n    def __init__(self,value):\n        self.value=value\ndef add(left,right=0):\n    return left+right\nbox=Box(4)\nprint(demo.protocols(box,add))\nprint(box.value)";
    let mut output = Vec::new();
    vm.run(&compile(source, "c-api-protocols").unwrap(), &mut output)
        .unwrap();
    assert_eq!(output, b"42\n9\n");
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn extension_init_negotiates_the_same_table_and_cleans_its_scope() {
    let mut vm = Vm::new().unwrap();
    let size = std::mem::size_of::<tonic_runtime::TonicApi>() as u32;
    vm.initialize_c_extension(initialize_extension, TONIC_ABI_VERSION, size, CAP_CORE)
        .unwrap();
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn extension_init_panic_is_contained() {
    let mut vm = Vm::new().unwrap();
    let error = vm
        .initialize_c_extension(panic_in_init, TONIC_ABI_VERSION, 0, CAP_CORE)
        .unwrap_err();
    assert_eq!(error.kind, "RuntimeError");
    assert!(error.message.contains("contained"));
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn explicit_exception_status_reaches_guest_and_does_not_leak() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native("demo", "fail", 0, fail, TONIC_ABI_VERSION, CAP_CORE)
        .unwrap();
    let error = vm
        .run(
            &compile("import demo\ndemo.fail()", "c-api").unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "ValueError");
    assert_eq!(error.message, "failure from extension");
    assert_eq!(vm.active_handles(), 0);
    vm.run(&compile("print(5)", "after").unwrap(), &mut Vec::new())
        .unwrap();
}

#[test]
fn each_successful_api_operation_clears_prior_exception_state() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native(
        "demo",
        "clear",
        1,
        clear_previous_error,
        TONIC_ABI_VERSION,
        CAP_CORE,
    )
    .unwrap();
    let mut output = Vec::new();
    vm.run(
        &compile("import demo\nprint(demo.clear(1.5))", "c-api").unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, b"7\n");
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn extension_panic_is_contained_and_vm_remains_usable() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native(
        "demo",
        "panic",
        0,
        panic_in_extension,
        TONIC_ABI_VERSION,
        CAP_CORE,
    )
    .unwrap();
    let error = vm
        .run(
            &compile("import demo\ndemo.panic()", "c-api").unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "RuntimeError");
    assert!(error.message.contains("contained"));
    assert_eq!(vm.active_handles(), 0);
    let mut output = Vec::new();
    vm.run(&compile("print(9)", "after").unwrap(), &mut output)
        .unwrap();
    assert_eq!(output, b"9\n");
}

#[test]
fn stale_local_handle_is_rejected_after_its_call_scope() {
    *SAVED.lock().unwrap() = None;
    let mut vm = Vm::new().unwrap();
    vm.register_c_native("demo", "save", 1, save_local, TONIC_ABI_VERSION, CAP_CORE)
        .unwrap();
    vm.register_c_native(
        "demo",
        "load",
        0,
        use_stale_local,
        TONIC_ABI_VERSION,
        CAP_CORE,
    )
    .unwrap();
    let error = vm
        .run(
            &compile("import demo\ndemo.save(3)\ndemo.load()", "c-api").unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "HandleError");
    assert_eq!(vm.active_handles(), 0);
    *SAVED.lock().unwrap() = None;
}

#[test]
fn c_registration_rejects_incompatible_contracts_before_mutation() {
    let mut vm = Vm::new().unwrap();
    assert_eq!(
        vm.register_c_native("bad", "version", 0, fail, 2, 0)
            .unwrap_err()
            .kind,
        "NativeAbiError"
    );
    assert_eq!(
        vm.register_c_native("bad", "capability", 0, fail, 1, 1 << 63)
            .unwrap_err()
            .kind,
        "NativeAbiError"
    );
    assert_eq!(vm.active_handles(), 0);
    // Keep an explicit null pointer assertion near the external contract.
    assert!(ptr::null::<TonicContext>().is_null());
}

#[test]
fn c_buffer_descriptor_exports_shape_stride_owner_and_zero_copy_data() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native(
        "native_buffer",
        "sum",
        1,
        c_sum,
        TONIC_ABI_VERSION,
        CAP_BUFFER_V1,
    )
    .unwrap();
    let mut output = Vec::new();
    vm.run(
        &compile(
            "import fastmath\nimport native_buffer\nvalues=fastmath.array([1.5,2.5,3.0])\nprint(native_buffer.sum(values))",
            "c-buffer",
        )
        .unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, b"7.0\n");
    assert_eq!(vm.stats.buffer_copies, 1);
    assert_eq!(vm.stats.buffer_exports, 1);
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn writable_export_mutates_nonmoving_storage_and_preserves_owner() {
    let mut vm = Vm::new().unwrap();
    vm.register_native("native_buffer", "make", 0, make_writable)
        .unwrap();
    vm.register_c_native(
        "native_buffer",
        "increment",
        1,
        increment_buffer,
        TONIC_ABI_VERSION,
        CAP_BUFFER_V1,
    )
    .unwrap();
    let mut output = Vec::new();
    vm.run(
        &compile(
            "import fastmath\nimport native_buffer\nvalues=native_buffer.make()\nnative_buffer.increment(values)\nprint(fastmath.sum(values))",
            "c-buffer",
        )
        .unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, b"9.0\n");
    assert_eq!(vm.stats.buffer_copies, 1);
    assert_eq!(vm.stats.buffer_exports, 2);
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn writable_request_for_read_only_buffer_sets_exception_without_owner_leak() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native(
        "native_buffer",
        "write",
        1,
        require_writable,
        TONIC_ABI_VERSION,
        CAP_BUFFER_V1,
    )
    .unwrap();
    let error = vm
        .run(
            &compile(
                "import fastmath\nimport native_buffer\nvalues=fastmath.array([1.0])\nnative_buffer.write(values)",
                "c-buffer",
            )
            .unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "BufferError");
    assert_eq!(vm.stats.buffer_exports, 0);
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn c_native_can_reenter_nested_tonic_frames_with_precise_roots() {
    let mut vm = Vm::new().unwrap();
    vm.gc_interval = Some(1);
    vm.register_c_native(
        "bridge",
        "invoke",
        2,
        invoke_tonic,
        TONIC_ABI_VERSION,
        CAP_CORE,
    )
    .unwrap();
    let source = "import bridge\ndef inner(value):\n    kept=[value]\n    return kept[0]+1\ndef outer(value):\n    kept=[value]\n    return bridge.invoke(inner,kept[0])+1\nprint(bridge.invoke(outer,40))";
    let mut output = Vec::new();
    vm.run(&compile(source, "reentry").unwrap(), &mut output)
        .unwrap();
    assert_eq!(output, b"42\n");
    assert_eq!(vm.stats.callback_calls, 2);
    assert_eq!(vm.stats.native_calls, 2);
    assert!(vm.stats.gc_collections > 0);
    assert_eq!(vm.active_handles(), 0);

    let error = vm
        .run(
            &compile(
                "import bridge\ndef fail(value):\n    return 1//value\nbridge.invoke(fail,0)",
                "reentry-error",
            )
            .unwrap(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind, "ZeroDivisionError");
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn c_persistent_handle_roundtrip_has_distinct_ownership_and_no_leak() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native(
        "persistent",
        "roundtrip",
        1,
        persistent_roundtrip,
        TONIC_ABI_VERSION,
        CAP_CORE,
    )
    .unwrap();
    let mut output = Vec::new();
    vm.run(
        &compile(
            "import persistent\nprint(persistent.roundtrip(42))",
            "persistent-c-api",
        )
        .unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, b"42\n");
    assert_eq!(vm.active_handles(), 0);
}

#[test]
fn runtime_owner_routes_deferred_release_without_retaining_vm_address() {
    let mut vm = Vm::new().unwrap();
    vm.register_c_native(
        "persistent",
        "defer",
        1,
        deferred_persistent_release,
        TONIC_ABI_VERSION,
        tonic_runtime::c_api::CAP_RUNTIME_OWNER_V1,
    )
    .unwrap();
    let mut output = Vec::new();
    vm.run(
        &compile(
            "import persistent\nprint(persistent.defer(41))",
            "deferred-owner",
        )
        .unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, b"41\n");
    assert_eq!(vm.active_handles(), 0);
}
