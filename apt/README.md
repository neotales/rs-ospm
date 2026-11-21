# neotales-ospm-apt

Loads the APT native C ABI and queries installed packages.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-apt neotales-ospm-core
```

The crates are not published yet. Build the `neotales-ospm-apt-shim` shared
library with its `native` feature and install `libapt-pkg-dev`.

## What you can do

**Query an installed package.** 🧰 Loading a shared library is unsafe; use a
trusted shim and ensure no other code accesses libapt-pkg concurrently.

```rust,no_run
use neotales_ospm_apt::Apt;
use neotales_ospm_core::{IsInstalled, PackageName};

// SAFETY: This path is trusted, and no other code uses libapt-pkg.
let apt = unsafe { Apt::load("/usr/local/lib/libneotales_ospm_apt_shim.so")? };
let installed = apt.is_installed(&PackageName::new("curl")?)?;
println!("{installed}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

The shim links GPL-2.0-or-later `libapt-pkg`; review the obligations when
distributing the combined software. Native install and remove are not implemented.

## Licence

[MIT](LICENSE.md)
