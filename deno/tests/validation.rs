//! Constructor checks that run without a Deno installation.

use std::path::Path;

use neotales_ospm_deno_cli::{
    ExecutableName, InvalidExecutableName, InvalidPackage, InvalidPackageName, InvalidScript,
    PackageName, Registry, RegistryPackage, ScriptSource,
};

#[test]
fn executable_names_accept_portable_segments() {
    for name in ["tool", "tool-2", "_tool", "tool+extra", "A1"] {
        let name = ExecutableName::new(name).unwrap();
        assert!(!name.as_str().is_empty());
    }
}

#[test]
fn executable_names_reject_paths_flags_and_windows_devices() {
    for name in [
        "",
        ".",
        "..",
        ".tool",
        "tool.name",
        "a/b",
        "a\\b",
        "-tool",
        "+tool",
    ] {
        assert!(
            ExecutableName::new(name).is_err(),
            "{name:?} should be rejected"
        );
    }
    for name in ["CON", "con", "NUL", "COM1", "com9", "LPT1"] {
        assert_eq!(
            ExecutableName::new(name).unwrap_err(),
            InvalidExecutableName::ReservedWindowsName
        );
    }
    assert_eq!(
        ExecutableName::new("a b").unwrap_err(),
        InvalidExecutableName::Character
    );
    assert_eq!(
        ExecutableName::new("a".repeat(129)).unwrap_err(),
        InvalidExecutableName::TooLong
    );
}

#[test]
fn registry_packages_require_a_complete_version() {
    for version in [
        "1.2.3",
        "0.0.0-alpha.0",
        "1.2.3-rc.1",
        "1.2.3-rc.1+build.5",
        "1.2.3+build.5",
    ] {
        let package = RegistryPackage::npm("example-tool", version).unwrap();
        assert_eq!(package.version(), version);
    }
    for version in [
        "",
        "1",
        "1.2",
        "v1.2.3",
        "^1.2.3",
        "~1.2.3",
        "latest",
        "1.2.3 ",
        " 1.2.3",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "1.2.3-",
        "1.2.3+",
        "1.2.3+build+again",
    ] {
        assert_eq!(
            RegistryPackage::npm("example-tool", version).unwrap_err(),
            InvalidPackage::Version,
            "{version:?} should be rejected"
        );
    }
}

#[test]
fn jsr_packages_are_scoped_and_pinned() {
    let package = RegistryPackage::jsr("@example/tool", "1.2.3").unwrap();
    assert_eq!(package.registry(), Registry::Jsr);
    assert_eq!(package.specifier(), "jsr:@example/tool@1.2.3");
    assert_eq!(
        RegistryPackage::jsr("tool", "1.2.3").unwrap_err(),
        InvalidPackage::UnscopedJsr
    );
    assert_eq!(
        RegistryPackage::jsr("@example/Tool", "1.2.3").unwrap_err(),
        InvalidPackage::Name
    );
}

#[test]
fn npm_packages_accept_scoped_and_unscoped_names() {
    assert_eq!(
        RegistryPackage::npm("example-tool", "1.2.3")
            .unwrap()
            .specifier(),
        "npm:example-tool@1.2.3"
    );
    assert_eq!(
        RegistryPackage::npm("@example/tool", "1.2.3")
            .unwrap()
            .specifier(),
        "npm:@example/tool@1.2.3"
    );
    for name in ["", "-tool", "@example", "a/b", "@example/tool/extra"] {
        assert_eq!(
            RegistryPackage::npm(name, "1.2.3").unwrap_err(),
            InvalidPackage::Name,
            "{name:?} should be rejected"
        );
    }
}

#[test]
fn package_names_for_removal_are_validated() {
    assert_eq!(
        PackageName::jsr("@example/tool").unwrap().as_str(),
        "@example/tool"
    );
    assert_eq!(
        PackageName::npm("example-tool").unwrap().as_str(),
        "example-tool"
    );
    assert_eq!(
        PackageName::jsr("example-tool").unwrap_err(),
        InvalidPackageName::UnscopedJsr
    );
    assert_eq!(
        PackageName::npm("-tool").unwrap_err(),
        InvalidPackageName::Name
    );
}

#[test]
fn script_urls_must_be_https_and_carry_no_credentials() {
    let source = ScriptSource::url("https://example.test/tool.ts").unwrap();
    assert_eq!(source.as_argument(), "https://example.test/tool.ts");
    for url in [
        "http://example.test/tool.ts",
        "file:///tmp/tool.ts",
        "https://",
        "https://user:pass@example.test/tool.ts",
        "https://example.test/a\"b",
    ] {
        assert!(
            ScriptSource::url(url).is_err(),
            "{url:?} should be rejected"
        );
    }
    assert_eq!(
        ScriptSource::url("http://example.test/tool.ts").unwrap_err(),
        InvalidScript::UnsupportedScheme
    );
    assert_eq!(
        ScriptSource::url("https://user@example.test/tool.ts").unwrap_err(),
        InvalidScript::UrlCredentials
    );
}

#[test]
fn scripts_require_an_absolute_utf8_path() {
    let absolute = std::env::temp_dir().join("ospm-deno-script.ts");
    let source = ScriptSource::path(&absolute).unwrap();
    assert_eq!(source.as_argument(), absolute.to_str().unwrap());
    assert_eq!(
        ScriptSource::path(Path::new("relative.ts")).unwrap_err(),
        InvalidScript::RelativePath
    );
}
