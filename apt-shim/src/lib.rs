//! C ABI over libapt-pkg. Requires the `native` feature and system APT headers.
//!
//! This shim is LGPL-2.1-or-later; libapt-pkg is GPL-2.0-or-later.

#[cfg(all(target_os = "linux", feature = "native"))]
mod native {
    use std::ffi::{CStr, c_char};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    /// Version of the C ABI.
    #[unsafe(no_mangle)]
    pub extern "C" fn neotales_ospm_apt_abi_version() -> u32 {
        1
    }

    /// Query package installation status.
    ///
    /// # Safety
    /// `name` must be a valid, NUL-terminated string for the call's duration.
    /// All accesses to libapt-pkg in the process must be serialized.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn neotales_ospm_apt_is_installed(name: *const c_char) -> i32 {
        if name.is_null() {
            return -1;
        }
        // SAFETY: The caller promises a valid NUL-terminated string.
        let name = match unsafe { CStr::from_ptr(name) }.to_str() {
            Ok(name) if !name.is_empty() => name,
            _ => return -1,
        };
        catch_unwind(AssertUnwindSafe(|| {
            let mut cache = apt_pkg_native::Cache::get_singleton();
            let (name, arch) = name
                .split_once(':')
                .map_or((name, None), |(n, a)| (n, Some(a)));
            if name.is_empty() || arch == Some("") {
                return -1;
            }
            let mut iter = match arch {
                Some(arch) => cache.find_by_name_arch(name, arch),
                None => cache.find_by_name(name),
            };
            i32::from(
                iter.next()
                    .is_some_and(|pkg| pkg.current_version().is_some()),
            )
        }))
        .unwrap_or(-1)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn validates_input_and_finds_an_installed_package() {
            // SAFETY: Both pointers are null or valid static C strings.
            unsafe {
                assert_eq!(neotales_ospm_apt_is_installed(std::ptr::null()), -1);
                assert_eq!(neotales_ospm_apt_is_installed(c"".as_ptr()), -1);
                assert_eq!(neotales_ospm_apt_is_installed(c"base-files".as_ptr()), 1);
                assert_eq!(
                    neotales_ospm_apt_is_installed(c"not-a-real-neotales-package".as_ptr()),
                    0
                );
            }
        }
    }
}
