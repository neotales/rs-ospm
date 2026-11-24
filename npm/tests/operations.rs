//! Verify process arguments and error handling without touching real npm state.
#![cfg(unix)]

mod support;

use std::path::Path;

use neotales_ospm_core::{Install, List, PackageName, Remove};
use neotales_ospm_npm_cli::{InvalidPackageSpec, Npm, NpmError, NpmPackage};
use support::{FakeProcess, with_npm_retry};

fn package(name: &str) -> PackageName {
    PackageName::new(name).unwrap()
}

#[test]
fn default_executable_is_npm_from_path() {
    assert_eq!(Npm::new().executable(), Path::new("npm"));
}

#[test]
fn install_and_remove_use_global_prefix_and_separate_arguments() {
    let fake = FakeProcess::new("npm", "", "", 0);
    let prefix = fake.path().join("prefix");
    let prefix_text = prefix.to_str().unwrap();
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());
    let package = NpmPackage::versioned("@scope/tool", "1.2.3").unwrap();

    with_npm_retry(|| npm.install_package(&package)).unwrap();
    assert_eq!(
        fake.args(),
        vec![
            "install",
            "--global",
            "--prefix",
            prefix_text,
            "--no-audit",
            "--no-fund",
            "--progress=false",
            "--ignore-scripts",
            "@scope/tool@1.2.3",
        ]
    );

    with_npm_retry(|| npm.remove_package(&package)).unwrap();
    assert_eq!(
        fake.args(),
        vec![
            "uninstall",
            "--global",
            "--prefix",
            prefix_text,
            "--no-audit",
            "--no-fund",
            "--progress=false",
            "@scope/tool",
        ]
    );
}

#[test]
fn install_allows_scripts_only_when_enabled() {
    let fake = FakeProcess::new("npm", "", "", 0);
    let npm = Npm::with_executable(fake.executable()).allow_scripts(true);
    let package = NpmPackage::versioned("eslint", "9.0.0").unwrap();

    assert!(npm.scripts_allowed());
    with_npm_retry(|| npm.install_package(&package)).unwrap();
    assert_eq!(
        fake.args(),
        vec![
            "install",
            "--global",
            "--no-audit",
            "--no-fund",
            "--progress=false",
            "eslint@9.0.0",
        ]
    );
}

#[test]
fn accessors_preserve_the_configuration() {
    let fake = FakeProcess::new("npm", "", "", 0);
    let prefix = fake.path().join("prefix");
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    assert_eq!(npm.executable(), fake.executable());
    assert_eq!(npm.prefix(), Some(prefix.as_path()));
    assert!(!npm.scripts_allowed());
}

#[test]
fn failed_command_captures_status_and_stderr() {
    let fake = FakeProcess::new("npm", "", "npm error: network unavailable\n", 7);
    let npm = Npm::with_executable(fake.executable());
    let package = NpmPackage::versioned("eslint", "9.0.0").unwrap();

    let error = with_npm_retry(|| npm.install_package(&package)).unwrap_err();
    let message = error.to_string();
    match error {
        NpmError::CommandFailed { status, stderr } => {
            assert_eq!(status.code(), Some(7));
            assert_eq!(stderr, b"npm error: network unavailable\n");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(message.contains("npm exited with"));
    assert!(message.contains("network unavailable"));
}

#[test]
fn command_does_not_apply_the_prefix() {
    let fake = FakeProcess::new("npm", "", "", 0);
    let prefix = fake.path().join("prefix");
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    let command = npm.command();
    assert_eq!(command.get_program(), fake.executable().as_os_str());
    assert_eq!(command.get_args().count(), 0);
}

#[test]
fn portable_traits_use_the_portable_name() {
    let fake = FakeProcess::new("npm", "", "", 0);
    let npm = Npm::with_executable(fake.executable());

    with_npm_retry(|| npm.install(&package("eslint"))).unwrap();
    assert_eq!(fake.args().last().map(String::as_str), Some("eslint"));

    with_npm_retry(|| npm.remove(&package("eslint"))).unwrap();
    assert_eq!(fake.args().last().map(String::as_str), Some("eslint"));
}

#[test]
fn portable_install_rejects_a_name_npm_cannot_use() {
    let npm = Npm::with_executable("/nonexistent/npm-for-name-validation");

    let error = npm.install(&package("foo:bar")).unwrap_err();
    assert!(
        matches!(error, NpmError::InvalidPackage(InvalidPackageSpec::Name)),
        "{error:?}"
    );
}

#[test]
fn huge_stdout_is_bounded_and_the_child_is_reaped() {
    let fake = FakeProcess::with_body(
        "npm",
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" > \"$0.args\"\n\
         exec head -c 17000000 /dev/zero\n",
    );
    let npm = Npm::with_executable(fake.executable());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(
            error,
            NpmError::OutputTooLarge {
                stream: "stdout",
                limit: 16_777_216
            }
        ),
        "{error:?}"
    );
}

#[test]
fn huge_stderr_is_bounded_and_the_child_is_reaped() {
    let fake = FakeProcess::with_body(
        "npm",
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" > \"$0.args\"\n\
         exec head -c 17000000 /dev/zero >&2\n",
    );
    let npm = Npm::with_executable(fake.executable());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(
            error,
            NpmError::OutputTooLarge {
                stream: "stderr",
                limit: 16_777_216
            }
        ),
        "{error:?}"
    );
}

#[test]
fn overflow_does_not_wait_for_a_descendant_holding_the_pipes() {
    // The background sleep inherits stdout and stderr, so the reader threads
    // would block until it exits if only the direct child were killed.
    let fake = FakeProcess::with_body(
        "npm",
        "#!/bin/sh\n\
         printf '%s\\n' \"$@\" > \"$0.args\"\n\
         sleep 6 &\n\
         exec head -c 17000000 /dev/zero\n",
    );
    let npm = Npm::with_executable(fake.executable());

    let started = std::time::Instant::now();
    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    let elapsed = started.elapsed();

    assert!(
        matches!(
            error,
            NpmError::OutputTooLarge {
                stream: "stdout",
                limit: 16_777_216
            }
        ),
        "{error:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "overflow waited for the descendant: {elapsed:?}"
    );
}

#[cfg(windows)]
#[test]
fn windows_rejects_a_metacharacter_prefix_before_starting_npm() {
    let npm = Npm::with_executable("C:\\nonexistent\\npm.cmd").with_prefix("C:\\tools\\a&b");
    let package = NpmPackage::versioned("eslint", "1.2.3").unwrap();

    let error = npm.install_package(&package).unwrap_err();
    assert!(
        matches!(error, NpmError::UnsafeWindowsArgument("prefix")),
        "{error:?}"
    );
}
