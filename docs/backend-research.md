# Backend interfaces and licences

Research for the requested drivers, checked 2026-11-21. Licences below describe
the upstream project or library, not every dependency. An LGPL shim does not
remove the GPL obligations of a combined distribution; review the intended
distribution with counsel.

## OS and application package managers

| Backend | Native interface and limitations | Upstream licence | Direction |
| --- | --- | --- | --- |
| WinGet | [Microsoft.Management.Deployment COM API](https://github.com/microsoft/winget-cli/blob/master/doc/specs/%23888%20-%20Com%20Api.md); Windows deployment and identity requirements need platform tests. | [MIT](https://github.com/microsoft/winget-cli/blob/master/LICENSE) | Use COM/WinRT, not a CLI wrapper. |
| Chocolatey | [chocolatey.lib](https://docs.chocolatey.org/en-us/choco/release-notes/) is a .NET library. Its supported embedding surface and .NET hosting need validation. | [Apache-2.0 open-source edition](https://docs.chocolatey.org/en-us/information/legal/); commercial editions have different terms. | Investigate .NET hosting before choosing a CLI fallback. |
| pacman | libalpm exposes a C API. The current shim targets ABI 16 only. | GPL-2.0-or-later, per the Arch pacman package. | Separate CLI, LGPL shim, and MIT client. |
| DNF | [libdnf5](https://dnf5.readthedocs.io/en/latest/about.html) supports repository and package transactions in C++; its C bindings are not implemented upstream. DNF 4 uses a different API. | libdnf5 LGPL-2.1-or-later; DNF5 programs GPL-2.0-or-later. | Investigate a C++ shim for libdnf5 and treat DNF 4 separately. |
| YUM | Legacy YUM is Python-based; a native C transaction interface is not established here. On newer distributions `yum` may point to DNF rather than legacy YUM. | Legacy [YUM project](https://github.com/rpm-software-management/yum) is GPL; check exact version and files before distribution. | Detect the actual implementation. Investigate Python integration; do not assume DNF and YUM are interchangeable. |
| AUR | [AUR RPC v5](https://wiki.archlinux.org/title/Aurweb_RPC_interface) supports HTTP search and package info, not installation. Installation builds user-submitted PKGBUILDs and uses pacman. | The server licence is not needed merely to call its HTTP API; each PKGBUILD and upstream package has its own licence. | Implement HTTP search separately from build/install. Never execute unreviewed PKGBUILDs implicitly. |
| yay | [yay](https://github.com/Jguer/yay) is a Go AUR helper; no supported C ABI has been established here. It orchestrates AUR builds and pacman. | GPL-3.0-or-later. | Investigate a reusable native interface; otherwise expose an explicit CLI backend. Do not equate yay with AUR itself. |
| Homebrew | Homebrew is Ruby-based; its documented [CLI module is private](https://docs.brew.sh/rubydoc/Homebrew/CLI.html). Its public package metadata and `brew info --json` do not provide a full install transaction API. | [BSD-2-Clause](https://github.com/Homebrew/brew/blob/master/LICENSE.txt). | Use supported metadata APIs for reads; investigate a stable install API before falling back to CLI. |
| APK | Alpine's package manager; native interfaces and ABI stability need investigation. | Verify upstream components and version. | Prefer a supported native interface; otherwise use an explicit CLI backend. |
| Zypper | openSUSE's package manager; libzypp is the library to investigate. | Verify libzypp and linked dependencies. | Investigate the native interface and its licensing before selecting bindings. |
| Scoop | Windows user-scoped package manager. | Verify upstream components and version. | Investigate a supported API, with CLI fallback if needed. |
| FreeBSD Ports | Source-based build and installation framework, distinct from binary `pkg`. Building a port can execute third-party code. | Verify framework and individual port licences. | Keep build operations explicit and separate from binary packages. |
| FreeBSD pkg | FreeBSD binary package manager. | Verify libpkg and linked dependencies. | Investigate native libpkg before using the CLI. |
| Flatpak | Cross-distribution application package manager. | Verify libflatpak and linked dependencies. | Investigate the native library, installation scope, and permissions. |

## Programming-language tooling

These are requested separately from OS package managers. Treat CLI integration
as the starting point, while checking supported APIs before committing to a
backend. Installing a tool, managing a project's dependencies, and installing
or switching a runtime are different operations; do not imply that one
capability implements the others.

| Tool | Integration to investigate |
| --- | --- |
| Cargo | `cargo install` for executable crates; project dependency operations are separate. |
| Go | `go install` for commands; module dependency operations are separate. |
| .NET | `dotnet tool` for tools; NuGet project dependencies are separate. |
| Node | Node is a runtime, not itself a package manager. Investigate npm and runtime discovery separately. |
| Bun | Investigate Bun's project package and executable installation commands separately. |
| Deno | Investigate Deno's project dependencies and executable installation commands separately. |
| pnpm | Investigate project packages and globally installed tools separately. |

Do not treat this table as evidence that an unimplemented driver works. See the
root README for current implemented capabilities.
