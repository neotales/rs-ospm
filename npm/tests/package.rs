//! Verify npm package-name and version validation without running npm.

use neotales_ospm_npm_cli::{InvalidPackageSpec, NpmPackage};

#[test]
fn accepts_unscoped_and_scoped_names() {
    for name in [
        "eslint",
        "@types/node",
        "lodash.merge",
        "foo_bar",
        "foo~bar",
        "Legacy.Name",
        "a",
        "@scope/tool-name",
    ] {
        let package = NpmPackage::new(name).unwrap();
        assert_eq!(package.name(), name);
        assert_eq!(package.version(), None);
        assert_eq!(package.argument(), name);
    }
}

#[test]
fn accepts_exact_versions_tags_and_simple_ranges() {
    for version in [
        "1.2.3",
        "1.2.3-beta.1",
        "1.2.3+build.5",
        "1.x",
        "latest",
        "next",
        "beta",
    ] {
        let package = NpmPackage::versioned("eslint", version).unwrap();
        assert_eq!(package.version(), Some(version));
        assert_eq!(package.argument(), format!("eslint@{version}"));
    }
}

#[cfg(not(windows))]
#[test]
fn unix_accepts_range_operator_versions() {
    for version in [
        "^1.2.3",
        "~1.2.3",
        ">=1.0.0 <2.0.0",
        "*",
        "^0.1.0 || ^0.2.0",
        "1.0.0 - 2.0.0",
    ] {
        let package = NpmPackage::versioned("eslint", version).unwrap();
        assert_eq!(package.version(), Some(version));
        assert_eq!(package.argument(), format!("eslint@{version}"));
    }
}

#[cfg(windows)]
#[test]
fn windows_rejects_range_operator_versions() {
    for version in [
        "^1.2.3",
        "~1.2.3",
        ">=1.0.0 <2.0.0",
        "*",
        "^0.1.0 || ^0.2.0",
        "1.0.0 - 2.0.0",
    ] {
        assert_eq!(
            NpmPackage::versioned("eslint", version).unwrap_err(),
            InvalidPackageSpec::Version,
            "version {version:?} should be rejected on Windows"
        );
    }
}

#[test]
fn argument_joins_scoped_names_and_versions() {
    let package = NpmPackage::versioned("@types/node", "22.0.0").unwrap();
    assert_eq!(package.name(), "@types/node");
    assert_eq!(package.version(), Some("22.0.0"));
    assert_eq!(package.argument(), "@types/node@22.0.0");
}

#[test]
fn rejects_names_npm_cannot_use() {
    for name in [
        "",
        "@",
        "@scope",
        "@/name",
        "@scope/",
        "@scope/name/extra",
        "@scope/na@me",
        ".foo",
        "_foo",
        "-foo",
        "foo.",
        "foo bar",
        "foo/bar",
        "foo\\bar",
        "foo:bar",
        "foo@1.2.3",
        "foo%20bar",
        "foo\0bar",
    ] {
        assert_eq!(
            NpmPackage::new(name).unwrap_err(),
            InvalidPackageSpec::Name,
            "name {name:?} should be rejected"
        );
    }
}

#[test]
fn rejects_oversized_names() {
    assert!(NpmPackage::new("a".repeat(214)).is_ok());
    assert_eq!(
        NpmPackage::new("a".repeat(215)).unwrap_err(),
        InvalidPackageSpec::Name
    );
}

#[test]
fn rejects_versions_that_could_be_misread() {
    for version in [
        "",
        " ",
        " 1.2.3",
        "1.2.3 ",
        "1.2.3\n",
        "1.2.3\0",
        "1.2.3; rm -rf",
        "1.2.3$(id)",
        "1.2.3`id`",
        "1.2.3\\x",
        "1.2.3@latest",
        "1.2.3/2",
        "1.2.3:2",
    ] {
        assert_eq!(
            NpmPackage::versioned("eslint", version).unwrap_err(),
            InvalidPackageSpec::Version,
            "version {version:?} should be rejected"
        );
    }
}

#[test]
fn rejects_oversized_versions() {
    assert!(NpmPackage::versioned("eslint", "1".repeat(256)).is_ok());
    assert_eq!(
        NpmPackage::versioned("eslint", "1".repeat(257)).unwrap_err(),
        InvalidPackageSpec::Version
    );
}
