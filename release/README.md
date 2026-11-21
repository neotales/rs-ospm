# neotales-ospm-release

Selects a matching release asset using explicit mappings or platform inference.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-release
```

The crate has not been published yet.

## What you can do

**Select an asset.** 🧰 Ambiguous matches return an error.

```rust
use neotales_ospm_release::{Asset, AssetOptions, Target, select_asset};

let assets = vec![Asset {
    name: "tool-linux-amd64.tar.gz".into(),
    url: "https://example.org/tool-linux-amd64.tar.gz".into(),
}];
let target = Target { os: "linux".into(), arch: "x86_64".into() };
assert_eq!(select_asset(&assets, &target, "1.0", &AssetOptions::default())?.name,
           "tool-linux-amd64.tar.gz");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`AssetOptions` supports a default template, per-target templates keyed by
`os/arch`, and OS and architecture mappings. Templates use `{os}`, `{arch}`,
and `{version}`.

## Licence

[MIT](LICENSE.md)
