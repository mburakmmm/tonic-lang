#![deny(unsafe_code)]

mod buffer;
#[allow(unsafe_code)]
pub mod c_api;
mod classes;
mod dict;
#[allow(unsafe_code)]
mod foreign;
mod heap;
pub mod native;
mod number;
mod ops;
mod runtime_owner;
mod shapes;
mod value;
mod vm;

pub use buffer::{DType, F64BufferView, BUFFER_C_CONTIGUOUS, BUFFER_WRITABLE};
pub use c_api::{
    CExtensionInitFn, CNativeFn, TonicApi, TonicBuffer, TonicCapability, TonicContext,
    TonicExceptionKind, TonicHandle, TonicPersistentHandle, TonicRuntimeOwner, TonicStatus,
    TonicValueKind, TONIC_ABI_VERSION,
};
pub use foreign::{
    ForeignDestroyFn, ForeignTraceFn, TonicForeignVTable, TonicTraceVisitor, FOREIGN_OWNED,
};
pub use heap::CollectionStats;
pub use native::{Context, Handle, NativeFn, PersistentHandle};
pub use vm::{ExecutionMode, Limits, RuntimePhase, Stats, Vm};
