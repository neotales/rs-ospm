//! Validated values passed to the Deno command line.
//!
//! Every type in this module is constructed before a process starts. The
//! checks keep an executable name inside the installation bin directory, keep a
//! version exact, and keep an argument from breaking the launcher script Deno
//! generates.

use std::error::Error;
use std::fmt;
use std::path::Path;

const MAX_EXECUTABLE_NAME_LEN: usize = 128;
const MAX_PACKAGE_NAME_LEN: usize = 214;
const MAX_VERSION_LEN: usize = 256;
const MAX_URL_LEN: usize = 4096;

/// A global executable name that this provider will pass to `deno install`.
///
/// The name becomes a file name in the installation bin directory. It is
/// limited to ASCII letters, digits, `_`, `+` and `-`, and must start with a
/// letter, digit or `_`. A dot is rejected on every platform because Deno's
/// Windows installer replaces the file extension when it writes the `.cmd`
/// shim, so `tool.one` and `tool.two` would target the same `tool.cmd`.
///
/// Windows device names such as `CON`, `NUL`, `COM1` and `LPT1` are rejected
/// case-insensitively.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExecutableName(String);

impl ExecutableName {
    /// Validates an executable name.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidExecutableName> {
        let name = name.into();
        validate_executable_name(&name)?;
        Ok(Self(name))
    }

    /// The name as provided by the caller.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExecutableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An executable name that cannot be installed safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidExecutableName {
    /// The name is empty.
    Empty,
    /// The name is longer than 128 bytes.
    TooLong,
    /// The name starts with a reserved character or contains a character
    /// outside ASCII letters, digits, `_`, `+` and `-`.
    Character,
    /// The name is a Windows device name such as `CON` or `COM1`.
    ReservedWindowsName,
}

impl fmt::Display for InvalidExecutableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("executable name must not be empty"),
            Self::TooLong => f.write_str("executable name must be at most 128 bytes"),
            Self::Character => f.write_str(
                "executable name must start with a letter, digit or `_`, and use only ASCII letters, digits, `_`, `+` and `-`",
            ),
            Self::ReservedWindowsName => {
                f.write_str("executable name is reserved on Windows")
            }
        }
    }
}

impl Error for InvalidExecutableName {}

/// A registry package name in a Deno dependency.
///
/// Plain names such as `tool` and scoped names such as `@scope/tool` are
/// accepted for npm. A JSR package must be scoped and lowercase.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageName(String);

impl PackageName {
    /// Validates an npm package name, scoped or unscoped.
    pub fn npm(name: impl Into<String>) -> Result<Self, InvalidPackageName> {
        let name = name.into();
        validate_package_name(&name, false).map_err(|error| match error {
            InvalidPackage::Name => InvalidPackageName::Name,
            InvalidPackage::UnscopedJsr | InvalidPackage::Version => InvalidPackageName::Name,
        })?;
        Ok(Self(name))
    }

    /// Validates a scoped, lowercase JSR package name.
    pub fn jsr(name: impl Into<String>) -> Result<Self, InvalidPackageName> {
        let name = name.into();
        validate_package_name(&name, true).map_err(|error| match error {
            InvalidPackage::Name => InvalidPackageName::Name,
            InvalidPackage::UnscopedJsr => InvalidPackageName::UnscopedJsr,
            InvalidPackage::Version => InvalidPackageName::Name,
        })?;
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

/// A package name that cannot be passed to Deno safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidPackageName {
    /// The name is empty, too long, malformed, or contains an unsupported
    /// character.
    Name,
    /// A JSR package name is not scoped.
    UnscopedJsr,
}

impl fmt::Display for InvalidPackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name => f.write_str("invalid package name"),
            Self::UnscopedJsr => f.write_str("JSR package names must be scoped"),
        }
    }
}

impl Error for InvalidPackageName {}

/// The registry a package specifier addresses.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Registry {
    /// The JavaScript Registry, written as `jsr:`.
    Jsr,
    /// The npm registry, written as `npm:`.
    Npm,
}

impl Registry {
    /// The specifier prefix, including the colon.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Jsr => "jsr:",
            Self::Npm => "npm:",
        }
    }
}

/// A registry package pinned to one complete semantic version.
///
/// The version is passed to Deno as part of a `jsr:` or `npm:` specifier. A
/// range, dist-tag, branch name or abbreviated version is rejected because it
/// does not name one exact version.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RegistryPackage {
    registry: Registry,
    name: String,
    version: String,
    specifier: String,
}

impl RegistryPackage {
    /// Creates a scoped, lowercase JSR package pinned to `version`.
    pub fn jsr(
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, InvalidPackage> {
        Self::new(Registry::Jsr, name.into(), version.into())
    }

    /// Creates an npm package pinned to `version`.
    pub fn npm(
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, InvalidPackage> {
        Self::new(Registry::Npm, name.into(), version.into())
    }

    fn new(registry: Registry, name: String, version: String) -> Result<Self, InvalidPackage> {
        validate_package_name(&name, registry == Registry::Jsr)?;
        if !validate_version(&version) {
            return Err(InvalidPackage::Version);
        }
        let specifier = format!("{}{name}@{version}", registry.prefix());
        Ok(Self {
            registry,
            name,
            version,
            specifier,
        })
    }

    /// The registry this package addresses.
    pub fn registry(&self) -> Registry {
        self.registry
    }

    /// The package name, including a scope when present.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The exact version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The single `jsr:name@version` or `npm:name@version` argument.
    pub fn specifier(&self) -> &str {
        &self.specifier
    }
}

impl fmt::Display for RegistryPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.specifier)
    }
}

/// A package name or version that this provider will not pass to Deno.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidPackage {
    /// The name is empty, too long, malformed, or uses an unsupported
    /// character.
    Name,
    /// A JSR package name is not scoped.
    UnscopedJsr,
    /// The version is not a complete semantic version.
    Version,
}

impl fmt::Display for InvalidPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name => f.write_str("invalid package name"),
            Self::UnscopedJsr => f.write_str("JSR package names must be scoped"),
            Self::Version => {
                f.write_str("version must be a complete semantic version such as 1.2.3")
            }
        }
    }
}

impl Error for InvalidPackage {}

/// A script Deno can install as a global executable.
///
/// A script has no registry version, so this provider cannot pin one. Use
/// [`ScriptSource::path`] for a local file or [`ScriptSource::url`] for an
/// HTTPS module. Deno writes the resolved source into the launcher script, so a
/// URL must not contain user information or credentials.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScriptSource {
    argument: String,
}

impl ScriptSource {
    /// Accepts an absolute path to a local script.
    ///
    /// A relative path is rejected because the launcher is written into the
    /// installation bin directory and is run from an arbitrary working
    /// directory. The path must be valid UTF-8 and must not contain a double
    /// quote or a control character. On Windows, `!` is also rejected because
    /// the generated batch shim cannot reliably protect it from delayed
    /// expansion.
    pub fn path(path: impl AsRef<Path>) -> Result<Self, InvalidScript> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(InvalidScript::RelativePath);
        }
        let argument = path.to_str().ok_or(InvalidScript::NonUtf8Path)?.to_owned();
        validate_script_argument(&argument)?;
        Ok(Self { argument })
    }

    /// Accepts an absolute `https://` module URL.
    ///
    /// Plain HTTP, other schemes, empty hosts, user information and
    /// credential-bearing authority sections are rejected. A URL is written
    /// into the launcher script, so do not put a secret in its query string
    /// either; this constructor cannot detect every form of secret.
    pub fn url(url: impl Into<String>) -> Result<Self, InvalidScript> {
        let url = url.into();
        if url.len() > MAX_URL_LEN {
            return Err(InvalidScript::UrlTooLong);
        }
        let rest = url
            .strip_prefix("https://")
            .ok_or(InvalidScript::UnsupportedScheme)?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.is_empty() {
            return Err(InvalidScript::Url);
        }
        if authority.contains('@') {
            return Err(InvalidScript::UrlCredentials);
        }
        validate_script_argument(&url)?;
        Ok(Self { argument: url })
    }

    /// The absolute path or URL passed to Deno.
    pub fn as_argument(&self) -> &str {
        &self.argument
    }
}

/// A script source that cannot be installed safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidScript {
    /// A local script path is relative.
    RelativePath,
    /// A local script path is not valid UTF-8, so it cannot be written into a
    /// UTF-8 launcher script.
    NonUtf8Path,
    /// A URL does not use the `https://` scheme.
    UnsupportedScheme,
    /// A URL has no host, or is otherwise malformed.
    Url,
    /// A URL contains user information in its authority.
    UrlCredentials,
    /// A URL is longer than 4096 bytes.
    UrlTooLong,
    /// An argument contains a double quote, a control character, or on
    /// Windows, `!`.
    UnsafeCharacter,
}

impl fmt::Display for InvalidScript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath => f.write_str("local script path must be absolute"),
            Self::NonUtf8Path => f.write_str("local script path must be valid UTF-8"),
            Self::UnsupportedScheme => f.write_str("script URL must use https://"),
            Self::Url => f.write_str("invalid script URL"),
            Self::UrlCredentials => f.write_str("script URL must not contain user information"),
            Self::UrlTooLong => f.write_str("script URL must be at most 4096 bytes"),
            Self::UnsafeCharacter => f.write_str(
                "script argument contains a character the generated launcher cannot protect",
            ),
        }
    }
}

impl Error for InvalidScript {}

/// A global executable and the source Deno installs it from.
///
/// The executable name is explicit. Deno can infer a name from a package, but
/// this provider requires one so the target path and an existing installation
/// are known before the process starts.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GlobalTool {
    name: ExecutableName,
    target: GlobalTarget,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum GlobalTarget {
    Package(RegistryPackage),
    Script(ScriptSource),
}

impl GlobalTool {
    /// Installs a registry package under `name`.
    pub fn package(
        name: impl Into<String>,
        package: RegistryPackage,
    ) -> Result<Self, InvalidExecutableName> {
        Ok(Self {
            name: ExecutableName::new(name)?,
            target: GlobalTarget::Package(package),
        })
    }

    /// Installs a script under `name`.
    pub fn script(
        name: impl Into<String>,
        script: ScriptSource,
    ) -> Result<Self, InvalidExecutableName> {
        Ok(Self {
            name: ExecutableName::new(name)?,
            target: GlobalTarget::Script(script),
        })
    }

    /// The executable name.
    pub fn name(&self) -> &ExecutableName {
        &self.name
    }

    /// The requested exact version, or `None` for a script.
    pub fn version(&self) -> Option<&str> {
        match &self.target {
            GlobalTarget::Package(package) => Some(package.version()),
            GlobalTarget::Script(_) => None,
        }
    }

    /// The package specifier or script path passed to Deno.
    pub fn argument(&self) -> &str {
        match &self.target {
            GlobalTarget::Package(package) => package.specifier(),
            GlobalTarget::Script(script) => script.as_argument(),
        }
    }
}

/// The project manifest section a dependency is added to.
///
/// The section flag only changes a `package.json` project. Deno 2.9 records a
/// dependency in `deno.json` under `imports` regardless of this value, as
/// `deno add --help` states for `--dev` and `--save-optional`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DependencySection {
    /// `dependencies`. No section flag is passed.
    Regular,
    /// `devDependencies` in a `package.json` project.
    Development,
    /// `optionalDependencies` in a `package.json` project.
    Optional,
}

impl DependencySection {
    pub(crate) fn flag(self) -> Option<&'static str> {
        match self {
            Self::Regular => None,
            Self::Development => Some("--dev"),
            Self::Optional => Some("--save-optional"),
        }
    }
}

fn validate_executable_name(name: &str) -> Result<(), InvalidExecutableName> {
    if name.is_empty() {
        return Err(InvalidExecutableName::Empty);
    }
    if name.len() > MAX_EXECUTABLE_NAME_LEN {
        return Err(InvalidExecutableName::TooLong);
    }
    let mut bytes = name.bytes();
    let first = bytes.next().expect("the name is not empty");
    if !(first.is_ascii_alphanumeric() || first == b'_') {
        return Err(InvalidExecutableName::Character);
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-'))
    {
        return Err(InvalidExecutableName::Character);
    }
    if is_windows_reserved_name(name) {
        return Err(InvalidExecutableName::ReservedWindowsName);
    }
    Ok(())
}

fn is_windows_reserved_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = upper.as_bytes();
    bytes.len() == 4
        && (bytes.starts_with(b"COM") || bytes.starts_with(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
}

fn validate_package_name(name: &str, jsr: bool) -> Result<(), InvalidPackage> {
    if name.is_empty() || name.len() > MAX_PACKAGE_NAME_LEN || name.starts_with('-') {
        return Err(InvalidPackage::Name);
    }
    let (scope, package) = if let Some(rest) = name.strip_prefix('@') {
        let (scope, package) = rest.split_once('/').ok_or(InvalidPackage::Name)?;
        if package.contains('/') {
            return Err(InvalidPackage::Name);
        }
        (Some(scope), package)
    } else {
        if name.contains('@') || name.contains('/') {
            return Err(InvalidPackage::Name);
        }
        (None, name)
    };
    if jsr {
        if scope.is_none() {
            return Err(InvalidPackage::UnscopedJsr);
        }
        if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return Err(InvalidPackage::Name);
        }
    }
    if let Some(scope) = scope
        && !valid_package_part(scope)
    {
        return Err(InvalidPackage::Name);
    }
    if !valid_package_part(package) {
        return Err(InvalidPackage::Name);
    }
    Ok(())
}

fn valid_package_part(part: &str) -> bool {
    !part.is_empty()
        && !part.starts_with(['.', '_', '-'])
        && !part.ends_with('.')
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn validate_version(version: &str) -> bool {
    if version.is_empty() || version.len() > MAX_VERSION_LEN || version.trim() != version {
        return false;
    }
    let (without_build, build) = match version.split_once('+') {
        Some((rest, build)) => {
            if build.contains('+') {
                return false;
            }
            (rest, Some(build))
        }
        None => (version, None),
    };
    let (core, pre) = match without_build.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (without_build, None),
    };
    let mut parts = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    for part in [major, minor, patch] {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return false;
        }
    }
    if let Some(pre) = pre
        && !valid_identifiers(pre, true)
    {
        return false;
    }
    if let Some(build) = build
        && !valid_identifiers(build, false)
    {
        return false;
    }
    true
}

fn valid_identifiers(value: &str, numeric_leading_zero: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !(numeric_leading_zero
                    && identifier.len() > 1
                    && identifier.starts_with('0')
                    && identifier.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

fn validate_script_argument(argument: &str) -> Result<(), InvalidScript> {
    if argument.is_empty() || !argument_is_safe_for(argument, cfg!(windows)) {
        return Err(InvalidScript::UnsafeCharacter);
    }
    Ok(())
}

/// Returns whether an argument survives the launcher script Deno generates.
///
/// On Unix, Deno quotes each argument for a POSIX shell. On Windows, it writes a
/// batch file that wraps every argument in double quotes and doubles `%`. A
/// double quote or a line break cannot be protected on either platform. `!` is
/// rejected on Windows because cmd.exe can expand it when delayed expansion is
/// enabled by the caller's shell; Deno does not escape it.
fn argument_is_safe_for(argument: &str, windows: bool) -> bool {
    !argument.chars().any(|character| {
        character.is_control() || character == '"' || (windows && character == '!')
    })
}

#[cfg(test)]
mod tests {
    use super::argument_is_safe_for;

    #[test]
    fn unix_arguments_allow_batch_only_metacharacters() {
        for argument in ["https://example.test/a?x=1&y=2", "a!b", "a%b", "a^b"] {
            assert!(
                argument_is_safe_for(argument, false),
                "{argument:?} should be accepted on Unix"
            );
        }
    }

    #[test]
    fn windows_arguments_reject_delayed_expansion() {
        assert!(!argument_is_safe_for("a!b", true));
        assert!(argument_is_safe_for("a%b", true));
        assert!(argument_is_safe_for("a&b", true));
        assert!(!argument_is_safe_for("a\"b", true));
        assert!(!argument_is_safe_for("a\nb", true));
    }
}
