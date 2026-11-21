# OSPM APT container e2e

Runs the real `neotales-ospm-apt-cli` driver against a disposable Debian container.

This package is not shipped: `publish = false` and the release workflow's
explicit publish list do not name it. It is a normal member of the repository
workspace, so the committed root `Cargo.lock` pins its transitive dependencies.

## Requirements

- Docker, with the daemon reachable through the default socket or `DOCKER_HOST`.
- A Unix host, because the injection wrappers are POSIX shell scripts.
- Rust 1.100.0, the repository toolchain.
- Container network egress to the Debian mirror, for `apt-get update` and
  `hello` installation.

No host package command is run. There is no `sudo`, no privileged container, no
bind mount and no published port.

## Image pin

```text
debian:12-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
```

The multi-architecture index digest was resolved on 2026-11-21 with:

```sh
docker pull debian:12-slim
docker image inspect --format '{{index .RepoDigests 0}}' debian:12-slim
```

The tag text does not override the digest. `testcontainers` is pinned to
`=0.28.0`.

## Running

From the repository root:

```sh
NEOTALES_OSPM_E2E=1 cargo test -p neotales-ospm-e2e --locked
```

Without `NEOTALES_OSPM_E2E=1` the test prints a skip message and returns without
touching Docker, so the ordinary workspace test lane stays green. With the
variable set and no reachable daemon, the test fails and says so instead of
pretending to pass.

The Linux CI check lane runs exactly the command above, after the generic
workspace test. CI passes `--locked` so a manifest change that was not locked
fails the lane instead of resolving a different graph.

The dev-dependencies are gated to `cfg(unix)`. macOS still compiles the test
(and skips it without the opt-in); Windows compiles an empty test binary because
the file is `#![cfg(unix)]`.

## Dependency note

`testcontainers` 0.28.0, the current release, depends on
`astral-tokio-tar`, which RUSTSEC-2026-0440 marks unmaintained. The repository
`deny.toml` therefore reports it under `unmaintained = "transitive"`. This
package does not ignore the advisory; removing the finding needs either a
testcontainers release that drops the dependency or a deny policy decision
outside this package.

## What the test covers

One test drives the existing `Apt` type through these operations, in order:

- `List`: `dpkg-query` reports `bash` with a version.
- `Pin` and `Unpin`: `apt-mark` holds and releases `bash`.
- `UpdateSources`: `apt-get update` refreshes container package lists.
- `Search`: `apt-cache` finds `hello`, its version and architecture.
- `Update`: `apt-get install --only-upgrade` runs for the installed `bash`.
- `Install`: `hello` is installed in the container.
- `Remove`: `hello` is removed and no longer appears in `List`.

`InstallFile` is not covered. The driver canonicalises the path on the host
process, so a container-local file has no host path to pass.

## How the driver reaches the container

`Apt::with_executable` and `Apt::with_tools` are the documented injection points.
The test writes four wrappers into a temporary directory:

```sh
#!/bin/sh
exec docker exec --interactive \
  --env LC_ALL=C --env DEBIAN_FRONTEND=noninteractive \
  '<container-id>' apt-get "$@"
```

The container id comes from testcontainers and is quoted. The tool name is a
constant. Forwarded arguments remain positional and are never re-parsed. The
wrappers exist only while the test runs.

Non-interactive apt behaviour is configured inside the container through
`/etc/apt/apt.conf.d/99-ospm-e2e` so the driver's own argument construction is
not altered.

## Cleanup

testcontainers removes the container when `Container` is dropped, including on
panic unwinding. The temporary wrapper directory is removed by `tempfile`.
Package database changes and downloaded archives exist only inside the
disposable container.
