# neotales-ospm-brew-cli

Reads installed Homebrew formulae and casks, reports outdated packages, and
runs explicitly scoped install, uninstall and upgrade operations through the
`brew` command.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add once the crate name and first version exist. -->

## Install

```sh
cargo add neotales-ospm-brew-cli
```

The crate is not published yet.

## What you can do

**List installed formulae and casks.** `installed` runs
`brew info --json=v2 --installed` and returns the two package kinds separately.
The name is Homebrew's fully qualified name when it reports one, so a formula
from a tap comes back as `user/tap/formula`.

```rust,no_run
use neotales_ospm_brew_cli::Brew;

let installed = Brew::new().installed()?;
for formula in installed.formulae {
    println!("{} {}", formula.name, formula.versions.join(", "));
}
for cask in installed.casks {
    println!("{} {}", cask.token, cask.version);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

**See which packages Homebrew considers outdated.** `outdated` runs
`brew outdated --json=v2` and keeps formulae and casks separate. Pinned
packages are reported as Homebrew reports them; they are not filtered out.

```rust,no_run
use neotales_ospm_brew_cli::Brew;

for formula in Brew::new().outdated()?.formulae {
    println!(
        "{} {:?} -> {}",
        formula.name, formula.installed_versions, formula.current_version
    );
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Install, uninstall or upgrade one package with an explicit scope.**
[`FormulaName`] and [`CaskToken`] are separate types, and every mutation names
one scope. There is no method that guesses whether a package is a formula or a
cask.

```rust,no_run
use neotales_ospm_brew_cli::{Brew, CaskToken, FormulaName};

let brew = Brew::new();
brew.install_formula(&FormulaName::new("example-tool")?)?;
brew.uninstall_cask(&CaskToken::new("example-app")?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Use the raw command for anything else.** [`Brew::command`] returns a
`std::process::Command` with the configured executable and no arguments.
Arguments, environment, standard input, standard output and exit handling
belong to the caller. The raw command does not receive the safety environment
that the wrapped operations set.

```rust,no_run
use neotales_ospm_brew_cli::Brew;

let brew = Brew::new();
brew.command().args(["info", "--json=v2", "example-tool"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Backend choice

Homebrew publishes version 2 JSON for inventory and outdated queries but no
supported library interface for install transactions. The choice was checked
against the published documentation on 2026-11-21. This crate therefore uses
the `brew` command directly, parses the published JSON documents, and does not
parse human-readable output. It links no Homebrew internal module and reads no
configuration file; this is a code-first Rust API whose callers pass names and
an executable to typed methods.

## Scope and limitations

- Homebrew does not publish a supported library or transaction interface for
  installs. This crate therefore uses the documented command-line interface and
  parses only the JSON documents that Homebrew publishes for machine use. The
  human-readable output is not parsed. See
  [Querying Homebrew](https://docs.brew.sh/Querying-Brew).
- The `brew info --json=v2` schema is version 2. Fields may be added without a
  schema bump, so unknown fields are ignored. A document whose required shape
  is different becomes [`BrewError::InvalidOutput`] instead of a partial list.
- `brew list --json` is not used. Homebrew documents it as requiring
  `--versions`, no named arguments and an external JSON processor; the
  installed-inventory path above is narrower.
- Formula and cask operations are separate methods. There is no scope flag and
  no portable trait implementation, because the shared capability traits have
  no formula-or-cask argument.
- This crate selects no version. `brew install` installs the formula's or
  cask's current version from Homebrew's configured metadata. Versioned
  formulae are separate package names, and Homebrew has no arbitrary exact
  version selection here.
- Wrapped operations set `HOMEBREW_NO_AUTO_UPDATE`, `HOMEBREW_NO_AUTOREMOVE`,
  `HOMEBREW_NO_INSTALL_CLEANUP` and
  `HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK`. Install also sets
  `HOMEBREW_NO_INSTALL_UPGRADE`, so installing an already installed but
  outdated package does not silently upgrade it. These settings keep an
  operation from updating metadata, cleaning old files, autoremoving unused
  dependencies, or repairing unrelated dependents. A caller that wants
  Homebrew's defaults can build the operation with [`Brew::command`] and set
  the environment itself.
- `brew upgrade` with a named package requests only that package, but Homebrew
  may still upgrade dependencies needed by it. Pinned packages are not
  upgraded by Homebrew. `installed` and `outdated` do not change pin state.
- Cask uninstall is a plain uninstall. It does not pass `--zap`, which can
  remove files shared with other applications. Cask availability and required
  administrator approval are decided by Homebrew and the host; this crate does
  not elevate privileges.
- Mutation methods close standard input. A prompt that reads standard input
  therefore fails instead of blocking. A program that opens the controlling
  terminal directly can still block; this crate does not control that. Use
  [`Brew::command`] for an interactive operation. Formula builds and cask
  installs run their own code with the calling user's permissions; install only
  packages you trust.
- Search, description, tap and source management, metadata refresh, cleanup,
  autoremove, pin and unpin, reinstall, local package files, services and
  rollback are not implemented. They are absent rather than silently accepted.
- No configuration file, manifest or environment adapter is read. Names,
  scopes and the executable are supplied through the Rust API. A configuration
  adapter belongs in a separate package.
- The JSON fixtures in the tests are derived from the documented version 2
  output. A live Homebrew installation was not available in the build
  environment, so the tests exercise command construction, validation, JSON
  parsing and failures through fake processes only. No test mutates a real
  Homebrew installation.
- Homebrew targets macOS and Linux. This crate compiles as ordinary Rust, but
  it makes no claim about a Windows Homebrew installation, and its command
  fixtures run only on Unix-like systems.
- The executable inherits the caller's environment and Homebrew configuration,
  including proxy, tap and credential settings. This crate reads no credential
  and requests no elevation.

## Licence

[MIT](LICENSE.md)
