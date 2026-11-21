# neotales-ospm-go-cli

Installs Go commands with `go install` at an explicit module version.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-go-cli
```

The crate is not published yet.

## What you can do

**Install a command at an exact version.** 🧰 The module path must be fully
qualified and the version must be complete.

```rust,no_run
use neotales_ospm_go_cli::{Go, GoCommand};

let command = GoCommand::new("example.com/tool/cmd/tool", "v1.2.3")?;
Go::new().install(&command)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Go::install` spawns `go install example.com/tool/cmd/tool@v1.2.3` directly,
without a shell. It never elevates privileges. The process inherits the
caller's environment, so `GOBIN`, `GOPATH`, `GOPROXY`, and `GOFLAGS` keep their
usual meaning. It receives no interactive standard input, so a credential
prompt fails instead of blocking. Use `Go::command` or `Go::command_for` to
change arguments, environment, working directory, or output handling.

The version must be a complete semantic version beginning with `v`, such as
`v1.2.3`, a pseudo-version, or a release with `+incompatible`. Queries such as
`latest`, branch names, and abbreviated versions are rejected because they do
not name an exact version.

The module path must name one command, not a `...` wildcard. Each element
follows Go's module path rules: no leading or trailing dot, no
Windows-reserved name (case-insensitive, including with an extension such as
`con.txt`), and no Windows short-name-like suffix such as `foo~1`.

**Why there is no installed-version, list, or remove operation.** `go install`
records no package database that this crate can query. A binary's build
information is available only when a caller supplies the binary path, and a
module import path does not determine the installed binary's name. The crate
does not claim an installed version, listing, or removal that Go cannot prove.
It offers installation only.

**Why the portable traits are not implemented.** The portable `PackageName`
type accepts a conservative package-name subset and rejects `/`, so it cannot
carry a Go import path. This crate exposes `GoCommand` instead.

## Licence

[MIT](LICENSE.md)
