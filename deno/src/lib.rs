//! Explicit process backend for Deno global tools and project dependencies.
//!
//! [`Deno`] invokes an injected `deno` executable directly, without a shell.
//! Global executable installation and project dependency operations are
//! separate methods because a launcher in the installation bin directory is
//! not a dependency recorded in a project manifest.
//!
//! The provider is a code-first API. It does not read or write a provider
//! configuration file, and it does not parse `deno.json`, `package.json` or
//! `deno.lock` to decide what to do. A caller builds validated values in Rust
//! and passes them in.
//!
//! Versions are exact. A registry package carries a complete semantic version
//! in its `jsr:` or `npm:` specifier, and a project dependency is added with
//! `--save-exact`. A script source has no registry version, so the provider
//! says so instead of pretending it can pin one.
//!
//! # Windows
//!
//! The provider starts `deno.exe` directly; Deno for Windows is a native
//! executable, not a batch shim. Deno's *installer* writes two launchers for a
//! global tool: `<name>.cmd` for cmd.exe and PowerShell, and an extensionless
//! shell script for Git Bash and MINGW64. This crate checks `<name>.cmd` when
//! it asks whether a tool is installed, and it never executes either launcher.
//! See [`ExecutableName`] for the name restrictions that keep the generated
//! files ordinary, and [`GlobalTool`] for the source restrictions.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

mod capture;
mod spec;

pub use spec::{
    DependencySection, ExecutableName, GlobalTool, InvalidExecutableName, InvalidPackage,
    InvalidPackageName, InvalidScript, PackageName, Registry, RegistryPackage, ScriptSource,
};

use capture::CaptureError;

/// A Deno executable invoked as an external process.
///
/// The default executable is `deno`, which the platform resolves from `PATH`
/// (`deno.exe` on Windows). The process inherits the caller's environment, so
/// `DENO_DIR`, `DENO_INSTALL_ROOT`, registry authentication, proxy settings and
/// the module cache keep their usual meaning.
///
/// Global operations use the configured installation root and ignore the
/// project directory. Project operations use the configured project directory
/// and ignore the installation root. This type never invokes a shell and never
/// elevates privileges.
#[derive(Clone, Debug)]
pub struct Deno {
    executable: PathBuf,
    install_root: Option<PathBuf>,
    working_directory: Option<PathBuf>,
    project_directory: Option<PathBuf>,
    allow_all: bool,
}

impl Default for Deno {
    fn default() -> Self {
        Self::new()
    }
}

impl Deno {
    /// Uses `deno` from the current process's `PATH`.
    pub fn new() -> Self {
        Self::with_executable("deno")
    }

    /// Uses an explicit executable, useful for a configured installation or a
    /// test fixture.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            install_root: None,
            working_directory: None,
            project_directory: None,
            allow_all: false,
        }
    }

    /// Sets the working directory for every command.
    ///
    /// Project operations use [`with_project_directory`] instead when one is
    /// configured. This is useful when the caller does not want Deno to read a
    /// configuration file from the process working directory. It is not a
    /// sandbox: Deno still reads a configuration file it finds in the selected
    /// directory or above it.
    ///
    /// [`with_project_directory`]: Deno::with_project_directory
    pub fn with_working_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(directory.into());
        self
    }

    /// Sets the installation root for global operations.
    ///
    /// The value is passed to `deno install --root` and
    /// `deno uninstall --root`. Deno appends `bin` unless the path already ends
    /// with a `bin` component. A relative path is resolved against the working
    /// directory the command runs in: [`with_working_directory`] when set,
    /// otherwise the process working directory. An explicit root overrides
    /// `DENO_INSTALL_ROOT`.
    ///
    /// The provider computes the same bin directory for [`bin_directory`] and
    /// [`launcher_path`], without canonicalising symlinks or `..` components.
    /// An empty path is rejected by every operation that needs the root.
    ///
    /// [`with_working_directory`]: Deno::with_working_directory
    ///
    /// [`bin_directory`]: Deno::bin_directory
    /// [`launcher_path`]: Deno::launcher_path
    pub fn with_install_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.install_root = Some(root.into());
        self
    }

    /// Sets the working directory for project operations.
    ///
    /// Global operations ignore this directory. The path is passed to the
    /// spawned process as given, so a relative path is resolved against the
    /// caller's working directory. It is not a sandbox: Deno still reads a
    /// configuration file it finds in the directory or above it.
    pub fn with_project_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.project_directory = Some(directory.into());
        self
    }

    /// Grants every runtime permission to globally installed tools.
    ///
    /// This only affects [`install_global`](Deno::install_global) and
    /// [`replace_global`](Deno::replace_global). When enabled, `--allow-all` is
    /// passed to `deno install`, and Deno records it in the generated launcher.
    /// Every installed tool then runs with full filesystem, network,
    /// environment, process and FFI access under the calling user's identity.
    /// Enabling it is a deliberate trust decision. The default is `false`.
    pub fn allow_all(mut self, allow: bool) -> Self {
        self.allow_all = allow;
        self
    }

    /// The configured executable; a relative path is resolved when spawned.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// The configured installation root, or `None` for Deno's default.
    pub fn install_root(&self) -> Option<&Path> {
        self.install_root.as_deref()
    }

    /// The configured working directory for all commands, or `None` for the
    /// process default.
    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    /// The configured project directory, or `None` for the process default.
    pub fn project_directory(&self) -> Option<&Path> {
        self.project_directory.as_deref()
    }

    /// Whether global installs may pass `--allow-all`.
    pub fn allows_all_permissions(&self) -> bool {
        self.allow_all
    }

    /// Creates a native `deno` command for operations this backend does not
    /// wrap.
    ///
    /// The returned command has the configured executable and no arguments. It
    /// applies the configured working directory but not the project directory,
    /// the global flag or the installation root. Arguments, environment, stdio,
    /// privilege and exit-status handling belong to the caller. This method
    /// does not execute the command.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        if let Some(directory) = &self.working_directory {
            command.current_dir(directory);
        }
        command
    }

    /// Creates a native `deno` command in the configured project directory.
    ///
    /// Arguments, environment, stdio, privilege and exit-status handling
    /// belong to the caller. This method does not execute the command.
    pub fn project_command(&self) -> Command {
        let mut command = self.command();
        if let Some(directory) = &self.project_directory {
            command.current_dir(directory);
        }
        command
    }

    /// Creates the `deno install --global` command for a tool.
    ///
    /// The command includes the configured root, the explicit executable name,
    /// `--allow-all` when configured, and the package specifier or script path.
    /// It never includes `--force`. This method does not execute the command
    /// and does not check whether the tool is already installed.
    pub fn install_command(&self, tool: &GlobalTool) -> Result<Command, DenoError> {
        self.global_install_command(tool, false)
    }

    /// Creates the `deno install --global --force` command for a tool.
    ///
    /// This method does not execute the command. Calling it is the caller's
    /// explicit decision to overwrite an existing launcher.
    pub fn replace_command(&self, tool: &GlobalTool) -> Result<Command, DenoError> {
        self.global_install_command(tool, true)
    }

    fn global_install_command(&self, tool: &GlobalTool, force: bool) -> Result<Command, DenoError> {
        let mut command = self.command();
        command.arg("install").arg("--global");
        self.push_install_root(&mut command)?;
        command.arg("--name").arg(tool.name().as_str());
        if force {
            command.arg("--force");
        }
        if self.allow_all {
            command.arg("--allow-all");
        }
        command.arg(tool.argument());
        Ok(command)
    }

    /// Creates the `deno uninstall --global` command for an executable name.
    ///
    /// The configured root is included when one is set. This method does not
    /// execute the command.
    pub fn uninstall_command(&self, name: &ExecutableName) -> Result<Command, DenoError> {
        let mut command = self.command();
        command.arg("uninstall").arg("--global");
        self.push_install_root(&mut command)?;
        command.arg(name.as_str());
        Ok(command)
    }

    /// Creates the `deno add` command for a project dependency.
    ///
    /// The command runs in the configured project directory and always passes
    /// `--save-exact`, so the manifest records the exact version rather than a
    /// caret range. This method does not execute the command and does not read
    /// or edit the manifest itself.
    pub fn add_command(&self, package: &RegistryPackage, section: DependencySection) -> Command {
        let mut command = self.project_command();
        command.arg("add");
        if let Some(flag) = section.flag() {
            command.arg(flag);
        }
        command.arg("--save-exact").arg(package.specifier());
        command
    }

    /// Creates the `deno remove` command for a project dependency.
    ///
    /// The command runs in the configured project directory. This method does
    /// not execute the command.
    pub fn remove_command(&self, name: &PackageName) -> Command {
        let mut command = self.project_command();
        command.arg("remove").arg(name.as_str());
        command
    }

    /// Creates the `deno install` command for the configured project.
    ///
    /// This method does not execute the command and does not add a lockfile or
    /// lifecycle-script flag.
    pub fn install_project_command(&self) -> Command {
        let mut command = self.project_command();
        command.arg("install");
        command
    }

    /// Returns the installation bin directory used by global operations.
    ///
    /// The directory is resolved from the configured root, then from a
    /// non-empty `DENO_INSTALL_ROOT`, then from `$HOME/.deno` on Unix or
    /// `%USERPROFILE%\\.deno` on Windows. `bin` is appended unless the path
    /// already ends with a `bin` component, matching Deno's `--root` rule. A
    /// relative root is joined to the configured working directory, because
    /// that is the directory Deno resolves it in.
    ///
    /// The directory is not created and no process runs. An empty configured
    /// root is [`DenoError::InstallRootInvalid`]. A missing home directory is
    /// [`DenoError::HomeNotSet`].
    pub fn bin_directory(&self) -> Result<PathBuf, DenoError> {
        let root = self.resolve_root()?;
        Ok(if root.ends_with("bin") {
            root
        } else {
            root.join("bin")
        })
    }

    /// Returns the launcher path for an executable name.
    ///
    /// On Unix this is `<bin>/<name>`. On Windows it is `<bin>/<name>.cmd`,
    /// which is the launcher cmd.exe and PowerShell use.
    pub fn launcher_path(&self, name: &ExecutableName) -> Result<PathBuf, DenoError> {
        Ok(launcher_path(&self.bin_directory()?, name))
    }

    /// Checks the installation bin directory for a launcher.
    ///
    /// This is a filesystem check, not a `deno` invocation. It reports whether
    /// the expected launcher entry exists. It does not run the launcher, prove
    /// that its target resolves, or read its recorded version. A missing bin
    /// directory is `false`; a bin path that is not a directory is
    /// [`DenoError::InstallRootInvalid`].
    pub fn is_global_installed(&self, name: &ExecutableName) -> Result<bool, DenoError> {
        let bin = self.bin_directory()?;
        match fs::metadata(&bin) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(DenoError::InstallRootInvalid(bin));
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                if error.kind() == io::ErrorKind::NotADirectory {
                    return Err(DenoError::InstallRootInvalid(bin));
                }
                return Ok(false);
            }
            Err(error) => return Err(DenoError::Io(error)),
        }
        match fs::symlink_metadata(launcher_path(&bin, name)) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(DenoError::Io(error)),
        }
    }

    /// Installs a global tool without overwriting an existing launcher.
    ///
    /// Runs `deno install --global` with the configured root, an explicit
    /// `--name`, and `--allow-all` when enabled. If the expected launcher
    /// already exists, this returns [`DenoError::AlreadyInstalled`] before
    /// Deno starts. Deno has the same refusal, so the preflight makes the
    /// outcome deterministic and avoids a download first.
    ///
    /// A successful exit is not trusted alone: the expected launcher must
    /// exist as a file, otherwise [`DenoError::MissingLauncher`] is returned.
    /// The returned version is the version that was requested. This method
    /// does not read Deno's success text or inspect the installed source.
    ///
    /// Standard input is closed, so an authentication prompt fails instead of
    /// blocking. Standard output and standard error are captured; Deno writes
    /// its progress and success messages to standard error. A non-zero exit
    /// becomes [`DenoError::Exit`] with both streams.
    pub fn install_global(&self, tool: &GlobalTool) -> Result<InstalledExecutable, DenoError> {
        self.install_global_inner(tool, false)
    }

    /// Installs a global tool and overwrites an existing installation.
    ///
    /// This is [`install_global`](Deno::install_global) with `--force`. The
    /// existing launcher and its metadata directory are replaced. There is no
    /// backup. Calling this method is the explicit decision to overwrite.
    pub fn replace_global(&self, tool: &GlobalTool) -> Result<InstalledExecutable, DenoError> {
        self.install_global_inner(tool, true)
    }

    fn install_global_inner(
        &self,
        tool: &GlobalTool,
        force: bool,
    ) -> Result<InstalledExecutable, DenoError> {
        let launcher = self.launcher_path(tool.name())?;
        if !force && self.is_global_installed(tool.name())? {
            return Err(DenoError::AlreadyInstalled {
                name: tool.name().clone(),
                launcher,
            });
        }
        let command = if force {
            self.replace_command(tool)?
        } else {
            self.install_command(tool)?
        };
        run_status(command)?;
        match fs::metadata(&launcher) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                return Err(DenoError::MissingLauncher {
                    name: tool.name().clone(),
                    launcher,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(DenoError::MissingLauncher {
                    name: tool.name().clone(),
                    launcher,
                });
            }
            Err(error) => return Err(DenoError::Io(error)),
        }
        Ok(InstalledExecutable {
            name: tool.name().clone(),
            requested_version: tool.version().map(str::to_owned),
            launcher,
        })
    }

    /// Removes a global tool.
    ///
    /// Runs `deno uninstall --global` with the configured root. Deno removes
    /// the launcher and its `.name` metadata directory. A name that is not
    /// installed is an error from Deno, [`DenoError::Exit`], not a silent
    /// success. The process receives no standard input and its output is
    /// captured. This method does not edit `PATH`; it never added the directory.
    pub fn uninstall_global(&self, name: &ExecutableName) -> Result<(), DenoError> {
        run_status(self.uninstall_command(name)?)
    }

    /// Adds a project dependency at an exact version.
    ///
    /// Runs `deno add [--dev|--save-optional] --save-exact` in the configured
    /// project directory. The exact version is written to the project's
    /// configuration by Deno. In a `deno.json` project Deno records the entry
    /// under `imports` even when a section flag is passed; the flag only
    /// changes a `package.json` project. The process receives no standard
    /// input and its output is captured.
    pub fn add_project(
        &self,
        package: &RegistryPackage,
        section: DependencySection,
    ) -> Result<(), DenoError> {
        run_status(self.add_command(package, section))
    }

    /// Removes a project dependency.
    ///
    /// Runs `deno remove` in the configured project directory. Deno removes
    /// the dependency from the project's configuration. The provider does not
    /// edit the manifest itself and does not prune the module cache.
    pub fn remove_project(&self, name: &PackageName) -> Result<(), DenoError> {
        run_status(self.remove_command(name))
    }

    /// Installs the configured project's dependencies.
    ///
    /// Runs plain `deno install` in the configured project directory. Deno
    /// reads the project's own configuration and lockfile and populates its
    /// cache and any `node_modules` directory the configuration selects. This
    /// provider does not pass `--frozen-lockfile` and does not approve npm
    /// lifecycle scripts; Deno 2 requires an explicit `--allow-scripts` for
    /// those. Use [`project_command`](Deno::project_command) for either option.
    ///
    /// The process receives no standard input and its output is captured.
    pub fn install_project(&self) -> Result<(), DenoError> {
        run_status(self.install_project_command())
    }

    fn push_install_root(&self, command: &mut Command) -> Result<(), DenoError> {
        if let Some(root) = &self.install_root {
            if root.as_os_str().is_empty() {
                return Err(DenoError::InstallRootInvalid(root.clone()));
            }
            command.arg("--root").arg(root);
        }
        Ok(())
    }

    fn resolve_root(&self) -> Result<PathBuf, DenoError> {
        let root = if let Some(root) = &self.install_root {
            if root.as_os_str().is_empty() {
                return Err(DenoError::InstallRootInvalid(root.clone()));
            }
            root.clone()
        } else if let Some(root) = env::var_os("DENO_INSTALL_ROOT")
            && !root.is_empty()
        {
            PathBuf::from(root)
        } else {
            let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
            let home = env::var_os(key).ok_or(DenoError::HomeNotSet)?;
            PathBuf::from(home).join(".deno")
        };
        Ok(match &self.working_directory {
            Some(base) if !root.is_absolute() => base.join(root),
            _ => root,
        })
    }
}

fn launcher_path(bin: &Path, name: &ExecutableName) -> PathBuf {
    launcher_path_for(bin, name, cfg!(windows))
}

fn launcher_path_for(bin: &Path, name: &ExecutableName, windows: bool) -> PathBuf {
    let path = bin.join(name.as_str());
    if windows {
        path.with_extension("cmd")
    } else {
        path
    }
}

fn run_status(command: Command) -> Result<(), DenoError> {
    let output = run_captured(command)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(DenoError::Exit {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

fn run_captured(command: Command) -> Result<Output, DenoError> {
    capture::run(command).map_err(|error| match error {
        CaptureError::Io(error) => DenoError::Io(error),
        CaptureError::OutputTooLarge { stream, limit } => {
            DenoError::OutputTooLarge { stream, limit }
        }
    })
}

/// A global tool that this provider installed successfully.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledExecutable {
    /// The validated executable name.
    pub name: ExecutableName,
    /// The exact version that was requested, or `None` for a script. This is
    /// not read back from the installed artifact.
    pub requested_version: Option<String>,
    /// The launcher path the provider verified after the install.
    pub launcher: PathBuf,
}

/// Failure invoking Deno or inspecting an installation.
#[derive(Debug)]
pub enum DenoError {
    /// The process could not be started or its output could not be read.
    Io(io::Error),
    /// Deno returned an unsuccessful exit status.
    Exit {
        /// The process exit status.
        status: ExitStatus,
        /// Captured standard output.
        stdout: Vec<u8>,
        /// Captured standard error. Deno writes progress, success and error
        /// messages here.
        stderr: Vec<u8>,
    },
    /// A stream exceeded the capture limit, so the child was killed.
    OutputTooLarge {
        /// `stdout` or `stderr`.
        stream: &'static str,
        /// The per-stream limit in bytes.
        limit: usize,
    },
    /// The installation root is empty, or the computed bin path is not a
    /// directory.
    InstallRootInvalid(PathBuf),
    /// No home directory was available to compute Deno's default root.
    HomeNotSet,
    /// The launcher already exists and `--force` was not requested.
    AlreadyInstalled {
        /// The executable name.
        name: ExecutableName,
        /// The existing launcher.
        launcher: PathBuf,
    },
    /// Deno reported success but the expected launcher is absent or is not a
    /// file.
    MissingLauncher {
        /// The executable name.
        name: ExecutableName,
        /// The launcher path that was checked.
        launcher: PathBuf,
    },
}

impl fmt::Display for DenoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "deno process: {error}"),
            Self::Exit { status, stderr, .. } => {
                let stderr = String::from_utf8_lossy(stderr);
                if stderr.trim().is_empty() {
                    write!(f, "deno exited with {status}")
                } else {
                    write!(f, "deno exited with {status}: {}", stderr.trim_end())
                }
            }
            Self::OutputTooLarge { stream, limit } => {
                write!(f, "deno {stream} exceeded the {limit}-byte capture limit")
            }
            Self::InstallRootInvalid(path) => {
                write!(f, "deno installation root is not usable: {}", path.display())
            }
            Self::HomeNotSet => f.write_str(
                "HOME on Unix or USERPROFILE on Windows is not set, so Deno's default installation root cannot be computed",
            ),
            Self::AlreadyInstalled { name, launcher } => write!(
                f,
                "deno executable `{name}` is already installed at {}; use replace_global to overwrite it",
                launcher.display()
            ),
            Self::MissingLauncher { name, launcher } => write!(
                f,
                "deno reported success but the launcher for `{name}` is missing at {}",
                launcher.display()
            ),
        }
    }
}

impl Error for DenoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Exit { .. }
            | Self::OutputTooLarge { .. }
            | Self::InstallRootInvalid(_)
            | Self::HomeNotSet
            | Self::AlreadyInstalled { .. }
            | Self::MissingLauncher { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ExecutableName, launcher_path_for};

    #[test]
    fn launcher_extension_follows_the_platform() {
        let name = ExecutableName::new("example-tool").unwrap();
        assert_eq!(
            launcher_path_for(Path::new("bin"), &name, false),
            Path::new("bin/example-tool")
        );
        assert_eq!(
            launcher_path_for(Path::new("bin"), &name, true),
            Path::new("bin/example-tool.cmd")
        );
    }
}
