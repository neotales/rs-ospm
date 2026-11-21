# neotales-ospm-apt-shim

Exports a C ABI for querying `libapt-pkg`.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-apt-shim --features native
```

The crate is not published yet. Building the shared library also requires the
`libapt-pkg-dev` system package and a C++ compiler.

## What you can do

**Build the C ABI.** 🧰 The header is in `include/neotales_ospm_apt.h`.

```sh
cargo build -p neotales-ospm-apt-shim --features native
```

The shim is LGPL-2.1-or-later and links GPL-2.0-or-later `libapt-pkg`.
These separate licences do not remove the GPL obligations of the combined
software. All access to libapt-pkg in the process must be serialized.

## Licence

[LGPL-2.1-or-later](LICENSE.md)
