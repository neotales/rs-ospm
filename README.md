# rs-ospm

Rust crates for operating system package managers.

| Crate | Purpose |
| --- | --- |
| [`neotales-ospm-core`](core/) | Portable, independently implemented capabilities |
| [`neotales-ospm-apt`](apt/) | APT support; an opt-in CLI fallback is available |

Prefer a supported native API or FFI binding for each operation. Use a command
only when a suitable API is unavailable, and make that fallback explicit. Each
manager can expose its own native API alongside the portable traits; unsupported
capabilities need not be implemented. No FFI backend is implemented yet.

APT's current CLI fallback is opt-in and does not elevate privileges or supply
automatic confirmation. Future backends should document their native API,
library dependencies, privilege requirements and any CLI fallback.

All crates share a version. See [RELEASE.md](RELEASE.md) for publishing.
