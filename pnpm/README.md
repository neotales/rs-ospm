# neotales-ospm-pnpm-cli

Runs pnpm for global tools and project dependencies through its command line.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add once the crate name and first version exist. -->

## Install

```sh
cargo add neotales-ospm-pnpm-cli
```

The crate is not published yet.

## What you can do

**Install a global tool at an exact version.** 🧰 The package name is validated
and the version is a complete semantic version.

```rust,no_run
use neotales_ospm_pnpm_cli::{PackageSpec, Pnpm};

Pnpm::new().add_global(&PackageSpec::exact("example-tool", "1.2.3")?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`add_global` runs `pnpm add --global --save-exact example-tool@1.2.3` directly,
without a shell. It closes standard input, so an authentication prompt fails
instead of blocking.

**Add a project dependency to one manifest section.** The project directory is
explicit, so the call does not depend on the process working directory. The
regular section passes `--save-prod`, so a package that was already a
development dependency moves to `dependencies`.

```rust,no_run
use neotales_ospm_pnpm_cli::{DependencySection, PackageSpec, Pnpm};

let pnpm = Pnpm::new().with_project_directory("project");
pnpm.add_project(
    &PackageSpec::new("example-tool")?,
    DependencySection::Development,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Install only the selected project.** 🧰 `install_project` passes
`--ignore-workspace`, so pnpm does not install or run scripts for the rest of a
workspace. `install_workspace` is the explicit whole-workspace opt-in.

```rust,no_run
use neotales_ospm_pnpm_cli::Pnpm;

let pnpm = Pnpm::new().with_project_directory("project");
pnpm.install_project()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Read what pnpm reports.** `list_global` and `list_project` ask pnpm for JSON
at depth zero, so only direct dependencies are returned.

```rust,no_run
use neotales_ospm_pnpm_cli::Pnpm;

for package in Pnpm::new().list_global()? {
    println!("{} {}", package.name, package.version);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Search the configured registry.** Results are the entries pnpm returns, with a
name, version and optional description.

```rust,no_run
use neotales_ospm_pnpm_cli::{Pnpm, SearchOptions};

let found = Pnpm::new().search("example", &SearchOptions::default())?;
for result in found {
    println!("{} {}", result.name, result.version);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Scope and limitations

- Global tools and project dependencies are separate methods. There is no
  scope flag and no portable trait implementation, because a linked global
  executable is not a dependency in a project manifest.
- `install_project` runs `pnpm install --ignore-workspace`. It installs only the
  configured project and runs that project's lifecycle scripts rather than the
  scripts of other workspace projects. Because pnpm treats the project as
  standalone, the install writes a lockfile inside the project and cannot
  resolve `workspace:` dependency specifications. `install_workspace` runs
  plain `pnpm install`; inside a workspace that installs every project and runs
  every project's lifecycle scripts, so calling it is the explicit opt-in.
- `DependencySection::Regular` passes `--save-prod`, not pnpm's default
  section inference. A package that is already a development dependency is
  moved to `dependencies`.
- `add` and `remove` run the lifecycle scripts that pnpm decides to run. This
  backend adds no sandbox. Use `command()` or `project_command()` for
  `--ignore-scripts` or any other option the wrapper does not expose.
- `add`, `remove` and both install methods pass standard output through and
  capture standard error. A long install shows no progress, and a failure
  returns the captured error in `PnpmError::Exit`.
- Arguments are passed directly. The backend starts no shell and never
  elevates privileges. The process inherits pnpm's environment and
  configuration, including registry, authentication, proxy, store and
  workspace files.
- `with_project_directory` sets the working directory for project operations
  only. It is not a sandbox: pnpm still reads a workspace file above the
  directory when one exists.
- The command and JSON shapes were verified against pnpm 12.18.0. Another
  pnpm version may differ; an unexpected JSON shape becomes
  `PnpmError::InvalidOutput`.
- The JSON parsers cover the depth-zero fields this crate returns. A
  `peerDependencies` section is returned when pnpm reports one.
- Update, store, cache, patch, publish, script, workspace-filter and runtime
  operations are not implemented here. They are absent rather than silently
  accepted.
- Global executables land in the directory configured by the pnpm
  installation. This crate does not change `PATH` and does not run an
  installation setup step.
- `command()` and `project_command()` hand arguments, environment, standard
  input, standard output and exit handling to the caller.

## Licence

[MIT](LICENSE.md)
