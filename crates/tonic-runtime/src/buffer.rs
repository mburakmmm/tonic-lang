use std::cell::Cell;
use tonic_core::diagnostic::{Diagnostic, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct DType(u32);

impl DType {
    pub const I8: Self = Self(1);
    pub const U8: Self = Self(2);
    pub const I16: Self = Self(3);
    pub const U16: Self = Self(4);
    pub const I32: Self = Self(5);
    pub const U32: Self = Self(6);
    pub const I64: Self = Self(7);
    pub const U64: Self = Self(8);
    pub const F16: Self = Self(9);
    pub const F32: Self = Self(10);
    pub const F64: Self = Self(11);
    pub const BOOL: Self = Self(12);
    pub const COMPLEX64: Self = Self(13);
    pub const COMPLEX128: Self = Self(14);
}

pub const BUFFER_WRITABLE: u64 = 1 << 0;
pub const BUFFER_C_CONTIGUOUS: u64 = 1 << 1;

#[derive(Debug)]
pub(crate) struct Buffer {
    data: Box<[f64]>,
    shape: Box<[usize]>,
    strides: Box<[isize]>,
    writable: bool,
    mutation: Box<BufferMutationState>,
}

#[derive(Debug)]
struct BufferMutationState {
    version: Cell<u64>,
    writable_exports: Cell<u32>,
    native_access_disabled: Cell<bool>,
}

#[derive(Clone, Copy)]
pub(crate) struct ExportParts {
    pub data: *mut u8,
    pub byte_len: usize,
    pub shape: *const usize,
    pub strides: *const isize,
    pub ndim: u32,
    pub writable: bool,
}

pub struct F64BufferView<'a> {
    data: &'a [f64],
    shape: &'a [usize],
    strides: &'a [isize],
    writable: bool,
}

impl F64BufferView<'_> {
    pub fn as_slice(&self) -> &[f64] {
        self.data
    }
    pub fn shape(&self) -> &[usize] {
        self.shape
    }
    pub fn strides(&self) -> &[isize] {
        self.strides
    }
    pub fn is_writable(&self) -> bool {
        self.writable
    }
}

impl Buffer {
    pub fn f64(values: &[f64], shape: &[usize], writable: bool) -> Result<Self> {
        u32::try_from(shape.len())
            .map_err(|_| Diagnostic::new("BufferError", "buffer rank exceeds u32"))?;
        let elements = shape.iter().try_fold(1usize, |product, dimension| {
            product.checked_mul(*dimension).ok_or_else(|| {
                Diagnostic::new("BufferError", "buffer shape element count overflows usize")
            })
        })?;
        if elements != values.len() {
            return Err(Diagnostic::new(
                "BufferError",
                format!(
                    "buffer shape describes {elements} elements, got {}",
                    values.len()
                ),
            ));
        }
        let mut stride = std::mem::size_of::<f64>();
        let mut strides = vec![0isize; shape.len()];
        for (index, dimension) in shape.iter().enumerate().rev() {
            strides[index] = isize::try_from(stride)
                .map_err(|_| Diagnostic::new("BufferError", "buffer stride does not fit isize"))?;
            stride = stride.checked_mul(*dimension).ok_or_else(|| {
                Diagnostic::new("BufferError", "buffer byte length overflows usize")
            })?;
        }
        Ok(Self {
            data: values.to_vec().into_boxed_slice(),
            shape: shape.to_vec().into_boxed_slice(),
            strides: strides.into_boxed_slice(),
            writable,
            mutation: Box::new(BufferMutationState {
                version: Cell::new(0),
                writable_exports: Cell::new(0),
                native_access_disabled: Cell::new(false),
            }),
        })
    }
    pub fn view(&self) -> F64BufferView<'_> {
        F64BufferView {
            data: &self.data,
            shape: &self.shape,
            strides: &self.strides,
            writable: self.writable,
        }
    }
    pub fn export(&self) -> ExportParts {
        ExportParts {
            data: self.data.as_ptr().cast_mut().cast::<u8>(),
            byte_len: self.data.len() * std::mem::size_of::<f64>(),
            shape: self.shape.as_ptr(),
            strides: self.strides.as_ptr(),
            ndim: self.shape.len() as u32,
            writable: self.writable,
        }
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub(crate) fn data_mut_ptr(&mut self) -> *mut f64 {
        self.data.as_mut_ptr()
    }
    pub(crate) fn mutation_version_ptr(&self) -> *mut u64 {
        self.mutation.version.as_ptr()
    }
    #[cfg(test)]
    fn mutation_version(&self) -> u64 {
        self.mutation.version.get()
    }
    pub(crate) fn has_active_writable_exports(&self) -> bool {
        self.mutation.writable_exports.get() != 0
    }
    pub(crate) fn native_access_disabled(&self) -> bool {
        self.mutation.native_access_disabled.get()
    }
    pub(crate) fn begin_export(&self, writable: bool) -> Result<()> {
        if !writable {
            return Ok(());
        }
        let exports = self
            .mutation
            .writable_exports
            .get()
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("BufferError", "writable export count exhausted"))?;
        self.mutation.writable_exports.set(exports);
        Ok(())
    }
    pub(crate) fn end_export(&self, writable: bool) {
        if !writable {
            return;
        }
        let exports = self.mutation.writable_exports.get();
        debug_assert!(exports > 0, "writable buffer export underflow");
        self.mutation
            .writable_exports
            .set(exports.saturating_sub(1));
        if let Some(version) = self.mutation.version.get().checked_add(1) {
            self.mutation.version.set(version);
        } else {
            // An external writer may already have changed storage. Future
            // native access must fail closed when its epoch cannot advance.
            self.mutation.native_access_disabled.set(true);
        }
    }
    fn advance_mutation(&self) -> Result<()> {
        let version =
            self.mutation.version.get().checked_add(1).ok_or_else(|| {
                Diagnostic::new("RuntimeError", "buffer mutation version exhausted")
            })?;
        self.mutation.version.set(version);
        Ok(())
    }
    pub fn set_f64(&mut self, index: usize, value: f64) -> Result<()> {
        if !self.writable {
            return Err(Diagnostic::new("TypeError", "buffer is read-only"));
        }
        if index >= self.data.len() {
            return Err(Diagnostic::new("IndexError", "buffer index out of range"));
        }
        self.advance_mutation()?;
        self.data[index] = value;
        Ok(())
    }
    pub fn estimated_bytes(&self) -> usize {
        self.data.len() * std::mem::size_of::<f64>()
            + self.shape.len() * std::mem::size_of::<usize>()
            + self.strides.len() * std::mem::size_of::<isize>()
            + std::mem::size_of::<BufferMutationState>()
    }
}

#[cfg(test)]
mod tests {
    use super::Buffer;

    #[test]
    fn mutation_epoch_and_writable_export_lifetime_are_explicit() {
        let mut buffer = Buffer::f64(&[1.0, 2.0], &[2], true).unwrap();
        assert_eq!(buffer.mutation_version(), 0);
        assert!(!buffer.has_active_writable_exports());

        buffer.begin_export(true).unwrap();
        assert!(buffer.has_active_writable_exports());
        buffer.end_export(true);
        assert!(!buffer.has_active_writable_exports());
        assert_eq!(buffer.mutation_version(), 1);

        buffer.set_f64(0, 3.0).unwrap();
        assert_eq!(buffer.mutation_version(), 2);
        assert_eq!(buffer.view().as_slice(), &[3.0, 2.0]);

        buffer.begin_export(false).unwrap();
        buffer.end_export(false);
        assert_eq!(buffer.mutation_version(), 2);
    }
}
