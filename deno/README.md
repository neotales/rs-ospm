# neotales-ospm-deno-cli

Installs Deno global executables and manages project dependencies through the Deno command line.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add once the crate name and first version exist. -->

## Install

```sh
cargo add neotales-ospm-deno-cli
```

The crate is not published yet.

## What you can do

**Install a global tool at an exact version.** 🧰 A registry package carries a
complete semantic version, and the executable name is explicit, so the target
launcher is known before Deno starts.

```rust,no_run
use neotales_ospm_deno_cli::{Deno, GlobalTool, RegistryPackage};

let package = RegistryPackage::jsr("@example/tool", "1.2.3")?;
let tool = GlobalTool::package("example-tool", package)?;
let installed = Deno::new()
    .with_install_root("/home/me/.deno")
    .install_global(&tool)?;
println!("{}", installed.launcher.display());
# Ok::<(), Box<dyn std::error::Error>>(())
```

The call runs `deno install --global --root /home/me/.deno --name example-tool
jsr:@example/tool@1.2.3` directly. It refuses to overwrite an existing launcher.

**Install a local script.** A script has no registry version, so it is a
separate constructor and the result reports `requested_version: None`. The path must be
absolute.

```rust,no_run
use neotales_ospm_deno_cli::{Deno, GlobalTool, ScriptSource};

let source = ScriptSource::path("/opt/tools/hello.ts")?;
let tool = GlobalTool::script("hello", source)?;
Deno::new().install_global(&tool)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Replace only when you say so.** `install_global` returns
`DenoError::AlreadyInstalled`. `replace_global` passes `--force` and replaces
the launcher and its metadata directory. There is no backup.

```rust,no_run
use neotales_ospm_deno_cli::{Deno, GlobalTool, RegistryPackage};

let package = RegistryPackage::npm("example-tool", "2.0.0")?;
let tool = GlobalTool::package("example-tool", package)?;
Deno::new().replace_global(&tool)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Add a project dependency at an exact version.** Project operations run in the
configured directory and pass `--save-exact`, so the manifest does not get a
caret range.

```rust,no_run
use neotales_ospm_deno_cli::{DependencySection, Deno, RegistryPackage};

let package = RegistryPackage::jsr("@example/tool", "1.2.3")?;
Deno::new()
    .with_project_directory("project")
    .add_project(&package, DependencySection::Regular)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Install a project.** `install_project` runs plain `deno install` in the
project directory. It does not pass a lockfile or lifecycle-script flag; use
`project_command()` for `--frozen-lockfile` or `--allow-scripts`.

```rust,no_run
use neotales_ospm_deno_cli::Deno;

Deno::new().with_project_directory("project").install_project()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Check an installation without asking Deno.** The check is a filesystem lookup
of the expected launcher. It does not run the launcher or prove that its target
resolves.

```rust,no_run
use neotales_ospm_deno_cli::{Deno, ExecutableName};

let name = ExecutableName::new("example-tool")?;
let deno = Deno::new().with_install_root("/home/me/.deno");
if deno.is_global_installed(&name)? {
    println!("{}", deno.launcher_path(&name)?.display());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Versions, output and exit status

- A global registry install embeds the exact version in the specifier. A
  project add passes `--save-exact`. Ranges, dist-tags, branches and abbreviated
  versions are rejected by `RegistryPackage`.
- A script URL or local path cannot be pinned to a registry version.
  `InstalledExecutable::requested_version` is `None` for it rather than a guessed value.
- Deno writes install progress, the success message and errors to standard
  error. This provider captures both streams. A non-zero exit becomes
  `DenoError::Exit` with `status`, `stdout` and `stderr`; success returns the
  verified launcher path, not a parsed message.
- A zero exit is not trusted alone. `install_global` checks that the expected
  launcher exists as a file and returns `DenoError::MissingLauncher` otherwise.
- Standard input is closed for every operation, so an authentication prompt
  fails instead of blocking. Each stream is capped at 16 MiB.

## Windows launchers

The provider starts `deno.exe` directly. Deno for Windows is a native
executable, not a batch shim, so there is no intervening `cmd.exe` quoting layer
for the commands this crate runs.

Deno's installer writes two files for a global tool: `<name>.cmd` for cmd.exe
and PowerShell, and an extensionless shell script for Git Bash and MINGW64.
`is_global_installed` checks `<name>.cmd` on Windows and `<name>` on Unix. This
crate never executes either generated launcher.

Executable names are limited to ASCII letters, digits, `_`, `+` and `-`, and
must start with a letter, digit or `_`. A dot is rejected on every platform
because Deno's Windows installer writes the `.cmd` shim with
`Path::with_extension("cmd")`, so `tool.one` and `tool.two` would both target
`tool.cmd`. Windows device names such as `CON`, `NUL` and `COM1` are rejected.
A specifier or script argument may not contain a double quote or a control
character, and on Windows it may not contain `!`, which cmd.exe can expand when
delayed expansion is enabled. Deno doubles `%` in its own batch launcher.

## Scope and limitations

- Global tools and project dependencies are separate methods. There is no scope
  flag and no portable trait implementation, because a launcher in a bin
  directory is not a dependency in a project manifest.
- The provider does not read or write provider configuration, and it does not
  parse `deno.json`, `package.json` or `deno.lock`. Deno edits the project's own
  files; this crate does not.
- List, search, update, `deno compile`, `deno task`, runtime version switching
  and cache management are not implemented. Use `command()` or
  `project_command()` for those operations.
- Project additions accept `jsr:` and `npm:` package roots pinned to an exact
  version. URL and local-file dependency additions use `project_command()`.
- Global scripts are an absolute local path or an HTTPS URL. Other schemes and
  extra Deno flags use `command()` or `install_command()`.
- `allow_all(true)` records `--allow-all` in a global launcher. That grants the
  installed tool full runtime permissions under the calling user's identity. It
  is off by default and is a deliberate trust decision.
- The provider does not change `PATH`, does not create the installation
  directory, and never elevates privileges. Deno reports the directory to add
  when it is not already on `PATH`.
- Lifecycle scripts for npm dependencies are not approved. Deno 2 requires an
  explicit `--allow-scripts` for them.
- The command shapes and launcher layout were checked against Deno 2.9.8 on
  Linux. Another Deno version may change its output or file layout.

## Licence

[MIT](LICENSE.md)
