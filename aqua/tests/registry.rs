//! Registry subset, version selection, checksum handling, and safety tests.
use neotales_ospm_aqua::{Error, Registry};
use neotales_ospm_release::Target;
use std::path::Path;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn target(os: &str, arch: &str) -> Target {
    Target {
        os: os.into(),
        arch: arch.into(),
    }
}

fn plan_package(
    registry: &Registry,
    id: &str,
    target: &Target,
    version: &str,
) -> Result<neotales_ospm_aqua::Plan, Error> {
    registry
        .package(id)?
        .plan(target, version, Path::new("/tmp"), SHA)
}

const BASIC: &str = r#"
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
    let registry = Registry::parse(BASIC).unwrap();
    let plan = plan_package(&registry, "org/app", &target("windows", "x86_64"), "v1").unwrap();
    assert_eq!(plan.asset, "app-v1-windows-x86_64.zip");
    assert_eq!(plan.install.rename.as_deref(), Some("app.exe"));
    assert!(matches!(
        &plan.install.grep,
        neotales_ospm_archive::Selector::Regex(pattern) if pattern == "^bin/app\\.exe$"
    ));
}

#[test]
fn rejects_unsupported_security_features() {
    for field in [
        "cosign:\n    enabled: true",
        "slsa_provenance:\n    enabled: true",
        "minisign:\n    enabled: true",
        "packslip:\n    enabled: true",
        "github_artifact_attestations:\n    enabled: true",
        "github_immutable_release: true",
    ] {
        let yaml = BASIC.replace(
            "    format: tar.gz",
            &format!("    {field}\n    format: tar.gz"),
        );
        assert!(
            matches!(Registry::parse(&yaml), Err(Error::Yaml(_))),
            "{field} must be rejected"
        );
    }

    let yaml = BASIC.replace(
        "    format: tar.gz",
        "    checksum:\n      type: github_release\n      cosign:\n        enabled: true\n    format: tar.gz",
    );
    assert!(matches!(Registry::parse(&yaml), Err(Error::Yaml(_))));

    for field in [
        "cosign",
        "slsa_provenance",
        "minisign",
        "packslip",
        "github_artifact_attestations",
    ] {
        let yaml = BASIC.replace(
            "    format: tar.gz",
            &format!(
                "    checksum:\n      type: github_release\n      asset: sums\n      {field}:\n        enabled: true\n    format: tar.gz"
            ),
        );
        assert!(
            matches!(Registry::parse(&yaml), Err(Error::Yaml(_))),
            "checksum.{field} must be rejected"
        );
    }

    let yaml = BASIC.replace(
        "    format: tar.gz",
        "    version_constraint: \"false\"\n    version_overrides:\n      - version_constraint: \"true\"\n        github_artifact_attestations:\n          enabled: true\n    format: tar.gz",
    );
    assert!(matches!(Registry::parse(&yaml), Err(Error::Yaml(_))));

    let yaml = BASIC.replace(
        "      - goos: windows\n        format: zip",
        "      - goos: windows\n        format: zip\n        slsa_provenance:\n          enabled: true",
    );
    assert!(matches!(Registry::parse(&yaml), Err(Error::Yaml(_))));
}

#[test]
fn rejects_unknown_template_and_missing_package() {
    let yaml = BASIC.replace("{{.Version}}", "{{.Unknown}}");
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(registry.package("missing"), Err(Error::Package)));
    assert!(matches!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1"),
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
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1").unwrap();
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
    let linux = target("linux", "x86_64");
    let plan = plan_package(&registry, "org/app", &linux, "v1").unwrap();
    assert_eq!(plan.asset, "first-linux-amd64.tar.gz");
    let mac = target("macos", "aarch64");
    assert!(matches!(
        plan_package(&registry, "org/app", &mac, "v1"),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn empty_supported_envs_rejects_every_target() {
    let yaml = BASIC.replace(
        "    format: tar.gz",
        "    supported_envs: []\n    format: tar.gz",
    );
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1"),
        Err(Error::Unsupported(_))
    ));
}

const VERSIONED: &str = r#"
packages:
  - type: github_release
    repo_owner: golang-migrate
    repo_name: migrate
    asset: "migrate.{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: migrate
    version_constraint: 'semver("> 4.14.1")'
    version_overrides:
      - version_constraint: 'semver("<= 4.14.1")'
        files:
          - name: migrate
            src: "migrate.{{.OS}}-{{.Arch}}"
"#;

#[test]
fn selects_version_override_before_planning() {
    let registry = Registry::parse(VERSIONED).unwrap();
    let linux = target("linux", "x86_64");

    let new = plan_package(&registry, "golang-migrate/migrate", &linux, "v4.15.0").unwrap();
    assert_eq!(new.asset, "migrate.linux-amd64.tar.gz");
    assert!(
        matches!(&new.install.grep, neotales_ospm_archive::Selector::Regex(pattern) if pattern == "^migrate$")
    );

    let old = plan_package(&registry, "golang-migrate/migrate", &linux, "v4.14.1").unwrap();
    assert!(
        matches!(&old.install.grep, neotales_ospm_archive::Selector::Regex(pattern) if pattern == "^migrate\\.linux\\-amd64$")
    );
}

#[test]
fn rejects_versions_outside_every_constraint() {
    let registry = Registry::parse(VERSIONED).unwrap();
    assert!(matches!(
        plan_package(
            &registry,
            "golang-migrate/migrate",
            &target("linux", "x86_64"),
            "nightly"
        ),
        Err(Error::Unsupported("no matching version constraint"))
    ));
}

#[test]
fn first_matching_version_override_wins() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    version_constraint: "false"
    version_overrides:
      - version_constraint: 'Version == "v0.1.0"'
        asset: old-{{.OS}}-{{.Arch}}.tar.gz
      - version_constraint: "true"
        asset: new-{{.OS}}-{{.Arch}}.tar.gz
"#;
    let registry = Registry::parse(yaml).unwrap();
    let linux = target("linux", "x86_64");
    assert_eq!(
        plan_package(&registry, "org/app", &linux, "v0.1.0")
            .unwrap()
            .asset,
        "old-linux-amd64.tar.gz"
    );
    assert_eq!(
        plan_package(&registry, "org/app", &linux, "v0.2.0")
            .unwrap()
            .asset,
        "new-linux-amd64.tar.gz"
    );
}

#[test]
fn empty_version_override_values_inherit_the_base_recipe() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    version_constraint: "false"
    version_overrides:
      - version_constraint: "true"
        asset: ""
        format: ""
        repo_name: ""
"#;
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1").unwrap();
    assert_eq!(plan.asset, "app-linux-amd64.tar.gz");
    assert_eq!(plan.repository, "app");
}

#[test]
fn evaluates_list_and_prefix_version_constraints() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    version_constraint: "false"
    version_overrides:
      - version_constraint: 'Version in ["v0.2.2", "v0.2.3"]'
        asset: listed-{{.OS}}-{{.Arch}}.tar.gz
      - version_constraint: 'semverWithVersion(">= 0.6.0", trimPrefix(Version, "release-cli-"))'
        asset: trimmed-{{.OS}}-{{.Arch}}.tar.gz
"#;
    let registry = Registry::parse(yaml).unwrap();
    let linux = target("linux", "x86_64");
    assert_eq!(
        plan_package(&registry, "org/app", &linux, "v0.2.3")
            .unwrap()
            .asset,
        "listed-linux-amd64.tar.gz"
    );
    assert_eq!(
        plan_package(&registry, "org/app", &linux, "release-cli-0.6.0")
            .unwrap()
            .asset,
        "trimmed-linux-amd64.tar.gz"
    );
}

#[test]
fn applies_version_prefix_to_semver_and_templates() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.SemVer}}-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    version_prefix: "cli/"
    version_constraint: 'semver(">= 1.0.0")'
"#;
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(
        &registry,
        "org/app",
        &target("linux", "x86_64"),
        "cli/v1.2.0",
    )
    .unwrap();
    assert_eq!(plan.asset, "app-v1.2.0-linux-amd64.tar.gz");

    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    version_prefix: "cli/"
    version_constraint: 'semver(">= 1.0.0")'
    version_overrides:
      - version_constraint: "true"
        version_prefix: ""
        asset: fallback-{{.OS}}-{{.Arch}}.tar.gz
"#;
    let registry = Registry::parse(yaml).unwrap();
    assert_eq!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "1.2.0")
            .unwrap()
            .asset,
        "fallback-linux-amd64.tar.gz"
    );

    let yaml = yaml.replace("        version_prefix: \"\"\n", "");
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "1.2.0"),
        Err(Error::Unsupported("no matching version constraint"))
    ));
}

#[test]
fn applies_version_filter_and_rejects_excluded_versions() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    version_filter: 'not (Version startsWith "nightly-")'
"#;
    let registry = Registry::parse(yaml).unwrap();
    assert!(plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1.0.0").is_ok());
    assert!(matches!(
        plan_package(
            &registry,
            "org/app",
            &target("linux", "x86_64"),
            "nightly-1"
        ),
        Err(Error::Unsupported("version_filter excludes this version"))
    ));
}

#[test]
fn rejects_unsupported_version_expressions() {
    let yaml = VERSIONED.replace("semver(\"> 4.14.1\")", "Version =~ \"^v\"");
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(
            &registry,
            "golang-migrate/migrate",
            &target("linux", "x86_64"),
            "v4.15.0"
        ),
        Err(Error::Unsupported("version constraint expression"))
    ));
}

#[test]
fn default_files_use_the_repository_or_package_name() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
"#;
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1").unwrap();
    assert_eq!(plan.install.rename.as_deref(), Some("app"));

    let yaml = yaml.replace("repo_name: app", "repo_name: app\n    name: org/custom");
    let registry = Registry::parse(&yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1").unwrap();
    assert_eq!(plan.install.rename.as_deref(), Some("custom"));
}

#[test]
fn applies_format_overrides_and_extension_completion() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}"
    format: tar.gz
    files:
      - name: app
    format_overrides:
      - goos: windows
        format: zip
"#;
    let registry = Registry::parse(yaml).unwrap();
    assert_eq!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1")
            .unwrap()
            .asset,
        "app-linux-amd64.tar.gz"
    );
    let windows = plan_package(&registry, "org/app", &target("windows", "x86_64"), "v1").unwrap();
    assert_eq!(windows.asset, "app-windows-amd64.zip");
    assert_eq!(windows.install.rename.as_deref(), Some("app.exe"));

    let yaml = yaml.replace(
        "    format: tar.gz",
        "    format: tar.gz\n    append_ext: false",
    );
    let registry = Registry::parse(&yaml).unwrap();
    assert_eq!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1")
            .unwrap()
            .asset,
        "app-linux-amd64"
    );
}

#[test]
fn completes_windows_file_source_and_destination() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.zip"
    format: zip
    files:
      - name: app
        src: bin/app
"#;
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("windows", "x86_64"), "v1").unwrap();
    assert_eq!(plan.install.rename.as_deref(), Some("app.exe"));
    assert!(matches!(
        &plan.install.grep,
        neotales_ospm_archive::Selector::Regex(pattern) if pattern == "^bin/app\\.exe$"
    ));
}

#[test]
fn rosetta_and_windows_arm_emulation_change_the_arch_template() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    rosetta2: true
    supported_envs: [darwin/amd64]
"#;
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("macos", "aarch64"), "v1").unwrap();
    assert_eq!(plan.asset, "app-darwin-amd64.tar.gz");

    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.zip"
    format: zip
    files:
      - name: app
    windows_arm_emulation: true
    supported_envs: [windows/amd64]
"#;
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("windows", "aarch64"), "v1").unwrap();
    assert_eq!(plan.asset, "app-windows-amd64.zip");
}

#[test]
fn describes_an_enabled_upstream_checksum_without_trusting_it() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.Version}}-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    checksum:
      type: github_release
      asset: "app_{{trimV .Version}}_checksums.txt"
      file_format: regexp
      algorithm: sha256
      pattern:
        checksum: '^(\b[A-Fa-f0-9]{64}\b)'
        file: '^\b[A-Fa-f0-9]{64}\b\s+(\S+)$'
"#;
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1.2.3").unwrap();
    let checksum = plan.upstream_checksum.unwrap();
    assert_eq!(checksum.kind, "github_release");
    assert_eq!(checksum.asset.as_deref(), Some("app_1.2.3_checksums.txt"));
    assert_eq!(checksum.file_format, "regexp");
    assert_eq!(checksum.algorithm, "sha256");
    assert_eq!(
        checksum.checksum_pattern.as_deref(),
        Some(r"^(\b[A-Fa-f0-9]{64}\b)")
    );
    assert!(checksum.file_pattern.is_some());
}

#[test]
fn checksum_templates_use_checksum_replacements() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
    replacements:
      amd64: x86_64
    rosetta2: true
    checksum:
      type: github_release
      asset: "app_{{.OS}}_{{.Arch}}_checksums.txt"
      algorithm: sha256
      replacements:
        darwin: Mac
"#;
    let registry = Registry::parse(yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("macos", "aarch64"), "v1").unwrap();
    let checksum = plan.upstream_checksum.unwrap();
    assert_eq!(
        checksum.asset.as_deref(),
        Some("app_Mac_x86_64_checksums.txt")
    );
}

#[test]
fn checksum_can_be_disabled_or_http_and_rejects_unknowns() {
    let yaml = BASIC.replace(
        "    format: tar.gz",
        "    checksum:\n      enabled: false\n      type: github_release\n    format: tar.gz",
    );
    let registry = Registry::parse(&yaml).unwrap();
    assert!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1")
            .unwrap()
            .upstream_checksum
            .is_none()
    );

    let yaml = BASIC.replace(
        "    format: tar.gz",
        "    checksum:\n      type: http\n      url: \"https://example.invalid/app_{{trimV .Version}}.sha256\"\n    format: tar.gz",
    );
    let registry = Registry::parse(&yaml).unwrap();
    let plan = plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1").unwrap();
    assert_eq!(
        plan.upstream_checksum.unwrap().url.as_deref(),
        Some("https://example.invalid/app_1.sha256")
    );

    let yaml = BASIC.replace(
        "    format: tar.gz",
        "    checksum:\n      type: forgejo_release\n      asset: sums\n    format: tar.gz",
    );
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1"),
        Err(Error::Unsupported("checksum type"))
    ));

    let yaml = BASIC.replace(
        "    format: tar.gz",
        "    checksum:\n      type: github_release\n      asset: sums\n      algorithm: blake2\n    format: tar.gz",
    );
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1"),
        Err(Error::Unsupported("checksum algorithm"))
    ));
}

#[test]
fn requires_a_trusted_sha256_digest() {
    let registry = Registry::parse(BASIC).unwrap();
    let recipe = registry.package("org/app").unwrap();
    let linux = target("linux", "x86_64");
    assert!(matches!(
        recipe.plan(&linux, "v1", Path::new("/tmp"), "abc"),
        Err(Error::Unsupported("trusted SHA-256"))
    ));
    assert!(matches!(
        recipe.plan(&linux, "v1", Path::new("/tmp"), &"z".repeat(64)),
        Err(Error::Unsupported("trusted SHA-256"))
    ));
}

#[test]
fn rejects_no_asset_and_unsupported_archive_shapes() {
    let yaml = VERSIONED.replace(
        "      - version_constraint: 'semver(\"<= 4.14.1\")'\n        files:\n          - name: migrate\n            src: \"migrate.{{.OS}}-{{.Arch}}\"",
        "      - version_constraint: 'semver(\"<= 4.14.1\")'\n        no_asset: true",
    );
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(
            &registry,
            "golang-migrate/migrate",
            &target("linux", "x86_64"),
            "v4.14.1"
        ),
        Err(Error::Unsupported("version has no release asset"))
    ));

    let yaml = BASIC.replace("format: tar.gz", "format: raw");
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1"),
        Err(Error::Unsupported("archive format"))
    ));

    let yaml = BASIC.replace(
        "      - name: app.exe\n        src: \"bin/app.exe\"",
        "      - name: app.exe\n        src: \"bin/app.exe\"\n      - name: second",
    );
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1"),
        Err(Error::Unsupported("exactly one file is required"))
    ));

    let yaml = BASIC.replace(
        "      - name: app.exe\n        src: \"bin/app.exe\"",
        "      - name: app.exe\n        src: \"bin/app.exe\"\n        link: alias",
    );
    let registry = Registry::parse(&yaml).unwrap();
    assert!(matches!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1"),
        Err(Error::Unsupported("file link aliases"))
    ));
}

#[test]
fn ignores_safe_metadata_fields() {
    let yaml = r#"
packages:
  - type: github_release
    repo_owner: org
    repo_name: app
    name: org/app
    description: A tool
    link: https://example.invalid
    search_words: [tool]
    aliases:
      - name: app2
    asset: "app-{{.OS}}-{{.Arch}}.tar.gz"
    format: tar.gz
    files:
      - name: app
"#;
    let registry = Registry::parse(yaml).unwrap();
    assert_eq!(
        plan_package(&registry, "org/app", &target("linux", "x86_64"), "v1")
            .unwrap()
            .asset,
        "app-linux-amd64.tar.gz"
    );
}

#[test]
fn install_rejects_cross_target_before_network() {
    let registry = Registry::parse(BASIC).unwrap();
    let host = Target::current();
    let other = if host.os == "windows" {
        "linux"
    } else {
        "windows"
    };
    let plan = plan_package(&registry, "org/app", &target(other, "x86_64"), "v1").unwrap();
    assert_eq!(plan.target.os, other);

    let client = neotales_ospm_github::Github::new().unwrap();
    let error = plan.install(&client).unwrap_err();
    assert!(matches!(error, Error::Target("cross-target installation")));
}
