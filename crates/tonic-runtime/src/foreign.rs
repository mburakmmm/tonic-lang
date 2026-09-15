//! Stable foreign-payload metadata and guarded callback trampolines.
//!
//! The managed wrapper stores only an integer form of the external pointer and
//! copied function pointers. Foreign code is trusted, but panics from Rust test
//! extensions are contained before they can cross the C ABI boundary.

use crate::{
    c_api::{TonicHandle, TonicStatus, TONIC_ABI_VERSION},
    native::Handle,
    value::Value,
};
use std::{
    ffi::c_void,
    mem,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr,
};
use tonic_core::diagnostic::{Diagnostic, Result};

pub const FOREIGN_OWNED: u64 = 1 << 0;

pub type ForeignTraceFn = unsafe extern "C-unwind" fn(
    payload: *mut c_void,
    visitor: *mut TonicTraceVisitor,
) -> TonicStatus;
pub type ForeignDestroyFn = unsafe extern "C-unwind" fn(payload: *mut c_void);
pub type TraceVisitFn =
    unsafe extern "C" fn(visitor: *mut TonicTraceVisitor, reference: TonicHandle) -> TonicStatus;
pub type TracePromoteFn = unsafe extern "C" fn(
    visitor: *mut TonicTraceVisitor,
    reference: TonicHandle,
    output: *mut crate::c_api::TonicPersistentHandle,
) -> TonicStatus;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TonicForeignVTable {
    pub struct_size: u32,
    pub abi_version: u32,
    pub adapter_id: u64,
    pub flags: u64,
    pub trace: Option<ForeignTraceFn>,
    pub destroy: Option<ForeignDestroyFn>,
}

#[repr(C)]
#[derive(Debug)]
pub struct TonicTraceVisitor {
    pub struct_size: u32,
    pub reserved: u32,
    pub state: *mut c_void,
    pub visit: TraceVisitFn,
    /// Reports a reference owned by the foreign payload rather than by this
    /// managed wrapper. The runtime resolves it for this trace but never
    /// releases it when the wrapper dies or stops reporting the edge.
    pub visit_borrowed: TraceVisitFn,
    /// Creates a persistent root from a borrowed foreign reference. This is
    /// used when another collector discovers an external root after previously
    /// demoting a bridge edge.
    pub promote: TracePromoteFn,
}

struct TraceState {
    owned: Vec<Handle>,
    borrowed: Vec<Handle>,
    handles: *mut crate::native::HandleTable,
}

#[derive(Default)]
pub(crate) struct TracedHandles {
    pub owned: Vec<Handle>,
    pub borrowed: Vec<Handle>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ForeignSpec {
    pub adapter_id: u64,
    pub owned: bool,
    pub trace: Option<ForeignTraceFn>,
    pub destroy: Option<ForeignDestroyFn>,
}

impl ForeignSpec {
    pub(crate) fn validate(vtable: &TonicForeignVTable) -> Result<Self> {
        if vtable.struct_size < mem::size_of::<TonicForeignVTable>() as u32 {
            return Err(Diagnostic::new(
                "ForeignError",
                "foreign vtable is smaller than ABI v1",
            ));
        }
        if vtable.abi_version != TONIC_ABI_VERSION {
            return Err(Diagnostic::new(
                "ForeignError",
                "foreign vtable ABI version is incompatible",
            ));
        }
        if vtable.flags & !FOREIGN_OWNED != 0 {
            return Err(Diagnostic::new(
                "ForeignError",
                "foreign vtable contains unknown flags",
            ));
        }
        let owned = vtable.flags & FOREIGN_OWNED != 0;
        if owned && vtable.destroy.is_none() {
            return Err(Diagnostic::new(
                "ForeignError",
                "owned foreign payload requires a destroy callback",
            ));
        }
        if !owned && vtable.destroy.is_some() {
            return Err(Diagnostic::new(
                "ForeignError",
                "borrowed foreign payload cannot define a destroy callback",
            ));
        }
        Ok(Self {
            adapter_id: vtable.adapter_id,
            owned,
            trace: vtable.trace,
            destroy: vtable.destroy,
        })
    }
}

unsafe extern "C" fn visit_reference(
    visitor: *mut TonicTraceVisitor,
    reference: TonicHandle,
) -> TonicStatus {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if visitor.is_null() {
            return TonicStatus::INVALID_ARGUMENT;
        }
        // SAFETY: the runtime constructs the visitor and keeps it alive for the
        // complete trace callback. Foreign code must pass that same pointer back.
        let visitor = unsafe { &mut *visitor };
        if visitor.struct_size < mem::size_of::<TonicTraceVisitor>() as u32
            || visitor.state.is_null()
        {
            return TonicStatus::INVALID_ARGUMENT;
        }
        // SAFETY: state points at the TraceState created by `trace_handles` below.
        let state = unsafe { &mut *visitor.state.cast::<TraceState>() };
        state.owned.push(reference.into_internal());
        TonicStatus::OK
    }));
    result.unwrap_or(TonicStatus::PANIC)
}

unsafe extern "C" fn visit_borrowed_reference(
    visitor: *mut TonicTraceVisitor,
    reference: TonicHandle,
) -> TonicStatus {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if visitor.is_null() {
            return TonicStatus::INVALID_ARGUMENT;
        }
        // SAFETY: the runtime constructs the visitor for one synchronous trace.
        let visitor = unsafe { &mut *visitor };
        if visitor.struct_size < mem::size_of::<TonicTraceVisitor>() as u32
            || visitor.state.is_null()
        {
            return TonicStatus::INVALID_ARGUMENT;
        }
        // SAFETY: state points at the TraceState created by `trace_handles` below.
        let state = unsafe { &mut *visitor.state.cast::<TraceState>() };
        state.borrowed.push(reference.into_internal());
        TonicStatus::OK
    }));
    result.unwrap_or(TonicStatus::PANIC)
}

unsafe extern "C" fn promote_reference(
    visitor: *mut TonicTraceVisitor,
    reference: TonicHandle,
    output: *mut crate::c_api::TonicPersistentHandle,
) -> TonicStatus {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if visitor.is_null() || output.is_null() {
            return TonicStatus::INVALID_ARGUMENT;
        }
        // SAFETY: the runtime constructs the visitor for one synchronous trace.
        let visitor = unsafe { &mut *visitor };
        if visitor.struct_size < mem::size_of::<TonicTraceVisitor>() as u32
            || visitor.state.is_null()
        {
            return TonicStatus::INVALID_ARGUMENT;
        }
        // SAFETY: state and its handle table pointer remain live for the callback.
        let state = unsafe { &mut *visitor.state.cast::<TraceState>() };
        let Some(handles) = (unsafe { state.handles.as_mut() }) else {
            return TonicStatus::INVALID_ARGUMENT;
        };
        match handles.promote_foreign_reference(reference.into_internal()) {
            Ok(handle) => {
                // SAFETY: output was validated non-null above.
                unsafe { output.write(crate::c_api::TonicPersistentHandle::from_internal(handle)) };
                TonicStatus::OK
            }
            Err(_) => TonicStatus::INVALID_ARGUMENT,
        }
    }));
    result.unwrap_or(TonicStatus::PANIC)
}

pub(crate) fn trace_handles(
    spec: ForeignSpec,
    payload: usize,
    handles: &mut crate::native::HandleTable,
) -> Result<TracedHandles> {
    let Some(trace) = spec.trace else {
        return Ok(TracedHandles::default());
    };
    let mut state = TraceState {
        owned: Vec::new(),
        borrowed: Vec::new(),
        handles,
    };
    let mut visitor = TonicTraceVisitor {
        struct_size: mem::size_of::<TonicTraceVisitor>() as u32,
        reserved: 0,
        state: ptr::from_mut(&mut state).cast(),
        visit: visit_reference,
        visit_borrowed: visit_borrowed_reference,
        promote: promote_reference,
    };
    let status = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the extension supplied the callback and payload. The visitor
        // remains valid only for this synchronous call.
        unsafe { trace(payload as *mut c_void, &mut visitor) }
    }))
    .map_err(|_| {
        Diagnostic::new(
            "ForeignError",
            "foreign trace panic was contained at the Tonic ABI boundary",
        )
    })?;
    if status != TonicStatus::OK {
        return Err(Diagnostic::new(
            "ForeignError",
            format!("foreign trace callback failed with status {status:?}"),
        ));
    }
    state.owned.sort_unstable_by_key(|handle| handle.raw());
    state.borrowed.sort_unstable_by_key(|handle| handle.raw());
    let duplicate_owned = state
        .owned
        .windows(2)
        .any(|pair| pair[0].raw() == pair[1].raw());
    let duplicate_borrowed = state
        .borrowed
        .windows(2)
        .any(|pair| pair[0].raw() == pair[1].raw());
    let mixed = state.owned.iter().any(|owned| {
        state
            .borrowed
            .binary_search_by_key(&owned.raw(), |h| h.raw())
            .is_ok()
    });
    if duplicate_owned || duplicate_borrowed || mixed {
        return Err(Diagnostic::new(
            "ForeignError",
            "foreign trace callback reported a reference more than once",
        ));
    }
    Ok(TracedHandles {
        owned: state.owned,
        borrowed: state.borrowed,
    })
}

#[derive(Debug)]
pub(crate) struct ForeignObject {
    pub adapter_id: u64,
    payload: usize,
    spec: ForeignSpec,
    references: Vec<Value>,
    reference_handles: Vec<Handle>,
    borrowed_handles: Vec<Handle>,
    active: bool,
}

impl ForeignObject {
    pub(crate) fn new(
        payload: usize,
        spec: ForeignSpec,
        reference_handles: Vec<Handle>,
        borrowed_handles: Vec<Handle>,
        references: Vec<Value>,
    ) -> Self {
        Self {
            adapter_id: spec.adapter_id,
            payload,
            spec,
            references,
            reference_handles,
            borrowed_handles,
            active: true,
        }
    }

    pub(crate) fn trace(&self, visit: impl FnMut(Value)) {
        self.references.iter().copied().for_each(visit);
    }

    pub(crate) fn refresh(
        &mut self,
        handles: &mut crate::native::HandleTable,
    ) -> Result<Vec<Handle>> {
        let traced = trace_handles(self.spec, self.payload, handles)?;
        let references = traced
            .owned
            .iter()
            .chain(&traced.borrowed)
            .copied()
            .map(|handle| handles.resolve_foreign_reference(handle))
            .collect::<Result<Vec<_>>>()?;
        let removed = self
            .reference_handles
            .iter()
            .copied()
            .filter(|old| {
                traced
                    .owned
                    .binary_search_by_key(&old.raw(), |handle| handle.raw())
                    .is_err()
            })
            .collect();
        self.reference_handles = traced.owned;
        self.borrowed_handles = traced.borrowed;
        self.references = references;
        Ok(removed)
    }

    pub(crate) fn references(&self) -> &[Value] {
        &self.references
    }

    pub(crate) fn payload(&self) -> usize {
        self.payload
    }

    pub(crate) fn has_trace(&self) -> bool {
        self.spec.trace.is_some()
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.references.capacity() * mem::size_of::<Value>()
            + self.reference_handles.capacity() * mem::size_of::<Handle>()
            + self.borrowed_handles.capacity() * mem::size_of::<Handle>()
    }

    pub(crate) fn take_finalizer(&mut self) -> PendingForeign {
        let active = mem::replace(&mut self.active, false);
        PendingForeign {
            payload: mem::take(&mut self.payload),
            owned: active && self.spec.owned,
            destroy: self.spec.destroy,
            reference_handles: mem::take(&mut self.reference_handles),
        }
    }
}

pub(crate) struct PendingForeign {
    payload: usize,
    owned: bool,
    destroy: Option<ForeignDestroyFn>,
    pub reference_handles: Vec<Handle>,
}

impl PendingForeign {
    pub(crate) fn will_destroy(&self) -> bool {
        self.owned && self.destroy.is_some()
    }
    /// Returns true if a foreign panic was contained. The payload is consumed
    /// before invocation, so a panic can never cause a second destructor call.
    pub(crate) fn run(&mut self) -> bool {
        let payload = mem::take(&mut self.payload);
        if !self.owned {
            return false;
        }
        let Some(destroy) = self.destroy.take() else {
            return false;
        };
        catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: ownership transferred to Tonic at successful wrapper
            // creation and this queue consumes that ownership exactly once.
            unsafe { destroy(payload as *mut c_void) }
        }))
        .is_err()
    }
}

impl Drop for PendingForeign {
    fn drop(&mut self) {
        let payload = mem::take(&mut self.payload);
        if !self.owned {
            return;
        }
        let Some(destroy) = self.destroy.take() else {
            return;
        };
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: this is the last-resort runtime-drop finalization path;
            // taking the callback above prevents any later duplicate call.
            unsafe { destroy(payload as *mut c_void) }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vtable() -> TonicForeignVTable {
        TonicForeignVTable {
            struct_size: mem::size_of::<TonicForeignVTable>() as u32,
            abi_version: TONIC_ABI_VERSION,
            adapter_id: 9,
            flags: 0,
            trace: None,
            destroy: None,
        }
    }

    #[test]
    fn validation_rejects_truncated_unknown_and_inconsistent_vtables() {
        let mut table = vtable();
        table.struct_size = 0;
        assert_eq!(
            ForeignSpec::validate(&table).unwrap_err().kind,
            "ForeignError"
        );
        table = vtable();
        table.abi_version += 1;
        assert!(ForeignSpec::validate(&table).is_err());
        table = vtable();
        table.flags = 1 << 63;
        assert!(ForeignSpec::validate(&table).is_err());
        table = vtable();
        table.flags = FOREIGN_OWNED;
        assert!(ForeignSpec::validate(&table).is_err());
    }
}
