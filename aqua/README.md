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

## Supported recipe subset

This crate handles the parts of a registry recipe that decide which GitHub
release asset to download and which file to install:

- `github_release` packages with a GitHub repository.
- `asset`, `format`, `replacements`, `files`, and the default file name from
  the repository or package name.
- `overrides` by `goos`, `goarch`, and `envs`, including `rosetta2` and
  `windows_arm_emulation`.
- `format_overrides`, `supported_envs`, `append_ext`, and Windows executable
  extension completion.
- `version_constraint` and `version_overrides`, evaluated in declaration order
  with Aqua's first-match rule, plus `version_prefix`.
- `version_filter` for an explicitly requested version.
- The template variables and functions used by real assets: `.OS`, `.Arch`,
  `.GOOS`, `.GOARCH`, `.Version`, `.SemVer`, `.Format`, `.Asset`,
  `.AssetWithoutExt`, `.FileName`, `trimV`, `trimPrefix`, `trimSuffix`,
  `replace`, `title`, `lower`, `upper`, and pipelines.

A recipe whose version constraints match nothing is rejected instead of falling
back to its base settings.

Aqua plans disable the archive layer's `.exe` normalization. The recipe's `src`
is exact, and Aqua target completion has already run, so a literal
`bin/tool.exe` selects that member even on a non-Windows host.

## Checksums

A checksum file published beside the release asset is not an independent trust
source. The recipe's `checksum` block is parsed and reported as
`Plan::upstream_checksum`, but it is never used to verify an install. The caller
must pass a trusted SHA-256 digest to `Recipe::plan`; the digest is checked for
the expected shape before planning.

## Security

Features this crate cannot enforce are rejected while parsing, including when
nested in a checksum block:

- `cosign`
- `slsa_provenance`
- `minisign`
- `packslip`
- `github_artifact_attestations`
- `github_immutable_release`

They are not ignored, and a registry file containing one does not parse.

## Limits

This is not full Aqua compatibility. The following are rejected or unsupported:

- Package types other than `github_release`.
- More than one installed file, and `files[].link` or `files[].hard` aliases.
- Archive formats other than `zip`, `tar`, `tar.gz`/`tgz`, `tar.xz`/`txz`, and
  `tar.bz2`/`tbz`/`tbz2`. Raw binaries, `dmg`, `pkg`, `7z`, `br`, `lz4`, and
  `zst` need an installer this crate does not have.
- Templates that need structured semver helpers, `.Vars`, environment access,
  or other functions outside the list above.
- Behaviour that cannot be represented without a real Aqua runtime, such as
  `variants`, `no_asset`, `error_message`, a non-`github_tag` `version_source`,
  build or cargo sections, and package variables.
- Installing a plan whose target operating system differs from the host. The
  archive layer normalizes executable extensions for the host, so cross-target
  downloads are rejected before any request instead of selecting a wrong file.
- Fetching or parsing an upstream checksum file.

Unsupported fields and values fail with an error rather than being guessed.
The caller still has to verify registry authenticity and supply the trusted
checksum.

## Licence

[MIT](LICENSE.md)
