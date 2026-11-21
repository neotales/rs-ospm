# neotales-ospm-archive

Downloads verified archives and installs a selected executable.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-archive
```

The crate is not published yet.

## What you can do

**Install one verified file.** 🧰 Supply the SHA-256 from a trusted source.

```rust,no_run
use neotales_ospm_archive::{Format, InstallOptions, Selector, install_https};

let mut options = InstallOptions::new(
    Format::TarGz,
    Selector::Glob("*/bin/tool".into()),
    "/home/me/.local/bin",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
);
options.rename = Some("tool".into());
let path = install_https("https://example.org/tool.tar.gz", &options)?;
println!("{}", path.display());
# Ok::<(), Box<dyn std::error::Error>>(())
```

ZIP, tar, tar.gz, tar.xz and tar.bz2 are supported. The installer requires one
matching regular file, rejects existing destinations, limits file sizes, and
does not extract archive paths. On non-Windows systems it removes `.exe` from
the selection pattern and destination name. `post_install` runs a command only
when explicitly provided; it runs with the caller's privileges.

## Licence

[MIT](LICENSE.md)
