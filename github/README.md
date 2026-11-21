# neotales-ospm-github

Discovers GitHub release assets and installs verified archives.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-github neotales-ospm-release
```

The crates are not published yet.

## What you can do

**Find a release asset.** 🧰 Automatic selection rejects ambiguous matches.

```rust,no_run
use neotales_ospm_github::Github;
use neotales_ospm_release::{AssetOptions, Target};

let release = Github::new()?.release("owner", "repository", None)?;
let asset = release.select(&Target::current(), &AssetOptions::default())?;
println!("{}", asset.name);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Release::install` requires an independently trusted SHA-256 and archive
options. Asset templates and platform mappings are provided by
`neotales-ospm-release`. Private repositories and authentication are not yet
supported.

## Licence

[MIT](LICENSE.md)
