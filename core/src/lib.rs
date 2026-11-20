//! Independent capabilities for operating system package managers.
//!
//! A backend implements only the operations it supports. The portable interface
//! does not elevate privileges or assume that package names are interchangeable
//! across managers.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

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

/// Installs a package using its manager-specific name.
pub trait Install {
    /// Backend-specific error, including native API errors when applicable.
    type Error: Error + Send + Sync + 'static;

    /// Install a package; privilege and interaction policy belong to the caller.
    fn install(&self, package: &PackageName) -> Result<(), Self::Error>;
}

/// Removes a package using its manager-specific name.
pub trait Remove {
    /// Backend-specific error, including native API errors when applicable.
    type Error: Error + Send + Sync + 'static;

    /// Remove a package without promising removal of configuration or dependencies.
    fn remove(&self, package: &PackageName) -> Result<(), Self::Error>;
}
