//! Real Bun semantics, run only when a real Bun executable is supplied.
//!
//! Every test creates a unique temporary directory and a wrapper executable.
//! The wrapper clears the inherited environment with `env -i` and points the
//! Bun home, global root, cache and temporary directories into that directory.
//! Each test checks the effective cache and global bin paths before it runs a
//! mutation. Only local `file:` dependencies are used, so no registry, user
//! cache or user configuration is read or written.
//!
//! Run them explicitly with the real Bun executable:
//!
//! ```text
//! NEOTALES_OSPM_TEST_BUN=/path/to/bun \
//!   cargo test --test real_bun -- --ignored --test-threads=1
//! ```
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use neotales_ospm_bun_cli::{Bun, PackageName};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "ospm-bun-real-{name}-{}-{}",
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

    fn write(&self, relative: &str, body: &str) {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
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

/// The real Bun executable named by `NEOTALES_OSPM_TEST_BUN`.
///
/// A version-manager launcher shim is not suitable here: clearing the
/// environment removes the configuration the shim needs to find the real
/// executable. Pass the executable itself.
fn real_bun() -> PathBuf {
    let value = std::env::var("NEOTALES_OSPM_TEST_BUN").unwrap_or_else(|_| {
        panic!("set NEOTALES_OSPM_TEST_BUN to the real Bun executable before running this test")
    });
    assert!(
        !value.is_empty() && value != "1",
        "NEOTALES_OSPM_TEST_BUN must name the real Bun executable, not a toggle"
    );
    // Resolve relative to the current directory without following the final
    // symlink: a launcher shim dispatches on its own file name.
    let path = std::path::absolute(&value)
        .unwrap_or_else(|error| panic!("cannot resolve NEOTALES_OSPM_TEST_BUN={value:?}: {error}"));
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

/// A Bun executable whose entire environment and configuration live in
/// `scratch`.
///
/// `env -i` removes the inherited environment, so no user Bun, registry,
/// proxy or credential variable reaches Bun. The explicit paths below are the
/// only configuration Bun sees.
fn isolated_bun(scratch: &Scratch) -> Bun {
    let real = real_bun();
    for directory in ["home", "tmp", "inst", "cache", "config", "data", "state"] {
        fs::create_dir_all(scratch.path().join(directory)).unwrap();
    }

    let assignments = [
        format!("PATH={}", shell_quote("/usr/bin:/bin")),
        assignment("HOME", &scratch.path().join("home")),
        assignment("TMPDIR", &scratch.path().join("tmp")),
        assignment("BUN_INSTALL", &scratch.path().join("inst")),
        assignment("BUN_INSTALL_CACHE_DIR", &scratch.path().join("cache")),
        assignment("XDG_CONFIG_HOME", &scratch.path().join("config")),
        assignment("XDG_DATA_HOME", &scratch.path().join("data")),
        assignment("XDG_STATE_HOME", &scratch.path().join("state")),
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
    Bun::with_executable(scratch.write_executable("bun-wrapper", &script))
}

fn bun_output(bun: &Bun, args: &[&str]) -> String {
    let mut command = bun.command();
    command.args(args);
    let output = command.stdin(Stdio::null()).output().unwrap();
    assert!(
        output.status.success(),
        "bun {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn assert_isolated(bun: &Bun, scratch: &Scratch) {
    for args in [&["pm", "cache"][..], &["pm", "bin", "-g"][..]] {
        let value = bun_output(bun, args);
        assert!(
            Path::new(value.trim()).starts_with(scratch.path()),
            "bun {args:?} resolved to {value:?}, outside {}",
            scratch.path().display()
        );
    }
}

#[test]
#[ignore = "requires Bun; set NEOTALES_OSPM_TEST_BUN to the real Bun executable and run with --ignored"]
fn help_reports_the_commands_this_backend_uses() {
    let scratch = Scratch::new("help");
    let bun = isolated_bun(&scratch);

    let help = bun_output(&bun, &["--help"]);
    for command in ["install", "add", "remove", "pm"] {
        assert!(help.contains(command), "--help does not mention {command}");
    }

    let add = bun_output(&bun, &["add", "--help"]);
    for flag in [
        "--global",
        "--exact",
        "--save-prod",
        "--dev",
        "--optional",
        "--peer",
    ] {
        assert!(add.contains(flag), "add --help does not mention {flag}");
    }

    let pm = bun_output(&bun, &["pm", "--help"]);
    for command in ["pkg", "ls", "bin"] {
        assert!(pm.contains(command), "pm --help does not mention {command}");
    }
}

#[test]
#[ignore = "requires Bun; set NEOTALES_OSPM_TEST_BUN to the real Bun executable and run with --ignored"]
fn install_project_installs_a_local_dependency_in_isolation() {
    let scratch = Scratch::new("install-project");
    scratch.write(
        "dep/package.json",
        r#"{ "name": "dep", "version": "1.0.0" }"#,
    );
    scratch.write(
        "app/package.json",
        r#"{
  "name": "app",
  "version": "1.0.0",
  "dependencies": { "dep": "file:../dep" }
}
"#,
    );
    let app = scratch.path().join("app");
    let bun = isolated_bun(&scratch).with_project_directory(&app);
    assert_isolated(&bun, &scratch);

    bun.install_project().unwrap();

    assert!(
        app.join("node_modules")
            .join("dep")
            .join("package.json")
            .is_file(),
        "the local dependency was not installed in the project"
    );
    assert!(
        app.join("bun.lock").is_file(),
        "the project lockfile was not written"
    );
    assert!(
        scratch.path().join("cache").is_dir(),
        "the isolated cache directory is missing"
    );
}

#[test]
#[ignore = "requires Bun; set NEOTALES_OSPM_TEST_BUN to the real Bun executable and run with --ignored"]
fn list_project_reads_declared_sections() {
    let scratch = Scratch::new("list-project");
    scratch.write(
        "app/package.json",
        r#"{
  "name": "app",
  "version": "1.0.0",
  "dependencies": { "regular": "1.0.0" },
  "devDependencies": { "dev": "^2.0.0" },
  "optionalDependencies": { "optional": "file:../optional" },
  "peerDependencies": { "peer": "workspace:*" }
}
"#,
    );
    let bun = isolated_bun(&scratch).with_project_directory(scratch.path().join("app"));

    let declared = bun.list_project().unwrap();

    assert_eq!(declared.dependencies.len(), 1);
    assert_eq!(declared.dependencies[0].name, "regular");
    assert_eq!(declared.dependencies[0].spec, "1.0.0");
    assert_eq!(declared.dev_dependencies[0].name, "dev");
    assert_eq!(declared.dev_dependencies[0].spec, "^2.0.0");
    assert_eq!(declared.optional_dependencies[0].name, "optional");
    assert_eq!(declared.optional_dependencies[0].spec, "file:../optional");
    assert_eq!(declared.peer_dependencies[0].name, "peer");
    assert_eq!(declared.peer_dependencies[0].spec, "workspace:*");
}

#[test]
#[ignore = "requires Bun; set NEOTALES_OSPM_TEST_BUN to the real Bun executable and run with --ignored"]
fn list_global_reads_the_isolated_global_manifest() {
    let scratch = Scratch::new("list-global");
    let bun = isolated_bun(&scratch);
    let global = scratch.path().join("inst").join("install").join("global");
    fs::create_dir_all(&global).unwrap();
    fs::write(
        global.join("package.json"),
        r#"{ "dependencies": { "global-tool": "file:../../dep" } }"#,
    )
    .unwrap();

    let declared = bun.list_global().unwrap();

    assert_eq!(declared.dependencies.len(), 1);
    assert_eq!(declared.dependencies[0].name, "global-tool");
    assert_eq!(declared.dependencies[0].spec, "file:../../dep");
}

#[test]
#[ignore = "requires Bun; set NEOTALES_OSPM_TEST_BUN to the real Bun executable and run with --ignored"]
fn remove_global_removes_a_local_global_package() {
    let scratch = Scratch::new("remove-global");
    scratch.write(
        "dep/package.json",
        r#"{ "name": "ospm-probe-dep", "version": "1.0.0" }"#,
    );
    let bun = isolated_bun(&scratch);
    assert_isolated(&bun, &scratch);

    let mut command = bun.command();
    command
        .args(["add", "--global"])
        .arg(scratch.path().join("dep"));
    let output = command.stdin(Stdio::null()).output().unwrap();
    assert!(
        output.status.success(),
        "bun add --global failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let declared = bun.list_global().unwrap();
    assert!(
        declared
            .dependencies
            .iter()
            .any(|package| package.name == "ospm-probe-dep"),
        "the local global package was not recorded"
    );

    bun.remove_global(&PackageName::new("ospm-probe-dep").unwrap())
        .unwrap();

    let declared = bun.list_global().unwrap();
    assert!(
        !declared
            .dependencies
            .iter()
            .any(|package| package.name == "ospm-probe-dep"),
        "the local global package was not removed"
    );
}
