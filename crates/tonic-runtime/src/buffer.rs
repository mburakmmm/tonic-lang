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
    pub fn estimated_bytes(&self) -> usize {
        self.data.len() * std::mem::size_of::<f64>()
            + self.shape.len() * std::mem::size_of::<usize>()
            + self.strides.len() * std::mem::size_of::<isize>()
    }
}
