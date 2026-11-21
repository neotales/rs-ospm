# neotales-ospm-apt-cli

Provides an opt-in APT command-line fallback for package management.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-apt-cli --features cli
cargo add neotales-ospm-core
```

The first release has not been published yet.

## What you can do

**Install through a capability.** 🧰 The CLI fallback must be explicitly enabled.

```rust,no_run
# #[cfg(feature = "cli")]
# fn example() -> Result<(), Box<dyn std::error::Error>> {
use neotales_ospm_apt_cli::Apt;
use neotales_ospm_core::{Install, PackageName};

let package = PackageName::new("curl")?;
Apt::new().install(&package)?;
# Ok::<(), Box<dyn std::error::Error>>(())
# }
```

`apt-get` may require elevated privileges and confirmation. The crate does not
elevate privileges. The separate `neotales-ospm-apt` crate provides native APT
queries.

The `List` capability returns installed versions from `dpkg-query`, matching
the query as a name substring (an empty query lists all). `Search` uses APT's
regular-expression search syntax and returns versions selected by `apt-cache`;
when a version is specified, it checks all available versions for an exact
match. Results include architecture and description when available. Neither
operation currently supports provider-specific options. `Update` upgrades a
named package, and `UpdateSources` refreshes
repository metadata. Holds and local `.deb` installation are supported;
source add and remove are not. No command runs through a shell.

## Licence

[MIT](LICENSE.md)
