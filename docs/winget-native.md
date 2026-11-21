# Native WinGet backend plan

This is a design and verification plan, not a working implementation. The
backend must use the WinGet COM/WinRT API, not launch or parse a command-line
client. It should expose only capabilities whose behavior has been verified.

## Design

1. Create a Windows-only `winget/` crate with a narrow adapter to
   `Microsoft.Management.Deployment`, separate from the similarly named Windows
   deployment API. Generate Rust bindings from a pinned, versioned WinGet
   `.winmd` using `windows-bindgen`; document the metadata source, version,
   license, generation command, and generated-code provenance. Establish
   compatibility between that contract and the minimum supported App Installer
   version before choosing the pin.
2. Initialize COM on the calling thread and implement object activation and
   lifetime management based on Microsoft's current implementation. The original
   COM specification describes out-of-process activation via `CoCreateInstance`;
   the current factory also distinguishes elevated and in-process activation.
   Do not assume ordinary WinRT activation or copy production/debug identifiers
   interchangeably. Return actionable errors for missing registration, unsupported
   contract versions, policy restrictions, and activation failure.
3. For installed packages, connect to the predefined local installed catalog;
   for search, connect to explicitly selected configured catalogs. Map package
   IDs and installed or default-install versions to the core types, retaining
   source identity in metadata. Check connection and search statuses as well as
   HRESULTs; treat truncated results as incomplete, not as a complete list.
   Define empty-query, version-filter, and unsupported-option behavior explicitly.
4. Resolve exact IDs and source ambiguity before any mutation. Add install,
   update, or removal only after validating their current IDL contracts, completion
   statuses, cancellation, privilege and interaction behavior. Do not silently
   choose among ambiguous matches or elevate privileges.

## Packaging and activation risks

- App Installer must be present and registered for the relevant user; its
  availability and API contract version vary by machine. An installed Rust
  binary alone does not ensure the COM server is available.
- Choose and document out-of-process versus in-process deployment deliberately.
  In-process deployment may require architecture-specific native binaries,
  manifests, and dependency redistribution; verify their licensing and servicing.
  Do not bundle or load an unverified binary from an untrusted location.
- COM apartment/thread affinity, elevated activation, architecture, package
  identity, policy, and asynchronous completion can affect behavior. Validate
  each supported configuration rather than assuming the ordinary-user path
  covers all of them.

## Verification gates

- Generate bindings reproducibly from the pinned input and review the ABI and
  dependencies. Run formatting, linting, and tests on supported targets; a Linux
  build or Windows cross-compilation is not a Windows runtime test.
- On Windows with a known App Installer version, exercise real COM activation,
  catalog connection, an exact-ID search, installed-package enumeration and
  version mapping. Test missing registration, failed catalog connection, no
  matches, ambiguous IDs, unsupported options and truncated results. Record
  OS, architecture, App Installer version, privileges, and command outcomes.
- Test elevated activation separately if it is supported; otherwise return a
  documented error. Test unsupported versions and non-Windows behavior.
- Run install/update/removal tests only with explicit authorization in a
  disposable Windows environment, using a controlled package and checking
  postconditions and cleanup. Avoid relying on public catalog availability for
  deterministic tests.
- The existing Windows CI runs workspace members; integrating this crate and
  enabling native tests requires a separate workspace/CI change. Do not treat
  an unexecuted CI plan as verification.

## Primary sources

- [WinGet COM API specification and examples](https://github.com/microsoft/winget-cli/blob/master/doc/specs/%23888%20-%20Com%20Api.md)
  — initial contract and out-of-process activation design; check the current
  IDL for subsequent changes.
- [Current WinGet deployment IDL](https://github.com/microsoft/winget-cli/blob/master/src/Microsoft.Management.Deployment/PackageManager.idl)
  and [Microsoft's activation factory](https://github.com/microsoft/winget-cli/blob/master/src/PowerShell/Microsoft.WinGet.Client.Engine/Helpers/ManagementDeploymentFactory.cs).
- [Microsoft `windows-bindgen` guidance](https://github.com/microsoft/windows-rs/blob/master/docs/crates/windows-bindgen.md)
  — generation from Windows metadata and committed bindings.
- [Microsoft WinGet installation documentation](https://learn.microsoft.com/en-us/windows/package-manager/winget/)
  — App Installer distribution and supported environments.
