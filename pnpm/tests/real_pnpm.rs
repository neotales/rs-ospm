//! Real pnpm workspace and manifest semantics, run only when a real pnpm
//! executable is supplied.
//!
//! Every test creates a unique temporary workspace and a wrapper executable.
//! The wrapper clears the inherited environment with `env -i` and points every
//! pnpm home, store, cache, state and config path into that workspace. Each
//! test then asserts the effective paths with `pnpm config get` and
//! `pnpm store path` before it installs. Only local `file:` dependencies are
//! used, with offline resolution. No registry, user pnpm store, user cache or
//! user config is read or written.
//!
//! Run them explicitly with the real pnpm executable, not a launcher shim:
//!
//! ```text
//! NEOTALES_OSPM_TEST_PNPM=/path/to/pnpm \
//!   cargo test --test real_pnpm -- --ignored --test-threads=1
//! ```
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use neotales_ospm_pnpm_cli::Pnpm;
use serde_json::Value;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "ospm-pnpm-real-{name}-{}-{}",
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

/// The real pnpm executable named by `NEOTALES_OSPM_TEST_PNPM`.
///
/// A launcher shim is rejected on purpose: clearing the environment removes
/// the configuration a shim needs to find the real tool.
fn real_pnpm() -> PathBuf {
    let value = std::env::var("NEOTALES_OSPM_TEST_PNPM").unwrap_or_else(|_| {
        panic!("set NEOTALES_OSPM_TEST_PNPM to the real pnpm executable before running this test")
    });
    assert!(
        !value.is_empty() && value != "1",
        "NEOTALES_OSPM_TEST_PNPM must name the real pnpm executable, not a toggle"
    );
    let path = fs::canonicalize(&value).unwrap_or_else(|error| {
        panic!("cannot resolve NEOTALES_OSPM_TEST_PNPM={value:?}: {error}")
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

/// A pnpm executable whose entire environment and config live in `scratch`.
///
/// `env -i` removes the inherited environment, so no user `PNPM_*`,
/// `npm_config_*`, proxy, credential or config variable reaches pnpm. The
/// explicit paths below are the only config pnpm sees.
fn isolated_pnpm(scratch: &Scratch) -> Pnpm {
    let real = real_pnpm();
    for directory in [
        "home",
        "tmp",
        "pnpm-home",
        "config",
        "cache",
        "data",
        "state",
        "store",
    ] {
        fs::create_dir_all(scratch.path().join(directory)).unwrap();
    }

    let assignments = [
        format!("PATH={}", shell_quote("/usr/bin:/bin")),
        assignment("HOME", &scratch.path().join("home")),
        assignment("TMPDIR", &scratch.path().join("tmp")),
        assignment("PNPM_HOME", &scratch.path().join("pnpm-home")),
        assignment("XDG_CONFIG_HOME", &scratch.path().join("config")),
        assignment("XDG_CACHE_HOME", &scratch.path().join("cache")),
        assignment("XDG_DATA_HOME", &scratch.path().join("data")),
        assignment("XDG_STATE_HOME", &scratch.path().join("state")),
        assignment("PNPM_CONFIG_STORE_DIR", &scratch.path().join("store")),
        assignment("PNPM_CONFIG_CACHE_DIR", &scratch.path().join("cache")),
        assignment("PNPM_CONFIG_STATE_DIR", &scratch.path().join("state")),
        assignment("PNPM_CONFIG_USERCONFIG", &scratch.path().join("user-npmrc")),
        assignment(
            "PNPM_CONFIG_GLOBALCONFIG",
            &scratch.path().join("global-npmrc"),
        ),
        "PNPM_CONFIG_OFFLINE='true'".to_owned(),
        "PNPM_CONFIG_IGNORE_SCRIPTS='false'".to_owned(),
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
    Pnpm::with_executable(scratch.write_executable("pnpm-wrapper", &script))
}

fn config_get(pnpm: &Pnpm, key: &str) -> String {
    let mut command = pnpm.project_command();
    command.args(["config", "get", key]);
    let output = command.stdin(Stdio::null()).output().unwrap();
    assert!(
        output.status.success(),
        "pnpm config get {key} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn store_path(pnpm: &Pnpm) -> String {
    let mut command = pnpm.project_command();
    command.args(["store", "path"]);
    let output = command.stdin(Stdio::null()).output().unwrap();
    assert!(
        output.status.success(),
        "pnpm store path failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn assert_isolated(pnpm: &Pnpm, scratch: &Scratch) {
    for key in ["store-dir", "cache-dir", "state-dir"] {
        let value = config_get(pnpm, key);
        assert!(
            Path::new(&value).starts_with(scratch.path()),
            "{key} resolved to {value:?}, outside {}",
            scratch.path().display()
        );
    }
    assert_eq!(config_get(pnpm, "offline"), "true");
    assert_eq!(config_get(pnpm, "ignore-scripts"), "false");
    let global = config_get(pnpm, "globalconfig");
    assert!(
        Path::new(&global).starts_with(scratch.path()),
        "globalconfig resolved to {global:?}, outside {}",
        scratch.path().display()
    );
    let store = store_path(pnpm);
    assert!(
        Path::new(&store).starts_with(scratch.path()),
        "store path resolved to {store:?}, outside {}",
        scratch.path().display()
    );
}

fn assert_store_used(scratch: &Scratch) {
    let store = scratch.path().join("store");
    assert!(
        fs::read_dir(&store).unwrap().next().is_some(),
        "pnpm did not write to the scratch store"
    );
}

/// A root project, two member projects, and a local dependency. Each project
/// writes a marker from its `postinstall` script so the test can see exactly
/// which lifecycle scripts pnpm ran.
fn write_workspace(scratch: &Scratch) {
    scratch.write("pnpm-workspace.yaml", "packages:\n  - packages/*\n");
    scratch.write(
        "package.json",
        r#"{
  "name": "root",
  "private": true,
  "scripts": { "postinstall": "printf root > root-ran" }
}
"#,
    );
    scratch.write(
        "packages/a/package.json",
        r#"{
  "name": "a",
  "version": "1.0.0",
  "scripts": { "postinstall": "printf a > a-ran" },
  "devDependencies": { "dep": "file:../dep" }
}
"#,
    );
    scratch.write(
        "packages/b/package.json",
        r#"{
  "name": "b",
  "version": "1.0.0",
  "scripts": { "postinstall": "printf b > b-ran" }
}
"#,
    );
    scratch.write(
        "packages/dep/package.json",
        r#"{ "name": "dep", "version": "1.0.0" }
"#,
    );
}

fn manifest(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn run_add(pnpm: &Pnpm, store: &Path, save_prod: bool, spec: &str) -> Value {
    let mut command = pnpm.project_command();
    command
        .arg("add")
        .arg("--offline")
        .arg("--store-dir")
        .arg(store);
    if save_prod {
        command.arg("--save-prod");
    }
    command.arg(spec);
    let output = command.stdin(Stdio::null()).output().unwrap();
    assert!(
        output.status.success(),
        "pnpm add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    manifest(&pnpm.project_directory().unwrap().join("package.json"))
}

#[test]
#[ignore = "requires pnpm; set NEOTALES_OSPM_TEST_PNPM to the real pnpm executable and run with --ignored"]
fn install_project_does_not_touch_the_workspace() {
    let scratch = Scratch::new("install-project");
    write_workspace(&scratch);
    let project = scratch.path().join("packages/a");
    let pnpm = isolated_pnpm(&scratch).with_project_directory(&project);
    assert_isolated(&pnpm, &scratch);

    pnpm.install_project().unwrap();
    assert_store_used(&scratch);

    assert!(
        project.join("a-ran").exists(),
        "the selected project must run"
    );
    assert!(
        !scratch.path().join("root-ran").exists(),
        "the workspace root must not run"
    );
    assert!(
        !scratch.path().join("packages/b/b-ran").exists(),
        "another workspace project must not run"
    );
    assert!(
        project.join("pnpm-lock.yaml").exists(),
        "the isolated install writes a project lockfile"
    );
    assert!(
        !scratch.path().join("pnpm-lock.yaml").exists(),
        "the workspace lockfile must be untouched"
    );
    assert!(
        !scratch.path().join("node_modules").exists(),
        "the workspace root must not be installed"
    );
}

#[test]
#[ignore = "requires pnpm; set NEOTALES_OSPM_TEST_PNPM to the real pnpm executable and run with --ignored"]
fn install_workspace_runs_every_workspace_project() {
    let scratch = Scratch::new("install-workspace");
    write_workspace(&scratch);
    let project = scratch.path().join("packages/a");
    let pnpm = isolated_pnpm(&scratch).with_project_directory(&project);
    assert_isolated(&pnpm, &scratch);

    pnpm.install_workspace().unwrap();
    assert_store_used(&scratch);

    assert!(scratch.path().join("root-ran").exists(), "root must run");
    assert!(
        project.join("a-ran").exists(),
        "the selected project must run"
    );
    assert!(
        scratch.path().join("packages/b/b-ran").exists(),
        "every workspace project must run"
    );
    assert!(
        scratch.path().join("pnpm-lock.yaml").exists(),
        "the workspace lockfile is written"
    );
    assert!(
        !project.join("pnpm-lock.yaml").exists(),
        "a workspace install must not write a nested lockfile"
    );
}

#[test]
#[ignore = "requires pnpm; set NEOTALES_OSPM_TEST_PNPM to the real pnpm executable and run with --ignored"]
fn save_prod_moves_an_existing_dev_dependency() {
    let scratch = Scratch::new("save-prod");
    scratch.write(
        "dep/package.json",
        r#"{ "name": "dep", "version": "1.0.0" }"#,
    );
    scratch.write(
        "with-flag/package.json",
        r#"{
  "name": "with-flag",
  "version": "1.0.0",
  "devDependencies": { "dep": "file:../dep" }
}
"#,
    );
    scratch.write(
        "without-flag/package.json",
        r#"{
  "name": "without-flag",
  "version": "1.0.0",
  "devDependencies": { "dep": "file:../dep" }
}
"#,
    );

    let store = scratch.path().join("store");

    let base = isolated_pnpm(&scratch);
    let with_flag = base
        .clone()
        .with_project_directory(scratch.path().join("with-flag"));
    assert_isolated(&with_flag, &scratch);
    let manifest = run_add(&with_flag, &store, true, "file:../dep");
    assert_eq!(manifest["dependencies"]["dep"], "file:../dep");
    assert!(
        manifest
            .get("devDependencies")
            .and_then(|section| section.get("dep"))
            .is_none(),
        "--save-prod must remove the development dependency entry"
    );

    let without_flag = base.with_project_directory(scratch.path().join("without-flag"));
    let manifest = run_add(&without_flag, &store, false, "file:../dep");
    assert_eq!(manifest["devDependencies"]["dep"], "file:../dep");
    assert!(
        manifest
            .get("dependencies")
            .and_then(|section| section.get("dep"))
            .is_none(),
        "without --save-prod pnpm leaves the development dependency in place"
    );
    assert_store_used(&scratch);
}
