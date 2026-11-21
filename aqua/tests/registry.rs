//! Registry subset and safety tests.
use neotales_ospm_aqua::{Error, Registry};
use neotales_ospm_release::Target;
use std::path::Path;

const REGISTRY: &str = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.Version}}-{{.OS}}-{{.Arch}}.{{.Format}}"
    format: tar.gz
    files:
      - name: app.exe
        src: "bin/app.exe"
    replacements:
      amd64: x86_64
    overrides:
      - goos: windows
        format: zip
"#;

#[test]
fn plans_explicit_target_and_override() {
    let registry = Registry::parse(REGISTRY).unwrap();
    let target = Target {
        os: "windows".into(),
        arch: "x86_64".into(),
    };
    let plan = registry
        .package("org/app")
        .unwrap()
        .plan(&target, "v1", Path::new("/tmp"), &"a".repeat(64))
        .unwrap();
    assert_eq!(plan.asset, "app-v1-windows-x86_64.zip");
    assert_eq!(plan.install.rename.as_deref(), Some("app.exe"));
}

#[test]
fn rejects_unsupported_security_fields() {
    let yaml = REGISTRY.replace(
        "    format: tar.gz",
        "    checksum:\n      type: github_release\n    format: tar.gz",
    );
    assert!(matches!(Registry::parse(&yaml), Err(Error::Yaml(_))));
}

#[test]
fn rejects_unknown_template_and_missing_package() {
    let yaml = REGISTRY.replace("{{.Version}}", "{{.Unknown}}");
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(registry.package("missing"), Err(Error::Package)));
    let target = Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    };
    assert!(matches!(
        registry.package("org/app").unwrap().plan(
            &target,
            "v1",
            Path::new("/tmp"),
            &"a".repeat(64)
        ),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn infers_format_from_static_asset_suffix() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app_{{.OS}}_{{.Arch}}.tar.gz"
    files:
      - name: app
"#;
    let target = Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    };
    let registry = Registry::parse(yaml).unwrap();
    let plan = registry
        .package("org/app")
        .unwrap()
        .plan(&target, "v1", Path::new("/tmp"), &"a".repeat(64))
        .unwrap();
    assert_eq!(plan.asset, "app_linux_amd64.tar.gz");
    assert!(
        matches!(&plan.install.grep, neotales_ospm_archive::Selector::Regex(pattern) if pattern == "^app$")
    );
}

#[test]
fn supported_envs_and_first_matching_override() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    supported_envs: [linux/amd64, windows]
    files:
      - name: app
    overrides:
      - envs: [linux/amd64]
        asset: first-linux-amd64.tar.gz
      - goos: linux
        asset: second-linux-amd64.tar.gz
"#;
    let registry = Registry::parse(yaml).unwrap();
    let recipe = registry.package("org/app").unwrap();
    let linux = Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    };
    let plan = recipe
        .plan(&linux, "v1", Path::new("/tmp"), &"a".repeat(64))
        .unwrap();
    assert_eq!(plan.asset, "first-linux-amd64.tar.gz");
    let mac = Target {
        os: "macos".into(),
        arch: "aarch64".into(),
    };
    assert!(matches!(
        recipe.plan(&mac, "v1", Path::new("/tmp"), &"a".repeat(64)),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn empty_supported_envs_rejects_every_target() {
    let yaml = REGISTRY.replace(
        "    format: tar.gz",
        "    supported_envs: []\n    format: tar.gz",
    );
    let registry = Registry::parse(&yaml).unwrap();
    let target = Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    };
    assert!(matches!(
        registry.package("org/app").unwrap().plan(
            &target,
            "v1",
            Path::new("/tmp"),
            &"a".repeat(64)
        ),
        Err(Error::Unsupported(_))
    ));
}
