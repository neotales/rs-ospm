# neotales-ospm-apt

Provides an opt-in APT command-line fallback for package management.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-apt --features cli
cargo add neotales-ospm-core
```

The first release has not been published yet.

## What you can do

**Install through a capability.** 🧰 The CLI fallback must be explicitly enabled.

```rust,no_run
# #[cfg(feature = "cli")]
# fn example() -> Result<(), Box<dyn std::error::Error>> {
use neotales_ospm_apt::Apt;
use neotales_ospm_core::{Install, PackageName};

let package = PackageName::new("curl")?;
Apt::new().install(&package)?;
# Ok::<(), Box<dyn std::error::Error>>(())
# }
```

`apt-get` may require elevated privileges and confirmation. The crate does not
elevate privileges. Native APT API or FFI support is not implemented yet.

## Licence

[MIT](LICENSE.md)
