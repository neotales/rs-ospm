# neotales-ospm-pacman

Queries installed packages through the native pacman shim.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-pacman neotales-ospm-core
```

The crates are not yet published.

## What you can do

**List installed packages.** 🧰 Load a trusted shim and libalpm.

```rust,no_run
use neotales_ospm_core::List;
use neotales_ospm_pacman::Pacman;

// SAFETY: Both paths identify trusted compatible libraries.
let pacman = unsafe {
    Pacman::load("/usr/local/lib/libneotales_ospm_pacman_shim.so",
                 "/usr/lib/libalpm.so.16", "/", "/var/lib/pacman/")?
};
for package in pacman.list_all()? {
    println!("{} {}", package.name, package.version);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The LGPL shim loads GPL-2.0-or-later libalpm. Review the obligations when
distributing the combined software. This client currently only implements list.

## Licence

[MIT](LICENSE.md)
