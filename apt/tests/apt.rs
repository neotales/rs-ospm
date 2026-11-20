//! Tests for the portable API and the APT process boundary.
#![cfg(feature = "cli")]
use neotales_ospm_apt::{Apt, PackageError};
use neotales_ospm_core::{Install, PackageName, Remove};

#[test]
fn package_names_reject_options_and_shell_syntax() {
    for bad in ["", "-y", "foo bar", "foo;bar", "foo/bar", "foo\nbar", "é"] {
        assert!(PackageName::new(bad).is_err(), "{bad:?}");
    }
    for good in ["libssl-dev", "libc6:amd64", "g++", "python3.12"] {
        assert!(PackageName::new(good).is_ok(), "{good:?}");
    }
}

#[test]
fn native_command_exposes_the_configured_executable() {
    let apt = Apt::with_executable("custom-apt");
    assert_eq!(apt.command().get_program(), "custom-apt");
}

#[cfg(unix)]
#[test]
fn portable_operations_pass_arguments_and_report_errors() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let dir = std::env::temp_dir().join(format!(
        "ospm-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let script = dir.join("apt-get");
    fs::write(&script, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nif [ \"$1\" = remove ]; then echo refused >&2; exit 42; fi\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let apt = Apt::with_executable(&script);
    let package = PackageName::new("libc6:amd64").unwrap();
    apt.install(&package).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("apt-get.args")).unwrap(),
        "install\n--\nlibc6:amd64\n"
    );
    match apt.remove(&package).unwrap_err() {
        PackageError::CommandFailed { status, stderr } => {
            assert_eq!(status.code(), Some(42));
            assert_eq!(stderr, b"refused\n");
        }
        error => panic!("unexpected error: {error}"),
    }
    assert_eq!(
        fs::read_to_string(dir.join("apt-get.args")).unwrap(),
        "remove\n--\nlibc6:amd64\n"
    );
    fs::remove_file(dir.join("apt-get.args")).unwrap();
    fs::remove_file(script).unwrap();
    fs::remove_dir(dir).unwrap();
}
