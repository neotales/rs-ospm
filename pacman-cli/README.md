# neotales-ospm-pacman-cli

Invokes pacman for package installation and removal.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-pacman-cli neotales-ospm-core
```

The crates are not published yet.

## What you can do

**Install a package.** 🧰 The caller supplies privileges and confirmation.

```rust,no_run
use neotales_ospm_core::{Install, PackageName};
use neotales_ospm_pacman_cli::Pacman;

Pacman::new().install(&PackageName::new("curl")?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The process backend never invokes sudo or a shell.

## Licence

[MIT](LICENSE.md)
