//! APT support. The optional `cli` feature exposes an explicit process fallback.
//!
//! `apt-get` performs dependency resolution and system changes. This backend
//! never invokes sudo, changes repositories, or assumes noninteractive consent.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![cfg(feature = "cli")]

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

use neotales_ospm_core::{
    Install, InstallFile, List, Package, PackageName, Pin, QueryOptions, Remove, Search, Update,
    UpdateSources,
};

/// Failure while invoking the APT command-line fallback.
#[derive(Debug)]
pub enum PackageError {
    /// The process could not be started or read.
    Io(std::io::Error),
    /// The supplied search query cannot be passed to APT.
    InvalidQuery,
    /// A provider-specific option is not supported.
    UnsupportedOption(String),
    /// The manager's output was not valid UTF-8 or did not match the expected format.
    InvalidOutput,
    /// The command returned an unsuccessful status.
    CommandFailed {
        /// The process exit status.
        status: ExitStatus,
        /// The process standard error.
        stderr: Vec<u8>,
    },
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "package manager process: {error}"),
            Self::InvalidQuery => f.write_str("invalid empty or NUL-containing search query"),
            Self::UnsupportedOption(key) => write!(f, "unsupported APT query option: {key}"),
            Self::InvalidOutput => f.write_str("invalid package manager output"),
            Self::CommandFailed { status, stderr } => write!(
                f,
                "package manager exited with {status}: {}",
                String::from_utf8_lossy(stderr)
            ),
        }
    }
}

impl Error for PackageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::CommandFailed { .. }
            | Self::InvalidQuery
            | Self::UnsupportedOption(_)
            | Self::InvalidOutput => None,
        }
    }
}

/// An APT manager backed by the `apt-get` executable.
#[derive(Clone, Debug)]
pub struct Apt {
    executable: PathBuf,
    apt_cache: PathBuf,
    dpkg_query: PathBuf,
    apt_mark: PathBuf,
}

impl Default for Apt {
    fn default() -> Self {
        Self::new()
    }
}

impl Apt {
    /// Uses `apt-get` from the current process's PATH.
    pub fn new() -> Self {
        Self::with_executable("apt-get")
    }

    /// Uses an explicit executable, useful for configured installations or tests.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            apt_cache: "apt-cache".into(),
            dpkg_query: "dpkg-query".into(),
            apt_mark: "apt-mark".into(),
        }
    }

    /// Overrides the executables used for read-only queries and holds.
    pub fn with_tools(
        mut self,
        apt_cache: impl Into<PathBuf>,
        dpkg_query: impl Into<PathBuf>,
        apt_mark: impl Into<PathBuf>,
    ) -> Self {
        self.apt_cache = apt_cache.into();
        self.dpkg_query = dpkg_query.into();
        self.apt_mark = apt_mark.into();
        self
    }

    /// The configured executable; a relative path is resolved when spawned.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Creates a native `apt-get` command for manager-specific operations.
    ///
    /// Arguments, environment, stdio, privilege and exit-status handling are
    /// controlled by the caller. This method does not execute the command.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    /// Runs an APT operation with captured output and no automatic elevation.
    ///
    /// This native interface lets a caller parse, filter, or rewrite the output.
    /// Unlike the portable traits, it does not inherit stdin or stdout.
    pub fn output<I, S>(&self, args: I) -> std::io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command().args(args).output()
    }

    fn run(&self, action: &str, package: &PackageName) -> Result<(), PackageError> {
        let output = self
            .command()
            .args([
                OsStr::new(action),
                OsStr::new("--"),
                OsStr::new(package.as_str()),
            ])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .output()
            .map_err(PackageError::Io)?;
        check(output)
    }

    fn interactive(&self, command: &mut Command) -> Result<(), PackageError> {
        check(
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .output()
                .map_err(PackageError::Io)?,
        )
    }
}

fn check(output: Output) -> Result<(), PackageError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(PackageError::CommandFailed {
            status: output.status,
            stderr: output.stderr,
        })
    }
}

impl Install for Apt {
    type Error = PackageError;

    fn install(&self, package: &PackageName) -> Result<(), PackageError> {
        self.run("install", package)
    }
}

impl Remove for Apt {
    type Error = PackageError;

    fn remove(&self, package: &PackageName) -> Result<(), PackageError> {
        self.run("remove", package)
    }
}

impl Update for Apt {
    type Error = PackageError;
    fn update(&self, package: &PackageName) -> Result<(), Self::Error> {
        self.interactive(
            self.command()
                .args(["install", "--only-upgrade", "--", package.as_str()]),
        )
    }
}

impl UpdateSources for Apt {
    type Error = PackageError;
    fn update_sources(&self) -> Result<(), Self::Error> {
        self.interactive(self.command().arg("update"))
    }
}

impl Pin for Apt {
    type Error = PackageError;
    fn pin(&self, package: &PackageName) -> Result<(), Self::Error> {
        self.interactive(Command::new(&self.apt_mark).args(["hold", "--", package.as_str()]))
    }
    fn unpin(&self, package: &PackageName) -> Result<(), Self::Error> {
        self.interactive(Command::new(&self.apt_mark).args(["unhold", "--", package.as_str()]))
    }
}

impl InstallFile for Apt {
    type Error = PackageError;
    fn install_file(&self, path: &Path) -> Result<(), Self::Error> {
        let absolute = std::fs::canonicalize(path).map_err(PackageError::Io)?;
        self.interactive(self.command().args(["install", "--"]).arg(absolute))
    }
}

fn text_output(command: &mut Command) -> Result<String, PackageError> {
    let output = command
        .env("LC_ALL", "C")
        .output()
        .map_err(PackageError::Io)?;
    if !output.status.success() {
        return Err(PackageError::CommandFailed {
            status: output.status,
            stderr: output.stderr,
        });
    }
    String::from_utf8(output.stdout).map_err(|_| PackageError::InvalidOutput)
}

impl List for Apt {
    type Error = PackageError;
    fn list(&self, query: &str, options: &QueryOptions) -> Result<Vec<Package>, Self::Error> {
        validate_options(options)?;
        let output = text_output(Command::new(&self.dpkg_query).args([
            "--show",
            "--showformat=${binary:Package}\\t${Version}\\t${db:Status-Status}\\n",
        ]))?;
        let mut packages = Vec::new();
        for line in output.lines() {
            let mut fields = line.split('\t');
            let (Some(name), Some(version), Some(status), None) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                return Err(PackageError::InvalidOutput);
            };
            if status == "installed" {
                if name.is_empty() || version.is_empty() {
                    return Err(PackageError::InvalidOutput);
                }
                if !name.contains(query)
                    || options
                        .version
                        .as_ref()
                        .is_some_and(|wanted| wanted != version)
                {
                    continue;
                }
                packages.push(Package {
                    name: name.into(),
                    version: version.into(),
                    metadata: Default::default(),
                });
            }
        }
        Ok(packages)
    }
}

impl Search for Apt {
    type Error = PackageError;
    fn search(&self, query: &str, options: &QueryOptions) -> Result<Vec<Package>, Self::Error> {
        validate_options(options)?;
        if query.is_empty() || query.contains('\0') {
            return Err(PackageError::InvalidQuery);
        }
        let output = text_output(Command::new(&self.apt_cache).args(["search", "--", query]))?;
        let mut names = Vec::new();
        for line in output.lines() {
            let (name, _) = line.split_once(" - ").ok_or(PackageError::InvalidOutput)?;
            let name = PackageName::new(name).map_err(|_| PackageError::InvalidOutput)?;
            names.push(name);
        }
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let mut command = Command::new(&self.apt_cache);
        if options.version.is_none() {
            command.arg("--no-all-versions");
        }
        command.args(["show", "--"]);
        command.args(names.iter().map(PackageName::as_str));
        let details = text_output(&mut command)?;
        let mut packages = parse_details(&details)?;
        if let Some(version) = &options.version {
            packages.retain(|package| &package.version == version);
        }
        Ok(packages)
    }
}

fn validate_options(options: &QueryOptions) -> Result<(), PackageError> {
    if let Some(key) = options.extra.keys().next() {
        return Err(PackageError::UnsupportedOption(key.clone()));
    }
    Ok(())
}

fn parse_details(details: &str) -> Result<Vec<Package>, PackageError> {
    let mut result = Vec::new();
    for record in details
        .split("\n\n")
        .filter(|record| !record.trim().is_empty())
    {
        let mut name = None;
        let mut version = None;
        let mut metadata = std::collections::BTreeMap::new();
        for line in record.lines() {
            if line.starts_with([' ', '\t']) {
                continue;
            }
            let (key, value) = line.split_once(": ").ok_or(PackageError::InvalidOutput)?;
            match key {
                "Package" => name = Some(value.to_owned()),
                "Version" => version = Some(value.to_owned()),
                "Architecture" | "Description" => {
                    metadata.insert(key.to_ascii_lowercase(), value.to_owned());
                }
                _ => {}
            }
        }
        let (Some(name), Some(version)) = (name, version) else {
            return Err(PackageError::InvalidOutput);
        };
        if name.is_empty() || version.is_empty() {
            return Err(PackageError::InvalidOutput);
        }
        result.push(Package {
            name,
            version,
            metadata,
        });
    }
    Ok(result)
}
