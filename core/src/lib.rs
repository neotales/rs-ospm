//! Independent capabilities for operating system package managers.
//!
//! A backend implements only the operations it supports. The portable interface
//! does not elevate privileges or assume that package names are interchangeable
//! across managers.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::Path;

/// A package and version in the target manager's namespace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    /// Manager-specific package name, including any architecture qualifier.
    pub name: String,
    /// Version as reported by the manager, without cross-manager comparison.
    pub version: String,
    /// Optional manager-specific fields. Keys and values are backend-defined.
    pub metadata: BTreeMap<String, String>,
}

/// A package source in the target manager's namespace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    /// Manager-specific identifier used to select the source.
    pub id: String,
    /// Manager-specific source definition or location.
    pub location: String,
    /// Optional manager-specific fields.
    pub metadata: BTreeMap<String, String>,
}

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

/// Checks whether a package is installed.
pub trait IsInstalled {
    /// Backend-specific error.
    type Error: Error + Send + Sync + 'static;

    /// Checks the manager's package database.
    fn is_installed(&self, package: &PackageName) -> Result<bool, Self::Error>;
}

/// Upgrades one installed package to the manager's selected version.
pub trait Update {
    /// Backend-specific error.
    type Error: Error + Send + Sync + 'static;
    /// Upgrade the specified package, without updating source metadata.
    fn update(&self, package: &PackageName) -> Result<(), Self::Error>;
}

/// Lists installed packages.
pub trait List {
    /// Backend-specific error.
    type Error: Error + Send + Sync + 'static;
    /// Return installed packages with the installed version.
    fn list(&self) -> Result<Vec<Package>, Self::Error>;
}

/// Searches available packages.
pub trait Search {
    /// Backend-specific error.
    type Error: Error + Send + Sync + 'static;
    /// Search using the backend's query syntax. Versions are manager-selected
    /// or explicitly documented by the backend.
    fn search(&self, query: &str) -> Result<Vec<Package>, Self::Error>;
}

/// Optional source management capability.
pub trait Sources {
    /// Backend-specific error.
    type Error: Error + Send + Sync + 'static;
    /// List configured sources.
    fn list_sources(&self) -> Result<Vec<Source>, Self::Error>;
    /// Add the manager-specific source definition.
    fn add_source(&self, source: &Source) -> Result<(), Self::Error>;
    /// Remove a source selected by its manager-specific identifier.
    fn remove_source(&self, id: &str) -> Result<(), Self::Error>;
}

/// Optional capability to refresh source metadata, distinct from package upgrades.
pub trait UpdateSources {
    /// Backend-specific error.
    type Error: Error + Send + Sync + 'static;
    /// Refresh configured sources.
    fn update_sources(&self) -> Result<(), Self::Error>;
}

/// Optional capability to prevent or resume automatic package upgrades.
pub trait Pin {
    /// Backend-specific error.
    type Error: Error + Send + Sync + 'static;
    /// Hold the installed package at its current version.
    fn pin(&self, package: &PackageName) -> Result<(), Self::Error>;
    /// Remove the hold.
    fn unpin(&self, package: &PackageName) -> Result<(), Self::Error>;
}

/// Optional capability to install a local package file.
pub trait InstallFile {
    /// Backend-specific error.
    type Error: Error + Send + Sync + 'static;
    /// Install the package file, leaving dependency handling to the manager.
    fn install_file(&self, path: &Path) -> Result<(), Self::Error>;
}
