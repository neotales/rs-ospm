# neotales-ospm-cargo-cli

Installs, removes, lists and searches Cargo executable crates through the Cargo command line.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-cargo-cli neotales-ospm-core
```

The crates are not published yet.

## What you can do

**Install a crate's binaries.** 🧰 The caller supplies confirmation and any
privilege the install root needs. Cargo builds from source, runs the crate's
build script as the current user, and may replace executables that are already
installed. Confirm the package and destination first.

```rust,no_run
use neotales_ospm_cargo_cli::Cargo;
use neotales_ospm_core::{Install, PackageName};

Cargo::new().install(&PackageName::new("ripgrep")?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**See which crates are installed.** `list` returns crate names and the installed
versions reported by Cargo.

```rust,no_run
use neotales_ospm_cargo_cli::Cargo;
use neotales_ospm_core::List;

for package in Cargo::new().list_all()? {
    println!("{} {}", package.name, package.version);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Search the configured registry.** Cargo matches the query against names and
descriptions. Pass `limit` through `QueryOptions` to ask for more than Cargo's
default of ten results.

```rust,no_run
use neotales_ospm_cargo_cli::Cargo;
use neotales_ospm_core::{QueryOptions, Search};

let mut options = QueryOptions::default();
options.extra.insert("limit".into(), Box::new(50u32));
let _found = Cargo::new().search("serde", &options)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Keep installs in a project directory.** The configured root is passed to
install, remove and list, so all three see the same set of crates.

```rust,no_run
use neotales_ospm_cargo_cli::Cargo;

let _cargo = Cargo::new().with_install_root("/opt/example/tools");
```

The backend passes arguments directly. It starts no shell and never requests
elevation.

## Scope and limitations

- Only executable crates installed with `cargo install` are in scope. A
  project's dependency graph, the toolchain, and runtimes are different
  operations and are not reported here.
- `install` builds the crate and runs its build script with the caller's
  permissions. Cargo handles registry access and checksum verification; this
  backend adds no sandbox of its own.
- Cargo inherits the calling process's environment and configuration, including
  registry, network, rustc wrapper, and build settings. This backend does not
  reset them.
- Cargo decides whether an install is a no-op or a replacement. It rebuilds and
  replaces executables when the package version or source, the installed binary
  names, the feature set, the profile, or the target differs. A path install
  through `command()` always builds and installs unless another package owns a
  binary. This backend exposes no force, version, source, feature, profile, or
  target option, so it cannot guarantee that an existing installation is left
  alone.
- `install` passes only the crate name. Cargo chooses the registry version from
  its own configuration, so a later install can upgrade a previously installed
  version. Use `command()` for an exact version, a different source, or a
  forced reinstall.
- `list` exposes the crate name and installed version only. Binary names and the
  source of a path-installed crate are not returned.
- `install` and `remove` pass standard output through and capture standard
  error, so Cargo's progress on standard error is not shown while a long build
  runs. A failure returns `CargoError::CommandFailed` with the exit status and
  the captured error.
- `search` asks Cargo for the registry's current version of each matching crate.
  An exact version in `QueryOptions` filters that reported version; it does not
  enumerate all published versions, so an empty result is not proof that the
  version does not exist.
- `search` returns at most the requested `limit` (1 to 100), or ten by default.
  Cargo's own configuration and network policy still apply.
- Source management, holds, source-refresh, and local package-file operations
  have no Cargo equivalent here and are not implemented. `Update` is not
  implemented separately: `install` already asks Cargo to select and possibly
  replace the installed version.
- `command()` hands full control of arguments, environment, standard output and
  exit status to the caller. Use it for `--force`, `--path`, `--git`,
  `--version`, or a different profile or target.
- Package names are validated by `PackageName`. Cargo's `crate@version` syntax
  is not representable through the portable traits; use `command()` for a
  pinned install.

## Licence

[MIT](LICENSE.md)
