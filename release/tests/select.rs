//! Asset selection tests.
use neotales_ospm_release::{Asset, AssetOptions, SelectionError, Target, select_asset};

fn asset(name: &str) -> Asset {
    Asset {
        name: name.into(),
        url: format!("https://example.org/{name}"),
    }
}
fn target(os: &str, arch: &str) -> Target {
    Target {
        os: os.into(),
        arch: arch.into(),
    }
}

#[test]
fn guesses_target_and_does_not_match_checksum_files() {
    let assets = [
        asset("tool-linux-amd64.tar.gz"),
        asset("tool-windows-amd64.zip"),
        asset("tool-linux-amd64.tar.gz.sha256"),
    ];
    assert_eq!(
        select_asset(
            &assets,
            &target("linux", "x86_64"),
            "1.0",
            &AssetOptions::default()
        )
        .unwrap()
        .name,
        "tool-linux-amd64.tar.gz"
    );
}

#[test]
fn rejects_ambiguous_assets_and_cross_arch() {
    let assets = [
        asset("tool-linux-x86_64.tar.gz"),
        asset("tool-linux-amd64.zip"),
    ];
    assert_eq!(
        select_asset(
            &assets,
            &target("linux", "x86_64"),
            "1",
            &AssetOptions::default()
        ),
        Err(SelectionError::Ambiguous)
    );
    assert_eq!(
        select_asset(
            &assets,
            &target("linux", "x86"),
            "1",
            &AssetOptions::default()
        ),
        Err(SelectionError::NoMatch)
    );
}

#[test]
fn explicit_template_and_mappings() {
    let assets = [asset("tool-v1.2-darwin-arm64.zip")];
    let mut options = AssetOptions {
        template: Some("tool-{version}-{os}-{arch}.zip".into()),
        ..Default::default()
    };
    options.os_mapping.insert("macos".into(), "darwin".into());
    options
        .arch_mapping
        .insert("aarch64".into(), "arm64".into());
    assert_eq!(
        select_asset(&assets, &target("macos", "aarch64"), "v1.2", &options).unwrap(),
        &assets[0]
    );
    options.target_templates.insert(
        "macos/aarch64".into(),
        "tool-{version}-{os}-{arch}.zip".into(),
    );
    options.template = Some("{unknown}".into());
    assert_eq!(
        select_asset(&assets, &target("macos", "aarch64"), "v1.2", &options).unwrap(),
        &assets[0]
    );
    options.target_templates.clear();
    assert_eq!(
        select_asset(&assets, &target("macos", "aarch64"), "v1.2", &options),
        Err(SelectionError::InvalidTemplate)
    );
}
