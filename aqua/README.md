# neotales-ospm-aqua

Plans and installs a strict subset of Aqua GitHub release recipes.

[![Neotales](https://github.com/neotales/brand/raw/main/assets/logo.png)](https://github.com/neotales/rs-ospm)

<!-- badges: crates.io, docs.rs, CI. Add after the first release. -->

## Install

```sh
cargo add neotales-ospm-aqua neotales-ospm-release
```

The crates are not published yet.

## What you can do

**Plan a release install.** 🧰 Supply registry content and an independently
trusted SHA-256.

```rust
use std::path::Path;
use neotales_ospm_aqua::Registry;
use neotales_ospm_release::Target;

let yaml = r#"
packages:
  - type: github_release
    repo_owner: example
    repo_name: tool
    asset: "tool-{{.OS}}-{{.Arch}}.{{.Format}}"
    format: tar.gz
    files:
      - name: tool
"#;
let registry = Registry::parse(yaml)?;
let target = Target { os: "linux".into(), arch: "x86_64".into() };
let plan = registry.package("example/tool")?.plan(
    &target, "v1", Path::new("/tmp/bin"),
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
)?;
assert_eq!(plan.asset, "tool-linux-amd64.tar.gz");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Only `github_release` recipes with one file and simple templates and platform
overrides are supported. `supported_envs` is enforced, and the first matching
platform override is used. Unsupported fields, including checksum and version
constraints, are rejected rather than ignored. The caller must verify registry
authenticity and supply a trusted checksum.

## Licence

[MIT](LICENSE.md)
