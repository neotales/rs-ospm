//! Global executable installation through a fake Deno process.
#![cfg(unix)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use neotales_ospm_deno_cli::{
    Deno, DenoError, ExecutableName, GlobalTool, RegistryPackage, ScriptSource,
};
use support::{FakeDeno, with_deno_retry};
use tempfile::TempDir;

fn root(temp: &TempDir) -> PathBuf {
    temp.path().join("install-root")
}

fn exact_package() -> RegistryPackage {
    RegistryPackage::jsr("@example/tool", "1.2.3").unwrap()
}

#[test]
fn default_executable_is_deno_from_path() {
    assert_eq!(Deno::new().executable(), Path::new("deno"));
}

#[test]
fn install_global_passes_an_exact_specifier_and_explicit_name() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_install_root(&root);
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    let installed = with_deno_retry(|| deno.install_global(&tool)).unwrap();

    assert_eq!(
        fake.args(),
        vec![
            "install",
            "--global",
            "--root",
            root.to_str().unwrap(),
            "--name",
            "example-tool",
            "jsr:@example/tool@1.2.3",
        ]
    );
    assert_eq!(installed.name.as_str(), "example-tool");
    assert_eq!(installed.requested_version.as_deref(), Some("1.2.3"));
    assert_eq!(installed.launcher, root.join("bin").join("example-tool"));
    assert!(deno.is_global_installed(&installed.name).unwrap());
}

#[test]
fn install_global_installs_a_script_without_a_version() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_install_root(&root);
    let script = temp.path().join("hello.ts");
    fs::write(&script, "console.log('hello')\n").unwrap();
    let tool = GlobalTool::script("hello", ScriptSource::path(&script).unwrap()).unwrap();

    let installed = with_deno_retry(|| deno.install_global(&tool)).unwrap();

    assert_eq!(installed.requested_version, None);
    assert_eq!(fake.args().last().unwrap(), script.to_str().unwrap());
}

#[test]
fn install_global_refuses_an_existing_launcher_without_spawning() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin").join("example-tool"), "existing").unwrap();
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_install_root(&root);
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    let error = deno.install_global(&tool).unwrap_err();

    match error {
        DenoError::AlreadyInstalled { name, launcher } => {
            assert_eq!(name.as_str(), "example-tool");
            assert_eq!(launcher, root.join("bin").join("example-tool"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(!fake.was_run(), "the preflight must not start Deno");
}

#[test]
fn replace_global_passes_force() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin").join("example-tool"), "existing").unwrap();
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_install_root(&root);
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    with_deno_retry(|| deno.replace_global(&tool)).unwrap();

    assert_eq!(
        fake.args(),
        vec![
            "install",
            "--global",
            "--root",
            root.to_str().unwrap(),
            "--name",
            "example-tool",
            "--force",
            "jsr:@example/tool@1.2.3",
        ]
    );
}

#[test]
fn allow_all_is_opt_in() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    let fake = FakeDeno::new(0);
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    let default = Deno::with_executable(fake.executable())
        .with_install_root(&root)
        .install_command(&tool)
        .unwrap();
    assert!(!default.get_args().any(|argument| argument == "--allow-all"));

    let allowed = Deno::with_executable(fake.executable())
        .with_install_root(&root)
        .allow_all(true)
        .install_command(&tool)
        .unwrap();
    assert!(allowed.get_args().any(|argument| argument == "--allow-all"));
}

#[test]
fn install_failure_reports_both_streams_and_status() {
    let temp = TempDir::new().unwrap();
    let fake = FakeDeno::new(3);
    fake.set_stdout("partial progress\n");
    fake.set_stderr("error: no such package\n");
    let deno = Deno::with_executable(fake.executable()).with_install_root(root(&temp));
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    match with_deno_retry(|| deno.install_global(&tool)).unwrap_err() {
        DenoError::Exit {
            status,
            stdout,
            stderr,
        } => {
            assert_eq!(status.code(), Some(3));
            assert_eq!(stdout, b"partial progress\n");
            assert_eq!(stderr, b"error: no such package\n");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn success_without_a_launcher_is_rejected() {
    let temp = TempDir::new().unwrap();
    let fake = FakeDeno::with_body("#!/bin/sh\nexit 0\n");
    let deno = Deno::with_executable(fake.executable()).with_install_root(root(&temp));
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    match with_deno_retry(|| deno.install_global(&tool)).unwrap_err() {
        DenoError::MissingLauncher { name, launcher } => {
            assert_eq!(name.as_str(), "example-tool");
            assert_eq!(launcher, root(&temp).join("bin").join("example-tool"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn uninstall_global_passes_the_name() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_install_root(&root);
    let name = ExecutableName::new("example-tool").unwrap();

    with_deno_retry(|| deno.uninstall_global(&name)).unwrap();

    assert_eq!(
        fake.args(),
        vec![
            "uninstall",
            "--global",
            "--root",
            root.to_str().unwrap(),
            "example-tool",
        ]
    );
}

#[test]
fn is_global_installed_checks_the_launcher_entry() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    let deno = Deno::new().with_install_root(&root);
    let name = ExecutableName::new("example-tool").unwrap();

    assert!(!deno.is_global_installed(&name).unwrap());
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin").join("example-tool"), "").unwrap();
    assert!(deno.is_global_installed(&name).unwrap());
    fs::remove_file(root.join("bin").join("example-tool")).unwrap();
    assert!(!deno.is_global_installed(&name).unwrap());
}

#[test]
fn a_bin_path_that_is_not_a_directory_is_an_error() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("bin"), "not a directory").unwrap();
    let deno = Deno::new().with_install_root(&root);
    let name = ExecutableName::new("example-tool").unwrap();

    assert!(matches!(
        deno.is_global_installed(&name),
        Err(DenoError::InstallRootInvalid(_))
    ));
}

#[test]
fn bin_directory_appends_bin_unless_the_root_ends_with_it() {
    assert_eq!(
        Deno::new()
            .with_install_root("prefix")
            .bin_directory()
            .unwrap(),
        Path::new("prefix/bin")
    );
    assert_eq!(
        Deno::new()
            .with_install_root("prefix/bin")
            .bin_directory()
            .unwrap(),
        Path::new("prefix/bin")
    );
}

#[test]
fn an_empty_install_root_is_rejected_before_spawning() {
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_install_root("");
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    assert!(matches!(
        deno.install_global(&tool),
        Err(DenoError::InstallRootInvalid(_))
    ));
    assert!(!fake.was_run());
}

#[test]
fn a_relative_install_root_resolves_against_the_working_directory() {
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable())
        .with_install_root("root")
        .with_working_directory(&work);
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    assert_eq!(deno.bin_directory().unwrap(), work.join("root").join("bin"));
    let installed = with_deno_retry(|| deno.install_global(&tool)).unwrap();

    assert_eq!(fake.cwd(), work);
    assert_eq!(
        installed.launcher,
        work.join("root").join("bin").join("example-tool")
    );
    assert!(installed.launcher.is_file());
}

#[test]
fn global_operations_use_the_working_directory_not_the_project_directory() {
    let temp = TempDir::new().unwrap();
    let root = root(&temp);
    let work = temp.path().join("work");
    let project = temp.path().join("project");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&project).unwrap();
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable())
        .with_install_root(&root)
        .with_working_directory(&work)
        .with_project_directory(&project);
    let tool = GlobalTool::package("example-tool", exact_package()).unwrap();

    with_deno_retry(|| deno.install_global(&tool)).unwrap();

    assert_eq!(fake.cwd(), work);
}

#[test]
fn command_has_no_arguments() {
    let deno = Deno::with_executable("/opt/deno");
    let command = deno.command();
    assert_eq!(command.get_program(), Path::new("/opt/deno"));
    assert_eq!(command.get_args().count(), 0);
}
