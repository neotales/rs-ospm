# Provider priorities

Implement and validate these providers in order, with API-based access where a
supported API exists and an explicit CLI backend where it does not:

1. Aqua registry definitions.
2. GitHub releases.
3. GitLab releases.
4. WinGet native API.
5. Cargo.
6. Go.
7. Node tooling.
8. pnpm.
9. Bun.
10. Deno.
11. Homebrew.

Release providers should resolve a release and choose exactly one asset before
calling the archive installer. Allow explicit asset templates, OS and
architecture mappings, and an exact version. Do not silently choose between
ambiguous assets. Archive downloads require an expected checksum from a trusted
source; a checksum downloaded alongside an asset is not necessarily an
independent trust source.

The shared archive and release-selection crates are foundations, not evidence
that all listed providers are implemented. See the root README for current
implementation status.
