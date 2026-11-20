# neotales-ospm-core

Defines portable capabilities for operating system package managers.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-core
```

The first release has not been published yet.

## What you can do

**Write against a capability.** 🧰 Backends supply their own error type and
only implement supported traits.

```rust
use neotales_ospm_core::{Install, PackageName};

fn install_curl<M: Install>(manager: &M) -> Result<(), M::Error> {
    let package = PackageName::new("curl").expect("valid name");
    manager.install(&package)
}
```

Package names are manager-specific; the portable type accepts a conservative subset.

## Licence

[MIT](LICENSE.md)
