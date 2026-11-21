//! Verify process arguments and error handling without touching real Cargo state.
#![cfg(unix)]

mod support;

use neotales_ospm_cargo_cli::{Cargo, CargoError};
use neotales_ospm_core::{Install, IsInstalled, List, PackageName, Remove};
use support::{FakeCargo, with_retry};

fn package(name: &str) -> PackageName {
    PackageName::new(name).unwrap()
}

#[test]
fn install_and_remove_use_separate_arguments() {
    let fake = FakeCargo::new("", "", 0);
    let cargo = Cargo::with_executable(fake.executable());

    with_retry(|| cargo.install(&package("curl"))).unwrap();
    assert_eq!(fake.args(), vec!["install", "--", "curl"]);

    with_retry(|| cargo.remove(&package("curl"))).unwrap();
    assert_eq!(fake.args(), vec!["uninstall", "--", "curl"]);
}

#[test]
fn install_root_is_passed_to_install_remove_and_list() {
    let fake = FakeCargo::new("", "", 0);
    let root = fake.dir.join("root");
    let root_text = root.to_str().unwrap();
    let cargo = Cargo::with_executable(fake.executable()).with_install_root(root.as_path());

    with_retry(|| cargo.install(&package("curl"))).unwrap();
    assert_eq!(
        fake.args(),
        vec!["install", "--root", root_text, "--", "curl"]
    );

    with_retry(|| cargo.remove(&package("curl"))).unwrap();
    assert_eq!(
        fake.args(),
        vec!["uninstall", "--root", root_text, "--", "curl"]
    );

    with_retry(|| cargo.list_all()).unwrap();
    assert_eq!(fake.args(), vec!["install", "--list", "--root", root_text]);
}

#[test]
fn failed_command_captures_status_and_stderr() {
    let fake = FakeCargo::new("", "error: crate `missing` not found\n", 101);
    let cargo = Cargo::with_executable(fake.executable());

    let error = with_retry(|| cargo.install(&package("missing"))).unwrap_err();
    let message = error.to_string();
    match error {
        CargoError::CommandFailed { status, stderr } => {
            assert_eq!(status.code(), Some(101));
            assert_eq!(stderr, b"error: crate `missing` not found\n");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(message.contains("cargo exited with"));
    assert!(message.contains("crate `missing` not found"));
}

#[test]
fn is_installed_matches_an_exact_crate_name() {
    let fake = FakeCargo::new(
        "ripgrep v14.1.0:\n    rg\ncargo-edit v0.12.2:\n    cargo-add\n    cargo-rm\n",
        "",
        0,
    );
    let cargo = Cargo::with_executable(fake.executable());

    assert!(with_retry(|| cargo.is_installed(&package("ripgrep"))).unwrap());
    assert!(!with_retry(|| cargo.is_installed(&package("rip"))).unwrap());
}

#[test]
fn command_does_not_apply_the_install_root() {
    let fake = FakeCargo::new("", "", 0);
    let root = fake.dir.join("root");
    let cargo = Cargo::with_executable(fake.executable()).with_install_root(root.as_path());

    assert_eq!(cargo.executable(), fake.executable());
    assert_eq!(cargo.install_root(), Some(root.as_path()));
    let command = cargo.command();
    assert_eq!(command.get_program(), fake.executable().as_os_str());
    assert_eq!(command.get_args().count(), 0);
}
