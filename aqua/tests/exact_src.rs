//! Exact source selection when an archive contains both `tool` and `tool.exe`.
use std::fs;
use std::io::Cursor;

use neotales_ospm_aqua::Registry;
use neotales_ospm_archive::{Selector, install_reader};
use neotales_ospm_release::Target;
use sha2::{Digest, Sha256};
use tar::{Builder, Header};

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Build an uncompressed tar with the given regular files.
fn tar_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    for (name, data) in entries {
        let mut header = Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        builder.append_data(&mut header, name, *data).unwrap();
    }
    builder.into_inner().unwrap()
}

/// Plan and install one member from an archive containing both names.
fn install_member(src: &str) -> (String, Vec<u8>) {
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("out");
    fs::create_dir(&out).unwrap();

    let yaml = format!(
        r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app.tar"
    format: tar
    files:
      - name: app
        src: "{src}"
"#
    );
    let registry = Registry::parse(&yaml).unwrap();
    let mut plan = registry
        .package("org/app")
        .unwrap()
        .plan(&Target::current(), "v1", &out, SHA)
        .unwrap();
    assert!(!plan.install.normalize_exe_suffix);

    let expected_selector = if src.ends_with(".exe") || cfg!(windows) {
        r"^bin/tool\.exe$"
    } else {
        r"^bin/tool$"
    };
    match &plan.install.grep {
        Selector::Regex(pattern) => assert_eq!(pattern, expected_selector),
        other => panic!("unexpected selector: {other:?}"),
    }

    let archive = tar_fixture(&[("bin/tool.exe", b"WINDOWS"), ("bin/tool", b"UNIX")]);
    plan.install.sha256 = format!("{:x}", Sha256::digest(&archive));
    let installed = install_reader(Cursor::new(archive), &plan.install).unwrap();
    assert_eq!(installed.parent(), Some(out.as_path()));

    let name = installed
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(plan.install.rename.as_deref(), Some(name.as_str()));
    (name, fs::read(installed).unwrap())
}

#[test]
fn exact_dot_exe_source_selects_the_dot_exe_member() {
    let (name, bytes) = install_member("bin/tool.exe");
    let expected_name = if cfg!(windows) { "app.exe" } else { "app" };
    assert_eq!(name, expected_name);
    assert_eq!(bytes, b"WINDOWS");
}

#[test]
fn plain_source_selects_the_plain_member_on_non_windows() {
    let (name, bytes) = install_member("bin/tool");
    if cfg!(windows) {
        // Aqua completes a Windows executable extension before planning, so
        // the plain source resolves to `bin/tool.exe` on Windows.
        assert_eq!(name, "app.exe");
        assert_eq!(bytes, b"WINDOWS");
    } else {
        assert_eq!(name, "app");
        assert_eq!(bytes, b"UNIX");
    }
}
