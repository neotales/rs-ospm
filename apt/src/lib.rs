//! MIT-licensed Rust client for the APT C ABI shim.
//!
//! Loading this client does not make libapt-pkg's GPL licence LGPL or MIT.
#![doc = include_str!("../README.md")]

use std::ffi::{CString, c_char};
use std::fmt;
use std::path::Path;

use libloading::Library;
use neotales_ospm_core::{IsInstalled, PackageName};

/// An error loading or calling the APT shim.
#[derive(Debug)]
pub enum AptError {
    /// The shared library or a required symbol could not be loaded.
    Load(libloading::Error),
    /// The library's ABI is not supported by this client.
    AbiVersion(u32),
    /// The shim could not complete the request.
    Query,
}

impl fmt::Display for AptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(e) => write!(f, "APT shim: {e}"),
            Self::AbiVersion(v) => write!(f, "unsupported APT shim ABI version {v}"),
            Self::Query => f.write_str("APT native query failed"),
        }
    }
}
impl std::error::Error for AptError {}

/// A loaded APT native shim.
pub struct Apt {
    library: Library,
    query: unsafe extern "C" fn(*const c_char) -> i32,
}

impl fmt::Debug for Apt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Apt").finish_non_exhaustive()
    }
}

impl Apt {
    /// Loads the shim from an explicit path.
    ///
    /// # Safety
    /// The library must be a trusted, compatible implementation of the
    /// neotales APT ABI; loading a library executes its initializers. All
    /// libapt-pkg access in the process must be serialized by the caller.
    pub unsafe fn load(path: impl AsRef<Path>) -> Result<Self, AptError> {
        // SAFETY: The caller is responsible for trusting the library.
        let library = unsafe { Library::new(path.as_ref()) }.map_err(AptError::Load)?;
        // SAFETY: The caller promises the library implements this ABI.
        let version = unsafe {
            *library
                .get::<unsafe extern "C" fn() -> u32>(b"neotales_ospm_apt_abi_version\0")
                .map_err(AptError::Load)?
        };
        // SAFETY: The caller promises the library implements this ABI.
        let version = unsafe { version() };
        if version != 1 {
            return Err(AptError::AbiVersion(version));
        }
        // SAFETY: The caller promises the library implements this ABI.
        let query = unsafe {
            *library
                .get::<unsafe extern "C" fn(*const c_char) -> i32>(
                    b"neotales_ospm_apt_is_installed\0",
                )
                .map_err(AptError::Load)?
        };
        Ok(Self { library, query })
    }
}

impl IsInstalled for Apt {
    type Error = AptError;

    fn is_installed(&self, package: &PackageName) -> Result<bool, Self::Error> {
        let name = CString::new(package.as_str()).map_err(|_| AptError::Query)?;
        // Keep the shared library loaded for the entire call.
        let _keep_loaded = &self.library;
        // SAFETY: The ABI was checked at load and name is a valid C string.
        match unsafe { (self.query)(name.as_ptr()) } {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(AptError::Query),
        }
    }
}
