//! Verified archive download and single-file installation.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Duration;

use globset::{Glob, GlobMatcher};
use regex::Regex;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

/// Maximum compressed download size in bytes.
pub const DEFAULT_DOWNLOAD_LIMIT: u64 = 512 * 1024 * 1024;
/// Maximum installed executable size in bytes.
pub const DEFAULT_FILE_LIMIT: u64 = 256 * 1024 * 1024;

/// Supported archive formats. `tar.xz` is the XZ-compressed tar format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// ZIP archive.
    Zip,
    /// Uncompressed tar.
    Tar,
    /// Gzip-compressed tar.
    TarGz,
    /// XZ-compressed tar.
    TarXz,
    /// Bzip2-compressed tar.
    TarBz2,
}

/// Select an archive member by matching its entire path.
#[derive(Clone, Debug)]
pub enum Selector {
    /// Glob pattern, using `/` as the archive path separator.
    Glob(String),
    /// Regular expression; add anchors to require a full-path match.
    Regex(String),
}

/// An explicitly requested command to run after installation.
///
/// This command is not run through a shell. It can execute arbitrary code
/// with the caller's privileges; only use trusted commands and arguments.
#[derive(Clone, Debug)]
pub struct PostInstall {
    /// Program path or executable resolved through PATH.
    pub program: PathBuf,
    /// Arguments passed directly to the program.
    pub args: Vec<OsString>,
}

/// A script that the archive itself provides, selected for explicit execution.
///
/// The script runs only as part of a checksum-verified installation
/// ([`install_https`] or [`install_reader`]); [`install_verified_archive`]
/// rejects it because that function does not check the archive digest.
///
/// Before anything runs, the selected member must be exactly one regular file
/// (links and special files are rejected), its path must be relative without
/// `..` components, and its extracted size is bounded by
/// [`InstallOptions::max_file_bytes`]. The member is validated and staged
/// before the destination is written; it executes after the executable is
/// installed and before [`InstallOptions::post_install`]. The script is written
/// to a private temporary directory and executed directly with
/// [`std::process::Command`] without an explicit shell wrapper. On Unix
/// the member must begin with `#!` and name an absolute interpreter, which may
/// itself be a shell. Platform execution mechanisms can use shell fallbacks.
/// Archive-script execution is not currently supported on non-Unix platforms.
/// The script
/// inherits the caller's environment and privileges and may do anything the
/// caller can, so only request one from an archive and checksum you trust.
#[derive(Clone, Debug)]
pub struct ArchiveScript {
    /// Glob or regular expression selecting exactly one regular file.
    pub selector: Selector,
    /// Arguments passed directly to the script.
    pub args: Vec<OsString>,
}

impl ArchiveScript {
    /// Select an archive-provided script with no arguments.
    pub fn new(selector: Selector) -> Self {
        Self {
            selector,
            args: Vec::new(),
        }
    }
}

/// Options for one verified archive installation.
#[derive(Clone, Debug)]
pub struct InstallOptions {
    /// Archive format; supplied explicitly rather than inferred from an untrusted URL.
    pub format: Format,
    /// Glob or regular expression identifying exactly one regular file.
    pub grep: Selector,
    /// Destination filename; if absent, use the selected member's filename.
    pub rename: Option<String>,
    /// Existing directory in which to install the file.
    pub target_dir: PathBuf,
    /// Expected SHA-256 digest as 64 hexadecimal characters.
    pub sha256: String,
    /// Download limit in bytes.
    pub max_download_bytes: u64,
    /// Extracted file limit in bytes.
    pub max_file_bytes: u64,
    /// Optional command run only after successful installation.
    pub post_install: Option<PostInstall>,
    /// Optional archive-provided script to extract and execute after
    /// installation. Absent by default: running an archive script is opt-in.
    /// It is rejected by [`install_verified_archive`], which does not verify
    /// the checksum, and runs before `post_install`.
    pub archive_script: Option<ArchiveScript>,
}

impl InstallOptions {
    /// Construct options with bounded download and extraction limits.
    pub fn new(
        format: Format,
        grep: Selector,
        target_dir: impl Into<PathBuf>,
        sha256: impl Into<String>,
    ) -> Self {
        Self {
            format,
            grep,
            rename: None,
            target_dir: target_dir.into(),
            sha256: sha256.into(),
            max_download_bytes: DEFAULT_DOWNLOAD_LIMIT,
            max_file_bytes: DEFAULT_FILE_LIMIT,
            post_install: None,
            archive_script: None,
        }
    }
}

/// An installation or verification failure.
#[derive(Debug)]
pub enum InstallError {
    /// An input or destination was invalid.
    Invalid(&'static str),
    /// A pattern could not be compiled.
    Pattern(String),
    /// The archive, destination, or command could not be read or written.
    Io(io::Error),
    /// HTTPS request or response failed.
    Http(reqwest::Error),
    /// ZIP decoding failed.
    Zip(zip::result::ZipError),
    /// The supplied digest did not match the downloaded bytes.
    ChecksumMismatch,
    /// The requested size limit was exceeded.
    SizeLimit,
    /// No regular archive entry matched.
    NoMatch,
    /// More than one regular archive entry matched.
    MultipleMatches,
    /// The explicit post-install command failed after the file was installed.
    PostInstallFailed(ExitStatus),
    /// The explicitly requested archive script failed to run successfully.
    ScriptFailed(ExitStatus),
}
impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => write!(f, "invalid archive installation: {e}"),
            Self::Pattern(e) => write!(f, "invalid selection pattern: {e}"),
            Self::Io(e) => write!(f, "archive I/O: {e}"),
            Self::Http(e) => write!(f, "archive download: {e}"),
            Self::Zip(e) => write!(f, "ZIP archive: {e}"),
            Self::ChecksumMismatch => f.write_str("archive SHA-256 checksum mismatch"),
            Self::SizeLimit => f.write_str("archive size limit exceeded"),
            Self::NoMatch => f.write_str("no archive member matched"),
            Self::MultipleMatches => f.write_str("multiple archive members matched"),
            Self::PostInstallFailed(status) => write!(
                f,
                "post-install command failed after installation: {status}"
            ),
            Self::ScriptFailed(status) => {
                write!(f, "archive script failed after installation: {status}")
            }
        }
    }
}
impl Error for InstallError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Http(e) => Some(e),
            Self::Zip(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for InstallError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<zip::result::ZipError> for InstallError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::Zip(e)
    }
}

enum Matcher {
    Glob(GlobMatcher),
    Regex(Regex),
}
impl Matcher {
    fn new(selector: &Selector) -> Result<Self, InstallError> {
        match selector {
            Selector::Glob(pattern) => Glob::new(&normalize_exe(pattern))
                .map(|x| Self::Glob(x.compile_matcher()))
                .map_err(|e| InstallError::Pattern(e.to_string())),
            Selector::Regex(pattern) => Regex::new(&normalize_exe(pattern))
                .map(Self::Regex)
                .map_err(|e| InstallError::Pattern(e.to_string())),
        }
    }
    fn matches(&self, path: &str) -> bool {
        match self {
            Self::Glob(m) => m.is_match(path),
            Self::Regex(m) => m.is_match(path),
        }
    }
}

fn normalize_exe(value: &str) -> String {
    if cfg!(windows) {
        value.to_owned()
    } else {
        value.replace(r"\.exe", "").replace(".exe", "")
    }
}

fn validate_name(name: &str) -> Result<&str, InstallError> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None)
            if !name.contains(['/', '\\', ':']) && name != "." && name != ".." =>
        {
            Ok(name)
        }
        _ => Err(InstallError::Invalid(
            "destination must be a single filename",
        )),
    }
}

fn validate_digest(digest: &str) -> Result<String, InstallError> {
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(InstallError::Invalid(
            "SHA-256 must be 64 hexadecimal characters",
        ));
    }
    Ok(digest.to_ascii_lowercase())
}

/// Download over HTTPS, verify SHA-256, and install exactly one regular file.
///
/// Redirects are restricted to HTTPS. The destination must not exist. This
/// function is blocking and never elevates privileges. If a post-install
/// command or an explicitly requested [`InstallOptions::archive_script`]
/// fails, the installed file remains in place. Archive scripts run only after
/// the checksum matches.
pub fn install_https(url: &str, options: &InstallOptions) -> Result<PathBuf, InstallError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| InstallError::Invalid("invalid URL"))?;
    if parsed.scheme() != "https" {
        return Err(InstallError::Invalid("HTTPS is required"));
    }
    validate_digest(&options.sha256)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 || attempt.url().scheme() != "https" {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(InstallError::Http)?;
    let response = client
        .get(parsed)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(InstallError::Http)?;
    if !response.status().is_success() {
        return Err(InstallError::Invalid("HTTPS redirect was not followed"));
    }
    install_reader(response, options)
}

/// Verify a local or caller-provided archive stream, then install one file.
///
/// The stream is bounded by `max_download_bytes` and is never unpacked before
/// its SHA-256 matches. This method does not authenticate the source of the
/// caller-provided checksum. An [`InstallOptions::archive_script`] runs only
/// after the digest matches and is bounded by `max_file_bytes`.
pub fn install_reader(
    mut reader: impl Read,
    options: &InstallOptions,
) -> Result<PathBuf, InstallError> {
    let expected = validate_digest(&options.sha256)?;
    let mut archive = NamedTempFile::new()?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 32 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or(InstallError::SizeLimit)?;
        if total > options.max_download_bytes {
            return Err(InstallError::SizeLimit);
        }
        hasher.update(&buffer[..count]);
        archive.write_all(&buffer[..count])?;
    }
    if format!("{:x}", hasher.finalize()) != expected {
        return Err(InstallError::ChecksumMismatch);
    }
    archive.rewind()?;
    install_inner(archive.as_file_mut(), options)
}

/// Install from an already verified local archive.
///
/// This function does not verify `options.sha256`; use [`install_https`] for
/// downloads. No archive paths are extracted directly to the filesystem. For
/// the same reason it rejects [`InstallOptions::archive_script`], because an
/// archive-provided script must only run after a checksum match.
pub fn install_verified_archive(
    file: &mut File,
    options: &InstallOptions,
) -> Result<PathBuf, InstallError> {
    if options.archive_script.is_some() {
        return Err(InstallError::Invalid(
            "archive scripts require a checksum-verified install",
        ));
    }
    install_inner(file, options)
}

fn install_inner(file: &mut File, options: &InstallOptions) -> Result<PathBuf, InstallError> {
    let matcher = Matcher::new(&options.grep)?;
    if let Some(name) = &options.rename {
        validate_name(&normalize_exe(name))?;
    }
    // Validate and extract an explicitly requested script before touching the
    // destination, so an unsafe or oversized script aborts the install.
    let script = match &options.archive_script {
        Some(script) => Some(prepare_script(file, options, script)?),
        None => None,
    };
    let mut staged = NamedTempFile::new_in(&options.target_dir)?;
    let selected = select_member(file, options, &matcher, &mut staged)?;
    let name = options.rename.as_deref().unwrap_or(&selected);
    let name = normalize_exe(name);
    validate_name(&name)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        staged
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))?;
    }
    let target = options.target_dir.join(name);
    staged
        .persist_noclobber(&target)
        .map_err(|e| InstallError::Io(e.error))?;
    if let (Some(script), Some((dir, staged))) = (&options.archive_script, script) {
        run_script(&dir, &staged, script)?;
    }
    if let Some(command) = &options.post_install {
        let status = Command::new(&command.program)
            .args(&command.args)
            .status()?;
        if !status.success() {
            return Err(InstallError::PostInstallFailed(status));
        }
    }
    Ok(target)
}

fn select_member(
    file: &mut File,
    options: &InstallOptions,
    matcher: &Matcher,
    dest: &mut NamedTempFile,
) -> Result<String, InstallError> {
    file.rewind()?;
    match options.format {
        Format::Zip => select_zip(file, matcher, dest, options.max_file_bytes),
        Format::Tar => select_tar(file, matcher, dest, options.max_file_bytes),
        Format::TarGz => select_tar(
            flate2::read::GzDecoder::new(file),
            matcher,
            dest,
            options.max_file_bytes,
        ),
        Format::TarXz => select_tar(
            xz2::read::XzDecoder::new(file),
            matcher,
            dest,
            options.max_file_bytes,
        ),
        Format::TarBz2 => select_tar(
            bzip2::read::BzDecoder::new(file),
            matcher,
            dest,
            options.max_file_bytes,
        ),
    }
}

/// Extract one explicitly selected regular file into a private directory.
///
/// The returned directory is removed when dropped. Selection reuses the same
/// member rules as the executable: exactly one regular file, a relative path
/// without `..`, and a size bounded by `max_file_bytes`.
fn prepare_script(
    file: &mut File,
    options: &InstallOptions,
    script: &ArchiveScript,
) -> Result<(tempfile::TempDir, tempfile::TempPath), InstallError> {
    #[cfg(not(unix))]
    {
        let _ = (file, options, script);
        return Err(InstallError::Invalid(
            "archive scripts are not supported on this platform",
        ));
    }
    #[cfg(unix)]
    {
        let matcher = Matcher::new(&script.selector)?;
        let dir = tempfile::TempDir::new()?;
        let mut staged = NamedTempFile::new_in(dir.path())?;
        select_member(file, options, &matcher, &mut staged)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o700))?;
            validate_shebang(staged.as_file_mut())?;
        }
        staged.as_file().flush()?;
        // Closing the writable handle before execution avoids `ETXTBSY`, and the
        // `TempPath` still removes the script when it is dropped.
        Ok((dir, staged.into_temp_path()))
    }
}

/// Require a `#!` line that names an absolute interpreter.
///
/// `execvp` falls back to `/bin/sh` when an `execve` call reports `ENOEXEC`,
/// which can run a shebang-less file through a shell. A bare `#!` line can do
/// the same, so both are rejected here.
#[cfg(unix)]
fn validate_shebang(file: &mut File) -> Result<(), InstallError> {
    file.rewind()?;
    let mut head = Vec::new();
    (&*file)
        .take(256)
        .read_to_end(&mut head)
        .map_err(InstallError::Io)?;
    let line = head.strip_prefix(b"#!").ok_or(InstallError::Invalid(
        "archive script must begin with a shebang",
    ))?;
    let line = line.split(|b| *b == b'\n').next().unwrap_or(line);
    let interpreter = line
        .split(|b| matches!(*b, b' ' | b'\t' | b'\r'))
        .find(|part| !part.is_empty());
    match interpreter {
        Some(path) if path.starts_with(b"/") => Ok(()),
        _ => Err(InstallError::Invalid(
            "archive script shebang must name an absolute interpreter",
        )),
    }
}

/// Execute a prepared archive script without an explicit shell wrapper.
///
/// `args` are passed verbatim and the private temporary directory is the
/// working directory; the caller's environment is inherited.
fn run_script(
    dir: &tempfile::TempDir,
    staged: &tempfile::TempPath,
    script: &ArchiveScript,
) -> Result<ExitStatus, InstallError> {
    let status = Command::new(staged)
        .args(&script.args)
        .current_dir(dir.path())
        .status()?;
    if status.success() {
        Ok(status)
    } else {
        Err(InstallError::ScriptFailed(status))
    }
}

fn basename(path: &str) -> Result<String, InstallError> {
    if path.starts_with('/') || path.contains('\\') || path.split('/').any(|part| part == "..") {
        return Err(InstallError::Invalid("unsafe archive member path"));
    }
    let name = path
        .rsplit('/')
        .next()
        .ok_or(InstallError::Invalid("missing archive filename"))?;
    validate_name(name)?;
    Ok(name.to_owned())
}

fn copy_bounded(
    reader: &mut impl Read,
    dest: &mut NamedTempFile,
    max: u64,
) -> Result<(), InstallError> {
    let copied = io::copy(&mut reader.take(max.saturating_add(1)), dest)?;
    if copied > max {
        return Err(InstallError::SizeLimit);
    }
    Ok(())
}

fn select_zip(
    file: &mut File,
    matcher: &Matcher,
    dest: &mut NamedTempFile,
    max: u64,
) -> Result<String, InstallError> {
    let mut archive = zip::ZipArchive::new(file)?;
    let mut selected = None;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() || !matcher.matches(entry.name()) {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 != 0 && mode & 0o170000 != 0o100000)
        {
            return Err(InstallError::Invalid(
                "selected ZIP member is not a regular file",
            ));
        }
        if selected.is_some() {
            return Err(InstallError::MultipleMatches);
        }
        selected = Some(basename(entry.name())?);
        copy_bounded(&mut entry, dest, max)?;
    }
    selected.ok_or(InstallError::NoMatch)
}

fn select_tar(
    reader: impl Read,
    matcher: &Matcher,
    dest: &mut NamedTempFile,
    max: u64,
) -> Result<String, InstallError> {
    let mut archive = tar::Archive::new(reader);
    let mut selected = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry
            .path()?
            .to_str()
            .ok_or(InstallError::Invalid("archive path is not UTF-8"))?
            .to_owned();
        if !matcher.matches(&path) {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(InstallError::Invalid(
                "selected tar member is not a regular file",
            ));
        }
        if selected.is_some() {
            return Err(InstallError::MultipleMatches);
        }
        selected = Some(basename(&path)?);
        copy_bounded(&mut entry, dest, max)?;
    }
    selected.ok_or(InstallError::NoMatch)
}
