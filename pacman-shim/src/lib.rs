//! LGPL C ABI shim for read-only native libalpm access.
//!
//! libalpm is GPL-2.0-or-later. Review the licensing implications when
//! distributing this shim with libalpm.
#![doc = include_str!("../README.md")]

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fmt;
use std::path::Path;

use libloading::Library;
use neotales_ospm_core::{List, Package, QueryOptions};

#[repr(C)]
struct AlpmList {
    data: *mut c_void,
    prev: *mut AlpmList,
    next: *mut AlpmList,
}

type Initialize = unsafe extern "C" fn(*const c_char, *const c_char, *mut c_int) -> *mut c_void;
type Release = unsafe extern "C" fn(*mut c_void) -> c_int;
type GetDb = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type GetCache = unsafe extern "C" fn(*mut c_void) -> *mut AlpmList;
type GetString = unsafe extern "C" fn(*mut c_void) -> *const c_char;

/// An error loading or querying libalpm.
#[derive(Debug)]
pub enum Error {
    /// A library or required symbol could not be loaded.
    Load(libloading::Error),
    /// A path contains a NUL byte or cannot be represented as UTF-8.
    InvalidPath,
    /// The installed libalpm ABI is not supported.
    UnsupportedVersion(String),
    /// Initializing libalpm failed with the indicated libalpm error code.
    Initialize(i32),
    /// The local database could not be opened or contained invalid text.
    InvalidDatabase,
    /// An unknown provider-specific option was supplied.
    UnsupportedOption(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(e) => write!(f, "libalpm: {e}"),
            Self::InvalidPath => f.write_str("invalid libalpm path"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported libalpm version: {version}")
            }
            Self::Initialize(code) => write!(f, "libalpm initialization failed: {code}"),
            Self::InvalidDatabase => f.write_str("invalid libalpm local database"),
            Self::UnsupportedOption(key) => write!(f, "unsupported pacman option: {key}"),
        }
    }
}
impl std::error::Error for Error {}

/// A native libalpm client with a configurable root and database path.
pub struct Pacman {
    library: Library,
    root: CString,
    dbpath: CString,
}

impl fmt::Debug for Pacman {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pacman")
            .field("root", &self.root)
            .field("dbpath", &self.dbpath)
            .finish_non_exhaustive()
    }
}

impl Pacman {
    /// Load a trusted libalpm shared library and configure its local database.
    ///
    /// # Safety
    /// The given library must be a trusted, ABI-compatible libalpm. Loading a
    /// shared library can execute arbitrary initialization code.
    pub unsafe fn load(library: impl AsRef<Path>, root: &str, dbpath: &str) -> Result<Self, Error> {
        let root = CString::new(root).map_err(|_| Error::InvalidPath)?;
        let dbpath = CString::new(dbpath).map_err(|_| Error::InvalidPath)?;
        // SAFETY: the caller accepts responsibility for the loaded library.
        let library = unsafe { Library::new(library.as_ref()) }.map_err(Error::Load)?;
        // SAFETY: the caller guarantees that this library provides libalpm.
        let version = unsafe {
            let version = library
                .get::<unsafe extern "C" fn() -> *const c_char>(b"alpm_version\0")
                .map_err(Error::Load)?;
            let ptr = version();
            if ptr.is_null() {
                return Err(Error::InvalidDatabase);
            }
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        };
        // The ABI declarations below are verified against libalpm 16.
        if version.split('.').next() != Some("16") {
            return Err(Error::UnsupportedVersion(version));
        }
        Ok(Self {
            library,
            root,
            dbpath,
        })
    }
}

struct Handle {
    ptr: *mut c_void,
    release: Release,
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: ptr is a live handle returned by alpm_initialize.
        unsafe {
            (self.release)(self.ptr);
        }
    }
}

impl List for Pacman {
    type Error = Error;

    fn list(&self, query: &str, options: &QueryOptions) -> Result<Vec<Package>, Error> {
        if let Some(key) = options.extra.keys().next() {
            return Err(Error::UnsupportedOption(key.clone()));
        }
        // SAFETY: loading requires the caller to supply a trusted libalpm 16.
        unsafe {
            let initialize = *self
                .library
                .get::<Initialize>(b"alpm_initialize\0")
                .map_err(Error::Load)?;
            let release = *self
                .library
                .get::<Release>(b"alpm_release\0")
                .map_err(Error::Load)?;
            let get_db = *self
                .library
                .get::<GetDb>(b"alpm_get_localdb\0")
                .map_err(Error::Load)?;
            let get_cache = *self
                .library
                .get::<GetCache>(b"alpm_db_get_pkgcache\0")
                .map_err(Error::Load)?;
            let get_name = *self
                .library
                .get::<GetString>(b"alpm_pkg_get_name\0")
                .map_err(Error::Load)?;
            let get_version = *self
                .library
                .get::<GetString>(b"alpm_pkg_get_version\0")
                .map_err(Error::Load)?;
            let get_arch = *self
                .library
                .get::<GetString>(b"alpm_pkg_get_arch\0")
                .map_err(Error::Load)?;
            let get_desc = *self
                .library
                .get::<GetString>(b"alpm_pkg_get_desc\0")
                .map_err(Error::Load)?;
            let mut code = 0;
            let ptr = initialize(self.root.as_ptr(), self.dbpath.as_ptr(), &mut code);
            if ptr.is_null() {
                return Err(Error::Initialize(code));
            }
            let handle = Handle { ptr, release };
            let db = get_db(handle.ptr);
            if db.is_null() {
                return Err(Error::InvalidDatabase);
            }
            let mut node = get_cache(db);
            let mut packages = Vec::new();
            while let Some(item) = node.as_ref() {
                if item.data.is_null() {
                    return Err(Error::InvalidDatabase);
                }
                let text = |p: *const c_char| -> Result<String, Error> {
                    if p.is_null() {
                        return Err(Error::InvalidDatabase);
                    }
                    CStr::from_ptr(p)
                        .to_str()
                        .map(str::to_owned)
                        .map_err(|_| Error::InvalidDatabase)
                };
                let name = text(get_name(item.data))?;
                let version = text(get_version(item.data))?;
                if name.contains(query) && options.version.as_ref().is_none_or(|v| v == &version) {
                    let mut metadata = BTreeMap::new();
                    for (key, value) in [
                        ("architecture", get_arch(item.data)),
                        ("description", get_desc(item.data)),
                    ] {
                        if !value.is_null() {
                            metadata.insert(key.to_owned(), text(value)?);
                        }
                    }
                    packages.push(Package {
                        name,
                        version,
                        metadata,
                    });
                }
                node = item.next;
            }
            Ok(packages)
        }
    }
}

/// ABI version of the pacman shim.
#[unsafe(no_mangle)]
pub extern "C" fn neotales_ospm_pacman_abi_version() -> u32 {
    1
}

/// Enumerate installed packages using libalpm.
///
/// # Safety
/// All string pointers must be valid, NUL-terminated UTF-8 strings for the call.
/// `library` must name a trusted libalpm. The callback must not unwind and must
/// not retain the borrowed string pointers. `context` must be valid for it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn neotales_ospm_pacman_list(
    library: *const c_char,
    root: *const c_char,
    dbpath: *const c_char,
    callback: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32>,
    context: *mut c_void,
) -> i32 {
    if library.is_null() || root.is_null() || dbpath.is_null() || callback.is_none() {
        return -1;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: The caller guarantees the string pointers are valid.
        let (Ok(library), Ok(root), Ok(dbpath)) = (unsafe {
            (
                CStr::from_ptr(library).to_str(),
                CStr::from_ptr(root).to_str(),
                CStr::from_ptr(dbpath).to_str(),
            )
        }) else {
            return -1;
        };
        // SAFETY: The caller guarantees the library is trusted.
        let Ok(pacman) = (unsafe { Pacman::load(library, root, dbpath) }) else {
            return -1;
        };
        let Ok(packages) = pacman.list_all() else {
            return -1;
        };
        let callback = callback.expect("checked above");
        for package in packages {
            let (Ok(name), Ok(version)) =
                (CString::new(package.name), CString::new(package.version))
            else {
                return -1;
            };
            // SAFETY: Caller guarantees callback and context validity.
            if unsafe { callback(context, name.as_ptr(), version.as_ptr()) } != 0 {
                return -1;
            }
        }
        0
    }))
    .unwrap_or(-1)
}
