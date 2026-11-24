//! Verify the public validation rules on every supported platform. These tests
//! spawn no process and touch no Homebrew installation.

use std::path::Path;

use neotales_ospm_brew_cli::{Brew, CaskToken, FormulaName, InvalidName};

#[test]
fn default_executable_is_brew_from_path() {
    assert_eq!(Brew::new().executable(), Path::new("brew"));
}

#[test]
fn accepts_formula_names() {
    let longest = "a".repeat(255);
    for name in [
        "a",
        "hello",
        "python@3.12",
        "openssl@3",
        "example-tool",
        "a.b_c+d-e",
        "user/tap/tool",
        "homebrew/core/hello",
        "A1",
        &longest,
    ] {
        assert!(
            FormulaName::new(name).is_ok(),
            "formula name {name:?} should be accepted"
        );
    }
}

#[test]
fn rejects_unsafe_or_malformed_formula_names() {
    let too_long = "a".repeat(256);
    for name in [
        "",
        too_long.as_str(),
        "-example",
        ".example",
        "/example",
        "_example",
        "@example",
        "example/",
        "example//tool",
        "one/two/three/four",
        "example tool",
        "example\0tool",
        "example:tool",
        "example/tap/..",
        "..",
        ".",
        "example/tap/-tool",
        "example/tap/@tool",
        "example/tap/_tool",
        "example/tap/.tool",
        "example\ttool",
        "example\ntool",
    ] {
        assert_eq!(
            FormulaName::new(name).unwrap_err(),
            InvalidName,
            "formula name {name:?} should be rejected"
        );
    }
}

#[test]
fn accepts_cask_tokens() {
    let longest = "a".repeat(255);
    for token in [
        "a",
        "example-app",
        "example@2",
        "app+extra",
        "a.b_c-d",
        "user/tap/example-app",
        "A1",
        &longest,
    ] {
        assert!(
            CaskToken::new(token).is_ok(),
            "cask token {token:?} should be accepted"
        );
    }
}

#[test]
fn rejects_unsafe_or_malformed_cask_tokens() {
    let too_long = "a".repeat(256);
    for token in [
        "",
        too_long.as_str(),
        "-example",
        ".example",
        "/example",
        "_example",
        "@example",
        "example/",
        "example//app",
        "one/two/three/four",
        "example app",
        "example\0app",
        "example:app",
        "example/tap/..",
        "..",
        ".",
        "example/tap/-app",
        "example/tap/@app",
        "example/tap/_app",
        "example/tap/.app",
        "example\tapp",
        "example\napp",
    ] {
        assert_eq!(
            CaskToken::new(token).unwrap_err(),
            InvalidName,
            "cask token {token:?} should be rejected"
        );
    }
}

#[test]
fn names_expose_the_caller_text() {
    let formula = FormulaName::new("user/tap/tool").unwrap();
    assert_eq!(formula.as_str(), "user/tap/tool");
    assert_eq!(formula.to_string(), "user/tap/tool");

    let cask = CaskToken::new("example-app").unwrap();
    assert_eq!(cask.as_str(), "example-app");
    assert_eq!(cask.to_string(), "example-app");
}

#[test]
fn invalid_name_is_an_error_with_a_stable_message() {
    assert_eq!(InvalidName.to_string(), "invalid Homebrew package name");
    let error: &dyn std::error::Error = &InvalidName;
    assert!(error.source().is_none());
}
