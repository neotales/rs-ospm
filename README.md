# neotales-ospm

Provides capability-based interfaces to operating system package managers.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm
```

The first release has not been published yet.

## What you can do

**Install through a capability.** 🧰 Names belong to the target manager, and
the caller must arrange appropriate privileges.

```rust,no_run
use neotales_ospm::{Install, PackageName, apt::Apt};

let package = PackageName::new("curl")?;
Apt::new().install(&package)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Use APT directly.** Build a native command when the portable API does not
cover an operation. This example refreshes package metadata.

```rust,no_run
use neotales_ospm::apt::Apt;

let status = Apt::new().command().arg("update").status()?;
assert!(status.success());
# Ok::<(), Box<dyn std::error::Error>>(())
```

APT currently invokes `apt-get`; it does not elevate privileges or opt into
noninteractive confirmation. Other managers and native library backends are not
yet implemented.

## Licence

[MIT](LICENSE.md)
