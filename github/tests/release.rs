//! Release parsing and selection tests.
use neotales_ospm_github::{Error, Github, Release};
use neotales_ospm_release::{AssetOptions, Target};

#[test]
fn parses_and_selects_asset_from_release_response() {
    let release: Release = serde_json::from_str(r#"{
        "tag_name":"v1.0.0",
        "assets":[
            {"name":"tool-linux-amd64.tar.gz","browser_download_url":"https://example.org/tool.tar.gz","digest":"sha256:abc"},
            {"name":"tool-darwin-arm64.zip","browser_download_url":"https://example.org/tool.zip"}
        ]
    }"#).unwrap();
    let target = Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    };
    let asset = release.select(&target, &AssetOptions::default()).unwrap();
    assert_eq!(asset.name, "tool-linux-amd64.tar.gz");
    assert_eq!(asset.digest.as_deref(), Some("sha256:abc"));
}

#[test]
fn api_base_must_be_https_and_without_credentials() {
    assert!(matches!(
        Github::with_api_base("http://example.org"),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        Github::with_api_base("https://user:pass@example.org"),
        Err(Error::Invalid(_))
    ));
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
