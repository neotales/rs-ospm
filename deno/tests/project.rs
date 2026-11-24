//! Project dependency operations through a fake Deno process.
#![cfg(unix)]

mod support;

use std::fs;

use neotales_ospm_deno_cli::{Deno, DenoError, DependencySection, PackageName, RegistryPackage};
use support::{FakeDeno, with_deno_retry};
use tempfile::TempDir;

fn package() -> RegistryPackage {
    RegistryPackage::jsr("@example/tool", "1.2.3").unwrap()
}

#[test]
fn add_project_pins_exactly_and_runs_in_the_project_directory() {
    let temp = TempDir::new().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable())
        .with_install_root(temp.path().join("ignored-root"))
        .with_project_directory(&project);

    with_deno_retry(|| deno.add_project(&package(), DependencySection::Regular)).unwrap();

    assert_eq!(
        fake.args(),
        vec!["add", "--save-exact", "jsr:@example/tool@1.2.3",]
    );
    assert_eq!(fake.cwd(), project);
}

#[test]
fn add_project_passes_the_package_json_section_flag() {
    let temp = TempDir::new().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_project_directory(&project);

    with_deno_retry(|| deno.add_project(&package(), DependencySection::Development)).unwrap();
    assert_eq!(
        fake.args(),
        vec!["add", "--dev", "--save-exact", "jsr:@example/tool@1.2.3",]
    );

    with_deno_retry(|| deno.add_project(&package(), DependencySection::Optional)).unwrap();
    assert_eq!(
        fake.args(),
        vec![
            "add",
            "--save-optional",
            "--save-exact",
            "jsr:@example/tool@1.2.3",
        ]
    );
}

#[test]
fn remove_project_passes_the_package_name() {
    let temp = TempDir::new().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_project_directory(&project);
    let name = PackageName::jsr("@example/tool").unwrap();

    with_deno_retry(|| deno.remove_project(&name)).unwrap();

    assert_eq!(fake.args(), vec!["remove", "@example/tool"]);
    assert_eq!(fake.cwd(), project);
}

#[test]
fn install_project_runs_plain_install() {
    let temp = TempDir::new().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let fake = FakeDeno::new(0);
    let deno = Deno::with_executable(fake.executable()).with_project_directory(&project);

    with_deno_retry(|| deno.install_project()).unwrap();

    assert_eq!(fake.args(), vec!["install"]);
    assert_eq!(fake.cwd(), project);
}

#[test]
fn project_failure_reports_both_streams() {
    let temp = TempDir::new().unwrap();
    let fake = FakeDeno::new(5);
    fake.set_stdout("progress\n");
    fake.set_stderr("error: invalid manifest\n");
    let deno = Deno::with_executable(fake.executable()).with_project_directory(temp.path());

    match with_deno_retry(|| deno.install_project()).unwrap_err() {
        DenoError::Exit {
            status,
            stdout,
            stderr,
        } => {
            assert_eq!(status.code(), Some(5));
            assert_eq!(stdout, b"progress\n");
            assert_eq!(stderr, b"error: invalid manifest\n");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
