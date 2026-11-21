//! Release parsing, selection, and input validation tests.
use std::collections::BTreeMap;

use neotales_ospm_github::{Error, Github, Release};
use neotales_ospm_release::{AssetOptions, SelectionError, Target};

fn linux_target() -> Target {
    Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    }
}

fn release(tag: &str, assets: &[(&str, &str)]) -> Release {
    let assets: Vec<String> = assets
        .iter()
        .map(|(name, url)| format!(r#"{{"name":"{name}","browser_download_url":"{url}"}}"#))
        .collect();
    serde_json::from_str(&format!(
        r#"{{"tag_name":"{tag}","assets":[{}]}}"#,
        assets.join(",")
    ))
    .unwrap()
}

#[test]
fn parses_and_selects_asset_from_release_response() {
    let release: Release = serde_json::from_str(
        r#"{
        "tag_name":"v1.0.0",
        "assets":[
            {"name":"tool-linux-amd64.tar.gz","browser_download_url":"https://example.org/tool.tar.gz","digest":"sha256:abc"},
            {"name":"tool-darwin-arm64.zip","browser_download_url":"https://example.org/tool.zip"}
        ]
    }"#,
    )
    .unwrap();
    let asset = release
        .select(&linux_target(), &AssetOptions::default())
        .unwrap();
    assert_eq!(asset.name, "tool-linux-amd64.tar.gz");
    assert_eq!(asset.digest.as_deref(), Some("sha256:abc"));
}

#[test]
fn ignores_unknown_fields_and_accepts_an_empty_asset_list() {
    let release: Release = serde_json::from_str(
        r#"{"tag_name":"v1.0.0","assets":[],"draft":false,"prerelease":true,"extra":{"nested":1}}"#,
    )
    .unwrap();
    assert_eq!(release.tag_name, "v1.0.0");
    assert!(release.assets.is_empty());
    assert!(matches!(
        release.select(&linux_target(), &AssetOptions::default()),
        Err(Error::Selection(SelectionError::NoMatch))
    ));
}

#[test]
fn a_null_asset_digest_is_accepted() {
    let release: Release = serde_json::from_str(
        r#"{
        "tag_name":"v1.0.0",
        "assets":[{
            "name":"tool-linux-amd64.tar.gz",
            "browser_download_url":"https://example.org/tool.tar.gz",
            "digest":null
        }]
    }"#,
    )
    .unwrap();
    let asset = release
        .select(&linux_target(), &AssetOptions::default())
        .unwrap();
    assert!(asset.digest.is_none());
}

#[test]
fn a_release_needs_a_tag_and_an_asset_array() {
    assert!(serde_json::from_str::<Release>(r#"{"assets":[]}"#).is_err());
    assert!(serde_json::from_str::<Release>(r#"{"tag_name":"v1.0.0"}"#).is_err());
}

#[test]
fn template_uses_the_release_tag_as_the_version() {
    let release = release(
        "v2.1.0",
        &[(
            "tool-v2.1.0-linux-x86_64.tar.gz",
            "https://example.org/tool.tar.gz",
        )],
    );
    let options = AssetOptions {
        template: Some("tool-{version}-{os}-{arch}.tar.gz".into()),
        ..AssetOptions::default()
    };
    let asset = release.select(&linux_target(), &options).unwrap();
    assert_eq!(asset.name, "tool-v2.1.0-linux-x86_64.tar.gz");
}

#[test]
fn target_template_and_mappings_take_precedence() {
    let release = release(
        "v1.0.0",
        &[
            (
                "tool-Linux-amd64-v1.0.0.zip",
                "https://example.org/mapped.zip",
            ),
            (
                "tool-linux-x86_64-v1.0.0.tar.gz",
                "https://example.org/default.tar.gz",
            ),
        ],
    );
    let mut options = AssetOptions {
        template: Some("tool-{os}-{arch}-{version}.tar.gz".into()),
        ..AssetOptions::default()
    };
    options.target_templates.insert(
        "linux/x86_64".into(),
        "tool-{os}-{arch}-{version}.zip".into(),
    );
    options.os_mapping = BTreeMap::from([("linux".into(), "Linux".into())]);
    options.arch_mapping = BTreeMap::from([("x86_64".into(), "amd64".into())]);

    let asset = release.select(&linux_target(), &options).unwrap();
    assert_eq!(asset.name, "tool-Linux-amd64-v1.0.0.zip");
}

#[test]
fn ambiguous_assets_are_rejected_rather_than_guessed() {
    let release = release(
        "v1.0.0",
        &[
            ("tool-linux-amd64.tar.gz", "https://example.org/a.tar.gz"),
            ("tool-linux-x86_64.tar.gz", "https://example.org/b.tar.gz"),
        ],
    );
    let error = release
        .select(&linux_target(), &AssetOptions::default())
        .unwrap_err();
    assert!(matches!(
        &error,
        Error::Selection(SelectionError::Ambiguous)
    ));
    assert_eq!(
        error.to_string(),
        "GitHub release asset: multiple matching release assets"
    );
}

#[test]
fn an_explicit_template_selects_exactly_one_asset() {
    let release = release(
        "v1.0.0",
        &[
            ("tool-linux-amd64.tar.gz", "https://example.org/a.tar.gz"),
            ("tool-linux-x86_64.tar.gz", "https://example.org/b.tar.gz"),
        ],
    );
    let options = AssetOptions {
        template: Some("tool-linux-amd64.tar.gz".into()),
        ..AssetOptions::default()
    };
    assert_eq!(
        release.select(&linux_target(), &options).unwrap().name,
        "tool-linux-amd64.tar.gz"
    );
}

#[test]
fn unmatched_or_unsupported_targets_are_reported() {
    let release = release(
        "v1.0.0",
        &[("tool-darwin-arm64.zip", "https://example.org/tool.zip")],
    );
    assert!(matches!(
        release.select(&linux_target(), &AssetOptions::default()),
        Err(Error::Selection(SelectionError::NoMatch))
    ));

    let freebsd = Target {
        os: "freebsd".into(),
        arch: "x86_64".into(),
    };
    assert!(matches!(
        release.select(&freebsd, &AssetOptions::default()),
        Err(Error::Selection(SelectionError::UnsupportedTarget))
    ));
}

#[test]
fn an_invalid_template_is_reported() {
    let release = release(
        "v1.0.0",
        &[("tool-linux-amd64.tar.gz", "https://example.org/a.tar.gz")],
    );
    let options = AssetOptions {
        template: Some("tool-{os}-{arch}-{tool}.tar.gz".into()),
        ..AssetOptions::default()
    };
    assert!(matches!(
        release.select(&linux_target(), &options),
        Err(Error::Selection(SelectionError::InvalidTemplate))
    ));
}

#[test]
fn api_base_must_be_https_and_without_credentials() {
    for base in [
        "http://example.org",
        "https://user:pass@example.org",
        "https://user@example.org",
        "https://example.org/?x=1",
        "https://example.org/#fragment",
        "mailto:user@example.org",
        "",
    ] {
        assert!(
            matches!(Github::with_api_base(base), Err(Error::Invalid(_))),
            "accepted {base:?}"
        );
    }
    for base in [
        "https://api.github.com",
        "https://api.github.com/",
        "https://ghe.example/api/v3",
        "https://ghe.example/api/v3/",
        "https://example.org:8443/",
    ] {
        assert!(Github::with_api_base(base).is_ok(), "rejected {base:?}");
    }
}

#[test]
fn empty_and_dot_inputs_are_rejected_before_a_request() {
    let client = Github::with_api_base("https://api.github.com/").unwrap();
    for (owner, repository, tag) in [
        ("", "repo", None),
        ("owner", "", None),
        ("owner", "repo", Some("")),
        (".", "repo", None),
        ("..", "repo", None),
        ("owner", ".", None),
        ("owner", "..", None),
        ("owner", "repo", Some(".")),
        ("owner", "repo", Some("..")),
    ] {
        assert!(
            matches!(
                client.release(owner, repository, tag),
                Err(Error::Invalid(_))
            ),
            "accepted owner={owner:?} repository={repository:?} tag={tag:?}"
        );
    }
}

#[test]
fn public_release_when_configured() {
    if std::env::var_os("NEOTALES_OSPM_TEST_GITHUB").is_none() {
        return;
    }
    let release = Github::new()
        .unwrap()
        .release("BurntSushi", "ripgrep", None)
        .unwrap();
    assert!(!release.tag_name.is_empty());
    assert!(!release.assets.is_empty());
}
