#![allow(unsafe_code)]

use crate::{module_symbols, validate_universal_filename, Platform, UNIVERSAL_ABI};
use libloading::Library;
use std::{
    error::Error,
    ffi::c_void,
    fmt,
    mem::ManuallyDrop,
    path::{Path, PathBuf},
    ptr::NonNull,
};

type RequiredVersionFn = unsafe extern "C" fn() -> u32;
type InitializeContextFn = unsafe extern "C" fn(*mut c_void);
type InitializeModuleFn = unsafe extern "C" fn() -> *mut c_void;

/// A validated HPy Universal library whose mapping remains live for the
/// process lifetime.
///
/// HPy method, type and payload pointers may outlive an import frame. H1 has no
/// complete unload protocol, so dropping this value intentionally does not call
/// `dlclose`/`FreeLibrary`.
pub struct PinnedUniversalModule {
    module_name: String,
    path: PathBuf,
    required_major: u32,
    required_minor: u32,
    _initialize_context: InitializeContextFn,
    initialize_module: InitializeModuleFn,
    _library: ManuallyDrop<Library>,
}

impl fmt::Debug for PinnedUniversalModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PinnedUniversalModule")
            .field("module_name", &self.module_name)
            .field("path", &self.path)
            .field("required_abi", &(self.required_major, self.required_minor))
            .field("pinned", &true)
            .finish_non_exhaustive()
    }
}

impl PinnedUniversalModule {
    /// Open and validate a trusted native HPy Universal extension.
    ///
    /// # Safety
    ///
    /// The library and all of its process-load constructors must be trusted.
    /// Its four HPy init/version symbols must use the HPy 0.9 Universal C ABI;
    /// native code can otherwise violate Rust's process invariants before or
    /// during symbol calls.
    pub unsafe fn load(module_name: &str, path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let platform = Platform::current().ok_or(LoadError::UnsupportedPlatform)?;
        let path = path.as_ref();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| LoadError::InvalidPath(path.to_path_buf()))?;
        let symbols = module_symbols(module_name).map_err(|error| LoadError::ModuleName {
            message: error.to_string(),
        })?;
        validate_universal_filename(module_name, file_name, platform).map_err(|_| {
            LoadError::Filename {
                expected: format!("{module_name}{}", platform.universal_suffix()),
                actual: file_name.to_owned(),
            }
        })?;

        // SAFETY: the caller guarantees that process-load constructors in this
        // trusted native extension obey the host process contract.
        let library = unsafe { Library::new(path) }.map_err(|error| LoadError::Open {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        // SAFETY: symbol names were generated from a validated C identifier;
        // the caller guarantees the library implements the pinned HPy C ABI.
        let required_major: RequiredVersionFn =
            unsafe { resolve(&library, &symbols.required_major) }?;
        // SAFETY: same invariant as the major-version symbol above.
        let required_minor: RequiredVersionFn =
            unsafe { resolve(&library, &symbols.required_minor) }?;
        // SAFETY: the pointer is not called until a compatible context exists;
        // copying the function pointer while the library is live is valid.
        let initialize_context: InitializeContextFn =
            unsafe { resolve(&library, &symbols.initialize_context) }?;
        // SAFETY: the caller guarantees the symbol has HPy's no-argument module
        // definition signature. The pinned library keeps it valid indefinitely.
        let initialize_module: InitializeModuleFn =
            unsafe { resolve(&library, &symbols.initialize_module) }?;

        // SAFETY: both functions are required by HPy_MODINIT, take no arguments,
        // and were resolved under the trusted pinned-ABI precondition above.
        let required_major_value = unsafe { required_major() };
        // SAFETY: identical version-getter contract for the minor component.
        let required_minor_value = unsafe { required_minor() };
        if required_major_value != UNIVERSAL_ABI.major || required_minor_value > UNIVERSAL_ABI.minor
        {
            return Err(LoadError::AbiMismatch {
                required_major: required_major_value,
                required_minor: required_minor_value,
                supported_major: UNIVERSAL_ABI.major,
                supported_minor: UNIVERSAL_ABI.minor,
            });
        }

        Ok(Self {
            module_name: module_name.to_owned(),
            path: path.to_path_buf(),
            required_major: required_major_value,
            required_minor: required_minor_value,
            _initialize_context: initialize_context,
            initialize_module,
            _library: ManuallyDrop::new(library),
        })
    }

    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn required_abi(&self) -> (u32, u32) {
        (self.required_major, self.required_minor)
    }

    /// Return the extension's static `HPyModuleDef` pointer.
    ///
    /// # Safety
    ///
    /// The trusted library must continue to honor the HPy 0.9 Universal
    /// `HPyInit_<name>` signature promised to [`Self::load`].
    pub unsafe fn module_definition(&self) -> Result<NonNull<c_void>, LoadError> {
        // SAFETY: the load-time ABI precondition covers this copied function
        // pointer, and `library` is intentionally never unloaded.
        NonNull::new(unsafe { (self.initialize_module)() }).ok_or(LoadError::NullModuleDefinition)
    }

    pub(crate) unsafe fn initialize_context(&self, context: *mut c_void) {
        // SAFETY: the host supplies its stable HPy 0.9 context allocation and
        // `load` validated the extension initializer's C signature.
        unsafe { (self._initialize_context)(context) };
    }

    /// Expose whether the validated global-context initializer is present.
    #[must_use]
    pub const fn has_context_initializer(&self) -> bool {
        true
    }

    /// The mapping is intentionally leaked until process exit.
    #[must_use]
    pub const fn is_pinned(&self) -> bool {
        true
    }
}

unsafe fn resolve<T: Copy>(library: &Library, name: &str) -> Result<T, LoadError> {
    let terminated = format!("{name}\0");
    // SAFETY: the caller establishes the requested C signature. The terminating
    // NUL is explicit and the returned value is copied while `library` is live.
    let symbol = unsafe { library.get::<T>(terminated.as_bytes()) }.map_err(|error| {
        LoadError::MissingSymbol {
            symbol: name.to_owned(),
            message: error.to_string(),
        }
    })?;
    Ok(*symbol)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadError {
    UnsupportedPlatform,
    InvalidPath(PathBuf),
    ModuleName {
        message: String,
    },
    Filename {
        expected: String,
        actual: String,
    },
    Open {
        path: PathBuf,
        message: String,
    },
    MissingSymbol {
        symbol: String,
        message: String,
    },
    AbiMismatch {
        required_major: u32,
        required_minor: u32,
        supported_major: u32,
        supported_minor: u32,
    },
    NullModuleDefinition,
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("HPy Universal loading is unavailable on this platform")
            }
            Self::InvalidPath(path) => write!(
                formatter,
                "HPy library path has no valid UTF-8 filename: {}",
                path.display()
            ),
            Self::ModuleName { message } => formatter.write_str(message),
            Self::Filename { expected, actual } => write!(
                formatter,
                "HPy library filename must be '{expected}', got '{actual}'"
            ),
            Self::Open { path, message } => {
                write!(formatter, "cannot open HPy library {}: {message}", path.display())
            }
            Self::MissingSymbol { symbol, message } => {
                write!(formatter, "missing HPy symbol '{symbol}': {message}")
            }
            Self::AbiMismatch {
                required_major,
                required_minor,
                supported_major,
                supported_minor,
            } => write!(
                formatter,
                "HPy ABI {required_major}.{required_minor} is incompatible with host {supported_major}.{supported_minor}"
            ),
            Self::NullModuleDefinition => {
                formatter.write_str("HPy module initializer returned a null definition")
            }
        }
    }
}

impl Error for LoadError {}
