//! Explicit process backend for pnpm global tools and project dependencies.
//!
//! [`Pnpm`] invokes an injected pnpm executable directly, without a shell.
//! Global tool operations and project dependency operations are separate
//! methods because a globally linked executable is not the same thing as a
//! dependency recorded in a project manifest. The backend parses pnpm's JSON
//! output where pnpm supports it and returns a typed error otherwise.
//!
//! The crate does not implement the portable package-manager traits. Those
//! traits have no scope argument, so a caller could not tell whether a call
//! touched a global tool directory or one project's manifest.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use serde_json::Value;

/// A pnpm executable invoked as an external process.
///
/// The default executable is `pnpm` from `PATH`. The process inherits the
/// caller's environment, so registry, authentication, proxy, store and
/// workspace settings keep their usual meaning. This type never invokes a
/// shell and never elevates privileges.
#[derive(Clone, Debug)]
pub struct Pnpm {
    executable: PathBuf,
    project_directory: Option<PathBuf>,
}

impl Default for Pnpm {
    fn default() -> Self {
        Self::new()
    }
}

impl Pnpm {
    /// Uses `pnpm` from the current process's `PATH`.
    pub fn new() -> Self {
        Self::with_executable("pnpm")
    }

    /// Uses an explicit executable, useful for a configured installation or a
    /// test fixture.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            project_directory: None,
        }
    }

    /// Sets the working directory for project dependency operations.
    ///
    /// Global tool operations ignore this directory. The path is passed to the
    /// spawned process as given, so a relative path is resolved against the
    /// caller's working directory. It is not a sandbox: pnpm can still read a
    /// workspace file above the directory.
    pub fn with_project_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.project_directory = Some(directory.into());
        self
    }

    /// The configured executable; a relative path is resolved when spawned.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// The configured project directory, or `None` for the process default.
    pub fn project_directory(&self) -> Option<&Path> {
        self.project_directory.as_deref()
    }

    /// Creates a native pnpm command for operations this backend does not
    /// wrap.
    ///
    /// The command inherits the calling process's environment and working
    /// directory until the caller changes them. Arguments, environment, stdio,
    /// privilege and exit-status handling are entirely the caller's
    /// responsibility. This method does not execute the command and does not
    /// apply the configured project directory.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    /// Creates a native pnpm command in the configured project directory.
    ///
    /// The project directory is applied when it was configured. Arguments,
    /// environment, stdio, privilege and exit-status handling belong to the
    /// caller. This method does not execute the command.
    pub fn project_command(&self) -> Command {
        let mut command = self.command();
        if let Some(directory) = &self.project_directory {
            command.current_dir(directory);
        }
        command
    }

    /// Installs or updates a global tool.
    ///
    /// Runs `pnpm add --global` with the package argument. When the spec
    /// carries an exact version, `--save-exact` is also passed so the global
    /// manifest records that version rather than a range. The process receives
    /// no interactive standard input, so an authentication prompt fails
    /// instead of blocking.
    pub fn add_global(&self, package: &PackageSpec) -> Result<(), PnpmError> {
        let mut command = self.command();
        command.arg("add").arg("--global");
        push_exactness(&mut command, package);
        command.arg(package.argument());
        run(&mut command)
    }

    /// Removes a global tool.
    ///
    /// Runs `pnpm remove --global` with the package name. The command does not
    /// remove anything outside pnpm's global package directory.
    pub fn remove_global(&self, package: &PackageName) -> Result<(), PnpmError> {
        let mut command = self.command();
        command.arg("remove").arg("--global").arg(package.as_str());
        run(&mut command)
    }

    /// Lists global tools as pnpm reports them.
    ///
    /// Runs `pnpm list --global --depth 0 --json` and returns the entries in
    /// the `dependencies` section. Only direct globally installed packages are
    /// returned; transitive dependencies are not.
    pub fn list_global(&self) -> Result<Vec<InstalledPackage>, PnpmError> {
        let mut command = self.command();
        command.args(["list", "--global", "--depth", "0", "--json"]);
        let value = run_json(&mut command)?;
        let projects = value.as_array().ok_or(PnpmError::InvalidOutput)?;
        let mut packages = Vec::new();
        for project in projects {
            let object = project.as_object().ok_or(PnpmError::InvalidOutput)?;
            packages.extend(parse_section(object.get("dependencies"))?);
        }
        Ok(packages)
    }

    /// Adds a dependency to the configured project.
    ///
    /// Runs `pnpm add` in the configured project directory with the selected
    /// section flag. [`DependencySection::Regular`] passes `--save-prod`, so an
    /// existing development dependency is moved to `dependencies` instead of
    /// staying where it was. An exact version in the spec also passes
    /// `--save-exact`. The process receives no interactive standard input.
    pub fn add_project(
        &self,
        package: &PackageSpec,
        section: DependencySection,
    ) -> Result<(), PnpmError> {
        let mut command = self.project_command();
        command.arg("add");
        if let Some(flag) = section.flag() {
            command.arg(flag);
        }
        push_exactness(&mut command, package);
        command.arg(package.argument());
        run(&mut command)
    }

    /// Removes a dependency from the configured project.
    ///
    /// Runs `pnpm remove` in the configured project directory. Without a
    /// section flag pnpm removes the package from every manifest section that
    /// contains it.
    pub fn remove_project(&self, package: &PackageName) -> Result<(), PnpmError> {
        let mut command = self.project_command();
        command.arg("remove").arg(package.as_str());
        run(&mut command)
    }

    /// Installs dependencies for only the configured project.
    ///
    /// Runs `pnpm install --ignore-workspace` in the configured project
    /// directory. `--ignore-workspace` keeps pnpm from selecting an enclosing
    /// workspace, so no other workspace project, lockfile or lifecycle script
    /// is touched. The trade-off is that pnpm treats the project as
    /// standalone: it writes a lockfile inside the project and cannot resolve
    /// `workspace:` dependency specifications. For a workspace-wide install,
    /// use [`install_workspace`](Pnpm::install_workspace).
    pub fn install_project(&self) -> Result<(), PnpmError> {
        let mut command = self.project_command();
        command.args(["install", "--ignore-workspace"]);
        run(&mut command)
    }

    /// Installs dependencies for the configured project and its workspace.
    ///
    /// Runs plain `pnpm install` in the configured project directory. When the
    /// project belongs to a workspace, pnpm selects the workspace root,
    /// installs every project in the workspace, and runs the lifecycle scripts
    /// of every project. Calling this method is the explicit opt-in to that
    /// workspace-wide behaviour. Use [`install_project`](Pnpm::install_project)
    /// to keep the install to one project.
    pub fn install_workspace(&self) -> Result<(), PnpmError> {
        let mut command = self.project_command();
        command.arg("install");
        run(&mut command)
    }

    /// Lists dependencies for the configured project.
    ///
    /// Runs `pnpm list --depth 0 --json` in the configured project directory.
    /// pnpm can report more than one project when a workspace is involved, so
    /// the result keeps pnpm's project boundaries. Direct dependencies in the
    /// `dependencies`, `devDependencies`, `optionalDependencies` and
    /// `peerDependencies` sections are returned; transitive dependencies are
    /// not.
    pub fn list_project(&self) -> Result<Vec<Project>, PnpmError> {
        let mut command = self.project_command();
        command.args(["list", "--depth", "0", "--json"]);
        let value = run_json(&mut command)?;
        let projects = value.as_array().ok_or(PnpmError::InvalidOutput)?;
        projects.iter().map(parse_project).collect()
    }

    /// Searches the configured registry.
    ///
    /// Runs `pnpm search --json` with the query after `--`, so a query that
    /// begins with a hyphen is still a search term. An empty or
    /// whitespace-only query is rejected. `options.limit` becomes
    /// `--search-limit`; `None` leaves pnpm's default in place. Search does
    /// not use the configured project directory.
    pub fn search(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>, PnpmError> {
        if query.trim().is_empty() || query.contains('\0') {
            return Err(PnpmError::InvalidQuery);
        }
        let mut command = self.command();
        command.args(["search", "--json"]);
        if let Some(limit) = options.limit {
            if limit == 0 {
                return Err(PnpmError::InvalidLimit);
            }
            command.arg("--search-limit").arg(limit.to_string());
        }
        command.arg("--").arg(query);
        let value = run_json(&mut command)?;
        let results = value.as_array().ok_or(PnpmError::InvalidOutput)?;
        results.iter().map(parse_search_result).collect()
    }
}

fn push_exactness(command: &mut Command, package: &PackageSpec) {
    if package.version().is_some() {
        command.arg("--save-exact");
    }
}

fn run(command: &mut Command) -> Result<(), PnpmError> {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .output()
        .map_err(PnpmError::Io)?;
    check_status(output.status, output.stderr)
}

fn run_json(command: &mut Command) -> Result<Value, PnpmError> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(PnpmError::Io)?;
    check_status(output.status, output.stderr)?;
    serde_json::from_slice(&output.stdout).map_err(|_| PnpmError::InvalidOutput)
}

fn check_status(status: ExitStatus, stderr: Vec<u8>) -> Result<(), PnpmError> {
    if status.success() {
        Ok(())
    } else {
        Err(PnpmError::Exit { status, stderr })
    }
}

/// A registry package name that pnpm can receive as one argument.
///
/// Plain names such as `example-tool` and scoped names such as
/// `@example/tool` are accepted. The name may contain ASCII letters, digits,
/// `.`, `_`, `-` and `~`; a scope adds one leading `@` and one `/`. A name
/// that starts with `-`, `.` or `_` is rejected, as are NUL bytes, whitespace
/// and an embedded `@`. Those rules keep the name from being read as an option
/// or as separate shell words. The constructor does not check whether the
/// package exists in a registry.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageName(String);

impl PackageName {
    /// Validates a registry package name.
    ///
    /// The accepted syntax is a conservative subset of pnpm's package
    /// argument. Local paths, URLs, aliases and version suffixes are rejected;
    /// pass a version through [`PackageSpec`] instead.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidPackage> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self(name))
    }

    /// The name as provided by the caller.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A package name with an optional exact version.
///
/// A spec without a version is just a [`PackageName`]. A spec with a version
/// accepts a complete semantic version such as `1.2.3`, `1.2.3-rc.1` or
/// `1.2.3+build.5`; ranges, tags and abbreviated versions are rejected because
/// they do not name an exact version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageSpec {
    name: PackageName,
    version: Option<String>,
}

impl PackageSpec {
    /// Creates a spec for the registry version pnpm selects.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidPackage> {
        Ok(Self {
            name: PackageName::new(name)?,
            version: None,
        })
    }

    /// Creates a spec pinned to an exact semantic version.
    pub fn exact(
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, InvalidPackage> {
        let name = PackageName::new(name)?;
        let version = version.into();
        validate_version(&version)?;
        Ok(Self {
            name,
            version: Some(version),
        })
    }

    /// The validated package name.
    pub fn name(&self) -> &PackageName {
        &self.name
    }

    /// The exact version, or `None` when pnpm selects one.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The single package argument passed to pnpm.
    ///
    /// The result is `name` or `name@version`. A scoped name therefore becomes
    /// `@scope/name@version`.
    pub fn argument(&self) -> String {
        match &self.version {
            Some(version) => format!("{}@{version}", self.name),
            None => self.name.to_string(),
        }
    }
}

/// The manifest section a project dependency belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencySection {
    /// `dependencies`. `add_project` passes `--save-prod`, so a package that
    /// was already a development dependency is moved here.
    Regular,
    /// `devDependencies`.
    Development,
    /// `optionalDependencies`.
    Optional,
    /// `peerDependencies`. pnpm also installs a peer dependency as a
    /// development dependency, so this section does not mean "metadata only".
    Peer,
}

impl DependencySection {
    fn flag(self) -> Option<&'static str> {
        match self {
            Self::Regular => Some("--save-prod"),
            Self::Development => Some("--save-dev"),
            Self::Optional => Some("--save-optional"),
            Self::Peer => Some("--save-peer"),
        }
    }
}

/// One dependency pnpm reports for a project or the global directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledPackage {
    /// Package name as pnpm reports it.
    pub name: String,
    /// Installed version as pnpm reports it.
    pub version: String,
}

/// One project in pnpm's dependency listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    /// Project name from its manifest, when pnpm reports one.
    pub name: Option<String>,
    /// Project version from its manifest, when pnpm reports one.
    pub version: Option<String>,
    /// Project path as pnpm reports it, when present.
    pub path: Option<String>,
    /// Direct regular dependencies.
    pub dependencies: Vec<InstalledPackage>,
    /// Direct development dependencies.
    pub dev_dependencies: Vec<InstalledPackage>,
    /// Direct optional dependencies.
    pub optional_dependencies: Vec<InstalledPackage>,
    /// Direct peer dependencies, when pnpm reports a `peerDependencies`
    /// section.
    pub peer_dependencies: Vec<InstalledPackage>,
}

/// One registry search result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    /// Package name.
    pub name: String,
    /// Registry version pnpm reports.
    pub version: String,
    /// Package description, when the registry provides one.
    pub description: Option<String>,
}

/// Options for [`Pnpm::search`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchOptions {
    /// Maximum number of results. `None` uses pnpm's default. Zero is
    /// rejected by [`Pnpm::search`] with [`PnpmError::InvalidLimit`].
    pub limit: Option<u32>,
}

/// A package name or version that cannot be passed to pnpm safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidPackage {
    /// The name is empty, too long, starts with a reserved character,
    /// contains an unsupported character, or has a malformed scope.
    Name,
    /// The version is not a complete semantic version.
    Version,
}

impl fmt::Display for InvalidPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name => f.write_str("invalid pnpm package name"),
            Self::Version => {
                f.write_str("version must be a complete semantic version such as 1.2.3")
            }
        }
    }
}

impl Error for InvalidPackage {}

/// Failure invoking pnpm or reading its output.
#[derive(Debug)]
pub enum PnpmError {
    /// The process could not be started or its output could not be read.
    Io(std::io::Error),
    /// pnpm returned an unsuccessful exit status.
    Exit {
        /// The process exit status.
        status: ExitStatus,
        /// Captured standard error, which can contain registry or build
        /// diagnostics.
        stderr: Vec<u8>,
    },
    /// pnpm's JSON output did not have the shape this backend expects.
    InvalidOutput,
    /// The search query is empty, whitespace-only, or contains a NUL byte.
    InvalidQuery,
    /// The requested search limit is zero.
    InvalidLimit,
}

impl fmt::Display for PnpmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "pnpm process: {error}"),
            Self::Exit { status, stderr } => write!(
                f,
                "pnpm exited with {status}: {}",
                String::from_utf8_lossy(stderr).trim_end()
            ),
            Self::InvalidOutput => f.write_str("invalid pnpm JSON output"),
            Self::InvalidQuery => {
                f.write_str("invalid pnpm search query: empty or contains a NUL byte")
            }
            Self::InvalidLimit => f.write_str("pnpm search limit must be greater than zero"),
        }
    }
}

impl Error for PnpmError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Exit { .. } | Self::InvalidOutput | Self::InvalidQuery | Self::InvalidLimit => {
                None
            }
        }
    }
}

impl From<std::io::Error> for PnpmError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

fn parse_project(value: &Value) -> Result<Project, PnpmError> {
    let object = value.as_object().ok_or(PnpmError::InvalidOutput)?;
    Ok(Project {
        name: optional_string(object.get("name"))?,
        version: optional_string(object.get("version"))?,
        path: optional_string(object.get("path"))?,
        dependencies: parse_section(object.get("dependencies"))?,
        dev_dependencies: parse_section(object.get("devDependencies"))?,
        optional_dependencies: parse_section(object.get("optionalDependencies"))?,
        peer_dependencies: parse_section(object.get("peerDependencies"))?,
    })
}

fn optional_string(value: Option<&Value>) -> Result<Option<String>, PnpmError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(PnpmError::InvalidOutput),
    }
}

fn parse_section(value: Option<&Value>) -> Result<Vec<InstalledPackage>, PnpmError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let object = value.as_object().ok_or(PnpmError::InvalidOutput)?;
    let mut packages = Vec::with_capacity(object.len());
    for (name, entry) in object {
        let entry = entry.as_object().ok_or(PnpmError::InvalidOutput)?;
        let version = entry
            .get("version")
            .and_then(Value::as_str)
            .ok_or(PnpmError::InvalidOutput)?;
        if name.is_empty() || version.is_empty() {
            return Err(PnpmError::InvalidOutput);
        }
        packages.push(InstalledPackage {
            name: name.clone(),
            version: version.to_owned(),
        });
    }
    Ok(packages)
}

fn parse_search_result(value: &Value) -> Result<SearchResult, PnpmError> {
    let object = value.as_object().ok_or(PnpmError::InvalidOutput)?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or(PnpmError::InvalidOutput)?;
    let version = object
        .get("version")
        .and_then(Value::as_str)
        .ok_or(PnpmError::InvalidOutput)?;
    let description = optional_string(object.get("description"))?;
    if name.is_empty() || version.is_empty() {
        return Err(PnpmError::InvalidOutput);
    }
    Ok(SearchResult {
        name: name.to_owned(),
        version: version.to_owned(),
        description,
    })
}

/// Validates the conservative registry-name subset accepted by
/// [`PackageName::new`].
fn validate_name(name: &str) -> Result<(), InvalidPackage> {
    // The registry limit for a full package name, including a scope.
    if name.is_empty() || name.len() > 214 {
        return Err(InvalidPackage::Name);
    }
    if name.starts_with(['-', '.', '_']) {
        return Err(InvalidPackage::Name);
    }

    let (scope, body) = match name.split_once('/') {
        Some((scope, body)) => {
            let Some(scope) = scope.strip_prefix('@') else {
                return Err(InvalidPackage::Name);
            };
            if scope.is_empty() || body.is_empty() || body.contains(['/', '@']) {
                return Err(InvalidPackage::Name);
            }
            if scope.starts_with(['-', '.', '_']) || body.starts_with(['-', '.', '_']) {
                return Err(InvalidPackage::Name);
            }
            (Some(scope), body)
        }
        None => {
            if name.contains(['@', '/']) {
                return Err(InvalidPackage::Name);
            }
            (None, name)
        }
    };

    if !name_chars_ok(body) {
        return Err(InvalidPackage::Name);
    }
    if scope.is_some_and(|scope| !name_chars_ok(scope)) {
        return Err(InvalidPackage::Name);
    }
    Ok(())
}

fn name_chars_ok(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b".-_~".contains(&byte))
}

/// Accepts a complete semantic version without a leading `v`.
///
/// The rules follow Semantic Versioning 2.0.0: three numeric identifiers
/// without leading zeros, an optional dot-separated prerelease, and an
/// optional dot-separated build metadata section.
fn validate_version(version: &str) -> Result<(), InvalidPackage> {
    let (rest, build) = match version.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (version, None),
    };
    let (core, prerelease) = match rest.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (rest, None),
    };

    let mut numbers = core.split('.');
    for _ in 0..3 {
        let Some(number) = numbers.next() else {
            return Err(InvalidPackage::Version);
        };
        if !is_numeric_identifier(number) {
            return Err(InvalidPackage::Version);
        }
    }
    if numbers.next().is_some() {
        return Err(InvalidPackage::Version);
    }

    if let Some(prerelease) = prerelease {
        if prerelease.is_empty() {
            return Err(InvalidPackage::Version);
        }
        for identifier in prerelease.split('.') {
            if !is_version_identifier(identifier) {
                return Err(InvalidPackage::Version);
            }
            if identifier.bytes().all(|byte| byte.is_ascii_digit())
                && identifier.len() > 1
                && identifier.starts_with('0')
            {
                return Err(InvalidPackage::Version);
            }
        }
    }

    if let Some(build) = build {
        if build.is_empty() {
            return Err(InvalidPackage::Version);
        }
        for identifier in build.split('.') {
            if !is_version_identifier(identifier) {
                return Err(InvalidPackage::Version);
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

fn is_version_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
