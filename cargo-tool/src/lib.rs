//! Explicit process-based Cargo executable installation.
//!
//! This backend installs, removes, lists and searches crates that provide
//! executables. It passes arguments directly to Cargo, starts no shell, and
//! never requests elevation. Installing builds from source and can replace
//! existing executables; the crate README lists the rules Cargo applies.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

use neotales_ospm_core::{
    Install, IsInstalled, List, Package, PackageName, QueryOptions, Remove, Search,
};

/// Failure while invoking Cargo.
#[derive(Debug)]
pub enum CargoError {
    /// The process could not be started or its output could not be read.
    Io(std::io::Error),
    /// The supplied package query is empty when required, or contains a NUL
    /// byte.
    InvalidQuery,
    /// A provider-specific query option is not supported by this operation.
    UnsupportedOption(String),
    /// A provider-specific query option has the wrong type or range.
    InvalidOption {
        /// Option key from [`QueryOptions::extra`].
        key: String,
        /// The accepted type and range, for the error message.
        expected: &'static str,
    },
    /// Cargo's output was not valid UTF-8 or did not match the expected format.
    InvalidOutput,
    /// Cargo returned an unsuccessful exit status.
    CommandFailed {
        /// The process exit status.
        status: ExitStatus,
        /// Captured standard error.
        stderr: Vec<u8>,
    },
}

impl std::fmt::Display for CargoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cargo process: {error}"),
            Self::InvalidQuery => f.write_str("invalid cargo query: empty or contains a NUL byte"),
            Self::UnsupportedOption(key) => write!(f, "unsupported cargo query option: {key}"),
            Self::InvalidOption { key, expected } => {
                write!(f, "invalid cargo query option `{key}`: expected {expected}")
            }
            Self::InvalidOutput => f.write_str("invalid cargo output"),
            Self::CommandFailed { status, stderr } => write!(
                f,
                "cargo exited with {status}: {}",
                String::from_utf8_lossy(stderr)
            ),
        }
    }
}

impl Error for CargoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidQuery
            | Self::UnsupportedOption(_)
            | Self::InvalidOption { .. }
            | Self::InvalidOutput
            | Self::CommandFailed { .. } => None,
        }
    }
}

/// A Cargo installation backed by the `cargo` executable.
///
/// The default constructor resolves `cargo` from `PATH`. The install root is
/// Cargo's own default unless [`with_install_root`](Cargo::with_install_root) is
/// used. Cargo's normal environment and configuration still apply.
#[derive(Debug, Clone)]
pub struct Cargo {
    executable: PathBuf,
    install_root: Option<PathBuf>,
}

impl Default for Cargo {
    fn default() -> Self {
        Self::new()
    }
}

impl Cargo {
    /// Uses `cargo` from the current process's `PATH`.
    pub fn new() -> Self {
        Self::with_executable("cargo")
    }

    /// Uses an explicit executable, useful for a configured installation or a
    /// test fixture.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            install_root: None,
        }
    }

    /// Uses an explicit install root for install, remove and list.
    ///
    /// The path is passed to Cargo as given, so a relative path is resolved
    /// against the caller's working directory. This affects installed
    /// executables and Cargo's install tracking under the root, not Cargo's
    /// registry or build cache.
    pub fn with_install_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.install_root = Some(root.into());
        self
    }

    /// The configured executable; a relative path is resolved when spawned.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// The configured install root, or `None` for Cargo's default.
    pub fn install_root(&self) -> Option<&Path> {
        self.install_root.as_deref()
    }

    /// Creates a native `cargo` command for operations this backend does not
    /// wrap.
    ///
    /// The command inherits the calling process's environment and working
    /// directory until the caller changes them. Arguments, environment, stdio,
    /// the install root, privilege and exit-status handling are entirely the
    /// caller's responsibility. This method does not execute the command.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    fn push_root(&self, command: &mut Command) {
        if let Some(root) = &self.install_root {
            command.arg("--root").arg(root);
        }
    }

    fn run(&self, action: &str, package: &PackageName) -> Result<(), CargoError> {
        let mut command = self.command();
        command.arg(action);
        self.push_root(&mut command);
        command.arg("--").arg(package.as_str());
        self.execute(&mut command)
    }

    fn execute(&self, command: &mut Command) -> Result<(), CargoError> {
        check(
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .output()
                .map_err(CargoError::Io)?,
        )
    }

    fn list_command(&self) -> Command {
        let mut command = self.command();
        command.args(["install", "--list"]);
        self.push_root(&mut command);
        // Parsing below expects one record per crate, without colour escapes.
        command.env("CARGO_TERM_COLOR", "never");
        command
    }
}

impl Install for Cargo {
    type Error = CargoError;

    /// Installs the crate's executables into the configured install root.
    ///
    /// Installing builds the crate and runs its build script with the calling
    /// user's permissions. Cargo selects the registry version from its own
    /// configuration. When the version or source, the binary names, the
    /// feature set, the profile, or the target differs from Cargo's tracking
    /// record, Cargo rebuilds and replaces the executables. When they match,
    /// Cargo leaves the installation alone and reports success. This backend
    /// exposes no force, version, source, feature, profile, or target option,
    /// so it cannot promise that an existing installation is unchanged; use
    /// [`command`](Cargo::command) for those operations.
    fn install(&self, package: &PackageName) -> Result<(), Self::Error> {
        self.run("install", package)
    }
}

impl Remove for Cargo {
    type Error = CargoError;

    /// Removes the crate's installed executables and tracking entry from the
    /// configured install root. Cargo's cache and registry data are not
    /// removed.
    fn remove(&self, package: &PackageName) -> Result<(), Self::Error> {
        self.run("uninstall", package)
    }
}

impl List for Cargo {
    type Error = CargoError;

    /// Lists installed executable crates.
    ///
    /// `query` matches a substring of the crate name; an empty query returns
    /// every installed crate. `options.version` must equal the installed
    /// version exactly. `options.extra` must be empty because Cargo's install
    /// listing has no provider options.
    fn list(&self, query: &str, options: &QueryOptions) -> Result<Vec<Package>, Self::Error> {
        reject_extra(options)?;
        if query.contains('\0') {
            return Err(CargoError::InvalidQuery);
        }
        let output = text_output(&mut self.list_command())?;
        let mut packages = parse_installed(&output)?;
        packages.retain(|package| {
            package.name.contains(query)
                && options
                    .version
                    .as_ref()
                    .is_none_or(|wanted| &package.version == wanted)
        });
        Ok(packages)
    }
}

impl IsInstalled for Cargo {
    type Error = CargoError;

    /// Checks Cargo's install tracking for an exact crate name.
    fn is_installed(&self, package: &PackageName) -> Result<bool, Self::Error> {
        Ok(self
            .list("", &QueryOptions::default())?
            .iter()
            .any(|installed| installed.name == package.as_str()))
    }
}

impl Search for Cargo {
    type Error = CargoError;

    /// Searches the configured registry for crate names and descriptions.
    ///
    /// `query` is required and is passed to Cargo as one argument. The optional
    /// `limit` key in `options.extra` must be a `u32` from 1 to 100 and becomes
    /// Cargo's `--limit`. `options.version` filters the single current version
    /// Cargo reports for each crate; it does not enumerate published versions.
    fn search(&self, query: &str, options: &QueryOptions) -> Result<Vec<Package>, Self::Error> {
        let limit = search_limit(options)?;
        if query.is_empty() || query.contains('\0') {
            return Err(CargoError::InvalidQuery);
        }
        let mut command = self.command();
        command.arg("search");
        if let Some(limit) = limit {
            command.arg("--limit").arg(limit.to_string());
        }
        command.arg("--").arg(query);
        command.env("CARGO_TERM_COLOR", "never");
        let output = text_output(&mut command)?;
        let mut packages = parse_search(&output)?;
        if let Some(wanted) = &options.version {
            packages.retain(|package| &package.version == wanted);
        }
        Ok(packages)
    }
}

fn check(output: Output) -> Result<(), CargoError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(CargoError::CommandFailed {
            status: output.status,
            stderr: output.stderr,
        })
    }
}

fn text_output(command: &mut Command) -> Result<String, CargoError> {
    let output = command.output().map_err(CargoError::Io)?;
    if !output.status.success() {
        return Err(CargoError::CommandFailed {
            status: output.status,
            stderr: output.stderr,
        });
    }
    String::from_utf8(output.stdout).map_err(|_| CargoError::InvalidOutput)
}

const LIMIT_EXPECTED: &str = "a u32 from 1 to 100";

fn reject_extra(options: &QueryOptions) -> Result<(), CargoError> {
    match options.extra.keys().next() {
        Some(key) => Err(CargoError::UnsupportedOption(key.clone())),
        None => Ok(()),
    }
}

fn search_limit(options: &QueryOptions) -> Result<Option<u32>, CargoError> {
    let mut limit = None;
    for (key, value) in &options.extra {
        if key != "limit" {
            return Err(CargoError::UnsupportedOption(key.clone()));
        }
        let value = value
            .downcast_ref::<u32>()
            .ok_or_else(|| CargoError::InvalidOption {
                key: key.clone(),
                expected: LIMIT_EXPECTED,
            })?;
        if !(1..=100).contains(value) {
            return Err(CargoError::InvalidOption {
                key: key.clone(),
                expected: LIMIT_EXPECTED,
            });
        }
        limit = Some(*value);
    }
    Ok(limit)
}

/// Parses `cargo install --list` output.
///
/// Records are unindented `name vVERSION:` lines. Indented binary names are
/// ignored, and a path or git source after the version is ignored.
fn parse_installed(output: &str) -> Result<Vec<Package>, CargoError> {
    let mut packages = Vec::new();
    for line in output.lines() {
        if line.is_empty() || line.starts_with([' ', '\t']) {
            continue;
        }
        let record = line.strip_suffix(':').ok_or(CargoError::InvalidOutput)?;
        let (name, rest) = record.split_once(' ').ok_or(CargoError::InvalidOutput)?;
        let version = rest
            .strip_prefix('v')
            .and_then(|rest| rest.split_whitespace().next())
            .filter(|version| !version.is_empty())
            .ok_or(CargoError::InvalidOutput)?;
        if name.is_empty() {
            return Err(CargoError::InvalidOutput);
        }
        PackageName::new(name).map_err(|_| CargoError::InvalidOutput)?;
        packages.push(Package {
            name: name.into(),
            version: version.into(),
            metadata: Default::default(),
        });
    }
    Ok(packages)
}

/// Parses `cargo search` output.
///
/// Each result is `name = "version"` followed by an optional description. The
/// closing count and the `note:` footer are skipped.
fn parse_search(output: &str) -> Result<Vec<Package>, CargoError> {
    let mut packages = Vec::new();
    for line in output.lines() {
        if line.is_empty() || line.starts_with("...") || line.starts_with("note:") {
            continue;
        }
        let (name, rest) = line.split_once(" = \"").ok_or(CargoError::InvalidOutput)?;
        let (version, _) = rest.split_once('"').ok_or(CargoError::InvalidOutput)?;
        if name.is_empty() || version.is_empty() {
            return Err(CargoError::InvalidOutput);
        }
        PackageName::new(name).map_err(|_| CargoError::InvalidOutput)?;
        packages.push(Package {
            name: name.into(),
            version: version.into(),
            metadata: Default::default(),
        });
    }
    Ok(packages)
}
