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

## Running a script supplied by the archive

An archive can carry an installation script. Nothing runs unless you set
`archive_script`: there is no automatic hook and no configuration file. The
script runs only after the archive SHA-256 matches, so the order is verify,
install, then execute.

The selected member must be exactly one regular file. Links, special files,
absolute paths, `..` components and non-UTF-8 paths are rejected, and the
extracted size is capped by `max_file_bytes`. These checks happen before the
destination is written, so an unsafe or oversized script aborts the install.

On Unix, the script is staged in a private temporary directory, started with
`std::process::Command`, and removed afterwards. It must begin with `#!` and
name an absolute interpreter, which may itself be a shell. No shell wrapper is
explicitly added; platform execution mechanisms may still use shell fallbacks.
Arguments are passed verbatim, the temporary directory is the working directory,
and the caller's environment and privileges are inherited. Archive-script
execution is not currently supported on Windows.

That means the script is **code from the archive running with your own
permissions**. Only opt in for an archive and checksum you trust.

```rust,no_run
use neotales_ospm_archive::{ArchiveScript, Format, InstallOptions, Selector, install_https};

let mut options = InstallOptions::new(
    Format::TarGz,
    Selector::Glob("*/bin/tool".into()),
    "/home/me/.local/bin",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
);
options.archive_script = Some(ArchiveScript::new(Selector::Glob(
    "*/scripts/install.sh".into(),
)));
let path = install_https("https://example.org/tool.tar.gz", &options)?;
println!("{}", path.display());
# Ok::<(), Box<dyn std::error::Error>>(())
```

An archive script runs before the optional `post_install` command. If the script
exits non-zero the installed executable stays in place and the call returns
`InstallError::ScriptFailed`.

## Licence

[MIT](LICENSE.md)
