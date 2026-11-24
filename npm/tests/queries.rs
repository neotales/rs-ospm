//! Verify list and search parsing, filtering, and option validation.
#![cfg(unix)]

mod support;

use std::fs;

use neotales_ospm_core::{IsInstalled, List, Package, PackageName, QueryOptions, Search};
use neotales_ospm_npm_cli::{Npm, NpmError, NpmPackage};
use support::{FakeProcess, with_npm_retry};

const INSTALLED: &str = r#"{
  "name": "lib",
  "dependencies": {
    "eslint": {"version": "9.0.0"},
    "@types/node": {"version": "22.0.0"},
    "typescript": {"version": "5.6.0"}
  }
}"#;

const SEARCH: &str = r#"[
  {"name": "eslint", "version": "9.0.0", "description": "Report JavaScript patterns"},
  {"name": "@types/eslint", "version": "9.0.1", "description": "TypeScript definitions for eslint"}
]"#;

const ENOENT: &str = r#"{
  "error": {
    "code": "ENOENT",
    "summary": "ENOENT: no such file or directory, lstat 'prefix/lib'",
    "detail": "This is related to npm not being able to find a file."
  }
}"#;

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
fn list_parses_scoped_and_unscoped_records() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let npm = Npm::with_executable(fake.executable());

    let installed = with_npm_retry(|| npm.list_all()).unwrap();
    assert_eq!(
        pairs(&installed),
        vec![
            ("eslint", "9.0.0"),
            ("@types/node", "22.0.0"),
            ("typescript", "5.6.0"),
        ]
    );
    assert!(installed.iter().all(|package| package.metadata.is_empty()));
    assert_eq!(fake.args(), vec!["ls", "--global", "--json", "--depth=0"]);
}

#[test]
fn list_passes_the_configured_prefix() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let prefix = fake.path().join("prefix");
    fs::create_dir_all(prefix.join("lib/node_modules")).unwrap();
    let prefix_text = prefix.to_str().unwrap();
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    with_npm_retry(|| npm.list_all()).unwrap();
    assert_eq!(
        fake.args(),
        vec![
            "ls",
            "--global",
            "--json",
            "--depth=0",
            "--prefix",
            prefix_text,
        ]
    );
}

#[test]
fn list_filters_by_name_substring_and_exact_version() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let npm = Npm::with_executable(fake.executable());

    let matching = with_npm_retry(|| npm.list("script", &QueryOptions::default())).unwrap();
    assert_eq!(pairs(&matching), vec![("typescript", "5.6.0")]);

    let scoped = with_npm_retry(|| npm.list("@types", &QueryOptions::default())).unwrap();
    assert_eq!(pairs(&scoped), vec![("@types/node", "22.0.0")]);

    let versioned = with_npm_retry(|| npm.list("", &options_with_version("22.0.0"))).unwrap();
    assert_eq!(pairs(&versioned), vec![("@types/node", "22.0.0")]);

    let absent = with_npm_retry(|| npm.list("", &options_with_version("9.9.9"))).unwrap();
    assert!(absent.is_empty());
}

#[test]
fn list_accepts_legacy_url_safe_names() {
    let fake = FakeProcess::new(
        "npm",
        r#"{"name": "lib", "dependencies": {"legacy!name": {"version": "1.0.0"}}}"#,
        "",
        0,
    );
    let npm = Npm::with_executable(fake.executable());

    let installed = with_npm_retry(|| npm.list_all()).unwrap();
    assert_eq!(pairs(&installed), vec![("legacy!name", "1.0.0")]);
}

#[test]
fn list_rejects_path_like_reported_names() {
    for name in [
        "../escape",
        "@scope/../escape",
        "@scope/..",
        "foo/bar",
        "@/name",
        "@scope/",
    ] {
        let stdout =
            format!(r#"{{"name": "lib", "dependencies": {{"{name}": {{"version": "1.0.0"}}}}}}"#);
        let fake = FakeProcess::new("npm", &stdout, "", 0);
        let npm = Npm::with_executable(fake.executable());
        let error = with_npm_retry(|| npm.list_all()).unwrap_err();
        assert!(
            matches!(error, NpmError::InvalidOutput),
            "name {name:?} produced {error:?}"
        );
    }
}

#[test]
fn list_rejects_malformed_output() {
    let fake = FakeProcess::new("npm", "this is not json\n", "", 0);
    let npm = Npm::with_executable(fake.executable());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(matches!(error, NpmError::InvalidOutput), "{error:?}");
}

#[test]
fn list_prefers_a_process_failure_over_malformed_output() {
    let fake = FakeProcess::new("npm", "this is not json\n", "npm failure\n", 1);
    let npm = Npm::with_executable(fake.executable());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(error, NpmError::CommandFailed { ref stderr, .. } if stderr == b"npm failure\n"),
        "{error:?}"
    );
}

#[test]
fn list_rejects_wrong_json_shapes() {
    for stdout in [
        "[]",
        r#"{"name": "lib", "dependencies": []}"#,
        r#"{"name": "lib", "dependencies": {"bad name": {"version": "1.0.0"}}}"#,
        r#"{"name": "lib", "dependencies": {"eslint": {}}}"#,
        r#"{"name": "lib", "dependencies": {"eslint": {"version": ""}}}"#,
    ] {
        let fake = FakeProcess::new("npm", stdout, "", 0);
        let npm = Npm::with_executable(fake.executable());
        let error = with_npm_retry(|| npm.list_all()).unwrap_err();
        assert!(
            matches!(error, NpmError::InvalidOutput),
            "stdout {stdout:?} produced {error:?}"
        );
    }
}

#[test]
fn list_reports_a_structured_npm_error() {
    let fake = FakeProcess::new("npm", ENOENT, "", 254);
    let npm = Npm::with_executable(fake.executable());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    match error {
        NpmError::Reported {
            status,
            code,
            summary,
        } => {
            assert_eq!(status.code(), Some(254));
            assert_eq!(code, "ENOENT");
            assert!(summary.contains("no such file or directory"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn list_reads_a_fresh_configured_prefix_as_empty_after_checking_npm() {
    let fake = FakeProcess::new("npm", "", "", 0);
    let npm = Npm::with_executable(fake.executable()).with_prefix(fake.path().join("prefix"));

    let installed = with_npm_retry(|| npm.list_all()).unwrap();
    assert!(installed.is_empty());
    assert_eq!(fake.args(), vec!["--version"]);
}

#[test]
fn list_missing_prefix_does_not_hide_a_missing_backend() {
    let npm =
        Npm::with_executable("/nonexistent/npm").with_prefix("/nonexistent/ospm-empty-prefix");

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(error, NpmError::Io(ref error) if error.kind() == std::io::ErrorKind::NotFound),
        "a missing npm must not be reported as an empty package list: {error:?}"
    );
}

#[test]
fn list_reads_a_prefix_without_a_package_root_as_empty() {
    let fake = FakeProcess::new("npm", "", "", 0);
    let prefix = fake.path().join("prefix");
    fs::create_dir_all(prefix.join("lib")).unwrap();
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    let installed = with_npm_retry(|| npm.list_all()).unwrap();
    assert!(installed.is_empty());
    assert_eq!(fake.args(), vec!["--version"]);
}

#[test]
fn list_reports_enoent_when_the_package_root_exists() {
    let fake = FakeProcess::new("npm", ENOENT, "", 254);
    let prefix = fake.path().join("prefix");
    fs::create_dir_all(prefix.join("lib/node_modules")).unwrap();
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(error, NpmError::Reported { ref code, .. } if code == "ENOENT"),
        "an ENOENT after a verified package root must be a failure: {error:?}"
    );
    assert!(fake.was_run());
}

#[test]
fn list_rejects_a_prefix_that_is_a_file() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let prefix = fake.path().join("prefix");
    fs::write(&prefix, b"not a directory").unwrap();
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(error, NpmError::PrefixInvalid(ref path) if path == &prefix),
        "{error:?}"
    );
    assert!(!fake.was_run());
}

#[test]
fn list_rejects_an_empty_prefix() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let npm = Npm::with_executable(fake.executable()).with_prefix("");

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(error, NpmError::PrefixInvalid(ref path) if path.as_os_str().is_empty()),
        "{error:?}"
    );
    assert!(!fake.was_run());
}

#[test]
fn list_rejects_a_prefix_with_a_non_directory_ancestor() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let blocked = fake.path().join("blocked");
    fs::write(&blocked, b"not a directory").unwrap();
    let prefix = blocked.join("prefix");
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(error, NpmError::PrefixInvalid(ref path) if path == &prefix),
        "{error:?}"
    );
    assert!(!fake.was_run());
}

#[test]
fn list_rejects_a_layout_directory_that_is_a_file() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let prefix = fake.path().join("prefix");
    fs::create_dir_all(&prefix).unwrap();
    fs::write(prefix.join("lib"), b"not a directory").unwrap();
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(error, NpmError::PrefixInvalid(ref path) if path == &prefix.join("lib")),
        "{error:?}"
    );
    assert!(!fake.was_run());
}

#[test]
fn list_rejects_a_package_root_that_is_a_file() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let prefix = fake.path().join("prefix");
    fs::create_dir_all(prefix.join("lib")).unwrap();
    fs::write(prefix.join("lib/node_modules"), b"not a directory").unwrap();
    let npm = Npm::with_executable(fake.executable()).with_prefix(prefix.as_path());

    let error = with_npm_retry(|| npm.list_all()).unwrap_err();
    assert!(
        matches!(error, NpmError::PrefixInvalid(ref path) if path == &prefix.join("lib/node_modules")),
        "{error:?}"
    );
    assert!(!fake.was_run());
}

#[test]
fn list_rejects_unsupported_options_and_nul_queries() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let npm = Npm::with_executable(fake.executable());

    let mut options = QueryOptions::default();
    options.extra.insert("limit".into(), Box::new(5u32));
    let error = npm.list("", &options).unwrap_err();
    assert!(
        matches!(error, NpmError::UnsupportedOption(ref key) if key == "limit"),
        "{error:?}"
    );

    let error = npm.list("e\0slint", &QueryOptions::default()).unwrap_err();
    assert!(matches!(error, NpmError::InvalidQuery), "{error:?}");
}

#[test]
fn search_parses_results_and_descriptions() {
    let fake = FakeProcess::new("npm", SEARCH, "", 0);
    let npm = Npm::with_executable(fake.executable());

    let found = with_npm_retry(|| npm.search("eslint", &QueryOptions::default())).unwrap();
    assert_eq!(
        pairs(&found),
        vec![("eslint", "9.0.0"), ("@types/eslint", "9.0.1")]
    );
    assert_eq!(
        found[0].metadata.get("description").map(String::as_str),
        Some("Report JavaScript patterns")
    );
    assert_eq!(fake.args(), vec!["search", "--json", "eslint"]);
}

#[test]
fn search_filters_the_reported_version() {
    let fake = FakeProcess::new("npm", SEARCH, "", 0);
    let npm = Npm::with_executable(fake.executable());

    let exact = with_npm_retry(|| npm.search("eslint", &options_with_version("9.0.1"))).unwrap();
    assert_eq!(pairs(&exact), vec![("@types/eslint", "9.0.1")]);

    let absent = with_npm_retry(|| npm.search("eslint", &options_with_version("3.0.0"))).unwrap();
    assert!(absent.is_empty());
}

#[test]
fn search_passes_a_validated_limit() {
    let fake = FakeProcess::new("npm", SEARCH, "", 0);
    let npm = Npm::with_executable(fake.executable());
    let mut options = QueryOptions::default();
    options.extra.insert("limit".into(), Box::new(25u32));

    with_npm_retry(|| npm.search("eslint", &options)).unwrap();
    assert_eq!(
        fake.args(),
        vec!["search", "--json", "--searchlimit=25", "eslint"]
    );
}

#[test]
fn search_validates_the_query_before_starting_npm() {
    let npm = Npm::with_executable("/nonexistent/npm-for-query-validation");

    for query in ["", "   ", "--force", "-x", "a\0b", "a\nb"] {
        let error = npm.search(query, &QueryOptions::default()).unwrap_err();
        assert!(
            matches!(error, NpmError::InvalidQuery),
            "query {query:?} produced {error:?}"
        );
    }
}

#[test]
fn search_rejects_bad_limits_and_unknown_options() {
    let npm = Npm::with_executable("/nonexistent/npm-for-option-validation");

    for limit in [0u32, 251] {
        let mut options = QueryOptions::default();
        options.extra.insert("limit".into(), Box::new(limit));
        let error = npm.search("eslint", &options).unwrap_err();
        assert!(
            matches!(error, NpmError::InvalidOption { ref key, .. } if key == "limit"),
            "{error:?}"
        );
    }

    let mut options = QueryOptions::default();
    options.extra.insert("limit".into(), Box::new("25"));
    let error = npm.search("eslint", &options).unwrap_err();
    assert!(
        matches!(error, NpmError::InvalidOption { ref key, .. } if key == "limit"),
        "{error:?}"
    );

    let mut options = QueryOptions::default();
    options.extra.insert("offline".into(), Box::new(true));
    let error = npm.search("eslint", &options).unwrap_err();
    assert!(
        matches!(error, NpmError::UnsupportedOption(ref key) if key == "offline"),
        "{error:?}"
    );
}

#[test]
fn search_rejects_malformed_output() {
    for stdout in [
        "garbage",
        "{}",
        r#"[{"version": "9.0.0"}]"#,
        r#"[{"name": "eslint"}]"#,
        r#"[{"name": "eslint", "version": ""}]"#,
    ] {
        let fake = FakeProcess::new("npm", stdout, "", 0);
        let npm = Npm::with_executable(fake.executable());
        let error = with_npm_retry(|| npm.search("eslint", &QueryOptions::default())).unwrap_err();
        assert!(
            matches!(error, NpmError::InvalidOutput),
            "stdout {stdout:?} produced {error:?}"
        );
    }
}

#[test]
fn search_reports_a_structured_npm_error() {
    let fake = FakeProcess::new("npm", ENOENT, "", 1);
    let npm = Npm::with_executable(fake.executable());

    let error = with_npm_retry(|| npm.search("eslint", &QueryOptions::default())).unwrap_err();
    assert!(
        matches!(error, NpmError::Reported { ref code, .. } if code == "ENOENT"),
        "{error:?}"
    );
}

#[test]
fn installed_checks_match_exact_names() {
    let fake = FakeProcess::new("npm", INSTALLED, "", 0);
    let npm = Npm::with_executable(fake.executable());

    assert!(
        with_npm_retry(|| npm.is_package_installed(&NpmPackage::new("eslint").unwrap())).unwrap()
    );
    assert!(
        with_npm_retry(|| { npm.is_package_installed(&NpmPackage::new("@types/node").unwrap()) })
            .unwrap()
    );
    assert!(
        !with_npm_retry(|| {
            npm.is_package_installed(&NpmPackage::new("eslint-config").unwrap())
        })
        .unwrap()
    );

    assert!(with_npm_retry(|| npm.is_installed(&PackageName::new("typescript").unwrap())).unwrap());
    assert!(!with_npm_retry(|| npm.is_installed(&PackageName::new("typ").unwrap())).unwrap());
}

#[cfg(windows)]
#[test]
fn windows_rejects_a_metacharacter_search_query_before_starting_npm() {
    let npm = Npm::with_executable("C:\\nonexistent\\npm.cmd");

    let error = npm
        .search("eslint&&calc", &QueryOptions::default())
        .unwrap_err();
    assert!(matches!(error, NpmError::InvalidQuery), "{error:?}");
}
