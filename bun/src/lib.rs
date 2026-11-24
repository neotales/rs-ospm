//! Explicit process backend for Bun project and global packages.
//!
//! [`Bun`] invokes an injected Bun executable directly, without a shell.
//! Project dependency operations and global package operations are separate
//! methods because a globally installed executable is not a dependency in a
//! project manifest. Reads use Bun's JSON package-manager command. Mutations
//! use the documented `add`, `remove` and `install` commands. The backend
//! never elevates privileges.
//!
//! The crate implements no portable package-manager trait. Those traits have no
//! scope argument, so a caller could not tell whether a call touched a global
//! package directory or one project's manifest.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use serde_json::Value;

/// A Bun executable invoked as an external process.
///
/// The default executable is `bun` from `PATH` (`bun.exe` on Windows). The
/// process inherits the caller's environment, so Bun's registry,
/// authentication, proxy, cache and global-location settings keep their usual
/// meaning. This type never invokes a shell and never elevates privileges.
#[derive(Clone, Debug)]
pub struct Bun {
    executable: PathBuf,
    project_directory: Option<PathBuf>,
    global_directory: Option<PathBuf>,
}

impl Default for Bun {
    fn default() -> Self {
        Self::new()
    }
}

impl Bun {
    /// Uses Bun from the current process's `PATH`.
    ///
    /// The executable is `bun` on Unix-like systems and `bun.exe` on Windows,
    /// where Bun is distributed as a normal executable rather than a batch-file
    /// shim.
    pub fn new() -> Self {
        if cfg!(windows) {
            Self::with_executable("bun.exe")
        } else {
            Self::with_executable("bun")
        }
    }

    /// Uses an explicit executable, useful for a configured installation or a
    /// test fixture.
    ///
    /// On Windows, pass a real executable such as `bun.exe`. A batch-file shim
    /// is started through the platform command interpreter, and this crate
    /// does not validate that escaping.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            project_directory: None,
            global_directory: None,
        }
    }

    /// Sets the working directory for project dependency operations.
    ///
    /// The path is passed to the spawned process as given, so a relative path
    /// is resolved against the caller's working directory. It is not a sandbox:
    /// Bun can still select an enclosing workspace, and it can still read
    /// configuration above the directory.
    pub fn with_project_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.project_directory = Some(directory.into());
        self
    }

    /// Sets the directory that contains Bun's global `package.json` and
    /// installed packages.
    ///
    /// This is normally the `install/global` directory below Bun's global
    /// install root, not the `bin` directory. It affects
    /// [`list_global`](Bun::list_global) only.
    /// [`add_global`](Bun::add_global) and [`remove_global`](Bun::remove_global)
    /// let Bun resolve its own global location from the process environment,
    /// so a configured directory must match that location. When this is unset,
    /// `list_global` uses `bun pm pkg get --global` and lets Bun resolve the
    /// global location.
    pub fn with_global_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.global_directory = Some(directory.into());
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

    /// The configured global directory, or `None` when Bun resolves it.
    pub fn global_directory(&self) -> Option<&Path> {
        self.global_directory.as_deref()
    }

    /// Creates a native Bun command for operations this backend does not wrap.
    ///
    /// The command inherits the calling process's environment and working
    /// directory until the caller changes them. Arguments, environment, stdio,
    /// privilege and exit-status handling are entirely the caller's
    /// responsibility. This method does not execute the command and does not
    /// apply the configured project directory.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    /// Creates a native Bun command in the configured project directory.
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

    /// Adds or updates a global package.
    ///
    /// Runs `bun add --global` with the package argument. When the spec carries
    /// an exact version, `--exact` is also passed so the global manifest keeps
    /// that version instead of a range. Bun resolves the global location from
    /// the process environment. The process receives no interactive standard
    /// input.
    pub fn add_global(&self, package: &PackageSpec) -> Result<(), BunError> {
        let mut command = self.command();
        command.arg("add").arg("--global");
        push_exactness(&mut command, package);
        command.arg(package.argument());
        run(&mut command)
    }

    /// Removes a global package.
    ///
    /// Runs `bun remove --global` with the package name. The command removes
    /// the package from Bun's global manifest and its global installation.
    pub fn remove_global(&self, package: &PackageName) -> Result<(), BunError> {
        let mut command = self.command();
        command.arg("remove").arg("--global").arg(package.as_str());
        run(&mut command)
    }

    /// Reads the dependency sections of Bun's global manifest.
    ///
    /// Without [`with_global_directory`], the manifest is read with
    /// `bun pm pkg get --global` and Bun resolves the global location. With a
    /// configured directory, that directory's `package.json` is read instead.
    /// The returned values are the specs recorded in the manifest, not resolved
    /// installed versions. A missing explicit directory or manifest is an
    /// error. Without an explicit directory, a failed Bun command is returned
    /// as an error, including when Bun reports a missing manifest.
    ///
    /// [`with_global_directory`]: Bun::with_global_directory
    pub fn list_global(&self) -> Result<DeclaredDependencies, BunError> {
        let Some(directory) = &self.global_directory else {
            return self.list_global_with_bun();
        };
        match path_kind(directory)? {
            PathKind::Missing => return Err(BunError::DirectoryInvalid(directory.clone())),
            PathKind::Directory => {}
            PathKind::Other => return Err(BunError::DirectoryInvalid(directory.clone())),
        }
        let manifest = directory.join("package.json");
        match manifest_kind(&manifest)? {
            ManifestKind::Missing => Err(BunError::ManifestMissing(manifest)),
            ManifestKind::File => self.read_declared_dependencies(directory),
            ManifestKind::Other => Err(BunError::ManifestInvalid(manifest)),
        }
    }

    /// Adds a dependency to the configured project.
    ///
    /// Runs `bun add` in the configured project directory with the selected
    /// section flag. [`DependencySection::Regular`] passes `--save-prod`, so an
    /// existing development dependency moves to `dependencies` instead of
    /// staying where it was. An exact version also passes `--exact`. The
    /// process receives no interactive standard input.
    pub fn add_project(
        &self,
        package: &PackageSpec,
        section: DependencySection,
    ) -> Result<(), BunError> {
        let mut command = self.project_command();
        command.arg("add").arg(section.flag());
        push_exactness(&mut command, package);
        command.arg(package.argument());
        run(&mut command)
    }

    /// Removes a dependency from the configured project.
    ///
    /// Runs `bun remove` in the configured project directory. Without a section
    /// flag Bun removes the package from every manifest section that contains
    /// it.
    pub fn remove_project(&self, package: &PackageName) -> Result<(), BunError> {
        let mut command = self.project_command();
        command.arg("remove").arg(package.as_str());
        run(&mut command)
    }

    /// Installs the dependencies declared by the configured project.
    ///
    /// Runs `bun install` in the configured project directory. When the
    /// directory belongs to a workspace, Bun selects the enclosing workspace;
    /// this backend cannot force the install to stay in one project. The
    /// process receives no interactive standard input.
    pub fn install_project(&self) -> Result<(), BunError> {
        let mut command = self.project_command();
        command.arg("install");
        run(&mut command)
    }

    /// Reads the dependency sections of the configured project's manifest.
    ///
    /// The manifest is read with `bun pm pkg get`. The returned values are the
    /// specs declared in the manifest, not resolved installed versions. The
    /// project directory defaults to the caller's working directory. A missing
    /// project directory or `package.json` is an error, unlike a missing global
    /// manifest.
    pub fn list_project(&self) -> Result<DeclaredDependencies, BunError> {
        let directory = match &self.project_directory {
            Some(directory) => directory.clone(),
            None => std::env::current_dir().map_err(BunError::Io)?,
        };
        match path_kind(&directory)? {
            PathKind::Directory => {}
            PathKind::Missing | PathKind::Other => {
                return Err(BunError::DirectoryInvalid(directory));
            }
        }
        let manifest = directory.join("package.json");
        match manifest_kind(&manifest)? {
            ManifestKind::File => self.read_declared_dependencies(&directory),
            ManifestKind::Missing => Err(BunError::ManifestMissing(manifest)),
            ManifestKind::Other => Err(BunError::ManifestInvalid(manifest)),
        }
    }

    fn read_declared_dependencies(
        &self,
        directory: &Path,
    ) -> Result<DeclaredDependencies, BunError> {
        let mut command = self.command();
        command.args(["pm", "pkg", "get"]);
        command.arg("--cwd").arg(directory);
        let value = run_json(&mut command)?;
        parse_declared_dependencies(&value)
    }

    fn list_global_with_bun(&self) -> Result<DeclaredDependencies, BunError> {
        let mut command = self.command();
        command.args(["pm", "pkg", "get", "--global"]);
        let output = command
            .stdin(Stdio::null())
            .output()
            .map_err(BunError::Io)?;
        if output.status.success() {
            let value =
                serde_json::from_slice(&output.stdout).map_err(|_| BunError::InvalidOutput)?;
            return parse_declared_dependencies(&value);
        }
        Err(BunError::Exit {
            status: output.status,
            stderr: output.stderr,
        })
    }
}

fn push_exactness(command: &mut Command, package: &PackageSpec) {
    if package.version().is_some() {
        command.arg("--exact");
    }
}

fn run(command: &mut Command) -> Result<(), BunError> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(BunError::Io)?;
    check_status(output.status, output.stderr)
}

fn run_json(command: &mut Command) -> Result<Value, BunError> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(BunError::Io)?;
    check_status(output.status, output.stderr)?;
    serde_json::from_slice(&output.stdout).map_err(|_| BunError::InvalidOutput)
}

fn check_status(status: ExitStatus, stderr: Vec<u8>) -> Result<(), BunError> {
    if status.success() {
        Ok(())
    } else {
        Err(BunError::Exit { status, stderr })
    }
}

/// A registry package name that Bun can receive as one argument.
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
    /// The accepted syntax is a conservative subset of Bun's package argument.
    /// Local paths, URLs, aliases and version suffixes are rejected; pass a
    /// version through [`PackageSpec`] instead.
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
    /// Creates a spec for the version Bun selects.
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

    /// The exact version, or `None` when Bun selects one.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The single package argument passed to Bun.
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
    /// `peerDependencies`.
    Peer,
}

impl DependencySection {
    fn flag(self) -> &'static str {
        match self {
            Self::Regular => "--save-prod",
            Self::Development => "--dev",
            Self::Optional => "--optional",
            Self::Peer => "--peer",
        }
    }
}

/// One dependency entry declared in a manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclaredPackage {
    /// Package name as Bun reports it.
    pub name: String,
    /// Dependency spec as Bun reports it, such as `1.2.3`, `^1.2.3`,
    /// `file:../dep` or `workspace:*`.
    pub spec: String,
}

/// The dependency sections Bun reports for a manifest.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeclaredDependencies {
    /// Direct regular dependencies.
    pub dependencies: Vec<DeclaredPackage>,
    /// Direct development dependencies.
    pub dev_dependencies: Vec<DeclaredPackage>,
    /// Direct optional dependencies.
    pub optional_dependencies: Vec<DeclaredPackage>,
    /// Direct peer dependencies.
    pub peer_dependencies: Vec<DeclaredPackage>,
}

impl DeclaredDependencies {
    /// Returns `true` when every section is empty.
    pub fn is_empty(&self) -> bool {
        self.dependencies.is_empty()
            && self.dev_dependencies.is_empty()
            && self.optional_dependencies.is_empty()
            && self.peer_dependencies.is_empty()
    }
}

/// A package name or version that cannot be passed to Bun safely.
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
            Self::Name => f.write_str("invalid Bun package name"),
            Self::Version => {
                f.write_str("version must be a complete semantic version such as 1.2.3")
            }
        }
    }
}

impl Error for InvalidPackage {}

/// Failure invoking Bun or reading its output.
#[derive(Debug)]
pub enum BunError {
    /// The process could not be started or its output could not be read.
    Io(std::io::Error),
    /// Bun returned an unsuccessful exit status.
    Exit {
        /// The process exit status.
        status: ExitStatus,
        /// Captured standard error, which can contain registry or build
        /// diagnostics.
        stderr: Vec<u8>,
    },
    /// Bun's JSON output did not have the shape this backend expects.
    InvalidOutput,
    /// The selected project directory has no `package.json`.
    ManifestMissing(PathBuf),
    /// The selected `package.json` exists but is not a regular file.
    ManifestInvalid(PathBuf),
    /// The selected directory is missing or is not a directory.
    DirectoryInvalid(PathBuf),
}

impl fmt::Display for BunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "bun process: {error}"),
            Self::Exit { status, stderr } => {
                let stderr = String::from_utf8_lossy(stderr);
                if stderr.trim().is_empty() {
                    write!(f, "bun exited with {status}")
                } else {
                    write!(f, "bun exited with {status}: {}", stderr.trim_end())
                }
            }
            Self::InvalidOutput => f.write_str("invalid bun JSON output"),
            Self::ManifestMissing(_) => f.write_str("no package.json in the selected directory"),
            Self::ManifestInvalid(_) => {
                f.write_str("the selected package.json is not a regular file")
            }
            Self::DirectoryInvalid(_) => {
                f.write_str("the selected directory is not a usable directory")
            }
        }
    }
}

impl Error for BunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Exit { .. }
            | Self::InvalidOutput
            | Self::ManifestMissing(_)
            | Self::ManifestInvalid(_)
            | Self::DirectoryInvalid(_) => None,
        }
    }
}

impl From<std::io::Error> for BunError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

fn parse_declared_dependencies(value: &Value) -> Result<DeclaredDependencies, BunError> {
    let object = value.as_object().ok_or(BunError::InvalidOutput)?;
    Ok(DeclaredDependencies {
        dependencies: parse_section(object.get("dependencies"))?,
        dev_dependencies: parse_section(object.get("devDependencies"))?,
        optional_dependencies: parse_section(object.get("optionalDependencies"))?,
        peer_dependencies: parse_section(object.get("peerDependencies"))?,
    })
}

fn parse_section(value: Option<&Value>) -> Result<Vec<DeclaredPackage>, BunError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let object = value.as_object().ok_or(BunError::InvalidOutput)?;
    let mut packages = Vec::with_capacity(object.len());
    for (name, spec) in object {
        validate_reported_name(name)?;
        let spec = spec.as_str().ok_or(BunError::InvalidOutput)?;
        if spec.is_empty() || spec.chars().any(char::is_control) {
            return Err(BunError::InvalidOutput);
        }
        packages.push(DeclaredPackage {
            name: name.clone(),
            spec: spec.to_owned(),
        });
    }
    Ok(packages)
}

/// Accepts a package name reported by Bun.
///
/// This check is deliberately looser than [`validate_name`]: a manifest can
/// contain names outside the subset accepted for new requests, but the output
/// is data rather than a command argument. The important checks are non-empty
/// text, a length bound, and no whitespace or control characters.
fn validate_reported_name(name: &str) -> Result<(), BunError> {
    const MAX_REPORTED_NAME_LEN: usize = 214;
    if name.is_empty()
        || name.len() > MAX_REPORTED_NAME_LEN
        || name.starts_with('-')
        || name
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b' ' | b'\\'))
    {
        return Err(BunError::InvalidOutput);
    }
    match name.split_once('/') {
        Some((scope, package)) => {
            if !scope.starts_with('@')
                || scope.len() == 1
                || package.is_empty()
                || package.contains('/')
            {
                return Err(BunError::InvalidOutput);
            }
        }
        None if name.starts_with('@') => return Err(BunError::InvalidOutput),
        None => {}
    }
    if name
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(BunError::InvalidOutput);
    }
    Ok(())
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

/// The state of a configured directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathKind {
    /// The path does not exist.
    Missing,
    /// The path resolves to a directory.
    Directory,
    /// The path exists but is not a usable directory.
    Other,
}

fn path_kind(path: &Path) -> Result<PathKind, BunError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(PathKind::Directory),
        Ok(_) => Ok(PathKind::Other),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(PathKind::Missing)
        }
        Err(error) => Err(BunError::Io(error)),
    }
}

/// The state of a manifest path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManifestKind {
    /// The path does not exist.
    Missing,
    /// The path resolves to a regular file.
    File,
    /// The path exists but is not a regular file.
    Other,
}

fn manifest_kind(path: &Path) -> Result<ManifestKind, BunError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(ManifestKind::File),
        Ok(_) => Ok(ManifestKind::Other),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(ManifestKind::Missing)
        }
        Err(error) => Err(BunError::Io(error)),
    }
}
