//! Tests for the portable API and the APT process boundary.
#![cfg(feature = "cli")]
use neotales_ospm_apt_cli::{Apt, PackageError};
use neotales_ospm_core::{
    Install, InstallFile, List, PackageName, Pin, QueryOptions, Remove, Search, Update,
    UpdateSources,
};

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
fn captured_output_is_available_to_callers() {
    let output = Apt::with_executable("printf")
        .output(["%s", "rewritable output"])
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"rewritable output");
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
    // Overlay filesystems may briefly report ETXTBSY after creating a script.
    for attempt in 0..20 {
        match apt.install(&package) {
            Ok(()) => break,
            Err(PackageError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("install failed: {error}"),
        }
    }
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

#[cfg(unix)]
#[test]
fn capabilities_use_the_correct_commands_and_parse_records() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let dir = std::env::temp_dir().join(format!(
        "ospm-capabilities-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let tool = dir.join("tool");
    fs::write(&tool, r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
case "$1" in
  --show) printf 'curl:amd64\t1:8.0-1\tinstalled\nold\t1.0\tconfig-files\n' ;;
  search) printf 'curl - transfer data\n' ;;
  --no-all-versions) printf 'Package: curl\nVersion: 1:8.0-1\nArchitecture: amd64\nDescription: transfer data\n extended description\n\n' ;;
  show) printf 'Package: curl\nVersion: 1:8.0-1\nArchitecture: amd64\n\nPackage: curl\nVersion: 1:7.0-1\nArchitecture: amd64\n\n' ;;
esac
"#).unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
    let manager = Apt::with_executable(&tool).with_tools(&tool, &tool, &tool);
    let package = PackageName::new("curl:amd64").unwrap();
    manager.update(&package).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("tool.args")).unwrap(),
        "install\n--only-upgrade\n--\ncurl:amd64\n"
    );
    manager.update_sources().unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("tool.args")).unwrap(),
        "update\n"
    );
    manager.pin(&package).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("tool.args")).unwrap(),
        "hold\n--\ncurl:amd64\n"
    );
    manager.unpin(&package).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("tool.args")).unwrap(),
        "unhold\n--\ncurl:amd64\n"
    );
    let file = dir.join("package.deb");
    fs::write(&file, "").unwrap();
    manager.install_file(&file).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("tool.args")).unwrap(),
        format!("install\n--\n{}\n", file.display())
    );
    let defaults = QueryOptions::default();
    let installed = manager.list("", &defaults).unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(
        (&*installed[0].name, &*installed[0].version),
        ("curl:amd64", "1:8.0-1")
    );
    assert_eq!(manager.list("not-matching", &defaults).unwrap().len(), 0);
    let found = manager.search("curl", &defaults).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!((&*found[0].name, &*found[0].version), ("curl", "1:8.0-1"));
    assert_eq!(found[0].metadata["architecture"], "amd64");
    let options = QueryOptions {
        version: Some("1:7.0-1".into()),
        ..Default::default()
    };
    assert_eq!(
        manager.search("curl", &options).unwrap()[0].version,
        "1:7.0-1"
    );
    assert!(manager.list("", &options).unwrap().is_empty());
    let mut unsupported = QueryOptions::default();
    unsupported.extra.insert("custom".into(), Box::new(42_u32));
    assert!(matches!(
        manager.search("curl", &unsupported),
        Err(PackageError::UnsupportedOption(_))
    ));
    assert!(matches!(
        manager.list("", &unsupported),
        Err(PackageError::UnsupportedOption(_))
    ));
    assert!(manager.search("", &defaults).is_err());
    fs::remove_file(file).unwrap();
    fs::remove_file(dir.join("tool.args")).unwrap();
    fs::remove_file(tool).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn real_apt_read_operations_when_configured() {
    if std::env::var_os("NEOTALES_OSPM_TEST_APT").is_none() {
        return;
    }
    let manager = Apt::new();
    let options = QueryOptions::default();
    let installed = manager.list("", &options).unwrap();
    assert!(
        installed
            .iter()
            .any(|p| p.name.starts_with("bash") && !p.version.is_empty())
    );
    let found = manager.search("^bash$", &options).unwrap();
    assert!(
        found
            .iter()
            .any(|p| p.name == "bash" && !p.version.is_empty())
    );
    let version = found
        .iter()
        .find(|p| p.name == "bash")
        .unwrap()
        .version
        .clone();
    let versioned = QueryOptions {
        version: Some(version.clone()),
        ..Default::default()
    };
    assert!(
        manager
            .search("^bash$", &versioned)
            .unwrap()
            .iter()
            .any(|p| p.version == version)
    );
}
