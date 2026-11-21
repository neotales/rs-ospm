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
`List` returns installed packages; `Search` returns available packages. Both
take a query string and `QueryOptions`, including an optional exact version and
provider-specific values. Results contain a name, version, and backend-specific
metadata. `list_all()` lists all installed packages with default options.
`Update` upgrades a
package, while the optional `UpdateSources` capability refreshes sources.
`Sources`, `Pin`, and `InstallFile` are separate optional capabilities.

## Licence

[MIT](LICENSE.md)
