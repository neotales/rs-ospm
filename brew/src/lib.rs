//! Explicit process backend for Homebrew formulae and casks.
//!
//! [`Brew`] invokes an injected `brew` executable directly, without a shell.
//! Formula and cask operations are separate methods because Homebrew keeps the
//! two namespaces separate. Inventory and outdated queries parse Homebrew's
//! published version 2 JSON documents; human-readable output is not parsed.
//! Homebrew does not publish a supported library interface for install
//! transactions, so mutations are explicit process calls.
//!
//! The crate does not implement the portable package-manager traits. Those
//! traits have no formula-or-cask argument, so a caller could not tell which
//! namespace a call touched.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use serde_json::Value;

/// Environment variables every wrapped operation sets.
///
/// These keep an operation from updating metadata, cleaning old files,
/// autoremoving unused dependencies, or repairing unrelated dependents. The
/// raw [`Brew::command`] does not apply them.
const OPERATION_ENVIRONMENT: [(&str, &str); 4] = [
    ("HOMEBREW_NO_AUTO_UPDATE", "1"),
    ("HOMEBREW_NO_AUTOREMOVE", "1"),
    ("HOMEBREW_NO_INSTALL_CLEANUP", "1"),
    ("HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK", "1"),
];

/// A Homebrew executable invoked as an external process.
///
/// The default executable is `brew` from `PATH`. The process inherits the
/// caller's environment, so Homebrew prefix, tap, proxy and credential
/// settings keep their usual meaning. This type never invokes a shell and
/// never elevates privileges.
#[derive(Clone, Debug)]
pub struct Brew {
    executable: PathBuf,
}

impl Default for Brew {
    fn default() -> Self {
        Self::new()
    }
}

impl Brew {
    /// Uses `brew` from the current process's `PATH`.
    pub fn new() -> Self {
        Self::with_executable("brew")
    }

    /// Uses an explicit executable, useful for a configured installation or a
    /// test fixture.
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    /// The configured executable; a relative path is resolved when spawned.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Creates a native `brew` command for operations this backend does not
    /// wrap.
    ///
    /// The command has the configured executable and no arguments, and it
    /// inherits the calling process's environment and working directory until
    /// the caller changes them. It does not receive the safety environment
    /// that the wrapped operations set. Arguments, environment, standard
    /// input, standard output, privilege and exit-status handling belong to
    /// the caller. This method does not execute the command.
    pub fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    fn operation_command(&self) -> Command {
        let mut command = self.command();
        for (key, value) in OPERATION_ENVIRONMENT {
            command.env(key, value);
        }
        command
    }

    /// Lists installed formulae and casks.
    ///
    /// Runs `brew info --json=v2 --installed` and returns the `formulae` and
    /// `casks` arrays. A package entry that Homebrew does not report as
    /// installed is omitted. A formula name uses Homebrew's fully qualified
    /// name when it reports one; a cask uses its full token when it reports
    /// one.
    pub fn installed(&self) -> Result<Installed, BrewError> {
        let mut command = self.operation_command();
        command.args(["info", "--json=v2", "--installed"]);
        let value = run_json(&mut command)?;
        parse_installed(&value)
    }

    /// Lists packages Homebrew considers outdated.
    ///
    /// Runs `brew outdated --json=v2` and returns the `formulae` and `casks`
    /// arrays. Pinned packages are reported as Homebrew reports them; this
    /// method does not filter them. Casks that Homebrew selects only with
    /// `--greedy` are not requested.
    pub fn outdated(&self) -> Result<Outdated, BrewError> {
        let mut command = self.operation_command();
        command.args(["outdated", "--json=v2"]);
        let value = run_json(&mut command)?;
        parse_outdated(&value)
    }

    /// Installs or checks the installation of one formula.
    ///
    /// Runs `brew install --formula --no-ask` with the formula name. Homebrew
    /// selects the current version; this backend cannot request an arbitrary
    /// version. The operation sets `HOMEBREW_NO_INSTALL_UPGRADE`, so an
    /// already installed but outdated formula is not upgraded; call
    /// [`upgrade_formula`](Brew::upgrade_formula) for that.
    ///
    /// Standard input is closed, so a prompt that reads standard input fails
    /// instead of blocking. A program that opens the controlling terminal
    /// directly can still block. The formula's build and install code runs
    /// with the calling user's permissions.
    pub fn install_formula(&self, formula: &FormulaName) -> Result<(), BrewError> {
        let mut command = self.operation_command();
        command.env("HOMEBREW_NO_INSTALL_UPGRADE", "1");
        command.args(["install", "--formula", "--no-ask"]);
        command.arg(formula.as_str());
        run(&mut command)
    }

    /// Installs or checks the installation of one cask.
    ///
    /// Runs `brew install --cask --no-ask` with the cask token. The operation
    /// sets `HOMEBREW_NO_INSTALL_UPGRADE`, so an already installed but
    /// outdated cask is not upgraded; call
    /// [`upgrade_cask`](Brew::upgrade_cask) for that.
    ///
    /// Standard input is closed. A prompt that reads standard input fails
    /// instead of blocking; a program that opens the controlling terminal
    /// directly can still block. A cask may require administrator approval,
    /// and this backend does not elevate privileges. Use
    /// [`command`](Brew::command) for an interactive install.
    pub fn install_cask(&self, cask: &CaskToken) -> Result<(), BrewError> {
        let mut command = self.operation_command();
        command.env("HOMEBREW_NO_INSTALL_UPGRADE", "1");
        command.args(["install", "--cask", "--no-ask"]);
        command.arg(cask.as_str());
        run(&mut command)
    }

    /// Uninstalls one formula.
    ///
    /// Runs `brew uninstall --formula` with the formula name. The operation
    /// sets `HOMEBREW_NO_AUTOREMOVE`, so Homebrew's automatic removal of
    /// unused formula dependencies is disabled. Configuration and cache files
    /// are not promised to be removed.
    pub fn uninstall_formula(&self, formula: &FormulaName) -> Result<(), BrewError> {
        let mut command = self.operation_command();
        command.args(["uninstall", "--formula"]);
        command.arg(formula.as_str());
        run(&mut command)
    }

    /// Uninstalls one cask without zapping it.
    ///
    /// Runs `brew uninstall --cask` with the cask token. This is a plain
    /// uninstall; it does not pass `--zap`, which can remove files shared with
    /// other applications. The operation sets `HOMEBREW_NO_AUTOREMOVE`.
    pub fn uninstall_cask(&self, cask: &CaskToken) -> Result<(), BrewError> {
        let mut command = self.operation_command();
        command.args(["uninstall", "--cask"]);
        command.arg(cask.as_str());
        run(&mut command)
    }

    /// Upgrades one installed formula.
    ///
    /// Runs `brew upgrade --formula --no-ask` with the formula name. Only the
    /// named formula is requested, but Homebrew may upgrade dependencies it
    /// needs. Homebrew does not upgrade a pinned formula. The operation sets
    /// `HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK`, so unrelated outdated
    /// dependents are not upgraded or reinstalled.
    pub fn upgrade_formula(&self, formula: &FormulaName) -> Result<(), BrewError> {
        let mut command = self.operation_command();
        command.args(["upgrade", "--formula", "--no-ask"]);
        command.arg(formula.as_str());
        run(&mut command)
    }

    /// Upgrades one installed cask.
    ///
    /// Runs `brew upgrade --cask --no-ask` with the cask token. Only the named
    /// cask is requested, but Homebrew may install its dependencies. Homebrew
    /// does not upgrade a pinned cask. The operation sets
    /// `HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK`, so unrelated outdated
    /// dependents are not upgraded or reinstalled.
    pub fn upgrade_cask(&self, cask: &CaskToken) -> Result<(), BrewError> {
        let mut command = self.operation_command();
        command.args(["upgrade", "--cask", "--no-ask"]);
        command.arg(cask.as_str());
        run(&mut command)
    }
}

fn run(command: &mut Command) -> Result<(), BrewError> {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .output()
        .map_err(BrewError::Io)?;
    check_status(output.status, output.stderr)
}

fn run_json(command: &mut Command) -> Result<Value, BrewError> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(BrewError::Io)?;
    check_status(output.status, output.stderr)?;
    serde_json::from_slice(&output.stdout).map_err(|_| BrewError::InvalidOutput)
}

fn check_status(status: ExitStatus, stderr: Vec<u8>) -> Result<(), BrewError> {
    if status.success() {
        Ok(())
    } else {
        Err(BrewError::Exit { status, stderr })
    }
}

/// A formula name that `brew` can receive as one argument.
///
/// A plain name such as `example-tool`, a versioned name such as
/// `example@1.2`, or a tap-qualified name such as `user/tap/example-tool` is
/// accepted. Each component may contain ASCII letters, digits, `.`, `_`, `@`,
/// `+` and `-`. A name that starts with `-`, `.`, `/`, `_` or `@` is rejected,
/// as are an empty component, a `.` or `..` component, more than three
/// components, whitespace and NUL bytes. Those rules keep the name from being
/// read as an option or as separate words. The constructor does not check
/// whether the formula exists.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FormulaName(String);

impl FormulaName {
    /// Validates a formula name.
    ///
    /// Local paths, URLs and option syntax are rejected. Homebrew accepts only
    /// a name in this constructor's grammar; use [`Brew::command`] for an
    /// operation this type cannot represent.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidName> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self(name))
    }

    /// The name as provided by the caller.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FormulaName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A cask token that `brew` can receive as one argument.
///
/// A plain token such as `example-app` or a tap-qualified token such as
/// `user/tap/example-app` is accepted. Each component may contain ASCII
/// letters, digits, `.`, `_`, `@`, `+` and `-`. A token that starts with `-`,
/// `.`, `/`, `_` or `@` is rejected, as are an empty component, a `.` or `..`
/// component, more than three components, whitespace and NUL bytes. Those
/// rules keep the token from being read as an option or as separate words. The
/// constructor does not check whether the cask exists.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CaskToken(String);

impl CaskToken {
    /// Validates a cask token.
    ///
    /// Local paths, URLs and option syntax are rejected. Homebrew accepts only
    /// a token in this constructor's grammar; use [`Brew::command`] for an
    /// operation this type cannot represent.
    pub fn new(token: impl Into<String>) -> Result<Self, InvalidName> {
        let token = token.into();
        validate_name(&token)?;
        Ok(Self(token))
    }

    /// The token as provided by the caller.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CaskToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A formula name or cask token that cannot be passed to `brew` safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidName;

impl fmt::Display for InvalidName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid Homebrew package name")
    }
}

impl Error for InvalidName {}

/// Installed formulae and casks reported by Homebrew.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Installed {
    /// Installed formulae.
    pub formulae: Vec<InstalledFormula>,
    /// Installed casks.
    pub casks: Vec<InstalledCask>,
}

/// One installed formula.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledFormula {
    /// Formula name as Homebrew reports it, using the fully qualified form
    /// when the formula comes from a tap.
    pub name: String,
    /// Installed keg versions, in Homebrew's order.
    pub versions: Vec<String>,
    /// The linked keg version, when Homebrew reports one.
    pub linked: Option<String>,
    /// Whether Homebrew reports the formula as pinned.
    pub pinned: bool,
    /// Whether Homebrew reports an available upgrade.
    pub outdated: bool,
}

/// One installed cask.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledCask {
    /// Cask token as Homebrew reports it, using the full token when the cask
    /// comes from a tap.
    pub token: String,
    /// Installed version as Homebrew reports it.
    pub version: String,
    /// Whether Homebrew reports the cask as pinned.
    pub pinned: bool,
    /// Whether Homebrew reports an available upgrade.
    pub outdated: bool,
}

/// Formulae and casks with an available upgrade.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Outdated {
    /// Outdated formulae.
    pub formulae: Vec<OutdatedPackage>,
    /// Outdated casks.
    pub casks: Vec<OutdatedPackage>,
}

/// One package with an available upgrade.
///
/// The same shape is used for a formula and a cask. [`name`](Self::name) is
/// the fully qualified formula name or the cask token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutdatedPackage {
    /// Fully qualified formula name or cask token.
    pub name: String,
    /// Installed versions Homebrew reports as outdated.
    pub installed_versions: Vec<String>,
    /// Version Homebrew would upgrade to.
    pub current_version: String,
    /// Whether the package is pinned.
    pub pinned: bool,
    /// The pinned version, when Homebrew reports one.
    pub pinned_version: Option<String>,
}

/// Failure invoking `brew` or reading its output.
#[derive(Debug)]
pub enum BrewError {
    /// The process could not be started or its output could not be read.
    Io(std::io::Error),
    /// `brew` returned an unsuccessful exit status.
    Exit {
        /// The process exit status.
        status: ExitStatus,
        /// Captured standard error, which can contain Homebrew diagnostics.
        stderr: Vec<u8>,
    },
    /// `brew`'s JSON output did not have the shape this backend expects.
    InvalidOutput,
}

impl fmt::Display for BrewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "brew process: {error}"),
            Self::Exit { status, stderr } => write!(
                f,
                "brew exited with {status}: {}",
                String::from_utf8_lossy(stderr).trim_end()
            ),
            Self::InvalidOutput => f.write_str("invalid brew JSON output"),
        }
    }
}

impl Error for BrewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Exit { .. } | Self::InvalidOutput => None,
        }
    }
}

impl From<std::io::Error> for BrewError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

fn parse_installed(value: &Value) -> Result<Installed, BrewError> {
    let object = value.as_object().ok_or(BrewError::InvalidOutput)?;
    let formulae = parse_installed_formulae(array(object.get("formulae"))?)?;
    let casks = parse_installed_casks(array(object.get("casks"))?)?;
    Ok(Installed { formulae, casks })
}

fn parse_installed_formulae(values: &[Value]) -> Result<Vec<InstalledFormula>, BrewError> {
    let mut formulae = Vec::new();
    for value in values {
        if let Some(formula) = parse_installed_formula(value)? {
            formulae.push(formula);
        }
    }
    Ok(formulae)
}

fn parse_installed_formula(value: &Value) -> Result<Option<InstalledFormula>, BrewError> {
    let object = value.as_object().ok_or(BrewError::InvalidOutput)?;
    let installed = array(object.get("installed"))?;
    if installed.is_empty() {
        return Ok(None);
    }
    let versions = installed
        .iter()
        .map(|entry| {
            let entry = entry.as_object().ok_or(BrewError::InvalidOutput)?;
            required_string(entry, "version")
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(InstalledFormula {
        name: preferred_string(object, "full_name", "name")?,
        versions,
        linked: optional_string(object.get("linked_keg"))?,
        pinned: bool_field(object, "pinned")?,
        outdated: bool_field(object, "outdated")?,
    }))
}

fn parse_installed_casks(values: &[Value]) -> Result<Vec<InstalledCask>, BrewError> {
    let mut casks = Vec::new();
    for value in values {
        if let Some(cask) = parse_installed_cask(value)? {
            casks.push(cask);
        }
    }
    Ok(casks)
}

fn parse_installed_cask(value: &Value) -> Result<Option<InstalledCask>, BrewError> {
    let object = value.as_object().ok_or(BrewError::InvalidOutput)?;
    let version = match object.get("installed") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(version)) if !version.is_empty() => version.clone(),
        Some(_) => return Err(BrewError::InvalidOutput),
    };
    Ok(Some(InstalledCask {
        token: preferred_string(object, "full_token", "token")?,
        version,
        pinned: bool_field(object, "pinned")?,
        outdated: bool_field(object, "outdated")?,
    }))
}

fn parse_outdated(value: &Value) -> Result<Outdated, BrewError> {
    let object = value.as_object().ok_or(BrewError::InvalidOutput)?;
    Ok(Outdated {
        formulae: parse_outdated_packages(array(object.get("formulae"))?)?,
        casks: parse_outdated_packages(array(object.get("casks"))?)?,
    })
}

fn parse_outdated_packages(values: &[Value]) -> Result<Vec<OutdatedPackage>, BrewError> {
    values.iter().map(parse_outdated_package).collect()
}

fn parse_outdated_package(value: &Value) -> Result<OutdatedPackage, BrewError> {
    let object = value.as_object().ok_or(BrewError::InvalidOutput)?;
    let installed_versions = array(object.get("installed_versions"))?
        .iter()
        .map(|version| {
            version
                .as_str()
                .filter(|version| !version.is_empty())
                .map(str::to_owned)
                .ok_or(BrewError::InvalidOutput)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if installed_versions.is_empty() {
        return Err(BrewError::InvalidOutput);
    }
    Ok(OutdatedPackage {
        name: required_string(object, "name")?,
        installed_versions,
        current_version: required_string(object, "current_version")?,
        pinned: bool_field(object, "pinned")?,
        pinned_version: optional_string(object.get("pinned_version"))?,
    })
}

fn array(value: Option<&Value>) -> Result<&[Value], BrewError> {
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or(BrewError::InvalidOutput)
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, BrewError> {
    match object.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(BrewError::InvalidOutput),
    }
}

fn preferred_string(
    object: &serde_json::Map<String, Value>,
    preferred: &str,
    fallback: &str,
) -> Result<String, BrewError> {
    for key in [preferred, fallback] {
        match object.get(key) {
            Some(Value::String(value)) if !value.is_empty() => return Ok(value.clone()),
            None | Some(Value::Null) => {}
            Some(_) => return Err(BrewError::InvalidOutput),
        }
    }
    Err(BrewError::InvalidOutput)
}

fn optional_string(value: Option<&Value>) -> Result<Option<String>, BrewError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(BrewError::InvalidOutput),
    }
}

fn bool_field(object: &serde_json::Map<String, Value>, key: &str) -> Result<bool, BrewError> {
    match object.get(key) {
        Some(Value::Bool(value)) => Ok(*value),
        None => Ok(false),
        _ => Err(BrewError::InvalidOutput),
    }
}

/// Validates the conservative name subset accepted by [`FormulaName::new`] and
/// [`CaskToken::new`].
fn validate_name(name: &str) -> Result<(), InvalidName> {
    // Homebrew names are much shorter; this bound keeps a malformed argument
    // from growing without limit while accepting tap-qualified names.
    if name.is_empty() || name.len() > 255 {
        return Err(InvalidName);
    }
    if name.starts_with(['-', '.', '/', '_', '@']) || name.ends_with('/') {
        return Err(InvalidName);
    }

    let components: Vec<&str> = name.split('/').collect();
    if components.len() > 3 {
        return Err(InvalidName);
    }
    for component in components {
        if component.is_empty() || component == "." || component == ".." {
            return Err(InvalidName);
        }
        if component.starts_with(['-', '.', '_', '@']) {
            return Err(InvalidName);
        }
        if !component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._@+-".contains(&byte))
        {
            return Err(InvalidName);
        }
    }
    Ok(())
}
