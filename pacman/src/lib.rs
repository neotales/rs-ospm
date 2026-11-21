//! MIT client for the LGPL pacman shim.
#![doc = include_str!("../README.md")]

use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use libloading::Library;
use neotales_ospm_core::{List, Package, QueryOptions};

type Callback = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32;
type Enumerate = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const c_char,
    Option<Callback>,
    *mut c_void,
) -> i32;

/// Error loading or using the pacman shim.
#[derive(Debug)]
pub enum Error {
    /// Shared library or symbol load failure.
    Load(libloading::Error),
    /// Unsupported shim ABI version.
    Version(u32),
    /// A string contained a NUL byte.
    InvalidInput,
    /// The native query or its callback failed.
    Query,
    /// An unknown provider-specific option was supplied.
    UnsupportedOption(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(e) => write!(f, "pacman shim: {e}"),
            Self::Version(v) => write!(f, "unsupported pacman shim ABI version {v}"),
            Self::InvalidInput => f.write_str("invalid native path"),
            Self::Query => f.write_str("libalpm package query failed"),
            Self::UnsupportedOption(key) => write!(f, "unsupported pacman option: {key}"),
        }
    }
}
impl std::error::Error for Error {}

/// Native pacman client backed by a loaded shim.
pub struct Pacman {
    library: Library,
    enumerate: Enumerate,
    alpm: CString,
    root: CString,
    dbpath: CString,
}
impl fmt::Debug for Pacman {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pacman").finish_non_exhaustive()
    }
}
impl Pacman {
    /// Load the shim, with explicit trusted libalpm and database paths.
    ///
    /// # Safety
    /// Both libraries must be trusted and implement their documented ABIs.
    /// Loading shared libraries can execute arbitrary initialization code.
    pub unsafe fn load(
        shim: impl AsRef<Path>,
        alpm: &str,
        root: &str,
        dbpath: &str,
    ) -> Result<Self, Error> {
        let alpm = CString::new(alpm).map_err(|_| Error::InvalidInput)?;
        let root = CString::new(root).map_err(|_| Error::InvalidInput)?;
        let dbpath = CString::new(dbpath).map_err(|_| Error::InvalidInput)?;
        // SAFETY: The caller accepts the library loading contract.
        let library = unsafe { Library::new(shim.as_ref()) }.map_err(Error::Load)?;
        // SAFETY: The caller promises the shim provides these ABI symbols.
        let (version, enumerate) = unsafe {
            let version = *library
                .get::<unsafe extern "C" fn() -> u32>(b"neotales_ospm_pacman_abi_version\0")
                .map_err(Error::Load)?;
            let enumerate = *library
                .get::<Enumerate>(b"neotales_ospm_pacman_list\0")
                .map_err(Error::Load)?;
            (version(), enumerate)
        };
        if version != 1 {
            return Err(Error::Version(version));
        }
        Ok(Self {
            library,
            enumerate,
            alpm,
            root,
            dbpath,
        })
    }
}

unsafe extern "C" fn collect(
    context: *mut c_void,
    name: *const c_char,
    version: *const c_char,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if context.is_null() || name.is_null() || version.is_null() {
            return -1;
        }
        // SAFETY: The trusted shim guarantees callback pointers remain valid.
        let (packages, name, version) = unsafe {
            (
                &mut *context.cast::<Vec<Package>>(),
                CStr::from_ptr(name),
                CStr::from_ptr(version),
            )
        };
        let (Ok(name), Ok(version)) = (name.to_str(), version.to_str()) else {
            return -1;
        };
        packages.push(Package {
            name: name.into(),
            version: version.into(),
            metadata: Default::default(),
        });
        0
    }))
    .unwrap_or(-1)
}

impl List for Pacman {
    type Error = Error;
    fn list(&self, query: &str, options: &QueryOptions) -> Result<Vec<Package>, Error> {
        if let Some(key) = options.extra.keys().next() {
            return Err(Error::UnsupportedOption(key.clone()));
        }
        let mut packages: Vec<Package> = Vec::new();
        let _keep_loaded = &self.library;
        // SAFETY: Shim and its dependencies were validated by the caller of load;
        // context points to the live vector throughout this synchronous call.
        let status = unsafe {
            (self.enumerate)(
                self.alpm.as_ptr(),
                self.root.as_ptr(),
                self.dbpath.as_ptr(),
                Some(collect),
                (&mut packages as *mut Vec<Package>).cast(),
            )
        };
        if status != 0 {
            return Err(Error::Query);
        }
        packages.retain(|p| {
            p.name.contains(query) && options.version.as_ref().is_none_or(|v| v == &p.version)
        });
        Ok(packages)
    }
}
