//! Explicit process backend for installing Go commands.
//!
//! [`GoCommand`] validates a fully qualified module command and a complete
//! module version. [`Go::install`] runs `go install` with one version-suffixed
//! argument, without a shell and without elevating privileges.
//!
//! The crate deliberately does not implement the portable package-manager
//! traits. A Go import path is not a portable package name: the portable
//! `PackageName` type rejects `/`. `go install` also maintains no queryable
//! installed-package database, so the crate does not report installed
//! versions, list commands, or remove them.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

/// A Go toolchain invoked as an external process.
///
/// The default executable is `go` from `PATH`. The process inherits the
/// caller's environment, so `GOBIN`, `GOPATH`, `GOPROXY`, and `GOFLAGS` keep
/// their usual meaning. This type never invokes a shell or elevates
/// privileges.
#[derive(Clone, Debug)]
pub struct Go {
    executable: PathBuf,
}

impl Default for Go {
    fn default() -> Self {
        Self::new()
    }
}

impl Go {
    /// Uses `go` from the current process's `PATH`.
    pub fn new() -> Self {
        Self::with_executable("go")
    }

    /// Uses an explicit executable, useful for configured toolchains or tests.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    /// The configured executable; a relative path is resolved when spawned.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Creates a native `go` command for caller-specific operations.
    ///
    /// Arguments, environment, stdio, and exit-status handling belong to the
    /// caller. This method does not execute the command.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    /// Creates a native `go install module@version` command.
    ///
    /// The returned command is not executed, so a caller can set the working
    /// directory, environment, or output handling before spawning it.
    pub fn command_for(&self, command: &GoCommand) -> Command {
        let argument = command.argument();
        let mut process = self.command();
        process.arg("install").arg(argument);
        process
    }

    /// Installs the command with an explicit module version.
    ///
    /// Runs `go install module@version` and captures its output. The process
    /// receives no interactive standard input, so a credential prompt fails
    /// instead of blocking. A non-zero exit becomes [`GoError::Exit`] with the
    /// captured standard error. The crate does not retry, elevate, or alter the
    /// caller's environment.
    pub fn install(&self, command: &GoCommand) -> Result<(), GoError> {
        let output = self.command_for(command).output().map_err(GoError::Io)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(GoError::Exit {
                status: output.status,
                stderr: output.stderr,
            })
        }
    }
}

/// A fully qualified Go module command and an explicit module version.
///
/// The module path is the import path of the package to install, including a
/// domain-like first element and any `/vN` major-version suffix. The version
/// is a complete semantic version: a release such as `v1.2.3`, a pseudo-version,
/// or a release with `+incompatible`. Queries such as `latest`, branch names,
/// and abbreviated versions are rejected because they do not identify an exact
/// version.
///
/// The module path must name one command. A `...` wildcard is rejected, and
/// each element follows Go's module path rules: no leading or trailing dot, no
/// Windows-reserved name (case-insensitively, even with an extension), and no
/// Windows short-name-like suffix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoCommand {
    module_path: String,
    version: String,
}

impl GoCommand {
    /// Validates a module command and the exact version to install.
    ///
    /// The module path must start with a dotted, lowercase first element
    /// (normally a domain), use `/` separators, name one command rather than a
    /// `...` pattern, and contain only elements that Go accepts in module
    /// paths. The version must be a complete semantic version beginning with
    /// `v`.
    pub fn new(
        module_path: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, InvalidCommand> {
        let module_path = module_path.into();
        let version = version.into();
        validate_module_path(&module_path)?;
        validate_version(&version)?;
        Ok(Self {
            module_path,
            version,
        })
    }

    /// The fully qualified module command path.
    pub fn module_path(&self) -> &str {
        &self.module_path
    }

    /// The exact module version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The single `module@version` argument passed to `go install`.
    pub fn argument(&self) -> String {
        format!("{}@{}", self.module_path, self.version)
    }
}

/// A module command or version that cannot be installed safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidCommand {
    /// The module path is empty, contains a `...` wildcard, starts with `-` or
    /// `/`, has an empty element, an element that starts or ends with `.`, a
    /// Windows-reserved element name, a Windows short-name-like suffix, or an
    /// unsupported character.
    ModulePath,
    /// The module path's first element does not contain a dot, so it is not a
    /// fully qualified module path.
    UnqualifiedModulePath,
    /// The version is not a complete semantic version beginning with `v`.
    Version,
}

impl fmt::Display for InvalidCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModulePath => f.write_str("module path must name one supported command"),
            Self::UnqualifiedModulePath => {
                f.write_str("module path first element must contain a dot")
            }
            Self::Version => {
                f.write_str("version must be a complete Go module version such as v1.2.3")
            }
        }
    }
}

impl Error for InvalidCommand {}

/// Failure invoking the Go toolchain.
#[derive(Debug)]
pub enum GoError {
    /// The process could not be started or its output could not be read.
    Io(std::io::Error),
    /// The Go toolchain returned an unsuccessful status.
    Exit {
        /// The process exit status.
        status: ExitStatus,
        /// Captured standard error, which may contain build diagnostics.
        stderr: Vec<u8>,
    },
}

impl fmt::Display for GoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "go process: {error}"),
            Self::Exit { status, stderr } => write!(
                f,
                "go exited with {status}: {}",
                String::from_utf8_lossy(stderr).trim_end()
            ),
        }
    }
}

impl Error for GoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Exit { .. } => None,
        }
    }
}

/// Accepts the conservative subset of Go module paths that can be passed
/// directly to `go install` without a shell or flag ambiguity.
///
/// Go treats `...` as a package pattern. The provider installs one command, so
/// a path containing that wildcard is rejected. Element rules follow Go's
/// `x/mod/module` checks for module paths, including the Windows-reserved and
/// short-name rules that keep the module cache usable on Windows.
fn validate_module_path(path: &str) -> Result<(), InvalidCommand> {
    if path.contains("...") {
        return Err(InvalidCommand::ModulePath);
    }
    if path.is_empty() || path.starts_with('-') || path.starts_with('/') || path.ends_with('/') {
        return Err(InvalidCommand::ModulePath);
    }

    let mut elements = path.split('/');
    let first = elements.next().unwrap_or_default();
    validate_module_element(first)?;
    if !first.contains('.') {
        return Err(InvalidCommand::UnqualifiedModulePath);
    }
    if !first.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
    }) {
        return Err(InvalidCommand::ModulePath);
    }

    for element in elements {
        validate_module_element(element)?;
    }

    Ok(())
}

/// Applies the per-element checks shared by every part of a module path.
///
/// The rules mirror `x/mod/module` for module paths: an element is non-empty,
/// does not start or end with a dot, uses only module-path characters, does not
/// use a Windows-reserved name before its first dot, and does not look like a
/// Windows short name.
fn validate_module_element(element: &str) -> Result<(), InvalidCommand> {
    if element.is_empty() || element.starts_with('.') || element.ends_with('.') {
        return Err(InvalidCommand::ModulePath);
    }
    if !element
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b".-_~".contains(&byte))
    {
        return Err(InvalidCommand::ModulePath);
    }

    // Windows ignores an extension when deciding whether a name is reserved,
    // so Go compares the part before the first dot, case-insensitively.
    let short = element.split_once('.').map_or(element, |(short, _)| short);
    if WINDOWS_RESERVED_NAMES
        .iter()
        .any(|reserved| short.eq_ignore_ascii_case(reserved))
    {
        return Err(InvalidCommand::ModulePath);
    }

    // A trailing tilde and digits looks like a Windows short name.
    if let Some(tilde) = short.rfind('~') {
        let suffix = &short[tilde + 1..];
        if !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(InvalidCommand::ModulePath);
        }
    }

    Ok(())
}

/// Windows device names that Go rejects in module path elements.
const WINDOWS_RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Accepts a complete semantic version in Go's canonical `v` form.
///
/// Build metadata is limited to `+incompatible`, the only form Go uses for
/// module versions.
fn validate_version(version: &str) -> Result<(), InvalidCommand> {
    let Some(rest) = version.strip_prefix('v') else {
        return Err(InvalidCommand::Version);
    };
    let (rest, build) = match rest.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (rest, None),
    };
    if build.is_some_and(|build| build != "incompatible") {
        return Err(InvalidCommand::Version);
    }
    let (core, prerelease) = match rest.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (rest, None),
    };

    let mut numbers = core.split('.');
    for _ in 0..3 {
        let Some(number) = numbers.next() else {
            return Err(InvalidCommand::Version);
        };
        if !is_numeric_identifier(number) {
            return Err(InvalidCommand::Version);
        }
    }
    if numbers.next().is_some() {
        return Err(InvalidCommand::Version);
    }

    if let Some(prerelease) = prerelease {
        if prerelease.is_empty() {
            return Err(InvalidCommand::Version);
        }
        for identifier in prerelease.split('.') {
            if identifier.is_empty()
                || !identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(InvalidCommand::Version);
            }
            if identifier.bytes().all(|byte| byte.is_ascii_digit())
                && identifier.len() > 1
                && identifier.starts_with('0')
            {
                return Err(InvalidCommand::Version);
            }
        }
    }

    Ok(())
}

fn is_numeric_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
}
