//! Verify list and search parsing, filtering and option validation.
#![cfg(unix)]

mod support;

use neotales_ospm_cargo_cli::{Cargo, CargoError};
use neotales_ospm_core::{List, Package, QueryOptions, Search};
use support::{FakeCargo, with_retry};

const INSTALLED: &str = "ripgrep v14.1.0:\n    rg\n\
cargo-edit v0.12.2:\n    cargo-add\n    cargo-rm\n\
local-tool v0.1.0 (/tmp/opencode/local):\n    local-tool\n";

const SEARCH: &str = "serde = \"1.0.229\"              # A generic serialization framework\n\
serde_json = \"1.0.154\"    # A JSON serialization file format\n\
... and 26096 crates more (use --limit N to see more)\n\
note: to learn more about a package, run `cargo info <name>`\n";

fn pairs(packages: &[Package]) -> Vec<(&str, &str)> {
    packages
        .iter()
        .map(|package| (package.name.as_str(), package.version.as_str()))
        .collect()
}

fn options_with_version(version: &str) -> QueryOptions {
    QueryOptions {
        version: Some(version.to_owned()),
        ..QueryOptions::default()
    }
}

#[test]
fn list_parses_records_and_ignores_binaries_and_sources() {
    let fake = FakeCargo::new(INSTALLED, "", 0);
    let cargo = Cargo::with_executable(fake.executable());

    let installed = with_retry(|| cargo.list_all()).unwrap();
    assert_eq!(
        pairs(&installed),
        vec![
            ("ripgrep", "14.1.0"),
            ("cargo-edit", "0.12.2"),
            ("local-tool", "0.1.0"),
        ]
    );
    assert!(installed.iter().all(|package| package.metadata.is_empty()));
}

#[test]
fn list_filters_by_name_substring_and_exact_version() {
    let fake = FakeCargo::new(INSTALLED, "", 0);
    let cargo = Cargo::with_executable(fake.executable());

    let matching = with_retry(|| cargo.list("cargo", &QueryOptions::default())).unwrap();
    assert_eq!(pairs(&matching), vec![("cargo-edit", "0.12.2")]);

    let versioned = with_retry(|| cargo.list("", &options_with_version("0.1.0"))).unwrap();
    assert_eq!(pairs(&versioned), vec![("local-tool", "0.1.0")]);

    let absent = with_retry(|| cargo.list("", &options_with_version("9.9.9"))).unwrap();
    assert!(absent.is_empty());
}

#[test]
fn list_rejects_malformed_output() {
    let fake = FakeCargo::new("this is not a record\n", "", 0);
    let cargo = Cargo::with_executable(fake.executable());

    let error = with_retry(|| cargo.list_all()).unwrap_err();
    assert!(matches!(error, CargoError::InvalidOutput), "{error:?}");
}

#[test]
fn list_rejects_unsupported_options_and_nul_queries() {
    let fake = FakeCargo::new(INSTALLED, "", 0);
    let cargo = Cargo::with_executable(fake.executable());

    let mut options = QueryOptions::default();
    options.extra.insert("limit".into(), Box::new(5u32));
    let error = cargo.list("", &options).unwrap_err();
    assert!(
        matches!(error, CargoError::UnsupportedOption(ref key) if key == "limit"),
        "{error:?}"
    );

    let error = cargo
        .list("rip\0grep", &QueryOptions::default())
        .unwrap_err();
    assert!(matches!(error, CargoError::InvalidQuery), "{error:?}");
}

#[test]
fn search_parses_results_and_skips_the_footer() {
    let fake = FakeCargo::new(SEARCH, "", 0);
    let cargo = Cargo::with_executable(fake.executable());

    let found = with_retry(|| cargo.search("serde", &QueryOptions::default())).unwrap();
    assert_eq!(
        pairs(&found),
        vec![("serde", "1.0.229"), ("serde_json", "1.0.154")]
    );
    assert_eq!(fake.args(), vec!["search", "--", "serde"]);
}

#[test]
fn search_filters_the_reported_version() {
    let fake = FakeCargo::new(SEARCH, "", 0);
    let cargo = Cargo::with_executable(fake.executable());

    let exact = with_retry(|| cargo.search("serde", &options_with_version("1.0.229"))).unwrap();
    assert_eq!(pairs(&exact), vec![("serde", "1.0.229")]);

    let absent = with_retry(|| cargo.search("serde", &options_with_version("3.0.0"))).unwrap();
    assert!(absent.is_empty());
}

#[test]
fn search_validates_the_query_before_starting_cargo() {
    let cargo = Cargo::with_executable("/nonexistent/cargo-for-query-validation");

    let error = cargo.search("", &QueryOptions::default()).unwrap_err();
    assert!(matches!(error, CargoError::InvalidQuery), "{error:?}");

    let error = cargo
        .search("ser\0de", &QueryOptions::default())
        .unwrap_err();
    assert!(matches!(error, CargoError::InvalidQuery), "{error:?}");
}

#[test]
fn search_passes_a_validated_limit() {
    let fake = FakeCargo::new(
        "serde = \"1.0.229\"    # A generic serialization framework\n",
        "",
        0,
    );
    let cargo = Cargo::with_executable(fake.executable());
    let mut options = QueryOptions::default();
    options.extra.insert("limit".into(), Box::new(25u32));

    let found = with_retry(|| cargo.search("serde", &options)).unwrap();
    assert_eq!(pairs(&found), vec![("serde", "1.0.229")]);
    assert_eq!(fake.args(), vec!["search", "--limit", "25", "--", "serde"]);
}

#[test]
fn search_rejects_bad_limits_and_unknown_options() {
    let cargo = Cargo::with_executable("/nonexistent/cargo-for-option-validation");

    for limit in [0u32, 101] {
        let mut options = QueryOptions::default();
        options.extra.insert("limit".into(), Box::new(limit));
        let error = cargo.search("serde", &options).unwrap_err();
        assert!(
            matches!(error, CargoError::InvalidOption { ref key, .. } if key == "limit"),
            "{error:?}"
        );
    }

    let mut options = QueryOptions::default();
    options.extra.insert("limit".into(), Box::new("25"));
    let error = cargo.search("serde", &options).unwrap_err();
    assert!(
        matches!(error, CargoError::InvalidOption { ref key, .. } if key == "limit"),
        "{error:?}"
    );

    let mut options = QueryOptions::default();
    options.extra.insert("offline".into(), Box::new(true));
    let error = cargo.search("serde", &options).unwrap_err();
    assert!(
        matches!(error, CargoError::UnsupportedOption(ref key) if key == "offline"),
        "{error:?}"
    );
}

#[test]
fn search_rejects_malformed_output() {
    let fake = FakeCargo::new("garbage without a version\n", "", 0);
    let cargo = Cargo::with_executable(fake.executable());

    let error = with_retry(|| cargo.search("serde", &QueryOptions::default())).unwrap_err();
    assert!(matches!(error, CargoError::InvalidOutput), "{error:?}");
}
