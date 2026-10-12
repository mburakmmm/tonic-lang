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
    origin: usize,
    elements: usize,
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
    pub c_contiguous: bool,
}

pub struct F64BufferView<'a> {
    data: &'a [f64],
    origin: usize,
    elements: usize,
    shape: &'a [usize],
    strides: &'a [isize],
    writable: bool,
    c_contiguous: bool,
}

impl F64BufferView<'_> {
    /// Returns the complete owned backing allocation. For a strided view this
    /// is not the logical element sequence; use `shape`, `strides`, and
    /// `origin` to interpret it.
    pub fn as_slice(&self) -> &[f64] {
        self.data
    }
    pub fn origin(&self) -> usize {
        self.origin
    }
    pub fn len(&self) -> usize {
        self.elements
    }
    pub fn is_empty(&self) -> bool {
        self.elements == 0
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
    pub fn is_c_contiguous(&self) -> bool {
        self.c_contiguous
    }
}

impl Buffer {
    pub fn f64(values: &[f64], shape: &[usize], writable: bool) -> Result<Self> {
        let strides = contiguous_strides(shape)?;
        let buffer = Self::f64_strided(values, shape, &strides, 0, writable)?;
        if buffer.elements != values.len() {
            return Err(Diagnostic::new(
                "BufferError",
                format!(
                    "buffer shape describes {} elements, got {}",
                    buffer.elements,
                    values.len()
                ),
            ));
        }
        Ok(buffer)
    }

    pub fn f64_strided(
        values: &[f64],
        shape: &[usize],
        strides: &[isize],
        origin: usize,
        writable: bool,
    ) -> Result<Self> {
        u32::try_from(shape.len())
            .map_err(|_| Diagnostic::new("BufferError", "buffer rank exceeds u32"))?;
        if strides.len() != shape.len() {
            return Err(Diagnostic::new(
                "BufferError",
                format!(
                    "buffer rank is {}, but {} strides were provided",
                    shape.len(),
                    strides.len()
                ),
            ));
        }
        let elements = shape.iter().try_fold(1usize, |product, dimension| {
            product.checked_mul(*dimension).ok_or_else(|| {
                Diagnostic::new("BufferError", "buffer shape element count overflows usize")
            })
        })?;
        elements
            .checked_mul(std::mem::size_of::<f64>())
            .ok_or_else(|| Diagnostic::new("BufferError", "buffer byte length overflows usize"))?;
        validate_layout(values.len(), shape, strides, origin, elements)?;
        Ok(Self {
            data: values.to_vec().into_boxed_slice(),
            origin,
            elements,
            shape: shape.to_vec().into_boxed_slice(),
            strides: strides.to_vec().into_boxed_slice(),
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
            origin: self.origin,
            elements: self.elements,
            shape: &self.shape,
            strides: &self.strides,
            writable: self.writable,
            c_contiguous: self.is_c_contiguous(),
        }
    }
    pub fn export(&mut self) -> ExportParts {
        ExportParts {
            // `origin` is validated as an in-allocation element or the one-past
            // position for an empty view. The mutable borrow provides pointer
            // provenance suitable for an explicitly writable export.
            data: self
                .data
                .as_mut_ptr()
                .wrapping_add(self.origin)
                .cast::<u8>(),
            byte_len: self.elements * std::mem::size_of::<f64>(),
            shape: self.shape.as_ptr(),
            strides: self.strides.as_ptr(),
            ndim: self.shape.len() as u32,
            writable: self.writable,
            c_contiguous: self.is_c_contiguous(),
        }
    }
    pub fn len(&self) -> usize {
        self.elements
    }
    pub(crate) fn data_mut_ptr(&mut self) -> *mut f64 {
        // SAFETY: layout validation guarantees an in-allocation origin, or the
        // one-past position for an empty logical buffer.
        self.data.as_mut_ptr().wrapping_add(self.origin)
    }
    pub(crate) fn is_c_contiguous(&self) -> bool {
        if self.elements == 0 {
            return true;
        }
        let mut expected = std::mem::size_of::<f64>();
        for (&dimension, &stride) in self.shape.iter().zip(&self.strides).rev() {
            if dimension > 1 && isize::try_from(expected).ok() != Some(stride) {
                return false;
            }
            let Some(next) = expected.checked_mul(dimension.max(1)) else {
                return false;
            };
            expected = next;
        }
        true
    }
    pub(crate) fn storage_index(&self, indices: &[usize]) -> Option<usize> {
        if indices.len() != self.shape.len() {
            return None;
        }
        let mut offset = self.origin as i128;
        for ((&index, &dimension), &stride) in indices.iter().zip(&self.shape).zip(&self.strides) {
            if index >= dimension {
                return None;
            }
            offset = offset.checked_add(
                (index as i128)
                    .checked_mul((stride / std::mem::size_of::<f64>() as isize) as i128)?,
            )?;
        }
        usize::try_from(offset)
            .ok()
            .filter(|&index| index < self.data.len())
    }
    pub(crate) fn flat_storage_index(&self, mut index: usize) -> Option<usize> {
        if index >= self.elements {
            return None;
        }
        if self.is_c_contiguous() {
            return self.origin.checked_add(index);
        }
        let mut offset = self.origin as i128;
        for (&dimension, &stride) in self.shape.iter().zip(&self.strides).rev() {
            let coordinate = index % dimension;
            index /= dimension;
            offset = offset.checked_add(
                (coordinate as i128)
                    .checked_mul((stride / std::mem::size_of::<f64>() as isize) as i128)?,
            )?;
        }
        usize::try_from(offset)
            .ok()
            .filter(|&index| index < self.data.len())
    }
    pub(crate) fn logical_sum(&self) -> f64 {
        if self.is_c_contiguous() {
            let end = self
                .origin
                .checked_add(self.elements)
                .expect("validated contiguous buffer range");
            return self.data[self.origin..end].iter().sum();
        }
        (0..self.elements)
            .map(|index| {
                let storage = self
                    .flat_storage_index(index)
                    .expect("validated buffer layout produces valid offsets");
                self.data[storage]
            })
            .sum()
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

fn contiguous_strides(shape: &[usize]) -> Result<Vec<isize>> {
    let mut stride = std::mem::size_of::<f64>();
    let mut strides = vec![0isize; shape.len()];
    for (index, dimension) in shape.iter().enumerate().rev() {
        strides[index] = isize::try_from(stride)
            .map_err(|_| Diagnostic::new("BufferError", "buffer stride does not fit isize"))?;
        stride = stride
            .checked_mul(*dimension)
            .ok_or_else(|| Diagnostic::new("BufferError", "buffer byte length overflows usize"))?;
    }
    Ok(strides)
}

fn validate_layout(
    backing_len: usize,
    shape: &[usize],
    strides: &[isize],
    origin: usize,
    elements: usize,
) -> Result<()> {
    if origin > backing_len {
        return Err(Diagnostic::new(
            "BufferError",
            "buffer origin is outside its backing allocation",
        ));
    }
    let item_size = std::mem::size_of::<f64>() as isize;
    if strides.iter().any(|stride| stride % item_size != 0) {
        return Err(Diagnostic::new(
            "BufferError",
            "buffer strides must be aligned to the f64 item size",
        ));
    }
    if elements == 0 {
        return Ok(());
    }
    let mut minimum = 0i128;
    let mut maximum = 0i128;
    for (&dimension, &stride) in shape.iter().zip(strides) {
        let element_stride = (stride / item_size) as i128;
        let extent = (dimension.saturating_sub(1) as i128)
            .checked_mul(element_stride)
            .ok_or_else(|| Diagnostic::new("BufferError", "buffer stride range overflows"))?;
        if extent < 0 {
            minimum = minimum
                .checked_add(extent)
                .ok_or_else(|| Diagnostic::new("BufferError", "buffer stride range overflows"))?;
        } else {
            maximum = maximum
                .checked_add(extent)
                .ok_or_else(|| Diagnostic::new("BufferError", "buffer stride range overflows"))?;
        }
    }
    let origin = origin as i128;
    let first = origin
        .checked_add(minimum)
        .ok_or_else(|| Diagnostic::new("BufferError", "buffer stride range overflows"))?;
    let last = origin
        .checked_add(maximum)
        .ok_or_else(|| Diagnostic::new("BufferError", "buffer stride range overflows"))?;
    if first < 0 || last >= backing_len as i128 {
        return Err(Diagnostic::new(
            "BufferError",
            "buffer shape, strides, and origin escape the backing allocation",
        ));
    }
    Ok(())
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

    #[test]
    fn strided_layout_maps_logical_indices_without_exposing_padding() {
        let mut buffer = Buffer::f64_strided(
            &[1.0, 99.0, 2.0, 99.0, 3.0, 99.0, 4.0],
            &[2, 2],
            &[32, 16],
            0,
            true,
        )
        .unwrap();
        assert_eq!(buffer.len(), 4);
        assert!(!buffer.is_c_contiguous());
        assert_eq!(buffer.storage_index(&[1, 0]), Some(4));
        assert_eq!(buffer.flat_storage_index(3), Some(6));
        assert_eq!(buffer.logical_sum(), 10.0);
        buffer
            .set_f64(buffer.storage_index(&[0, 1]).unwrap(), 8.0)
            .unwrap();
        assert_eq!(buffer.logical_sum(), 16.0);
        assert_eq!(buffer.view().as_slice()[1], 99.0);
    }

    #[test]
    fn negative_and_zero_strides_are_range_checked() {
        let reversed = Buffer::f64_strided(&[1.0, 2.0, 3.0], &[3], &[-8], 2, false).unwrap();
        assert_eq!(reversed.flat_storage_index(0), Some(2));
        assert_eq!(reversed.flat_storage_index(2), Some(0));
        assert_eq!(reversed.logical_sum(), 6.0);

        let broadcast = Buffer::f64_strided(&[5.0], &[4], &[0], 0, false).unwrap();
        assert_eq!(broadcast.logical_sum(), 20.0);
        assert!(!broadcast.is_c_contiguous());

        assert!(Buffer::f64_strided(&[1.0, 2.0], &[3], &[-8], 1, false).is_err());
        assert!(Buffer::f64_strided(&[1.0], &[1], &[4], 0, false).is_err());
        assert!(Buffer::f64_strided(&[1.0], &[1, 1], &[8], 0, false).is_err());
        assert!(Buffer::f64_strided(&[1.0], &[1], &[8], 2, false).is_err());
    }
}
