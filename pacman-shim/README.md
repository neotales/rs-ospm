# neotales-ospm-pacman-shim

Exports a C interface for native libalpm package queries.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-pacman-shim
```

The crate is not published yet. Build it as a shared library with Cargo.

## What you can do

**Build the C ABI.** 🧰 The header is in `include/neotales_ospm_pacman.h`.

```sh
cargo build -p neotales-ospm-pacman-shim
```

The shim is LGPL-2.1-or-later and loads GPL-2.0-or-later libalpm. Its licence
does not remove the GPL obligations of the combined software.

## Licence

[LGPL-2.1-or-later](LICENSE.md)
