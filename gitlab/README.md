# neotales-ospm-gitlab

Discovers GitLab release assets and installs verified archives.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-gitlab neotales-ospm-release
```

The crates are not published yet.

## What you can do

**Find a release asset.** 🧰 Automatic selection rejects ambiguous matches.

```rust,no_run
use neotales_ospm_gitlab::Gitlab;
use neotales_ospm_release::{AssetOptions, Target};

let release = Gitlab::new()?.release("group/project", None)?;
let asset = release.select(&Target::current(), &AssetOptions::default())?;
println!("{}", asset.name);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Release::install` requires an independently trusted SHA-256 and archive
options. Generated source archives are not considered release asset links.
Private projects and authentication are not yet supported.

## Licence

[MIT](LICENSE.md)
