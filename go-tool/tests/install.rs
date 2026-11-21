//! Verify validation and process invocation without a real Go toolchain.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neotales_ospm_go_cli::{Go, GoCommand, GoError, InvalidCommand};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "ospm-go-cli-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn write_script(&self, body: &str) -> PathBuf {
        let executable = self.0.join("go");
        fs::write(&executable, body).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        executable
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn install_with_retry(go: &Go, command: &GoCommand) -> Result<(), GoError> {
    for attempt in 0..20 {
        match go.install(command) {
            Err(GoError::Io(error)) if error.raw_os_error() == Some(26) && attempt < 19 => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
    unreachable!("retry loop always returns")
}

#[test]
fn default_executable_is_go_from_path() {
    assert_eq!(Go::new().executable(), Path::new("go"));
}

#[test]
fn install_passes_one_versioned_argument() {
    let scratch = Scratch::new("success");
    let executable = scratch.write_script("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\n");
    let go = Go::with_executable(&executable);
    let command = GoCommand::new("example.com/tool/cmd/tool", "v1.2.3").unwrap();

    install_with_retry(&go, &command).unwrap();

    let arguments = fs::read_to_string(executable.with_extension("args")).unwrap();
    assert_eq!(arguments, "install\nexample.com/tool/cmd/tool@v1.2.3\n");
}

#[test]
fn failure_reports_status_and_captured_stderr() {
    let scratch = Scratch::new("failure");
    let executable = scratch.write_script("#!/bin/sh\nprintf 'cannot build\\n' >&2\nexit 7\n");
    let go = Go::with_executable(&executable);
    let command = GoCommand::new("example.com/tool", "v1.2.3").unwrap();

    match install_with_retry(&go, &command).unwrap_err() {
        GoError::Exit { status, stderr } => {
            assert_eq!(status.code(), Some(7));
            assert_eq!(stderr, b"cannot build\n");
        }
        GoError::Io(error) => panic!("unexpected I/O error: {error}"),
    }
}

#[test]
fn accessors_preserve_the_validated_command() {
    let command = GoCommand::new("github.com/user/repo/v2/cmd/tool", "v2.1.0").unwrap();
    assert_eq!(command.module_path(), "github.com/user/repo/v2/cmd/tool");
    assert_eq!(command.version(), "v2.1.0");
    assert_eq!(
        command.argument(),
        "github.com/user/repo/v2/cmd/tool@v2.1.0"
    );
}

#[test]
fn accepts_fully_qualified_module_paths() {
    for path in [
        "example.com/tool",
        "example.com/tool/cmd/tool",
        "github.com/user/repo/v2/cmd/tool",
        "gopkg.in/yaml.v3",
        "example.com/foo_bar/cmd/tool",
        "example.com/foo~bar/cmd/tool",
        "example.com/foo..bar/cmd/tool",
        "example.com/foo~/cmd/tool",
        "example.com/conx/cmd/tool",
        "example.com/auxiliary/cmd/tool",
        "conx.example.com/tool",
    ] {
        assert!(
            GoCommand::new(path, "v1.2.3").is_ok(),
            "module path {path:?} should be accepted"
        );
    }
}

#[test]
fn rejects_wildcard_patterns() {
    // `...` is a Go package pattern. The provider installs exactly one command.
    for path in [
        "example.com/...",
        "example.com/.../tool",
        "example.com/tool/...",
        "example.com/foo...bar",
    ] {
        assert_eq!(
            GoCommand::new(path, "v1.2.3").unwrap_err(),
            InvalidCommand::ModulePath,
            "wildcard path {path:?} should be rejected"
        );
    }
}

#[test]
fn rejects_go_invalid_path_elements() {
    // Confirmed against go1.27.4 `go install` before module lookup: these are
    // rejected by x/mod/module's per-element checks.
    for path in [
        ".example.com/tool",
        "example.com./tool",
        "example.com/.hidden",
        "example.com/tool.",
        "example.com/CON",
        "example.com/con.txt",
        "example.com/Aux",
        "example.com/COM1",
        "example.com/lpt9.tar.gz",
        "con.example.com/tool",
        "example.com/foo~1",
        "example.com/foo~1.txt",
    ] {
        assert_eq!(
            GoCommand::new(path, "v1.2.3").unwrap_err(),
            InvalidCommand::ModulePath,
            "Go-invalid path {path:?} should be rejected"
        );
    }
}

#[test]
fn rejects_unqualified_module_paths() {
    for path in ["tool", "cmd/tool", "tool/subcommand"] {
        assert_eq!(
            GoCommand::new(path, "v1.2.3").unwrap_err(),
            InvalidCommand::UnqualifiedModulePath,
            "module path {path:?} should be unqualified"
        );
    }
}

#[test]
fn rejects_unsafe_or_malformed_module_paths() {
    for path in [
        "",
        ".",
        "..",
        "/example.com/tool",
        "-example.com/tool",
        "example.com/",
        "example.com//tool",
        "example.com/./tool",
        "example.com/../tool",
        "example.com/tool/",
        "example.com/tool@v1.0.0",
        "Example.com/tool",
        "example.com/tool name",
        "example.com/tool\\subcommand",
        "example.com/tool?query",
    ] {
        assert_eq!(
            GoCommand::new(path, "v1.2.3").unwrap_err(),
            InvalidCommand::ModulePath,
            "module path {path:?} should be rejected"
        );
    }
}

#[test]
fn accepts_complete_versions() {
    for version in [
        "v0.0.0",
        "v1.2.3",
        "v1.2.3-rc.1",
        "v1.2.3-0",
        "v2.0.0+incompatible",
        "v0.0.0-20240101120000-abcdef123456",
    ] {
        assert!(
            GoCommand::new("example.com/tool", version).is_ok(),
            "version {version:?} should be accepted"
        );
    }
}

#[test]
fn rejects_versions_that_are_not_exact() {
    for version in [
        "",
        "latest",
        "v1",
        "v1.2",
        "1.2.3",
        "V1.2.3",
        "v01.2.3",
        "v1.02.3",
        "v1.2.03",
        "v1.2.3.4",
        "v1.2.x",
        "v1.2.3-",
        "v1.2.3-01",
        "v1.2.3+foo",
        "v1.2.3-rc.1+build",
        "v1.2.3 ",
        " v1.2.3",
        "v1.2.3@latest",
        "-v1.2.3",
    ] {
        assert_eq!(
            GoCommand::new("example.com/tool", version).unwrap_err(),
            InvalidCommand::Version,
            "version {version:?} should be rejected"
        );
    }
}
