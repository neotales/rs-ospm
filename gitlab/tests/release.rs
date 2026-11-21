//! Release parsing, selection, and input validation tests.
use std::collections::BTreeMap;

use neotales_ospm_gitlab::{Error, Gitlab, Release};
use neotales_ospm_release::{AssetOptions, SelectionError, Target};

fn linux_target() -> Target {
    Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    }
}

fn release(tag: &str, links: &[(&str, &str)]) -> Release {
    let links: Vec<String> = links
        .iter()
        .map(|(name, url)| format!(r#"{{"name":"{name}","url":"{url}"}}"#))
        .collect();
    serde_json::from_str(&format!(
        r#"{{"tag_name":"{tag}","assets":{{"links":[{}]}}}}"#,
        links.join(",")
    ))
    .unwrap()
}

#[test]
fn parses_and_selects_link_from_release() {
    let release: Release = serde_json::from_str(
        r#"{
        "tag_name":"v1.0.0",
        "assets":{"links":[
            {"name":"tool-linux-amd64.tar.gz","url":"https://example.org/tool.tar.gz"},
            {"name":"tool-darwin-arm64.zip","url":"https://example.org/tool.zip"}
        ]}
    }"#,
    )
    .unwrap();
    assert_eq!(
        release
            .select(&linux_target(), &AssetOptions::default())
            .unwrap()
            .name,
        "tool-linux-amd64.tar.gz"
    );
}

#[test]
fn generated_sources_are_not_selectable() {
    let release: Release = serde_json::from_str(
        r#"{
        "tag_name":"v1.0.0",
        "assets":{
            "count":1,
            "sources":[{"format":"tar.gz","url":"https://example.org/project-v1.0.0.tar.gz"}],
            "links":[]
        },
        "upcoming_release":false
    }"#,
    )
    .unwrap();
    assert!(matches!(
        release.select(&linux_target(), &AssetOptions::default()),
        Err(Error::Selection(SelectionError::NoMatch))
    ));
}

#[test]
fn a_release_needs_a_tag_and_an_asset_link_collection() {
    assert!(serde_json::from_str::<Release>(r#"{"assets":{"links":[]}}"#).is_err());
    assert!(serde_json::from_str::<Release>(r#"{"tag_name":"v1.0.0"}"#).is_err());
    assert!(serde_json::from_str::<Release>(r#"{"tag_name":"v1.0.0","assets":{}}"#).is_err());
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
fn ambiguous_links_are_rejected_rather_than_guessed() {
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
        "GitLab release asset: multiple matching release assets"
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
            matches!(Gitlab::with_api_base(base), Err(Error::Invalid(_))),
            "accepted {base:?}"
        );
    }
    for base in [
        "https://gitlab.com/api/v4",
        "https://gitlab.com/api/v4/",
        "https://gitlab.example/api/v4/",
        "https://example.org:8443/api/v4/",
    ] {
        assert!(Gitlab::with_api_base(base).is_ok(), "rejected {base:?}");
    }
}

#[test]
fn empty_tag_and_malformed_project_paths_are_rejected_before_a_request() {
    let client = Gitlab::with_api_base("https://gitlab.com/api/v4/").unwrap();
    for (project, tag) in [
        ("", None),
        ("project", Some("")),
        (".", None),
        ("..", None),
        ("project", Some(".")),
        ("project", Some("..")),
        ("group/../project", None),
        ("group//project", None),
        ("group/project/", None),
        ("/group/project", None),
    ] {
        assert!(
            matches!(client.release(project, tag), Err(Error::Invalid(_))),
            "accepted project={project:?} tag={tag:?}"
        );
    }
}

#[test]
fn public_release_when_configured() {
    if std::env::var_os("NEOTALES_OSPM_TEST_GITLAB").is_none() {
        return;
    }
    let release = Gitlab::new()
        .unwrap()
        .release("gitlab-org/gitlab-runner", None)
        .unwrap();
    assert!(!release.tag_name.is_empty());
}
