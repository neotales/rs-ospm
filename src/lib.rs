//! Independent capabilities for operating system package managers.
//!
//! A backend implements only the operations it supports. The portable interface
//! does not elevate privileges or assume that package names are interchangeable
//! across managers.

pub mod apt;

use std::error::Error;
use std::fmt;
use std::process::ExitStatus;

/// A package identifier in the target manager's own namespace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageName(String);

impl PackageName {
    /// Validates a conservative, portable package identifier.
    ///
    /// Manager-specific syntax that cannot be represented here can be used
    /// through the backend's native API instead.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidPackageName> {
        let name = name.into();
        if name.is_empty()
            || name.starts_with('-')
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".+-_:".contains(&c))
        {
            return Err(InvalidPackageName);
        }
        Ok(Self(name))
    }

    /// Returns the identifier as provided by the caller.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An identifier cannot be used by the portable API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPackageName;

impl fmt::Display for InvalidPackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid package name")
    }
}

impl Error for InvalidPackageName {}

/// A package-manager operation failed.
#[derive(Debug)]
pub enum PackageError {
    /// The process could not be started or its output could not be read.
    Io(std::io::Error),
    /// The process returned a failure status.
    CommandFailed {
        /// Exit status, including signal termination where available.
        status: ExitStatus,
        /// Standard error as emitted by the manager.
        stderr: Vec<u8>,
    },
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "package manager process: {error}"),
            Self::CommandFailed { status, stderr } => {
                write!(
                    f,
                    "package manager exited with {status}: {}",
                    String::from_utf8_lossy(stderr)
                )
            }
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

/// Installs a package using its manager-specific name.
pub trait Install {
    /// Install a package; privilege and interaction policy belong to the caller.
    fn install(&self, package: &PackageName) -> Result<(), PackageError>;
}

/// Removes a package using its manager-specific name.
pub trait Remove {
    /// Remove a package without promising removal of configuration or dependencies.
    fn remove(&self, package: &PackageName) -> Result<(), PackageError>;
}
