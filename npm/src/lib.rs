//! Explicit process backend for npm global packages.
//!
//! This backend installs, removes, lists and searches packages in npm's
//! global prefix. On Unix it passes arguments directly to `npm` and starts no
//! shell. On Windows the default `npm.cmd` is a batch-file shim that the
//! platform starts through its command interpreter; that path is not validated
//! by this crate, so Windows builds reject version text, search queries, and
//! prefixes containing `cmd.exe` metacharacters before npm starts. Captured
//! commands run in their own process group on Unix or job object on Windows,
//! so an output overflow also terminates descendants. The backend never
//! requests elevation. Node is a runtime, not a package manager: runtime
//! discovery is separate through [`NodeRuntime`].
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

use neotales_ospm_core::{
    Install, IsInstalled, List, Package, PackageName, QueryOptions, Remove, Search,
};

mod capture;
mod json;
mod node;

pub use node::{NodeError, NodeRuntime};

use capture::CaptureError;
use json::Json;

/// An npm global-package installation backed by the `npm` executable.
///
/// The default constructor resolves `npm` from `PATH` (`npm.cmd` on Windows).
/// The process inherits the caller's environment, so npm configuration, the
/// registry, the cache, and proxy settings keep their usual meaning. On Unix
/// this type starts npm directly and never builds a shell command line. On
/// Windows the standard npm command is a batch file, which the platform starts
/// through `cmd.exe`; see [`NpmPackage`] and [`Search`] for the argument
/// restrictions that path requires. This type never elevates privileges.
/// Commands are captured with a per-stream size limit and run in their own
/// process group or job object, so an output overflow also terminates
/// descendants.
#[derive(Clone, Debug)]
pub struct Npm {
    executable: PathBuf,
    prefix: Option<PathBuf>,
    allow_scripts: bool,
}

impl Default for Npm {
    fn default() -> Self {
        Self::new()
    }
}

impl Npm {
    /// Uses npm from the current process's `PATH`.
    ///
    /// The executable is `npm` on Unix and `npm.cmd` on Windows, where the
    /// standard npm command is a batch-file shim. See the type documentation
    /// for the Windows argument restrictions.
    pub fn new() -> Self {
        if cfg!(windows) {
            Self::with_executable("npm.cmd")
        } else {
            Self::with_executable("npm")
        }
    }

    /// Uses an explicit executable, useful for a configured installation or a
    /// test fixture.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            prefix: None,
            allow_scripts: false,
        }
    }

    /// Uses an explicit global prefix for install, remove and list.
    ///
    /// The path is passed to npm as given, so a relative path is resolved
    /// against the caller's working directory. npm places packages and
    /// executables according to the platform's prefix layout: `<prefix>/lib/node_modules`
    /// on Unix and `<prefix>/node_modules` on Windows. A prefix does not change
    /// npm's cache, registry, or network configuration.
    ///
    /// [`List`] treats a prefix whose expected package directory is absent as
    /// an empty package list. Any other npm failure is returned, and an unsafe
    /// prefix on Windows is rejected before npm starts.
    pub fn with_prefix(mut self, prefix: impl Into<PathBuf>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Allows npm lifecycle scripts to run during install.
    ///
    /// Scripts are disabled by default because `preinstall`, `install` and
    /// `postinstall` run package code with the calling user's permissions.
    /// Enabling them is a deliberate trust decision.
    pub fn allow_scripts(mut self, allow: bool) -> Self {
        self.allow_scripts = allow;
        self
    }

    /// The configured executable; a relative path is resolved when spawned.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// The configured global prefix, or `None` for npm's default.
    pub fn prefix(&self) -> Option<&Path> {
        self.prefix.as_deref()
    }

    /// Whether install may run npm lifecycle scripts.
    pub fn scripts_allowed(&self) -> bool {
        self.allow_scripts
    }

    /// Creates a native `npm` command for operations this backend does not
    /// wrap.
    ///
    /// The returned command has the configured executable and no arguments.
    /// It does not include the global flag or the install prefix. Arguments,
    /// environment, stdio, privilege and exit-status handling belong to the
    /// caller. This method does not execute the command.
    ///
    /// The caller also owns platform argument escaping. On Windows npm is
    /// normally a batch file, so arguments that this backend would reject for
    /// Windows need the caller's own checks.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    fn push_prefix(&self, command: &mut Command) {
        if let Some(prefix) = &self.prefix {
            command.arg("--prefix").arg(prefix);
        }
    }

    /// Rejects an empty prefix on every platform and a Windows prefix that this
    /// crate cannot pass safely through the npm batch shim.
    fn check_prefix_arguments(&self) -> Result<(), NpmError> {
        let Some(prefix) = &self.prefix else {
            return Ok(());
        };
        if prefix.as_os_str().is_empty() {
            return Err(NpmError::PrefixInvalid(prefix.clone()));
        }
        if cfg!(windows) {
            let Some(prefix) = prefix.to_str() else {
                return Err(NpmError::UnsafeWindowsArgument("prefix"));
            };
            if !windows_argument_is_safe(prefix) {
                return Err(NpmError::UnsafeWindowsArgument("prefix"));
            }
        }
        Ok(())
    }

    /// Returns `true` when the configured prefix cannot contain global
    /// packages because its exact expected package directory is absent.
    ///
    /// A missing prefix and a prefix with no layout directory are empty
    /// package sets. A layout that exists but is not a directory is an error,
    /// not an empty list, and returns [`NpmError::PrefixInvalid`]. An existing
    /// package directory is listed by npm, so any npm error after this check is
    /// returned to the caller.
    fn prefix_has_no_packages(&self) -> Result<bool, NpmError> {
        let Some(prefix) = &self.prefix else {
            return Ok(false);
        };
        match path_state(prefix)? {
            PathState::Missing => return Ok(true),
            PathState::Directory => {}
            PathState::Other | PathState::AncestorNotDirectory => {
                return Err(NpmError::PrefixInvalid(prefix.clone()));
            }
        }
        let package_root = global_modules_dir(prefix);
        match path_state(&package_root)? {
            PathState::Directory => Ok(false),
            PathState::Other => Err(NpmError::PrefixInvalid(package_root)),
            PathState::Missing | PathState::AncestorNotDirectory => {
                if !cfg!(windows) {
                    let lib = prefix.join("lib");
                    match path_state(&lib)? {
                        PathState::Other | PathState::AncestorNotDirectory => {
                            return Err(NpmError::PrefixInvalid(lib));
                        }
                        PathState::Missing | PathState::Directory => {}
                    }
                }
                Ok(true)
            }
        }
    }

    /// Confirms that the configured npm executable can be started.
    ///
    /// A missing prefix is an empty package set, but that must not hide a
    /// missing or broken npm. `npm --version` is used because it needs no
    /// registry access and no project.
    fn verify_backend(&self) -> Result<(), NpmError> {
        let mut command = self.command();
        command.arg("--version");
        run_status(command)
    }

    fn list_command(&self) -> Command {
        let mut command = self.command();
        command.args(["ls", "--global", "--json", "--depth=0"]);
        self.push_prefix(&mut command);
        command
    }

    fn search_command(&self, query: &str, limit: Option<u32>) -> Command {
        let mut command = self.command();
        command.args(["search", "--json"]);
        if let Some(limit) = limit {
            command.arg(format!("--searchlimit={limit}"));
        }
        command.arg(query);
        command
    }

    /// Installs one package into the global prefix.
    ///
    /// `package` carries the name and optional version, range, or dist-tag.
    /// npm resolves the request against its configured registry. Lifecycle
    /// scripts are ignored unless [`allow_scripts`](Npm::allow_scripts)
    /// enabled them. A configured prefix is passed as `--prefix`; otherwise
    /// npm's global prefix applies. The process receives no interactive
    /// standard input, and its output is captured.
    pub fn install_package(&self, package: &NpmPackage) -> Result<(), NpmError> {
        self.check_prefix_arguments()?;
        let mut command = self.command();
        command.args(["install", "--global"]);
        self.push_prefix(&mut command);
        command.args(["--no-audit", "--no-fund", "--progress=false"]);
        if !self.allow_scripts {
            command.arg("--ignore-scripts");
        }
        command.arg(package.argument());
        run_status(command)
    }

    /// Removes one package from the global prefix.
    ///
    /// Only the package name is passed; a version or range on `package` is
    /// ignored. npm removes the package it has installed under that name.
    pub fn remove_package(&self, package: &NpmPackage) -> Result<(), NpmError> {
        self.check_prefix_arguments()?;
        let mut command = self.command();
        command.args(["uninstall", "--global"]);
        self.push_prefix(&mut command);
        command.args(["--no-audit", "--no-fund", "--progress=false"]);
        command.arg(package.name());
        run_status(command)
    }

    /// Checks whether the global prefix contains the package's name.
    ///
    /// A version or range on `package` is ignored because npm reports the
    /// installed version and this provider does not evaluate ranges.
    pub fn is_package_installed(&self, package: &NpmPackage) -> Result<bool, NpmError> {
        Ok(self
            .list("", &QueryOptions::default())?
            .iter()
            .any(|installed| installed.name == package.name()))
    }
}

impl Install for Npm {
    type Error = NpmError;

    /// Installs the package at npm's default dist-tag.
    ///
    /// The portable [`PackageName`] cannot carry a scoped name. Use
    /// [`Npm::install_package`] for a scoped package, an exact version, or a
    /// range.
    fn install(&self, package: &PackageName) -> Result<(), Self::Error> {
        let package = NpmPackage::new(package.as_str()).map_err(NpmError::InvalidPackage)?;
        self.install_package(&package)
    }
}

impl Remove for Npm {
    type Error = NpmError;

    /// Removes the package by its portable name.
    ///
    /// The portable [`PackageName`] cannot carry a scoped name. Use
    /// [`Npm::remove_package`] for a scoped package.
    fn remove(&self, package: &PackageName) -> Result<(), Self::Error> {
        let package = NpmPackage::new(package.as_str()).map_err(NpmError::InvalidPackage)?;
        self.remove_package(&package)
    }
}

impl IsInstalled for Npm {
    type Error = NpmError;

    /// Checks npm's global package list for an exact name.
    ///
    /// The portable [`PackageName`] cannot carry a scoped name. Use
    /// [`Npm::is_package_installed`] for a scoped name.
    fn is_installed(&self, package: &PackageName) -> Result<bool, Self::Error> {
        Ok(self
            .list("", &QueryOptions::default())?
            .iter()
            .any(|installed| installed.name == package.as_str()))
    }
}

impl List for Npm {
    type Error = NpmError;

    /// Lists installed global packages.
    ///
    /// `query` matches a substring of the package name; an empty query returns
    /// every installed package. `options.version` must equal the installed
    /// version exactly. `options.extra` must be empty because npm's global
    /// listing has no provider options.
    fn list(&self, query: &str, options: &QueryOptions) -> Result<Vec<Package>, Self::Error> {
        reject_extra(options)?;
        if query.contains('\0') {
            return Err(NpmError::InvalidQuery);
        }
        self.check_prefix_arguments()?;
        if self.prefix_has_no_packages()? {
            self.verify_backend()?;
            return Ok(Vec::new());
        }
        let output = run_captured(self.list_command())?;
        let value = match Json::parse(&output.stdout) {
            Ok(value) => value,
            Err(_) if !output.status.success() => {
                return Err(NpmError::CommandFailed {
                    status: output.status,
                    stderr: output.stderr,
                });
            }
            Err(_) => return Err(NpmError::InvalidOutput),
        };
        if let Some((code, summary)) = reported_error(&value) {
            return Err(NpmError::Reported {
                status: output.status,
                code,
                summary,
            });
        }
        if !output.status.success() {
            return Err(NpmError::CommandFailed {
                status: output.status,
                stderr: output.stderr,
            });
        }
        let mut packages = parse_installed(&value)?;
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

impl Search for Npm {
    type Error = NpmError;

    /// Searches the configured npm registry.
    ///
    /// `query` is required and is passed to npm as one argument. The optional
    /// `limit` key in `options.extra` must be a `u32` from 1 to 250 and becomes
    /// npm's `--searchlimit`. `options.version` filters the current version npm
    /// reports for each result; it does not enumerate published versions.
    ///
    /// On Windows the query is limited to ASCII letters, digits, and
    /// ` .-_~@/+` because npm is normally a batch file and this crate cannot
    /// validate `cmd.exe` argument escaping.
    fn search(&self, query: &str, options: &QueryOptions) -> Result<Vec<Package>, Self::Error> {
        let limit = search_limit(options)?;
        validate_search_query(query)?;
        let output = run_captured(self.search_command(query, limit))?;
        let value = match Json::parse(&output.stdout) {
            Ok(value) => value,
            Err(_) if !output.status.success() => {
                return Err(NpmError::CommandFailed {
                    status: output.status,
                    stderr: output.stderr,
                });
            }
            Err(_) => return Err(NpmError::InvalidOutput),
        };
        if let Some((code, summary)) = reported_error(&value) {
            return Err(NpmError::Reported {
                status: output.status,
                code,
                summary,
            });
        }
        if !output.status.success() {
            return Err(NpmError::CommandFailed {
                status: output.status,
                stderr: output.stderr,
            });
        }
        let mut packages = parse_search(&value)?;
        if let Some(wanted) = &options.version {
            packages.retain(|package| &package.version == wanted);
        }
        Ok(packages)
    }
}

/// A validated npm package request.
///
/// The name is either `name` or `@scope/name`. The optional version is an exact
/// version, a range, or a dist-tag. This type does not resolve the request; npm
/// does that against its configured registry.
///
/// On Windows npm is normally a batch file. Names use only characters that are
/// not `cmd.exe` metacharacters, but range expressions contain them, so Windows
/// builds accept only versions made of ASCII letters, digits, `.`, `-`, and
/// `+`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NpmPackage {
    name: String,
    version: Option<String>,
}

impl NpmPackage {
    /// Validates a package name with no version.
    ///
    /// npm installs its default dist-tag when no version is supplied.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidPackageSpec> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self {
            name,
            version: None,
        })
    }

    /// Validates a package name and an exact version, range, or dist-tag.
    ///
    /// The version is passed to npm as text. This type rejects characters that
    /// could act as a path, URL, or flag separator; it does not decide whether
    /// npm's semver parser accepts the expression. On Windows, `cmd.exe`
    /// metacharacters are also rejected, so range expressions with operators
    /// are not accepted there.
    pub fn versioned(
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, InvalidPackageSpec> {
        let name = name.into();
        let version = version.into();
        validate_name(&name)?;
        validate_version(&version)?;
        Ok(Self {
            name,
            version: Some(version),
        })
    }

    /// The validated package name, including a scope when present.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The validated version, range, or dist-tag when one was supplied.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The single `name` or `name@version` argument passed to npm.
    pub fn argument(&self) -> String {
        match &self.version {
            Some(version) => format!("{}@{version}", self.name),
            None => self.name.clone(),
        }
    }
}

/// A name or version that this provider will not pass to npm.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidPackageSpec {
    /// The name is empty, too long, or not `name` or `@scope/name` using
    /// URL-safe characters.
    Name,
    /// The version is empty, too long, or contains a character this provider
    /// does not pass to npm. On Windows, `cmd.exe` metacharacters are rejected,
    /// so range expressions with operators are not accepted.
    Version,
}

impl fmt::Display for InvalidPackageSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name => {
                f.write_str("name must be `name` or `@scope/name` using URL-safe characters")
            }
            Self::Version => f.write_str(
                "version must be an exact version, range, or dist-tag accepted on this platform",
            ),
        }
    }
}

impl Error for InvalidPackageSpec {}

/// Failure invoking npm.
#[derive(Debug)]
pub enum NpmError {
    /// The process could not be started or its output could not be read.
    Io(std::io::Error),
    /// The supplied query is empty when required, starts with `-` for search,
    /// or contains a NUL byte or control character.
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
    /// A portable package name cannot be represented as an npm package.
    InvalidPackage(InvalidPackageSpec),
    /// npm's output was not valid UTF-8 or did not match the expected JSON
    /// shape.
    InvalidOutput,
    /// npm returned an unsuccessful exit status without a structured error.
    CommandFailed {
        /// The process exit status.
        status: ExitStatus,
        /// Captured standard error.
        stderr: Vec<u8>,
    },
    /// npm returned an unsuccessful exit status with a structured JSON error.
    Reported {
        /// The process exit status.
        status: ExitStatus,
        /// npm's error code, such as `ENOENT`.
        code: String,
        /// npm's human-readable summary.
        summary: String,
    },
    /// A stream exceeded the capture limit, so the child was killed.
    OutputTooLarge {
        /// `stdout` or `stderr`.
        stream: &'static str,
        /// The per-stream limit in bytes.
        limit: usize,
    },
    /// The configured prefix could not be inspected.
    Prefix(std::io::Error),
    /// The configured prefix exists but its expected directory layout is not
    /// usable, such as a file where a directory is required.
    PrefixInvalid(PathBuf),
    /// A Windows argument contains a `cmd.exe` metacharacter that this crate
    /// cannot pass through the npm batch shim safely.
    UnsafeWindowsArgument(&'static str),
}

impl fmt::Display for NpmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "npm process: {error}"),
            Self::InvalidQuery => f.write_str(
                "invalid npm query: empty when required, starts with `-` for search, contains a control character, or contains a character this platform does not pass to npm",
            ),
            Self::UnsupportedOption(key) => write!(f, "unsupported npm query option: {key}"),
            Self::InvalidOption { key, expected } => {
                write!(f, "invalid npm query option `{key}`: expected {expected}")
            }
            Self::InvalidPackage(error) => write!(f, "invalid npm package: {error}"),
            Self::InvalidOutput => f.write_str("invalid npm output"),
            Self::CommandFailed { status, stderr } => {
                let stderr = String::from_utf8_lossy(stderr);
                if stderr.trim().is_empty() {
                    write!(f, "npm exited with {status}")
                } else {
                    write!(f, "npm exited with {status}: {}", stderr.trim_end())
                }
            }
            Self::Reported {
                status,
                code,
                summary,
            } => write!(f, "npm exited with {status} ({code}): {summary}"),
            Self::OutputTooLarge { stream, limit } => {
                write!(f, "npm {stream} exceeded the {limit}-byte capture limit")
            }
            Self::Prefix(error) => write!(f, "npm prefix: {error}"),
            Self::PrefixInvalid(path) => {
                write!(f, "npm prefix is not a usable directory: {}", path.display())
            }
            Self::UnsafeWindowsArgument(argument) => write!(
                f,
                "unsafe Windows argument for {argument}: contains a cmd.exe metacharacter"
            ),
        }
    }
}

impl Error for NpmError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) | Self::Prefix(error) => Some(error),
            Self::InvalidPackage(error) => Some(error),
            Self::InvalidQuery
            | Self::UnsupportedOption(_)
            | Self::InvalidOption { .. }
            | Self::InvalidOutput
            | Self::CommandFailed { .. }
            | Self::Reported { .. }
            | Self::OutputTooLarge { .. }
            | Self::PrefixInvalid(_)
            | Self::UnsafeWindowsArgument(_) => None,
        }
    }
}

fn run_captured(command: Command) -> Result<Output, NpmError> {
    capture::run(command).map_err(|error| match error {
        CaptureError::Io(error) => NpmError::Io(error),
        CaptureError::OutputTooLarge { stream, limit } => {
            NpmError::OutputTooLarge { stream, limit }
        }
    })
}

fn run_status(command: Command) -> Result<(), NpmError> {
    let output = run_captured(command)?;
    if output.status.success() {
        return Ok(());
    }
    if let Ok(value) = Json::parse(&output.stdout)
        && let Some((code, summary)) = reported_error(&value)
    {
        return Err(NpmError::Reported {
            status: output.status,
            code,
            summary,
        });
    }
    Err(NpmError::CommandFailed {
        status: output.status,
        stderr: output.stderr,
    })
}

fn reported_error(value: &Json) -> Option<(String, String)> {
    let error = value.get("error")?;
    let code = error.get("code")?.as_str()?.to_owned();
    let summary = error
        .get("summary")
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_owned();
    Some((code, summary))
}

const SEARCH_LIMIT_EXPECTED: &str = "a u32 from 1 to 250";

fn reject_extra(options: &QueryOptions) -> Result<(), NpmError> {
    match options.extra.keys().next() {
        Some(key) => Err(NpmError::UnsupportedOption(key.clone())),
        None => Ok(()),
    }
}

fn search_limit(options: &QueryOptions) -> Result<Option<u32>, NpmError> {
    let mut limit = None;
    for (key, value) in &options.extra {
        if key != "limit" {
            return Err(NpmError::UnsupportedOption(key.clone()));
        }
        let value = value
            .downcast_ref::<u32>()
            .ok_or_else(|| NpmError::InvalidOption {
                key: key.clone(),
                expected: SEARCH_LIMIT_EXPECTED,
            })?;
        if !(1..=250).contains(value) {
            return Err(NpmError::InvalidOption {
                key: key.clone(),
                expected: SEARCH_LIMIT_EXPECTED,
            });
        }
        limit = Some(*value);
    }
    Ok(limit)
}

fn validate_search_query(query: &str) -> Result<(), NpmError> {
    validate_search_query_for(query, cfg!(windows))
}

fn validate_search_query_for(query: &str, windows: bool) -> Result<(), NpmError> {
    if query.trim().is_empty() || query.starts_with('-') || query.chars().any(char::is_control) {
        return Err(NpmError::InvalidQuery);
    }
    if windows
        && !query.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b' ' | b'.' | b'-' | b'_' | b'~' | b'@' | b'/' | b'+')
        })
    {
        return Err(NpmError::InvalidQuery);
    }
    Ok(())
}

fn parse_installed(value: &Json) -> Result<Vec<Package>, NpmError> {
    if value.as_object().is_none() {
        return Err(NpmError::InvalidOutput);
    }
    let Some(dependencies) = value.get("dependencies") else {
        return Ok(Vec::new());
    };
    if dependencies.is_null() {
        return Ok(Vec::new());
    }
    let dependencies = dependencies.as_object().ok_or(NpmError::InvalidOutput)?;
    let mut packages = Vec::with_capacity(dependencies.len());
    for (name, entry) in dependencies {
        validate_reported_name(name)?;
        let version = entry
            .get("version")
            .and_then(Json::as_str)
            .ok_or(NpmError::InvalidOutput)?;
        if version.is_empty() || version.chars().any(char::is_control) {
            return Err(NpmError::InvalidOutput);
        }
        packages.push(Package {
            name: name.clone(),
            version: version.to_owned(),
            metadata: BTreeMap::new(),
        });
    }
    Ok(packages)
}

fn parse_search(value: &Json) -> Result<Vec<Package>, NpmError> {
    let entries = value.as_array().ok_or(NpmError::InvalidOutput)?;
    let mut packages = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = entry
            .get("name")
            .and_then(Json::as_str)
            .ok_or(NpmError::InvalidOutput)?;
        validate_reported_name(name)?;
        let version = entry
            .get("version")
            .and_then(Json::as_str)
            .ok_or(NpmError::InvalidOutput)?;
        if version.is_empty() || version.chars().any(char::is_control) {
            return Err(NpmError::InvalidOutput);
        }
        let mut metadata = BTreeMap::new();
        if let Some(description) = entry.get("description").and_then(Json::as_str) {
            metadata.insert("description".to_owned(), description.to_owned());
        }
        packages.push(Package {
            name: name.to_owned(),
            version: version.to_owned(),
            metadata,
        });
    }
    Ok(packages)
}

/// Accepts a package name reported by npm.
///
/// This check is deliberately looser than [`validate_name`]: npm's registry
/// still carries legacy names with URL-safe punctuation outside the subset
/// accepted for new requests. The output is data, not a command argument, so
/// the important checks are non-empty text, a length bound, and no whitespace
/// or control characters.
fn validate_reported_name(name: &str) -> Result<(), NpmError> {
    const MAX_REPORTED_NAME_LEN: usize = 214;
    if name.is_empty()
        || name.len() > MAX_REPORTED_NAME_LEN
        || name.starts_with('-')
        || name
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b' ' | b'\\'))
    {
        return Err(NpmError::InvalidOutput);
    }
    match name.split_once('/') {
        Some((scope, package)) => {
            if !scope.starts_with('@')
                || scope.len() == 1
                || package.is_empty()
                || package.contains('/')
            {
                return Err(NpmError::InvalidOutput);
            }
        }
        None if name.starts_with('@') => return Err(NpmError::InvalidOutput),
        None => {}
    }
    if name
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(NpmError::InvalidOutput);
    }
    Ok(())
}

/// Accepts a conservative npm package name and an optional scope.
///
/// The rules follow npm's URL-safe subset: ASCII letters, digits, and
/// `-._~`, with no leading `.`, `_`, or `-` and no trailing dot. A scoped name
/// has exactly one `/` after the scope. Uppercase is accepted for legacy
/// packages even though new npm names are lowercase.
fn validate_name(name: &str) -> Result<(), InvalidPackageSpec> {
    const MAX_NAME_LEN: usize = 214;
    if name.is_empty() || name.len() > MAX_NAME_LEN || name.starts_with('-') {
        return Err(InvalidPackageSpec::Name);
    }
    if let Some(rest) = name.strip_prefix('@') {
        let (scope, package) = rest.split_once('/').ok_or(InvalidPackageSpec::Name)?;
        if package.contains('/') {
            return Err(InvalidPackageSpec::Name);
        }
        validate_name_part(scope)?;
        validate_name_part(package)
    } else {
        if name.contains('@') || name.contains('/') {
            return Err(InvalidPackageSpec::Name);
        }
        validate_name_part(name)
    }
}

fn validate_name_part(part: &str) -> Result<(), InvalidPackageSpec> {
    if part.is_empty()
        || part.starts_with(['.', '_', '-'])
        || part.ends_with('.')
        || !part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
    {
        return Err(InvalidPackageSpec::Name);
    }
    Ok(())
}

/// Accepts the version text this provider passes to npm.
///
/// Exact versions, ranges, and dist-tags share a small character set. The
/// semver parser belongs to npm; this function only rejects characters that
/// could be read as a path, URL, or flag separator.
fn validate_version(version: &str) -> Result<(), InvalidPackageSpec> {
    validate_version_for(version, cfg!(windows))
}

/// Accepts the version text this provider passes to npm.
///
/// Exact versions, ranges, and dist-tags share a small character set. The
/// semver parser belongs to npm; this function only rejects characters that
/// could be read as a path, URL, or flag separator.
///
/// When `windows` is true, the version is also limited to ASCII letters,
/// digits, `.`, `-`, and `+`. npm is normally a batch file on Windows, and this
/// crate cannot validate `cmd.exe` argument escaping, so range operators and
/// whitespace are rejected before npm starts.
fn validate_version_for(version: &str, windows: bool) -> Result<(), InvalidPackageSpec> {
    const MAX_VERSION_LEN: usize = 256;
    if version.is_empty()
        || version.len() > MAX_VERSION_LEN
        || version.trim() != version
        || version.trim().is_empty()
        || !version.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b'-'
                        | b'+'
                        | b'^'
                        | b'~'
                        | b'<'
                        | b'>'
                        | b'='
                        | b'|'
                        | b'&'
                        | b'*'
                        | b' '
                )
        })
        || (windows
            && !version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')))
    {
        return Err(InvalidPackageSpec::Version);
    }
    Ok(())
}

/// A configured prefix path that cannot be used safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathState {
    /// The path does not exist.
    Missing,
    /// The path resolves to a directory.
    Directory,
    /// The path exists but is not a usable directory.
    Other,
    /// An ancestor of the path is not a directory, so the path cannot exist.
    AncestorNotDirectory,
}

fn path_state(path: &Path) -> Result<PathState, NpmError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => match fs::metadata(path) {
            Ok(target) if target.is_dir() => Ok(PathState::Directory),
            Ok(_) => Ok(PathState::Other),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(PathState::Other)
            }
            Err(error) => Err(NpmError::Prefix(error)),
        },
        Ok(metadata) if metadata.is_dir() => Ok(PathState::Directory),
        Ok(_) => Ok(PathState::Other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PathState::Missing),
        Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
            Ok(PathState::AncestorNotDirectory)
        }
        Err(error) => Err(NpmError::Prefix(error)),
    }
}

fn global_modules_dir(prefix: &Path) -> PathBuf {
    global_modules_dir_for(prefix, cfg!(windows))
}

fn global_modules_dir_for(prefix: &Path, windows: bool) -> PathBuf {
    if windows {
        prefix.join("node_modules")
    } else {
        prefix.join("lib").join("node_modules")
    }
}

/// Returns whether an argument is free of characters that `cmd.exe` treats
/// specially.
///
/// This is used only on Windows, where npm is normally a batch file. It is a
/// conservative allow-list check: ordinary paths with spaces, drive letters,
/// and backslashes pass, while redirection, piping, expansion, grouping, and
/// separator characters do not.
fn windows_argument_is_safe(argument: &str) -> bool {
    !argument.chars().any(|character| {
        matches!(
            character,
            '\0' | '\r'
                | '\n'
                | '"'
                | '%'
                | '!'
                | '^'
                | '&'
                | '|'
                | '<'
                | '>'
                | ';'
                | ','
                | '('
                | ')'
                | '*'
                | '?'
        )
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        global_modules_dir_for, validate_search_query_for, validate_version_for,
        windows_argument_is_safe,
    };

    #[test]
    fn package_root_follows_the_platform_layout() {
        let prefix = Path::new("prefix");
        assert_eq!(
            global_modules_dir_for(prefix, false),
            Path::new("prefix/lib/node_modules")
        );
        assert_eq!(
            global_modules_dir_for(prefix, true),
            Path::new("prefix/node_modules")
        );
    }

    #[test]
    fn unix_versions_accept_ranges() {
        for version in ["^1.2.3", ">=1.0.0 <2.0.0", "1.0.0 || 2.0.0", "*", "latest"] {
            assert!(
                validate_version_for(version, false).is_ok(),
                "version {version:?} should be accepted on Unix"
            );
        }
    }

    #[test]
    fn windows_versions_reject_cmd_metacharacters() {
        for version in ["^1.2.3", ">=1.0.0 <2.0.0", "1.0.0 || 2.0.0", "*", "1.2.3 x"] {
            assert!(
                validate_version_for(version, true).is_err(),
                "version {version:?} should be rejected on Windows"
            );
        }
        for version in [
            "1.2.3",
            "1.2.3-beta.1",
            "1.2.3+build.5",
            "1.x",
            "latest",
            "beta",
        ] {
            assert!(
                validate_version_for(version, true).is_ok(),
                "version {version:?} should be accepted on Windows"
            );
        }
    }

    #[test]
    fn windows_search_queries_reject_cmd_metacharacters() {
        for query in ["a&b", "a|b", "a^b", "a%b", "a!b", "a\"b", "a;b", "a(b"] {
            assert!(
                validate_search_query_for(query, true).is_err(),
                "query {query:?} should be rejected on Windows"
            );
        }
        for query in ["eslint", "@types/node", "eslint config", "1.2.3"] {
            assert!(
                validate_search_query_for(query, true).is_ok(),
                "query {query:?} should be accepted on Windows"
            );
        }
        assert!(validate_search_query_for("a&b", false).is_ok());
    }

    #[test]
    fn windows_prefixes_reject_cmd_metacharacters() {
        for prefix in [
            "C:\\Users\\dev\\npm-prefix",
            "C:\\Users\\dev\\npm prefix",
            "C:\\Users\\dev\\npm-prefix=1",
        ] {
            assert!(
                windows_argument_is_safe(prefix),
                "prefix {prefix:?} should be accepted"
            );
        }
        for prefix in [
            "C:\\Users\\dev\\a&b",
            "C:\\Users\\dev\\a|b",
            "C:\\Users\\dev\\a^b",
            "C:\\Users\\dev\\a%b",
            "C:\\Users\\dev\\a!b",
            "C:\\Users\\dev\\a\"b",
            "C:\\Users\\dev\\a;b",
            "C:\\Users\\dev\\a(b",
        ] {
            assert!(
                !windows_argument_is_safe(prefix),
                "prefix {prefix:?} should be rejected"
            );
        }
    }
}
