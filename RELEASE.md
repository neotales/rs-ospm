# Releasing

The crates share a version. The `v*` tag must match the literal version in both
all crate manifests. Keep dependencies on core in sync.

The first version, `0.0.0-alpha.0`, must be uploaded manually: run
`~/work/rs/publish.sh` for `core/` first, wait for the registry index, then run
it for `apt-shim/`, `apt/`, `apt-cli/`, `pacman-shim/`, `pacman/`, and
`pacman-cli/`, `archive/`, `release/`, `github/`, and `gitlab/`. Configure the trusted publisher on each crate for the
`neotales/rs-ospm` repository, `release.yml` workflow and an empty environment.
Do not put a token in the repository.

For later releases, bump both manifests and the dependency, update `Cargo.lock`,
run workspace checks, commit and push to `dev`, then push the matching tag.
The tag workflow checks and publishes core before APT using OIDC.
Manual workflow runs perform checks but do not publish.
