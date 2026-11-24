//! Real Deno global and project behaviour, run only when a real executable is
//! supplied.
//!
//! Every test creates a unique temporary directory, clears the inherited
//! environment with `env -i`, and points `HOME`, `TMPDIR` and `DENO_DIR` into
//! that directory. The installation root is a separate directory. No user
//! module cache, user installation root or user configuration is read or
//! written.
//!
//! Run the offline tests with a real Deno executable:
//!
//! ```text
//! NEOTALES_OSPM_TEST_DENO=/path/to/deno \
//!   cargo test --test real_deno -- --ignored --test-threads=1
//! ```
//!
//! `project_add_pins_an_exact_version` also contacts the JSR registry. Run it
//! with both variables set, or name that test explicitly:
//!
//! ```text
//! NEOTALES_OSPM_TEST_DENO=/path/to/deno NEOTALES_OSPM_TEST_DENO_NETWORK=1 \
//!   cargo test --test real_deno -- --ignored --test-threads=1
//! ```
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use neotales_ospm_deno_cli::{
    Deno, DependencySection, ExecutableName, GlobalTool, PackageName, RegistryPackage, ScriptSource,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "ospm-deno-real-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }

    fn write_executable(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn assignment(key: &str, path: &Path) -> String {
    format!("{key}={}", shell_quote(&path.to_string_lossy()))
}

/// The real Deno executable named by `NEOTALES_OSPM_TEST_DENO`.
fn real_deno() -> PathBuf {
    let value = std::env::var("NEOTALES_OSPM_TEST_DENO").unwrap_or_else(|_| {
        panic!("set NEOTALES_OSPM_TEST_DENO to the real deno executable before running this test")
    });
    assert!(
        !value.is_empty() && value != "1",
        "NEOTALES_OSPM_TEST_DENO must name the real deno executable, not a toggle"
    );
    let path = fs::canonicalize(&value).unwrap_or_else(|error| {
        panic!("cannot resolve NEOTALES_OSPM_TEST_DENO={value:?}: {error}")
    });
    let metadata = fs::metadata(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    assert!(metadata.is_file(), "{} is not a file", path.display());
    assert!(
        metadata.permissions().mode() & 0o111 != 0,
        "{} is not executable",
        path.display()
    );
    path
}

/// A Deno executable whose entire environment and cache live in `scratch`.
///
/// `env -i` removes inherited `DENO_*`, proxy, registry and credential
/// variables. The explicit paths below are the only environment Deno sees.
fn isolated_deno(scratch: &Scratch) -> Deno {
    let real = real_deno();
    for directory in ["home", "tmp", "cache", "project"] {
        fs::create_dir_all(scratch.path().join(directory)).unwrap();
    }
    let deno_parent = real.parent().expect("deno has a parent directory");
    let path = format!("{}:/usr/bin:/bin", deno_parent.display());
    let assignments = [
        format!("PATH={}", shell_quote(&path)),
        assignment("HOME", &scratch.path().join("home")),
        assignment("TMPDIR", &scratch.path().join("tmp")),
        assignment("DENO_DIR", &scratch.path().join("cache")),
        "NO_COLOR='1'".to_owned(),
        "DENO_NO_UPDATE_CHECK='1'".to_owned(),
    ];
    let environment = assignments
        .iter()
        .map(|line| format!("  {line} \\"))
        .collect::<Vec<_>>()
        .join("\n");
    let script = format!(
        "#!/bin/sh\nset -eu\nexec /usr/bin/env -i \\\n{environment}\n  {} \"$@\"\n",
        shell_quote(&real.to_string_lossy())
    );
    let wrapper = scratch.write_executable("deno-wrapper", &script);
    Deno::with_executable(wrapper)
        .with_install_root(scratch.path().join("install-root"))
        .with_working_directory(scratch.path().join("project"))
}

fn run_launcher(scratch: &Scratch, launcher: &Path) -> std::process::Output {
    let real = real_deno();
    let deno_parent = real.parent().expect("deno has a parent directory");
    Command::new(launcher)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", deno_parent.display()))
        .env("HOME", scratch.path().join("home"))
        .env("TMPDIR", scratch.path().join("tmp"))
        .env("DENO_DIR", scratch.path().join("cache"))
        .env("NO_COLOR", "1")
        .env("DENO_NO_UPDATE_CHECK", "1")
        .current_dir(scratch.path().join("project"))
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires deno; set NEOTALES_OSPM_TEST_DENO to the real deno executable and run with --ignored"]
fn installs_runs_and_removes_a_local_script() {
    let scratch = Scratch::new("script");
    let script = scratch.write("hello.ts", "console.log('ospm-deno-script-ok');\n");
    let tool = GlobalTool::script("ospm-deno-hello", ScriptSource::path(&script).unwrap()).unwrap();
    let deno = isolated_deno(&scratch);

    let installed = deno.install_global(&tool).unwrap();
    assert_eq!(installed.requested_version, None);
    assert_eq!(
        installed.launcher,
        scratch
            .path()
            .join("install-root")
            .join("bin")
            .join("ospm-deno-hello")
    );
    assert!(installed.launcher.is_file());
    assert!(deno.is_global_installed(&installed.name).unwrap());

    let output = run_launcher(&scratch, &installed.launcher);
    assert!(
        output.status.success(),
        "launcher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ospm-deno-script-ok"),
        "launcher stdout was {:?}",
        String::from_utf8_lossy(&output.stdout)
    );

    deno.uninstall_global(&installed.name).unwrap();
    assert!(!deno.is_global_installed(&installed.name).unwrap());
    assert!(!installed.launcher.exists());

    match deno.uninstall_global(&installed.name).unwrap_err() {
        neotales_ospm_deno_cli::DenoError::Exit { status, stderr, .. } => {
            assert_eq!(status.code(), Some(1));
            assert!(
                String::from_utf8_lossy(&stderr).contains("No installation found"),
                "stderr was {:?}",
                String::from_utf8_lossy(&stderr)
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
#[ignore = "requires deno; set NEOTALES_OSPM_TEST_DENO to the real deno executable and run with --ignored"]
fn a_second_install_is_refused_and_force_replaces_it() {
    let scratch = Scratch::new("replace");
    let first = scratch.write("first.ts", "console.log('ospm-deno-first');\n");
    let second = scratch.write("second.ts", "console.log('ospm-deno-second');\n");
    let deno = isolated_deno(&scratch);
    let name = ExecutableName::new("ospm-deno-replace").unwrap();
    let first_tool =
        GlobalTool::script(name.as_str(), ScriptSource::path(&first).unwrap()).unwrap();
    let second_tool =
        GlobalTool::script(name.as_str(), ScriptSource::path(&second).unwrap()).unwrap();

    let installed = deno.install_global(&first_tool).unwrap();

    match deno.install_global(&first_tool).unwrap_err() {
        neotales_ospm_deno_cli::DenoError::AlreadyInstalled {
            name: reported,
            launcher,
        } => {
            assert_eq!(reported, name);
            assert_eq!(launcher, installed.launcher);
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let replaced = deno.replace_global(&second_tool).unwrap();
    assert_eq!(replaced.launcher, installed.launcher);
    let output = run_launcher(&scratch, &replaced.launcher);
    assert!(
        output.status.success(),
        "launcher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ospm-deno-second"),
        "launcher stdout was {:?}",
        String::from_utf8_lossy(&output.stdout)
    );

    deno.uninstall_global(&name).unwrap();
}

#[test]
#[ignore = "requires deno and network; set NEOTALES_OSPM_TEST_DENO and NEOTALES_OSPM_TEST_DENO_NETWORK=1"]
fn project_add_pins_an_exact_version() {
    let Ok(network) = std::env::var("NEOTALES_OSPM_TEST_DENO_NETWORK") else {
        panic!(
            "set NEOTALES_OSPM_TEST_DENO_NETWORK=1 as well; this test contacts the JSR registry"
        );
    };
    assert_eq!(network, "1", "NEOTALES_OSPM_TEST_DENO_NETWORK must be 1");

    let scratch = Scratch::new("project");
    let project = scratch.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("deno.json"), "{}\n").unwrap();
    let deno = isolated_deno(&scratch).with_project_directory(&project);
    let package = RegistryPackage::jsr("@std/yaml", "1.0.0").unwrap();

    deno.add_project(&package, DependencySection::Regular)
        .unwrap();
    let manifest = fs::read_to_string(project.join("deno.json")).unwrap();
    assert!(
        manifest.contains("\"jsr:@std/yaml@1.0.0\""),
        "deno.json was {manifest}"
    );
    assert!(!manifest.contains("^1.0.0"), "deno.json was {manifest}");

    let name = PackageName::jsr("@std/yaml").unwrap();
    deno.remove_project(&name).unwrap();
    let manifest = fs::read_to_string(project.join("deno.json")).unwrap();
    assert!(!manifest.contains("@std/yaml"), "deno.json was {manifest}");
}
