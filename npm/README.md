# neotales-ospm-npm-cli

Installs, removes, lists and searches npm global packages through the npm command line.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-npm-cli neotales-ospm-core
```

The crates are not published yet.

## What you can do

**Install a package at an exact version.** 🧰 `NpmPackage` accepts an unscoped
or scoped name and an exact version, range, or dist-tag. npm resolves the
request against its configured registry.

```rust,no_run
use neotales_ospm_npm_cli::{Npm, NpmPackage};

let npm = Npm::new().with_prefix("npm-prefix");
let package = NpmPackage::versioned("@types/node", "22.0.0")?;
npm.install_package(&package)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Keep installs in a writable prefix and choose the script policy.** The
configured prefix is passed to install, remove and list, so all three see the
same set of packages. npm still uses its own cache and registry configuration.
Lifecycle scripts are ignored by default; `allow_scripts(true)` runs them with
the calling user's permissions, so enable it only for packages you trust.

```rust,no_run
use neotales_ospm_npm_cli::{Npm, NpmPackage};

let npm = Npm::new()
    .with_prefix("npm-prefix")
    .allow_scripts(true);
let package = NpmPackage::versioned("esbuild", "0.25.0")?;
npm.install_package(&package)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**See which packages are installed.** `list` returns package names and the
installed versions npm reports. Scoped names are included.

```rust,no_run
use neotales_ospm_core::List;
use neotales_ospm_npm_cli::Npm;

for package in Npm::new().list_all()? {
    println!("{} {}", package.name, package.version);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Search the configured registry.** Pass `limit` through `QueryOptions` to ask
for more than npm's default of twenty results.

```rust,no_run
use neotales_ospm_core::{QueryOptions, Search};
use neotales_ospm_npm_cli::Npm;

let mut options = QueryOptions::default();
options.extra.insert("limit".into(), Box::new(25u32));
let found = Npm::new().search("eslint", &options)?;
println!("{} results", found.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Check the Node runtime separately.** Node is a runtime, not a package
manager. `NodeRuntime` reports `node --version`; it does not install packages
or manage Node versions.

```rust,no_run
use neotales_ospm_npm_cli::NodeRuntime;

let version = NodeRuntime::new().version()?;
println!("node {version}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

The backend passes arguments directly on Unix and starts no shell there. On
Windows the standard npm command is `npm.cmd`, a batch-file shim that the
platform starts through its command interpreter. That path is not validated by
this crate, so Windows builds reject version text, search queries, and prefixes
containing `cmd.exe` metacharacters before npm starts. Captured commands run in
their own process group on Unix or job object on Windows, so an output overflow
also terminates descendants that inherited npm's pipes. The backend never
requests elevation.

## Scope and limitations

- Only npm global packages are in scope. Project dependencies, `npx`, Node
  version management, and pnpm, Bun, or Deno are different operations.
- npm and Node must already be installed. This crate installs neither.
- Install changes the selected global prefix and npm's cache. Use
  `with_prefix` for a writable location; the backend does not elevate.
- Install ignores lifecycle scripts by default. Package installation still
  executes npm itself and any enabled scripts with the caller's permissions.
- Without a version, npm installs its default dist-tag, which can upgrade an
  existing package. Pass an exact version to pin.
- `list` reports npm's global dependency tree. When a prefix is configured,
  `list` first checks the exact npm package directory (`<prefix>/lib/node_modules`
  on Unix, `<prefix>/node_modules` on Windows). A missing prefix or missing
  package directory is an empty list only after `npm --version` confirms the
  backend can start; a missing npm is an error, not a false success. An
  existing package directory is listed by npm. A path that exists but is not a
  usable directory is also an error. Any npm `ENOENT` or other non-zero status
  after that check is returned as a failure rather than a partial list.
- `search` requires network access and returns the current version npm reports
  for each result, not every published version. The provider caps `limit` at
  250; npm's registry may return fewer.
- The portable `Install`, `Remove` and `IsInstalled` traits take a
  `PackageName`, which cannot carry a scoped name. Use `install_package`,
  `remove_package`, and `is_package_installed` for npm-specific package
  requests. `List` and `Search` return the portable `Package` type.
- npm output is captured with a per-stream limit of 16 MiB. Captured commands
  run in their own process group on Unix and a job object on Windows, so an
  overflow kills the whole group or job, not only the direct child. The direct
  child is reaped and `OutputTooLarge` is returned. A descendant that
  deliberately starts a new session or breaks away from the job is outside
  this containment. Use `command()` when a caller needs interactive output or
  extra npm options.
- On Windows the default executable is `npm.cmd`, the standard batch-file
  shim. Rust's process API starts batch files through the platform command
  interpreter, and this crate cannot validate that escaping. Windows builds
  therefore accept only version text made of ASCII letters, digits, `.`, `-`,
  and `+` (exact versions, tags, and simple `x` ranges), and restrict search
  queries and prefixes to characters that are not `cmd.exe` metacharacters.
  Range expressions with `^ ~ < > = | & *` or spaces are rejected before npm
  starts. Unix builds keep the wider version and query syntax.
- `Update`, source management, holds, and local package files have no separate
  implementation. Install already asks npm to select and possibly replace the
  installed version.

## Licence

[MIT](LICENSE.md)
