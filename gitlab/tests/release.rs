//! Release parsing and selection tests.
use neotales_ospm_gitlab::{Error, Gitlab, Release};
use neotales_ospm_release::{AssetOptions, Target};

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
    let target = Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    };
    assert_eq!(
        release
            .select(&target, &AssetOptions::default())
            .unwrap()
            .name,
        "tool-linux-amd64.tar.gz"
    );
}

#[test]
fn api_base_must_be_https_and_without_credentials() {
    assert!(matches!(
        Gitlab::with_api_base("http://example.org"),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        Gitlab::with_api_base("https://user:pass@example.org"),
        Err(Error::Invalid(_))
    ));
}
