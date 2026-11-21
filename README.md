# rs-ospm

Rust crates for operating system package managers.

| Crate | Purpose |
| --- | --- |
| [`neotales-ospm-core`](core/) | Portable capability traits |
| [`neotales-ospm-apt`](apt/) | MIT client for the APT C ABI |
| [`neotales-ospm-apt-shim`](apt-shim/) | LGPL C ABI over `libapt-pkg` |
| [`neotales-ospm-apt-cli`](apt-cli/) | Explicit process-based APT backend |

The native implementation currently supports installed-status queries only.
The process backend supports install, remove, update, list, search, source
refresh, package holds and local package files. Source add and remove are not
implemented. There is no automatic fallback.

The shim depends on GPL-2.0-or-later `libapt-pkg`. The MIT and LGPL licences
of our code do not change the obligations of distributing the combined software.
