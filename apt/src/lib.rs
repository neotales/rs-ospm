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

use neotales_ospm_core::{Install, PackageName, Remove};

/// Failure while invoking the APT command-line fallback.
#[derive(Debug)]
pub enum PackageError {
    /// The process could not be started or read.
    Io(std::io::Error),
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
            Self::CommandFailed { .. } => None,
        }
    }
}

/// An APT manager backed by the `apt-get` executable.
#[derive(Clone, Debug)]
pub struct Apt {
    executable: PathBuf,
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
        }
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
