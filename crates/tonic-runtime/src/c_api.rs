//! Versioned C function-table ABI over opaque logical handles.
//!
//! Foreign code is trusted native code. Null/count pairs are validated, but an
//! arbitrary non-null pointer can still corrupt the process. No Rust layout or
//! managed heap address is part of this contract.

use crate::{
    buffer::DType,
    foreign::{ForeignSpec, TonicForeignVTable},
    native::{Context, Handle, ValueKind},
    BUFFER_C_CONTIGUOUS, BUFFER_WRITABLE,
};
use std::{
    mem,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr, slice,
    sync::Arc,
};
use tonic_core::diagnostic::{Diagnostic, Result};

pub const TONIC_ABI_VERSION: u32 = 1;
const CONTEXT_MAGIC: u64 = 0x544f_4e49_4341_4249;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct TonicHandle(u64);

impl TonicHandle {
    pub(crate) fn from_internal(handle: Handle) -> Self {
        Self(handle.raw())
    }
    pub(crate) fn into_internal(self) -> Handle {
        Handle::from_raw(self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct TonicPersistentHandle(u64);

impl TonicPersistentHandle {
    pub(crate) fn from_internal(handle: crate::native::PersistentHandle) -> Self {
        Self(handle.raw())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct TonicValueKind(u32);

impl TonicValueKind {
    pub const NONE: Self = Self(1);
    pub const BOOL: Self = Self(2);
    pub const INT: Self = Self(3);
    pub const FLOAT: Self = Self(4);
    pub const STR: Self = Self(5);
    pub const FOREIGN: Self = Self(6);
    pub const LIST: Self = Self(7);
    pub const TUPLE: Self = Self(8);
    pub const DICT: Self = Self(9);
    pub const OTHER: Self = Self(255);
}

/// Opaque in C (`typedef struct TonicContext TonicContext`).
#[repr(C)]
pub struct TonicContext {
    _private: [u8; 0],
}

/// Ref-counted stable runtime identity for delayed native work. Its allocation
/// contains an `Arc`, never a `Vm` pointer, and must be released through the API.
#[repr(C)]
pub struct TonicRuntimeOwner {
    _private: [u8; 0],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct TonicStatus(u32);

impl TonicStatus {
    pub const OK: Self = Self(0);
    pub const EXCEPTION: Self = Self(1);
    pub const INVALID_ARGUMENT: Self = Self(2);
    pub const ABI_MISMATCH: Self = Self(3);
    pub const UNSUPPORTED: Self = Self(4);
    pub const PANIC: Self = Self(5);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct TonicExceptionKind(u32);

impl TonicExceptionKind {
    pub const RUNTIME_ERROR: Self = Self(1);
    pub const TYPE_ERROR: Self = Self(2);
    pub const VALUE_ERROR: Self = Self(3);
    pub const OVERFLOW_ERROR: Self = Self(4);
    pub const HANDLE_ERROR: Self = Self(5);
    pub const PYTHON_ERROR: Self = Self(6);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct TonicCapability(u32);

impl TonicCapability {
    pub const CORE: Self = Self(1);
    pub const EXPLICIT_EXCEPTION_STATUS: Self = Self(2);
    pub const SCOPED_LOCAL_HANDLES: Self = Self(3);
    pub const PANIC_GUARD: Self = Self(4);
    pub const BUFFER_V1: Self = Self(5);
    pub const FOREIGN_OBJECT_V1: Self = Self(6);
    pub const PERSISTENT_HANDLES_V1: Self = Self(7);
    pub const RUNTIME_OWNER_V1: Self = Self(8);
    pub const CONTAINER_ACCESS_V1: Self = Self(9);
    pub const PROTOCOL_ACCESS_V1: Self = Self(10);
    pub const CROSS_COLLECTOR_V1: Self = Self(11);
}

pub const CAP_CORE: u64 = 1 << 0;
pub const CAP_EXPLICIT_EXCEPTION_STATUS: u64 = 1 << 1;
pub const CAP_SCOPED_LOCAL_HANDLES: u64 = 1 << 2;
pub const CAP_PANIC_GUARD: u64 = 1 << 3;
pub const CAP_BUFFER_V1: u64 = 1 << 4;
pub const CAP_FOREIGN_OBJECT_V1: u64 = 1 << 5;
pub const CAP_PERSISTENT_HANDLES_V1: u64 = 1 << 6;
pub const CAP_RUNTIME_OWNER_V1: u64 = 1 << 7;
pub const CAP_CONTAINER_ACCESS_V1: u64 = 1 << 8;
pub const CAP_PROTOCOL_ACCESS_V1: u64 = 1 << 9;
pub const CAP_CROSS_COLLECTOR_V1: u64 = 1 << 10;
pub const TONIC_CAPABILITIES: u64 = CAP_CORE
    | CAP_EXPLICIT_EXCEPTION_STATUS
    | CAP_SCOPED_LOCAL_HANDLES
    | CAP_PANIC_GUARD
    | CAP_BUFFER_V1
    | CAP_FOREIGN_OBJECT_V1
    | CAP_PERSISTENT_HANDLES_V1
    | CAP_RUNTIME_OWNER_V1
    | CAP_CONTAINER_ACCESS_V1
    | CAP_PROTOCOL_ACCESS_V1
    | CAP_CROSS_COLLECTOR_V1;

pub type CNativeFn = unsafe extern "C-unwind" fn(
    *mut TonicContext,
    *const TonicHandle,
    usize,
    *mut TonicHandle,
) -> TonicStatus;
pub type CExtensionInitFn =
    unsafe extern "C-unwind" fn(*const TonicApi, *mut TonicContext) -> TonicStatus;

type StatusClearFn = unsafe extern "C" fn(*mut TonicContext) -> TonicStatus;
type ExceptionTextFn =
    unsafe extern "C" fn(*mut TonicContext, *mut u8, usize, *mut usize) -> TonicStatus;
type RaiseFn =
    unsafe extern "C" fn(*mut TonicContext, TonicExceptionKind, *const u8, usize) -> TonicStatus;
type QueryCapabilityFn =
    unsafe extern "C" fn(*mut TonicContext, TonicCapability, u32, *mut u32) -> TonicStatus;
type NoneFn = unsafe extern "C" fn(*mut TonicContext, *mut TonicHandle) -> TonicStatus;
type ValueKindFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut TonicValueKind) -> TonicStatus;
type IsIdenticalFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, TonicHandle, *mut u32) -> TonicStatus;
type BoolFromFn = unsafe extern "C" fn(*mut TonicContext, u32, *mut TonicHandle) -> TonicStatus;
type BoolAsFn = unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut u32) -> TonicStatus;
type IntFromDecimalFn =
    unsafe extern "C" fn(*mut TonicContext, *const u8, usize, *mut TonicHandle) -> TonicStatus;
type IntDecimalFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut u8, usize, *mut usize) -> TonicStatus;
type ContainerNewFn = unsafe extern "C" fn(*mut TonicContext, *mut TonicHandle) -> TonicStatus;
type SequenceLenFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut usize) -> TonicStatus;
type SequenceGetFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, usize, *mut TonicHandle) -> TonicStatus;
type ListAppendFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, TonicHandle) -> TonicStatus;
type TupleNewFn = unsafe extern "C" fn(
    *mut TonicContext,
    *const TonicHandle,
    usize,
    *mut TonicHandle,
) -> TonicStatus;
type DictEntryFn = unsafe extern "C" fn(
    *mut TonicContext,
    TonicHandle,
    usize,
    *mut TonicHandle,
    *mut TonicHandle,
) -> TonicStatus;
type DictSetFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, TonicHandle, TonicHandle) -> TonicStatus;
type CallKwFn = unsafe extern "C" fn(
    *mut TonicContext,
    TonicHandle,
    *const TonicHandle,
    usize,
    TonicHandle,
    *mut TonicHandle,
) -> TonicStatus;
type AttrGetFn = unsafe extern "C" fn(
    *mut TonicContext,
    TonicHandle,
    *const u8,
    usize,
    *mut TonicHandle,
) -> TonicStatus;
type AttrSetFn = unsafe extern "C" fn(
    *mut TonicContext,
    TonicHandle,
    *const u8,
    usize,
    TonicHandle,
) -> TonicStatus;
type ReprFn = unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut TonicHandle) -> TonicStatus;
type IntFromFn = unsafe extern "C" fn(*mut TonicContext, i64, *mut TonicHandle) -> TonicStatus;
type IntAsFn = unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut i64) -> TonicStatus;
type FloatFromFn = unsafe extern "C" fn(*mut TonicContext, f64, *mut TonicHandle) -> TonicStatus;
type FloatAsFn = unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut f64) -> TonicStatus;
type StringFromUtf8Fn =
    unsafe extern "C" fn(*mut TonicContext, *const u8, usize, *mut TonicHandle) -> TonicStatus;
type StringUtf8Fn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut u8, usize, *mut usize) -> TonicStatus;
type BinaryFn = unsafe extern "C" fn(
    *mut TonicContext,
    TonicHandle,
    TonicHandle,
    *mut TonicHandle,
) -> TonicStatus;
type CallFn = unsafe extern "C" fn(
    *mut TonicContext,
    TonicHandle,
    *const TonicHandle,
    usize,
    *mut TonicHandle,
) -> TonicStatus;
type BufferExportFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, u64, *mut TonicBuffer) -> TonicStatus;
type BufferReleaseFn = unsafe extern "C" fn(*mut TonicContext, *mut TonicBuffer) -> TonicStatus;
type ForeignReferenceCreateFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut TonicHandle) -> TonicStatus;
type ForeignReferenceReleaseFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle) -> TonicStatus;
type ForeignReferenceBorrowFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut TonicHandle) -> TonicStatus;
type ForeignCreateFn = unsafe extern "C" fn(
    *mut TonicContext,
    *mut std::ffi::c_void,
    *const TonicForeignVTable,
    *mut TonicHandle,
) -> TonicStatus;
type ForeignBorrowPayloadFn = unsafe extern "C" fn(
    *mut TonicContext,
    TonicHandle,
    u64,
    *mut *mut std::ffi::c_void,
) -> TonicStatus;
type PersistentCreateFn =
    unsafe extern "C" fn(*mut TonicContext, TonicHandle, *mut TonicPersistentHandle) -> TonicStatus;
type PersistentBorrowFn =
    unsafe extern "C" fn(*mut TonicContext, TonicPersistentHandle, *mut TonicHandle) -> TonicStatus;
type PersistentReleaseFn =
    unsafe extern "C" fn(*mut TonicContext, TonicPersistentHandle) -> TonicStatus;
type RuntimeOwnerAcquireFn =
    unsafe extern "C" fn(*mut TonicContext, *mut *mut TonicRuntimeOwner) -> TonicStatus;
type RuntimeOwnerReleaseFn = unsafe extern "C" fn(*mut TonicRuntimeOwner) -> TonicStatus;
type RuntimeOwnerMatchesFn =
    unsafe extern "C" fn(*mut TonicContext, *const TonicRuntimeOwner, *mut u32) -> TonicStatus;
type RuntimeExecutionIdFn = unsafe extern "C" fn(*mut TonicContext, *mut u64) -> TonicStatus;
type PersistentReleaseDeferredFn =
    unsafe extern "C" fn(*const TonicRuntimeOwner, TonicPersistentHandle) -> TonicStatus;
type ForeignReferenceReleaseDeferredFn =
    unsafe extern "C" fn(*const TonicRuntimeOwner, TonicHandle) -> TonicStatus;
type RuntimeIdentityFn = unsafe extern "C" fn(*mut TonicContext, *mut u64) -> TonicStatus;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TonicBuffer {
    pub struct_size: u32,
    pub dtype: DType,
    pub flags: u64,
    pub data: *mut u8,
    pub byte_len: usize,
    pub item_size: usize,
    pub ndim: u32,
    pub reserved: u32,
    pub shape: *const usize,
    pub strides: *const isize,
    pub owner: TonicHandle,
}

impl TonicBuffer {
    fn empty() -> Self {
        Self {
            struct_size: mem::size_of::<Self>() as u32,
            dtype: DType::F64,
            flags: 0,
            data: ptr::null_mut(),
            byte_len: 0,
            item_size: 0,
            ndim: 0,
            reserved: 0,
            shape: ptr::null(),
            strides: ptr::null(),
            owner: TonicHandle(0),
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct TonicApi {
    pub struct_size: u32,
    pub abi_version: u32,
    pub capabilities: u64,
    pub status_clear: StatusClearFn,
    pub exception_kind: ExceptionTextFn,
    pub exception_message: ExceptionTextFn,
    pub raise_exception: RaiseFn,
    pub query_capability: QueryCapabilityFn,
    pub none: NoneFn,
    pub int_from_i64: IntFromFn,
    pub int_as_i64: IntAsFn,
    pub float_from_f64: FloatFromFn,
    pub float_as_f64: FloatAsFn,
    pub str_from_utf8: StringFromUtf8Fn,
    pub str_utf8: StringUtf8Fn,
    pub add: BinaryFn,
    pub call: CallFn,
    pub buffer_export: BufferExportFn,
    pub buffer_release: BufferReleaseFn,
    pub foreign_reference_create: ForeignReferenceCreateFn,
    pub foreign_reference_release: ForeignReferenceReleaseFn,
    pub foreign_create: ForeignCreateFn,
    pub foreign_borrow_payload: ForeignBorrowPayloadFn,
    pub persistent_create: PersistentCreateFn,
    pub persistent_borrow: PersistentBorrowFn,
    pub persistent_release: PersistentReleaseFn,
    pub runtime_owner_acquire: RuntimeOwnerAcquireFn,
    pub runtime_owner_release: RuntimeOwnerReleaseFn,
    pub runtime_owner_matches: RuntimeOwnerMatchesFn,
    pub runtime_execution_id: RuntimeExecutionIdFn,
    pub persistent_release_deferred: PersistentReleaseDeferredFn,
    pub value_kind: ValueKindFn,
    pub is_identical: IsIdenticalFn,
    pub bool_from: BoolFromFn,
    pub bool_as: BoolAsFn,
    pub int_from_decimal: IntFromDecimalFn,
    pub int_decimal: IntDecimalFn,
    pub list_new: ContainerNewFn,
    pub list_append: ListAppendFn,
    pub tuple_new: TupleNewFn,
    pub sequence_len: SequenceLenFn,
    pub sequence_get: SequenceGetFn,
    pub dict_new: ContainerNewFn,
    pub dict_len: SequenceLenFn,
    pub dict_entry: DictEntryFn,
    pub dict_set: DictSetFn,
    pub call_kw: CallKwFn,
    pub get_attr: AttrGetFn,
    pub set_attr: AttrSetFn,
    pub repr_value: ReprFn,
    pub foreign_reference_borrow: ForeignReferenceBorrowFn,
    pub runtime_identity: RuntimeIdentityFn,
    pub foreign_reference_release_deferred: ForeignReferenceReleaseDeferredFn,
}

struct CallContext<'context, 'heap> {
    magic: u64,
    context: &'context mut Context<'heap>,
    exception: Option<Diagnostic>,
}

enum BoundaryError {
    Diagnostic(Diagnostic),
    InvalidArgument(&'static str),
    Unsupported(&'static str),
}

impl From<Diagnostic> for BoundaryError {
    fn from(value: Diagnostic) -> Self {
        Self::Diagnostic(value)
    }
}

fn exception_name(kind: TonicExceptionKind) -> Option<&'static str> {
    match kind.0 {
        1 => Some("RuntimeError"),
        2 => Some("TypeError"),
        3 => Some("ValueError"),
        4 => Some("OverflowError"),
        5 => Some("HandleError"),
        6 => Some("PythonError"),
        _ => None,
    }
}

unsafe fn state<'a, 'heap>(context: *mut TonicContext) -> Option<&'a mut CallContext<'a, 'heap>> {
    if context.is_null() {
        return None;
    }
    // SAFETY: `invoke_native` is the sole producer of TonicContext pointers. The
    // ABI is trusted in-process code and the pointer is valid only during that call.
    let state = unsafe { &mut *context.cast::<CallContext<'a, 'heap>>() };
    (state.magic == CONTEXT_MAGIC).then_some(state)
}

unsafe fn boundary(
    context: *mut TonicContext,
    clear: bool,
    operation: impl FnOnce(&mut CallContext<'_, '_>) -> std::result::Result<(), BoundaryError>,
) -> TonicStatus {
    // SAFETY: pointer provenance is documented by `state`; null is rejected.
    let Some(state) = (unsafe { state(context) }) else {
        return TonicStatus::INVALID_ARGUMENT;
    };
    if clear {
        state.exception = None;
    }
    match catch_unwind(AssertUnwindSafe(|| operation(state))) {
        Ok(Ok(())) => TonicStatus::OK,
        Ok(Err(BoundaryError::Diagnostic(error))) => {
            state.exception = Some(error);
            TonicStatus::EXCEPTION
        }
        Ok(Err(BoundaryError::InvalidArgument(message))) => {
            state.exception = Some(Diagnostic::new("NativeApiError", message));
            TonicStatus::INVALID_ARGUMENT
        }
        Ok(Err(BoundaryError::Unsupported(message))) => {
            state.exception = Some(Diagnostic::new("NativeApiError", message));
            TonicStatus::UNSUPPORTED
        }
        Err(_) => {
            state.exception = Some(Diagnostic::new(
                "RuntimeError",
                "panic was contained at the Tonic C ABI boundary",
            ));
            TonicStatus::PANIC
        }
    }
}

fn require_out<T>(out: *mut T) -> std::result::Result<(), BoundaryError> {
    if out.is_null() {
        Err(BoundaryError::InvalidArgument("null output pointer"))
    } else {
        Ok(())
    }
}

unsafe fn write_text(
    bytes: &[u8],
    output: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> std::result::Result<(), BoundaryError> {
    require_out(required)?;
    // SAFETY: caller supplied a writable `usize` output pointer.
    unsafe { required.write(bytes.len()) };
    if output.is_null() && capacity == 0 {
        return Ok(());
    }
    if capacity < bytes.len() {
        return Err(BoundaryError::InvalidArgument(
            "exception text buffer is too small",
        ));
    }
    if !bytes.is_empty() {
        if output.is_null() {
            return Err(BoundaryError::InvalidArgument("null exception text buffer"));
        }
        // SAFETY: capacity was checked above and the ABI requires `output` to
        // reference at least `capacity` writable bytes without overlap.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
    }
    Ok(())
}

unsafe extern "C" fn status_clear(context: *mut TonicContext) -> TonicStatus {
    // SAFETY: all access is validated and protected by the boundary.
    unsafe { boundary(context, true, |_| Ok(())) }
}

unsafe extern "C" fn exception_kind(
    context: *mut TonicContext,
    output: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> TonicStatus {
    // SAFETY: `write_text` validates null/capacity pairs before writing.
    unsafe {
        boundary(context, false, |state| {
            let text = state.exception.as_ref().map_or("", |error| error.kind);
            write_text(text.as_bytes(), output, capacity, required)
        })
    }
}

unsafe extern "C" fn exception_message(
    context: *mut TonicContext,
    output: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> TonicStatus {
    // SAFETY: `write_text` validates null/capacity pairs before writing.
    unsafe {
        boundary(context, false, |state| {
            let text = state
                .exception
                .as_ref()
                .map_or("", |error| error.message.as_str());
            write_text(text.as_bytes(), output, capacity, required)
        })
    }
}

unsafe extern "C" fn raise_exception(
    context: *mut TonicContext,
    kind: TonicExceptionKind,
    message: *const u8,
    message_len: usize,
) -> TonicStatus {
    // SAFETY: the byte slice is built only after validating its pointer.
    let status = unsafe {
        boundary(context, true, |state| {
            if message_len != 0 && message.is_null() {
                return Err(BoundaryError::InvalidArgument("null exception message"));
            }
            let bytes = if message_len == 0 {
                &[]
            } else {
                // SAFETY: the ABI requires `message_len` readable bytes.
                slice::from_raw_parts(message, message_len)
            };
            let text = std::str::from_utf8(bytes)
                .map_err(|_| BoundaryError::InvalidArgument("exception message is not UTF-8"))?;
            let kind = exception_name(kind)
                .ok_or(BoundaryError::InvalidArgument("unknown exception kind"))?;
            state.exception = Some(Diagnostic::new(kind, text));
            Ok(())
        })
    };
    match status {
        TonicStatus::OK => TonicStatus::EXCEPTION,
        status => status,
    }
}

unsafe extern "C" fn query_capability(
    context: *mut TonicContext,
    capability: TonicCapability,
    minimum_version: u32,
    version: *mut u32,
) -> TonicStatus {
    // SAFETY: output is checked before writing.
    unsafe {
        boundary(context, true, |_| {
            require_out(version)?;
            let supported = match capability.0 {
                1..=11 => 1,
                _ => return Err(BoundaryError::Unsupported("capability is not supported")),
            };
            if minimum_version > supported {
                return Err(BoundaryError::Unsupported(
                    "capability version is not supported",
                ));
            }
            // SAFETY: `version` was validated above.
            version.write(supported);
            Ok(())
        })
    }
}

unsafe extern "C" fn none(context: *mut TonicContext, output: *mut TonicHandle) -> TonicStatus {
    // SAFETY: output is checked before writing.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let value = state.context.none()?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn value_kind(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut TonicValueKind,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let kind = match state.context.value_kind(value.into_internal())? {
                ValueKind::None => TonicValueKind::NONE,
                ValueKind::Bool => TonicValueKind::BOOL,
                ValueKind::Int => TonicValueKind::INT,
                ValueKind::Float => TonicValueKind::FLOAT,
                ValueKind::Str => TonicValueKind::STR,
                ValueKind::Foreign => TonicValueKind::FOREIGN,
                ValueKind::List => TonicValueKind::LIST,
                ValueKind::Tuple => TonicValueKind::TUPLE,
                ValueKind::Dict => TonicValueKind::DICT,
                ValueKind::Other => TonicValueKind::OTHER,
            };
            output.write(kind);
            Ok(())
        })
    }
}

unsafe extern "C" fn is_identical(
    context: *mut TonicContext,
    left: TonicHandle,
    right: TonicHandle,
    output: *mut u32,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(u32::from(
                state
                    .context
                    .is_identical(left.into_internal(), right.into_internal())?,
            ));
            Ok(())
        })
    }
}

unsafe extern "C" fn bool_from(
    context: *mut TonicContext,
    value: u32,
    output: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            if value > 1 {
                return Err(BoundaryError::InvalidArgument(
                    "bool input must be zero or one",
                ));
            }
            let value = state.context.from_bool(value != 0)?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn bool_as(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut u32,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(u32::from(state.context.to_bool(value.into_internal())?));
            Ok(())
        })
    }
}

unsafe extern "C" fn int_from_decimal(
    context: *mut TonicContext,
    decimal: *const u8,
    decimal_len: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            if decimal_len != 0 && decimal.is_null() {
                return Err(BoundaryError::InvalidArgument("null decimal integer input"));
            }
            let bytes = if decimal_len == 0 {
                &[]
            } else {
                slice::from_raw_parts(decimal, decimal_len)
            };
            let decimal = std::str::from_utf8(bytes)
                .map_err(|_| BoundaryError::InvalidArgument("decimal integer is not UTF-8"))?;
            let value = state.context.int_from_decimal(decimal)?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn int_decimal(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            let decimal = state.context.int_decimal(value.into_internal())?;
            write_text(decimal.as_bytes(), output, capacity, required)
        })
    }
}

unsafe extern "C" fn list_new(context: *mut TonicContext, output: *mut TonicHandle) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(TonicHandle::from_internal(state.context.new_list()?));
            Ok(())
        })
    }
}

unsafe extern "C" fn list_append(
    context: *mut TonicContext,
    list: TonicHandle,
    item: TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            state
                .context
                .list_append(list.into_internal(), item.into_internal())?;
            Ok(())
        })
    }
}

unsafe extern "C" fn tuple_new(
    context: *mut TonicContext,
    items: *const TonicHandle,
    item_count: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            if item_count != 0 && items.is_null() {
                return Err(BoundaryError::InvalidArgument("null tuple items"));
            }
            let items = if item_count == 0 {
                &[]
            } else {
                slice::from_raw_parts(items, item_count)
            };
            let items = items
                .iter()
                .copied()
                .map(TonicHandle::into_internal)
                .collect::<Vec<_>>();
            output.write(TonicHandle::from_internal(state.context.new_tuple(&items)?));
            Ok(())
        })
    }
}

unsafe extern "C" fn sequence_len(
    context: *mut TonicContext,
    sequence: TonicHandle,
    output: *mut usize,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(state.context.sequence_len(sequence.into_internal())?);
            Ok(())
        })
    }
}

unsafe extern "C" fn sequence_get(
    context: *mut TonicContext,
    sequence: TonicHandle,
    index: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let item = state
                .context
                .sequence_get(sequence.into_internal(), index)?;
            output.write(TonicHandle::from_internal(item));
            Ok(())
        })
    }
}

unsafe extern "C" fn dict_new(context: *mut TonicContext, output: *mut TonicHandle) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(TonicHandle::from_internal(state.context.new_dict()?));
            Ok(())
        })
    }
}

unsafe extern "C" fn dict_len(
    context: *mut TonicContext,
    dict: TonicHandle,
    output: *mut usize,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(state.context.dict_len(dict.into_internal())?);
            Ok(())
        })
    }
}

unsafe extern "C" fn dict_entry(
    context: *mut TonicContext,
    dict: TonicHandle,
    index: usize,
    key: *mut TonicHandle,
    value: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(key)?;
            require_out(value)?;
            let (entry_key, entry_value) = state.context.dict_entry(dict.into_internal(), index)?;
            key.write(TonicHandle::from_internal(entry_key));
            value.write(TonicHandle::from_internal(entry_value));
            Ok(())
        })
    }
}

unsafe extern "C" fn dict_set(
    context: *mut TonicContext,
    dict: TonicHandle,
    key: TonicHandle,
    value: TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            state.context.dict_set(
                dict.into_internal(),
                key.into_internal(),
                value.into_internal(),
            )?;
            Ok(())
        })
    }
}

unsafe extern "C" fn call_kw(
    context: *mut TonicContext,
    callable: TonicHandle,
    arguments: *const TonicHandle,
    argument_count: usize,
    keywords: TonicHandle,
    output: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            if argument_count != 0 && arguments.is_null() {
                return Err(BoundaryError::InvalidArgument("null call arguments"));
            }
            let arguments = if argument_count == 0 {
                &[]
            } else {
                slice::from_raw_parts(arguments, argument_count)
            };
            let arguments = arguments
                .iter()
                .copied()
                .map(TonicHandle::into_internal)
                .collect::<Vec<_>>();
            let value = state.context.call_with_keywords(
                callable.into_internal(),
                &arguments,
                keywords.into_internal(),
            )?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

fn attribute_name<'a>(
    name: *const u8,
    name_len: usize,
) -> std::result::Result<&'a str, BoundaryError> {
    if name_len != 0 && name.is_null() {
        return Err(BoundaryError::InvalidArgument("null attribute name"));
    }
    let bytes = if name_len == 0 {
        &[]
    } else {
        // SAFETY: callers validate that `name` spans `name_len` readable bytes.
        unsafe { slice::from_raw_parts(name, name_len) }
    };
    std::str::from_utf8(bytes)
        .map_err(|_| BoundaryError::InvalidArgument("attribute name is not UTF-8"))
}

unsafe extern "C" fn get_attr(
    context: *mut TonicContext,
    owner: TonicHandle,
    name: *const u8,
    name_len: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let name = attribute_name(name, name_len)?;
            let value = state.context.get_attr(owner.into_internal(), name)?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn set_attr(
    context: *mut TonicContext,
    owner: TonicHandle,
    name: *const u8,
    name_len: usize,
    value: TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            let name = attribute_name(name, name_len)?;
            state
                .context
                .set_attr(owner.into_internal(), name, value.into_internal())?;
            Ok(())
        })
    }
}

unsafe extern "C" fn repr_value(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let value = state.context.repr(value.into_internal())?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn int_from_i64(
    context: *mut TonicContext,
    value: i64,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: output is checked before writing.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let value = state.context.from_i64(value)?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn int_as_i64(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut i64,
) -> TonicStatus {
    // SAFETY: output is checked before writing.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(state.context.to_i64(value.into_internal())?);
            Ok(())
        })
    }
}

unsafe extern "C" fn float_from_f64(
    context: *mut TonicContext,
    value: f64,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: output is checked before writing.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let value = state.context.from_f64(value)?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn float_as_f64(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut f64,
) -> TonicStatus {
    // SAFETY: output is checked before writing.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(state.context.to_f64(value.into_internal())?);
            Ok(())
        })
    }
}

unsafe extern "C" fn str_from_utf8(
    context: *mut TonicContext,
    value: *const u8,
    value_len: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: null/length and output pairs are validated before foreign reads.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            if value_len != 0 && value.is_null() {
                return Err(BoundaryError::InvalidArgument("null UTF-8 input"));
            }
            let bytes = if value_len == 0 {
                &[]
            } else {
                slice::from_raw_parts(value, value_len)
            };
            let text = std::str::from_utf8(bytes)
                .map_err(|_| BoundaryError::InvalidArgument("string input is not UTF-8"))?;
            let value = state.context.from_str(text)?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn str_utf8(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> TonicStatus {
    // SAFETY: write_text validates all output pointer/capacity combinations.
    unsafe {
        boundary(context, true, |state| {
            let text = state.context.as_str(value.into_internal())?;
            write_text(text.as_bytes(), output, capacity, required)
        })
    }
}

unsafe extern "C" fn add(
    context: *mut TonicContext,
    left: TonicHandle,
    right: TonicHandle,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: handle resolution and output validation precede the write.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let value = state
                .context
                .add(left.into_internal(), right.into_internal())?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn call(
    context: *mut TonicContext,
    callable: TonicHandle,
    arguments: *const TonicHandle,
    argument_count: usize,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: null/count pairs and output are validated before foreign memory access.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            if argument_count != 0 && arguments.is_null() {
                return Err(BoundaryError::InvalidArgument("null callback arguments"));
            }
            let arguments = if argument_count == 0 {
                &[]
            } else {
                // SAFETY: the ABI requires `argument_count` readable handles.
                slice::from_raw_parts(arguments, argument_count)
            };
            let callable = callable.into_internal();
            let arguments = arguments
                .iter()
                .copied()
                .map(TonicHandle::into_internal)
                .collect::<Vec<_>>();
            let result = state.context.call(callable, &arguments)?;
            output.write(TonicHandle::from_internal(result));
            Ok(())
        })
    }
}

unsafe extern "C" fn buffer_export(
    context: *mut TonicContext,
    buffer: TonicHandle,
    requested_flags: u64,
    output: *mut TonicBuffer,
) -> TonicStatus {
    // SAFETY: output and exported metadata are validated within the boundary.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            if requested_flags & !(BUFFER_WRITABLE | BUFFER_C_CONTIGUOUS) != 0 {
                return Err(BoundaryError::Unsupported("unknown buffer request flags"));
            }
            let writable = requested_flags & BUFFER_WRITABLE != 0;
            let (owner, parts) = state
                .context
                .export_f64_buffer(buffer.into_internal(), writable)?;
            let mut flags = BUFFER_C_CONTIGUOUS;
            if parts.writable {
                flags |= BUFFER_WRITABLE;
            }
            output.write(TonicBuffer {
                struct_size: mem::size_of::<TonicBuffer>() as u32,
                dtype: DType::F64,
                flags,
                data: parts.data,
                byte_len: parts.byte_len,
                item_size: mem::size_of::<f64>(),
                ndim: parts.ndim,
                reserved: 0,
                shape: parts.shape,
                strides: parts.strides,
                owner: TonicHandle::from_internal(owner),
            });
            Ok(())
        })
    }
}

unsafe extern "C" fn buffer_release(
    context: *mut TonicContext,
    buffer: *mut TonicBuffer,
) -> TonicStatus {
    // SAFETY: descriptor pointer is validated before reading or clearing it.
    unsafe {
        boundary(context, true, |state| {
            require_out(buffer)?;
            let descriptor = buffer.read();
            if descriptor.struct_size < mem::size_of::<TonicBuffer>() as u32 {
                return Err(BoundaryError::InvalidArgument(
                    "buffer descriptor is smaller than ABI v1",
                ));
            }
            state
                .context
                .release_buffer_owner(descriptor.owner.into_internal())?;
            buffer.write(TonicBuffer::empty());
            Ok(())
        })
    }
}

unsafe extern "C" fn foreign_reference_create(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: output and logical handles are validated inside the boundary.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let reference = state
                .context
                .create_foreign_reference(value.into_internal())?;
            output.write(TonicHandle::from_internal(reference));
            Ok(())
        })
    }
}

unsafe extern "C" fn foreign_reference_release(
    context: *mut TonicContext,
    reference: TonicHandle,
) -> TonicStatus {
    // SAFETY: the logical handle is validated inside the boundary.
    unsafe {
        boundary(context, true, |state| {
            state
                .context
                .release_foreign_reference(reference.into_internal())?;
            Ok(())
        })
    }
}

unsafe extern "C" fn foreign_reference_borrow(
    context: *mut TonicContext,
    reference: TonicHandle,
    output: *mut TonicHandle,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let value = state
                .context
                .borrow_foreign_reference(reference.into_internal())?;
            output.write(TonicHandle::from_internal(value));
            Ok(())
        })
    }
}

unsafe extern "C" fn foreign_create(
    context: *mut TonicContext,
    payload: *mut std::ffi::c_void,
    vtable: *const TonicForeignVTable,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: vtable/output pointers are checked before reads or writes. The
    // payload is opaque trusted extension data and is never dereferenced here.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            if vtable.is_null() {
                return Err(BoundaryError::InvalidArgument("null foreign vtable"));
            }
            let spec = ForeignSpec::validate(&*vtable)?;
            let foreign = state.context.create_foreign(payload as usize, spec)?;
            output.write(TonicHandle::from_internal(foreign));
            Ok(())
        })
    }
}

unsafe extern "C" fn foreign_borrow_payload(
    context: *mut TonicContext,
    foreign: TonicHandle,
    adapter_id: u64,
    output: *mut *mut std::ffi::c_void,
) -> TonicStatus {
    // SAFETY: output is checked before writing and the returned pointer remains
    // borrowed only for this native scope, during which GC cannot run.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let payload = state
                .context
                .foreign_payload(foreign.into_internal(), adapter_id)?;
            output.write(payload as *mut std::ffi::c_void);
            Ok(())
        })
    }
}

unsafe extern "C" fn persistent_create(
    context: *mut TonicContext,
    value: TonicHandle,
    output: *mut TonicPersistentHandle,
) -> TonicStatus {
    // SAFETY: handles and output are validated inside the boundary.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let persistent = state.context.persist(value.into_internal())?;
            output.write(TonicPersistentHandle(persistent.raw()));
            Ok(())
        })
    }
}

unsafe extern "C" fn persistent_borrow(
    context: *mut TonicContext,
    persistent: TonicPersistentHandle,
    output: *mut TonicHandle,
) -> TonicStatus {
    // SAFETY: handles and output are validated inside the boundary.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let persistent = crate::native::PersistentHandle::from_raw(persistent.0);
            let local = state.context.borrow_persistent(&persistent)?;
            output.write(TonicHandle::from_internal(local));
            Ok(())
        })
    }
}

unsafe extern "C" fn persistent_release(
    context: *mut TonicContext,
    persistent: TonicPersistentHandle,
) -> TonicStatus {
    // SAFETY: the logical token is validated inside the boundary.
    unsafe {
        boundary(context, true, |state| {
            let persistent = crate::native::PersistentHandle::from_raw(persistent.0);
            state.context.release_persistent(&persistent)?;
            Ok(())
        })
    }
}

unsafe extern "C" fn runtime_owner_acquire(
    context: *mut TonicContext,
    output: *mut *mut TonicRuntimeOwner,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let owner = Box::new(state.context.runtime_owner());
            output.write(Box::into_raw(owner).cast());
            Ok(())
        })
    }
}

unsafe fn owner<'a>(
    owner: *const TonicRuntimeOwner,
) -> Option<&'a Arc<crate::runtime_owner::RuntimeOwner>> {
    if owner.is_null() {
        None
    } else {
        // SAFETY: the native contract requires a pointer returned by
        // runtime_owner_acquire that has not yet been released.
        Some(unsafe { &*owner.cast::<Arc<crate::runtime_owner::RuntimeOwner>>() })
    }
}

unsafe extern "C" fn runtime_owner_release(owner: *mut TonicRuntimeOwner) -> TonicStatus {
    if owner.is_null() {
        return TonicStatus::INVALID_ARGUMENT;
    }
    match catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: ownership is consumed exactly once by contract.
        drop(unsafe { Box::from_raw(owner.cast::<Arc<crate::runtime_owner::RuntimeOwner>>()) });
    })) {
        Ok(()) => TonicStatus::OK,
        Err(_) => TonicStatus::PANIC,
    }
}

unsafe extern "C" fn runtime_owner_matches(
    context: *mut TonicContext,
    owner_pointer: *const TonicRuntimeOwner,
    output: *mut u32,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            let owner =
                owner(owner_pointer).ok_or(BoundaryError::InvalidArgument("null runtime owner"))?;
            output.write(u32::from(state.context.owns_runtime(owner)));
            Ok(())
        })
    }
}

unsafe extern "C" fn runtime_execution_id(
    context: *mut TonicContext,
    output: *mut u64,
) -> TonicStatus {
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(state.context.execution_id());
            Ok(())
        })
    }
}

unsafe extern "C" fn persistent_release_deferred(
    owner_pointer: *const TonicRuntimeOwner,
    persistent: TonicPersistentHandle,
) -> TonicStatus {
    match catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: pointer lifetime is governed by runtime_owner_acquire/release.
        let Some(owner) = (unsafe { owner(owner_pointer) }) else {
            return TonicStatus::INVALID_ARGUMENT;
        };
        if owner.queue_persistent_release(persistent.0) {
            TonicStatus::OK
        } else {
            TonicStatus::UNSUPPORTED
        }
    })) {
        Ok(status) => status,
        Err(_) => TonicStatus::PANIC,
    }
}

unsafe extern "C" fn runtime_identity(context: *mut TonicContext, output: *mut u64) -> TonicStatus {
    // SAFETY: boundary validates the live context and output before writing.
    unsafe {
        boundary(context, true, |state| {
            require_out(output)?;
            output.write(state.context.runtime_owner().id());
            Ok(())
        })
    }
}

unsafe extern "C" fn foreign_reference_release_deferred(
    owner_pointer: *const TonicRuntimeOwner,
    reference: TonicHandle,
) -> TonicStatus {
    match catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: pointer lifetime is governed by runtime_owner_acquire/release.
        let Some(owner) = (unsafe { owner(owner_pointer) }) else {
            return TonicStatus::INVALID_ARGUMENT;
        };
        if owner.queue_foreign_reference_release(reference.0) {
            TonicStatus::OK
        } else {
            TonicStatus::UNSUPPORTED
        }
    })) {
        Ok(status) => status,
        Err(_) => TonicStatus::PANIC,
    }
}

static API: TonicApi = TonicApi {
    struct_size: mem::size_of::<TonicApi>() as u32,
    abi_version: TONIC_ABI_VERSION,
    capabilities: TONIC_CAPABILITIES,
    status_clear,
    exception_kind,
    exception_message,
    raise_exception,
    query_capability,
    none,
    int_from_i64,
    int_as_i64,
    float_from_f64,
    float_as_f64,
    str_from_utf8,
    str_utf8,
    add,
    call,
    buffer_export,
    buffer_release,
    foreign_reference_create,
    foreign_reference_release,
    foreign_create,
    foreign_borrow_payload,
    persistent_create,
    persistent_borrow,
    persistent_release,
    runtime_owner_acquire,
    runtime_owner_release,
    runtime_owner_matches,
    runtime_execution_id,
    persistent_release_deferred,
    value_kind,
    is_identical,
    bool_from,
    bool_as,
    int_from_decimal,
    int_decimal,
    list_new,
    list_append,
    tuple_new,
    sequence_len,
    sequence_get,
    dict_new,
    dict_len,
    dict_entry,
    dict_set,
    call_kw,
    get_attr,
    set_attr,
    repr_value,
    foreign_reference_borrow,
    runtime_identity,
    foreign_reference_release_deferred,
};

pub fn negotiate_api(
    abi_version: u32,
    minimum_struct_size: u32,
    required_capabilities: u64,
) -> Result<&'static TonicApi> {
    if abi_version != TONIC_ABI_VERSION {
        return Err(Diagnostic::new(
            "NativeAbiError",
            format!("native ABI {abi_version} is incompatible with ABI {TONIC_ABI_VERSION}"),
        ));
    }
    if minimum_struct_size > API.struct_size {
        return Err(Diagnostic::new(
            "NativeAbiError",
            "native API function table is smaller than the extension requires",
        ));
    }
    let missing = required_capabilities & !API.capabilities;
    if missing != 0 {
        return Err(Diagnostic::new(
            "NativeAbiError",
            format!("native API lacks required capability bits 0x{missing:x}"),
        ));
    }
    Ok(&API)
}

pub(crate) fn invoke_native(
    context: &mut Context<'_>,
    function: CNativeFn,
    arguments: &[Handle],
) -> Result<Handle> {
    let arguments: Vec<_> = arguments
        .iter()
        .copied()
        .map(TonicHandle::from_internal)
        .collect();
    let mut state = CallContext {
        magic: CONTEXT_MAGIC,
        context,
        exception: None,
    };
    let context = ptr::from_mut(&mut state).cast::<TonicContext>();
    let mut output = TonicHandle(0);
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: context, argument slice and output remain valid for the call.
        unsafe { function(context, arguments.as_ptr(), arguments.len(), &mut output) }
    }));
    state.magic = 0;
    state.context.drain_deferred_persistent_releases()?;
    match result {
        Err(_) => Err(Diagnostic::new(
            "RuntimeError",
            "native extension panic was contained at the Tonic C ABI boundary",
        )),
        Ok(TonicStatus::OK) if state.exception.is_none() => {
            let handle = output.into_internal();
            state.context.resolve(handle)?;
            Ok(handle)
        }
        Ok(TonicStatus::OK) => Err(state.exception.unwrap_or_else(|| {
            Diagnostic::new("NativeError", "native extension left an exception set")
        })),
        Ok(_) => Err(state.exception.unwrap_or_else(|| {
            Diagnostic::new(
                "NativeError",
                "native extension returned failure without setting an exception",
            )
        })),
    }
}

pub(crate) fn invoke_extension_init(
    context: &mut Context<'_>,
    init: CExtensionInitFn,
    api: &'static TonicApi,
) -> Result<()> {
    let mut state = CallContext {
        magic: CONTEXT_MAGIC,
        context,
        exception: None,
    };
    let opaque = ptr::from_mut(&mut state).cast::<TonicContext>();
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the immutable API table is static and context lives for the call.
        unsafe { init(ptr::from_ref(api), opaque) }
    }));
    state.magic = 0;
    match result {
        Err(_) => Err(Diagnostic::new(
            "RuntimeError",
            "native extension init panic was contained at the Tonic C ABI boundary",
        )),
        Ok(TonicStatus::OK) if state.exception.is_none() => Ok(()),
        Ok(TonicStatus::OK) => Err(state.exception.unwrap_or_else(|| {
            Diagnostic::new("NativeError", "extension init left an exception set")
        })),
        Ok(_) => Err(state.exception.unwrap_or_else(|| {
            Diagnostic::new(
                "NativeError",
                "extension init returned failure without setting an exception",
            )
        })),
    }
}

/// Negotiates the sole bootstrap symbol and returns the immutable function table.
///
/// # Safety
///
/// `output` must be null or point to writable storage for one `*const TonicApi`.
#[no_mangle]
pub unsafe extern "C" fn tonic_get_api(
    abi_version: u32,
    minimum_struct_size: u32,
    required_capabilities: u64,
    output: *mut *const TonicApi,
) -> TonicStatus {
    match catch_unwind(AssertUnwindSafe(|| {
        if output.is_null() {
            return TonicStatus::INVALID_ARGUMENT;
        }
        // SAFETY: output was checked and the caller promises a writable pointer.
        unsafe { output.write(ptr::null()) };
        match negotiate_api(abi_version, minimum_struct_size, required_capabilities) {
            Ok(api) => {
                // SAFETY: output was checked and the caller promises a writable pointer.
                unsafe { output.write(api) };
                TonicStatus::OK
            }
            Err(error) if error.message.contains("capability") => TonicStatus::UNSUPPORTED,
            Err(_) => TonicStatus::ABI_MISMATCH,
        }
    })) {
        Ok(status) => status,
        Err(_) => TonicStatus::PANIC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_size_and_capability_negotiation_is_strict() {
        let api = negotiate_api(
            TONIC_ABI_VERSION,
            mem::size_of::<TonicApi>() as u32,
            CAP_CORE,
        )
        .unwrap();
        assert_eq!(api.abi_version, 1);
        assert_eq!(
            usize::try_from(api.struct_size).unwrap(),
            mem::size_of::<TonicApi>()
        );
        assert_eq!(negotiate_api(2, 0, 0).unwrap_err().kind, "NativeAbiError");
        assert!(negotiate_api(1, api.struct_size + 1, 0).is_err());
        assert!(negotiate_api(1, 0, 1 << 63).is_err());
    }

    #[test]
    fn exported_api_lookup_never_returns_a_partial_table() {
        let mut api = ptr::null();
        // SAFETY: `api` is a valid output slot.
        let status = unsafe { tonic_get_api(1, 0, TONIC_CAPABILITIES, &mut api) };
        assert_eq!(status, TonicStatus::OK);
        assert_eq!(api, ptr::from_ref(&API));
        // SAFETY: `api` is a valid output slot.
        let status = unsafe { tonic_get_api(99, 0, 0, &mut api) };
        assert_eq!(status, TonicStatus::ABI_MISMATCH);
        assert!(api.is_null());
        // SAFETY: null is deliberately passed to verify rejection.
        assert_eq!(
            unsafe { tonic_get_api(1, 0, 0, ptr::null_mut()) },
            TonicStatus::INVALID_ARGUMENT
        );
    }

    #[test]
    fn boundary_contains_panics_and_records_a_guest_error() {
        let mut vm = crate::Vm::new().unwrap();
        let mut context = vm.context().unwrap();
        let mut call = CallContext {
            magic: CONTEXT_MAGIC,
            context: &mut context,
            exception: None,
        };
        let opaque = ptr::from_mut(&mut call).cast::<TonicContext>();
        // SAFETY: the opaque pointer is backed by `call` for the whole operation.
        let status = unsafe {
            boundary(
                opaque,
                true,
                |_| -> std::result::Result<(), BoundaryError> { panic!("extension bug") },
            )
        };
        assert_eq!(status, TonicStatus::PANIC);
        let error = call.exception.unwrap();
        assert_eq!(error.kind, "RuntimeError");
        assert!(error.message.contains("contained"));
    }

    #[test]
    fn exception_text_supports_length_query_without_clearing_the_error() {
        let mut vm = crate::Vm::new().unwrap();
        let mut context = vm.context().unwrap();
        let mut call = CallContext {
            magic: CONTEXT_MAGIC,
            context: &mut context,
            exception: Some(Diagnostic::new("ValueError", "bad value")),
        };
        let opaque = ptr::from_mut(&mut call).cast::<TonicContext>();
        let mut required = 0;
        // SAFETY: length-only query has a valid required-size slot.
        assert_eq!(
            unsafe { exception_message(opaque, ptr::null_mut(), 0, &mut required) },
            TonicStatus::OK
        );
        assert_eq!(required, 9);
        let mut bytes = vec![0; required];
        // SAFETY: output and required-size slots have the declared capacity.
        assert_eq!(
            unsafe { exception_message(opaque, bytes.as_mut_ptr(), bytes.len(), &mut required) },
            TonicStatus::OK
        );
        assert_eq!(&bytes, b"bad value");
        assert_eq!(call.exception.as_ref().unwrap().kind, "ValueError");
    }

    #[test]
    fn capability_query_is_versioned_and_unknown_ids_are_safe() {
        let mut vm = crate::Vm::new().unwrap();
        let mut context = vm.context().unwrap();
        let mut call = CallContext {
            magic: CONTEXT_MAGIC,
            context: &mut context,
            exception: None,
        };
        let opaque = ptr::from_mut(&mut call).cast::<TonicContext>();
        let mut version = 0;
        // SAFETY: context and version output live for both calls.
        assert_eq!(
            unsafe { query_capability(opaque, TonicCapability::PANIC_GUARD, 1, &mut version) },
            TonicStatus::OK
        );
        assert_eq!(version, 1);
        assert_eq!(
            unsafe { query_capability(opaque, TonicCapability::RUNTIME_OWNER_V1, 1, &mut version) },
            TonicStatus::OK
        );
        assert_eq!(version, 1);
        assert_eq!(
            unsafe {
                query_capability(opaque, TonicCapability::PROTOCOL_ACCESS_V1, 1, &mut version)
            },
            TonicStatus::OK
        );
        assert_eq!(version, 1);
        assert_eq!(
            unsafe {
                query_capability(opaque, TonicCapability::CROSS_COLLECTOR_V1, 1, &mut version)
            },
            TonicStatus::OK
        );
        assert_eq!(version, 1);
        // Transparent integer wrappers make unknown foreign values defined data.
        // SAFETY: context and version output remain valid.
        assert_eq!(
            unsafe { query_capability(opaque, TonicCapability(99), 1, &mut version) },
            TonicStatus::UNSUPPORTED
        );
        assert_eq!(call.exception.as_ref().unwrap().kind, "NativeApiError");
    }
}
