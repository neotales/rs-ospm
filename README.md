# rs-ospm

Rust crates for operating system package managers.

| Crate | Purpose |
| --- | --- |
| [`neotales-ospm-core`](core/) | Portable capability traits |
| [`neotales-ospm-apt`](apt/) | MIT client for the APT C ABI |
| [`neotales-ospm-apt-shim`](apt-shim/) | LGPL C ABI over `libapt-pkg` |
| [`neotales-ospm-apt-cli`](apt-cli/) | Explicit process-based APT backend |
| [`neotales-ospm-pacman`](pacman/) | MIT native pacman client |
| [`neotales-ospm-pacman-shim`](pacman-shim/) | LGPL C ABI shim for libalpm |
| [`neotales-ospm-pacman-cli`](pacman-cli/) | Explicit process-based pacman backend |
| [`neotales-ospm-archive`](archive/) | Verified archive download and executable installation |
| [`neotales-ospm-release`](release/) | Release asset selection and platform mappings |
| [`neotales-ospm-github`](github/) | GitHub releases and verified asset installation |
| [`neotales-ospm-gitlab`](gitlab/) | GitLab releases and verified asset installation |
| [`neotales-ospm-aqua`](aqua/) | Strict subset of Aqua GitHub release recipes |
| [`neotales-ospm-go-cli`](go-tool/) | Install explicitly versioned Go commands |

The native implementation currently supports installed-status queries only.
The process backend supports install, remove, update, list, search, source
refresh, package holds and local package files. Source add and remove are not
implemented. There is no automatic fallback.

The shim depends on GPL-2.0-or-later `libapt-pkg`. The MIT and LGPL licences
of our code do not change the obligations of distributing the combined software.
Pacman's shim similarly loads GPL-2.0-or-later libalpm. Pacman currently
supports native installed-package listing and CLI install and remove only.

See [backend research](docs/backend-research.md) for the other requested
OS managers, the separate programming-language tooling list, and their native
API and licensing status.

See [provider priorities](docs/provider-priorities.md) for the next
cross-platform providers.
