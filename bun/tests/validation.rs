//! Verify the public validation rules on every supported platform. These tests
//! spawn no process and touch no package installation.

use std::path::Path;

use neotales_ospm_bun_cli::{
    Bun, DeclaredDependencies, DependencySection, InvalidPackage, PackageName, PackageSpec,
};

#[test]
fn default_executable_matches_the_platform() {
    let expected = if cfg!(windows) { "bun.exe" } else { "bun" };
    assert_eq!(Bun::new().executable(), Path::new(expected));
    assert_eq!(Bun::new().project_directory(), None);
    assert_eq!(Bun::new().global_directory(), None);
}

#[test]
fn configured_paths_are_returned_as_given() {
    let bun = Bun::with_executable("custom-bun")
        .with_project_directory("project")
        .with_global_directory("global");
    assert_eq!(bun.executable(), Path::new("custom-bun"));
    assert_eq!(bun.project_directory(), Some(Path::new("project")));
    assert_eq!(bun.global_directory(), Some(Path::new("global")));
}

#[test]
fn accepts_registry_package_names() {
    let long = "a".repeat(214);
    let scoped_long = format!("@a/{}", "b".repeat(211));
    for name in [
        "a",
        "example-tool",
        "@example/tool",
        "Example-Tool",
        "a.b_c~d",
        &long,
        &scoped_long,
    ] {
        assert!(
            PackageName::new(name).is_ok(),
            "package name {name:?} should be accepted"
        );
    }
}

#[test]
fn rejects_unsafe_or_malformed_package_names() {
    let long = "a".repeat(215);
    for name in [
        "",
        long.as_str(),
        "-example-tool",
        ".example-tool",
        "_example-tool",
        "example tool",
        "example/tool",
        "@/tool",
        "@example/",
        "@example/tool/extra",
        "example-tool@1.2.3",
        "@example/tool@1.2.3",
        "example-tool:alias",
        "example-tool\nnext",
        "@-example/tool",
        "@.example/tool",
        "@example/-tool",
        "@example/.tool",
        "@example",
        "/example-tool",
        "..",
    ] {
        assert_eq!(
            PackageName::new(name).unwrap_err(),
            InvalidPackage::Name,
            "package name {name:?} should be rejected"
        );
    }
}

#[test]
fn accepts_complete_semantic_versions() {
    for version in [
        "0.0.0",
        "1.2.3",
        "10.20.30",
        "1.2.3-0",
        "1.2.3-rc.1",
        "1.2.3-alpha-1",
        "1.2.3+build.5",
        "1.2.3-rc.1+build.5",
    ] {
        assert!(
            PackageSpec::exact("example-tool", version).is_ok(),
            "version {version:?} should be accepted"
        );
    }
}

#[test]
fn rejects_versions_that_are_not_exact_semver() {
    for version in [
        "",
        "latest",
        "1",
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "1.2.3-",
        "1.2.3-01",
        "1.2.3+",
        "1.2.3+foo..bar",
        "^1.2.3",
        "~1.2.3",
        "=1.2.3",
        "1.2.3 ",
        " 1.2.3",
        "v1.2.3",
        "1.2.x",
        "1.2.3\n",
        "1.2.3-α",
        "1.2.3+build_1",
    ] {
        assert_eq!(
            PackageSpec::exact("example-tool", version).unwrap_err(),
            InvalidPackage::Version,
            "version {version:?} should be rejected"
        );
    }
}

#[test]
fn spec_argument_places_one_version_suffix() {
    assert_eq!(
        PackageSpec::exact("@example/tool", "1.2.3")
            .unwrap()
            .argument(),
        "@example/tool@1.2.3"
    );
    assert_eq!(
        PackageSpec::new("example-tool").unwrap().argument(),
        "example-tool"
    );
    let spec = PackageSpec::exact("example-tool", "1.2.3").unwrap();
    assert_eq!(spec.name().as_str(), "example-tool");
    assert_eq!(spec.version(), Some("1.2.3"));
}

#[test]
fn declared_dependencies_report_emptiness() {
    assert!(DeclaredDependencies::default().is_empty());
    assert!(
        !DeclaredDependencies {
            dependencies: vec![neotales_ospm_bun_cli::DeclaredPackage {
                name: "example-tool".to_owned(),
                spec: "1.2.3".to_owned(),
            }],
            ..DeclaredDependencies::default()
        }
        .is_empty()
    );
}

#[test]
fn dependency_sections_are_distinct() {
    assert_ne!(DependencySection::Regular, DependencySection::Development);
    assert_ne!(DependencySection::Optional, DependencySection::Peer);
}
