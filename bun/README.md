# neotales-ospm-bun-cli

Runs Bun for project dependencies and global packages through its command line.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add once the crate name and first version exist. -->

## Install

```sh
cargo add neotales-ospm-bun-cli
```

The crate is not published yet.

## What you can do

**Add a project dependency at an exact version.** 🧰 The project directory is
explicit, so the call does not depend on the process working directory.
`--exact` keeps the manifest entry pinned.

```rust,no_run
use neotales_ospm_bun_cli::{Bun, DependencySection, PackageSpec};

let bun = Bun::new().with_project_directory("app");
bun.add_project(
    &PackageSpec::exact("example-tool", "1.2.3")?,
    DependencySection::Development,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Add a global package at an exact version.** Global and project scopes are
separate methods. Global commands use Bun's own global location from the
process environment.

```rust,no_run
use neotales_ospm_bun_cli::{Bun, PackageSpec};

Bun::new().add_global(&PackageSpec::exact("example-tool", "1.2.3")?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Install the configured project.** The install runs in the explicit project
directory and closes standard input, so an authentication prompt fails instead
of blocking.

```rust,no_run
use neotales_ospm_bun_cli::Bun;

Bun::new().with_project_directory("app").install_project()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Read declared dependencies as JSON.** Both listing methods return the
dependency sections Bun reports for the manifest. The values are the specs
recorded in the manifest, not resolved versions.

```rust,no_run
use neotales_ospm_bun_cli::Bun;

let declared = Bun::new().with_project_directory("app").list_project()?;
for package in declared.dependencies {
    println!("{} {}", package.name, package.spec);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Scope and limitations

- Only Bun project dependencies and global packages are in scope. There is no
  portable trait implementation, because a global executable is not a
  dependency in a project manifest.
- `list_project` and `list_global` read the manifest with `bun pm pkg get` and
  return declared specs, not resolved installed versions. Bun 1.4.4 has no JSON
  dependency-listing command; the human-readable dependency tree is not parsed.
- `list_global` uses `bun pm pkg get --global` when no global directory is
  configured, so Bun resolves the global location itself. A configured
  directory is read with `bun pm pkg get --cwd` instead. `add_global` and
  `remove_global` also let Bun resolve their global location from the process
  environment, so a configured global directory must match that location.
- With an explicit global directory, a missing directory or manifest is an
  error. Without one, a failed Bun query is returned as an error, even if the
  global manifest is absent; this avoids masking unrelated failures.
- Bun may create its empty global directory while it resolves the global
  manifest. This backend does not create or remove that directory itself.
- `install_project` runs `bun install` in the configured directory. When the
  directory belongs to a workspace, Bun selects the enclosing workspace; this
  backend cannot force the install to stay in one project.
- `add`, `remove` and `install` run the lifecycle scripts Bun decides to run.
  This backend adds no sandbox. Use `command()` or `project_command()` for
  `--ignore-scripts` or another option the wrapper does not expose.
- Standard input is closed and output is captured, so a long install shows no
  progress. A failure returns the captured error in `BunError::Exit`.
- Package names are a conservative registry subset and versions are complete
  semantic versions. Ranges, tags, local paths and aliases are rejected; use
  `command()` for those.
- Search, update, audit, cache, link, patch, publish and workspace filtering
  are absent rather than silently accepted.
- The command and JSON shapes were verified against Bun 1.4.4. Another version
  may differ; an unexpected shape becomes `BunError::InvalidOutput`.
- On Windows the default executable is `bun.exe`, a normal executable, and
  arguments are passed without a shell. If `with_executable` is given a
  batch-file shim, the platform starts that shim through its command
  interpreter and this crate does not validate the escaping. The global
  directory follows Bun's own resolution, so drive letters and backslashes
  follow Bun.
- `command()` and `project_command()` hand arguments, environment, standard
  input, standard output and exit handling to the caller.

## Licence

[MIT](LICENSE.md)
