//! APT's process-based backend.
//!
//! `apt-get` performs dependency resolution and system changes. This backend
//! never invokes sudo, changes repositories, or assumes noninteractive consent.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::{Install, PackageError, PackageName, Remove};

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
    fn install(&self, package: &PackageName) -> Result<(), PackageError> {
        self.run("install", package)
    }
}

impl Remove for Apt {
    fn remove(&self, package: &PackageName) -> Result<(), PackageError> {
        self.run("remove", package)
    }
}
