//! Exercise the Rust client against a small test C ABI implementation.
#![cfg(target_os = "linux")]

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use neotales_ospm_apt::{Apt, AptError};
use neotales_ospm_core::{IsInstalled, PackageName};

#[test]
fn loads_and_calls_a_compatible_library() {
    let dir = std::env::temp_dir().join(format!(
        "ospm-ffi-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("stub.c");
    let library = dir.join("libstub.so");
    fs::write(
        &source,
        r#"
        #include <string.h>
        unsigned int neotales_ospm_apt_abi_version(void) { return 1; }
        int neotales_ospm_apt_is_installed(const char *name) {
            if (!strcmp(name, "error")) return -1;
            return !strcmp(name, "installed");
        }
    "#,
    )
    .unwrap();
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&library)
            .arg(&source)
            .status()
            .unwrap()
            .success()
    );
    // SAFETY: The test source above implements the documented ABI.
    let apt = unsafe { Apt::load(&library) }.unwrap();
    assert!(
        apt.is_installed(&PackageName::new("installed").unwrap())
            .unwrap()
    );
    assert!(
        !apt.is_installed(&PackageName::new("missing").unwrap())
            .unwrap()
    );
    assert!(matches!(
        apt.is_installed(&PackageName::new("error").unwrap()),
        Err(AptError::Query)
    ));
    drop(apt);
    fs::remove_file(library).unwrap();
    fs::remove_file(source).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn native_shim_when_configured() {
    let Some(path) = std::env::var_os("NEOTALES_OSPM_APT_SHIM") else {
        return;
    };
    // SAFETY: The test environment provides the shim built from this workspace,
    // and no other code in this process uses libapt-pkg.
    let apt = unsafe { Apt::load(path) }.unwrap();
    assert!(
        apt.is_installed(&PackageName::new("base-files").unwrap())
            .unwrap()
    );
    assert!(
        !apt.is_installed(&PackageName::new("not-a-real-neotales-package").unwrap())
            .unwrap()
    );
}
