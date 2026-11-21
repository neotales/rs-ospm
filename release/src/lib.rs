//! Deterministic selection of platform-specific release assets.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

/// A release asset supplied by a hosting provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Asset {
    /// Asset name as reported by the provider.
    pub name: String,
    /// Download URL as reported by the provider.
    pub url: String,
}

/// Platform values used for selection and templates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    /// Operating-system identifier, such as `linux`, `macos`, or `windows`.
    pub os: String,
    /// Architecture identifier, such as `x86_64` or `aarch64`.
    pub arch: String,
}
impl Target {
    /// The platform for which this program was compiled.
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        }
    }
}

/// Explicit selection template and provider-specific value mappings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssetOptions {
    /// Exact asset name template using `{os}`, `{arch}`, and `{version}`.
    /// If absent, conservative platform and architecture inference is used.
    pub template: Option<String>,
    /// Per-target templates keyed by `os/arch`, taking precedence over `template`.
    pub target_templates: BTreeMap<String, String>,
    /// Map canonical OS identifiers to names used in a template.
    pub os_mapping: BTreeMap<String, String>,
    /// Map canonical architecture identifiers to names used in a template.
    pub arch_mapping: BTreeMap<String, String>,
}

/// Failure to resolve exactly one asset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectionError {
    /// The template contains an unknown or malformed placeholder.
    InvalidTemplate,
    /// The platform or architecture is unsupported for inference.
    UnsupportedTarget,
    /// No asset matched.
    NoMatch,
    /// More than one asset matched.
    Ambiguous,
}
impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTemplate => "invalid asset template",
            Self::UnsupportedTarget => "unsupported platform for asset inference",
            Self::NoMatch => "no matching release asset",
            Self::Ambiguous => "multiple matching release assets",
        })
    }
}
impl std::error::Error for SelectionError {}

fn render(
    template: &str,
    target: &Target,
    version: &str,
    options: &AssetOptions,
) -> Result<String, SelectionError> {
    let os = options.os_mapping.get(&target.os).unwrap_or(&target.os);
    let arch = options
        .arch_mapping
        .get(&target.arch)
        .unwrap_or(&target.arch);
    let mut output = String::new();
    let mut rest = template;
    while let Some(index) = rest.find('{') {
        let prefix = &rest[..index];
        if prefix.contains('}') {
            return Err(SelectionError::InvalidTemplate);
        }
        output.push_str(prefix);
        let tail = &rest[index + 1..];
        let end = tail.find('}').ok_or(SelectionError::InvalidTemplate)?;
        output.push_str(match &tail[..end] {
            "os" => os,
            "arch" => arch,
            "version" => version,
            _ => return Err(SelectionError::InvalidTemplate),
        });
        rest = &tail[end + 1..];
    }
    if rest.contains('}') {
        return Err(SelectionError::InvalidTemplate);
    }
    output.push_str(rest);
    Ok(output)
}

fn contains_token(name: &str, token: &str) -> bool {
    name.match_indices(token).any(|(start, _)| {
        let before = name[..start].chars().next_back();
        let after = name[start + token.len()..].chars().next();
        before.is_none_or(|c| !c.is_ascii_alphanumeric())
            && after.is_none_or(|c| !c.is_ascii_alphanumeric())
    })
}

/// Select one asset, rejecting ambiguous results rather than guessing.
///
/// Templates match the whole asset name. Automatic matching accepts supported
/// archive extensions and requires explicit OS and architecture tokens.
pub fn select_asset<'a>(
    assets: &'a [Asset],
    target: &Target,
    version: &str,
    options: &AssetOptions,
) -> Result<&'a Asset, SelectionError> {
    let target_key = format!("{}/{}", target.os, target.arch);
    let exact = options
        .target_templates
        .get(&target_key)
        .or(options.template.as_ref())
        .map(String::as_str)
        .map(|t| render(t, target, version, options))
        .transpose()?;
    let mut matches = Vec::new();
    if let Some(exact) = exact {
        matches.extend(assets.iter().filter(|a| a.name == exact));
    } else {
        let os: &[&str] = match target.os.as_str() {
            "linux" => &["linux"],
            "windows" => &["windows", "win"],
            "macos" => &["darwin", "macos", "osx"],
            _ => return Err(SelectionError::UnsupportedTarget),
        };
        let arch: &[&str] = match target.arch.as_str() {
            "x86_64" => &["x86_64", "amd64", "x64"],
            "aarch64" => &["aarch64", "arm64"],
            "x86" => &["386", "i386", "i686", "x86"],
            "arm" => &["arm", "armv7", "armv7l"],
            _ => return Err(SelectionError::UnsupportedTarget),
        };
        matches.extend(assets.iter().filter(|asset| {
            let name = asset.name.to_ascii_lowercase();
            [
                ".zip", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".tbz2",
            ]
            .iter()
            .any(|ext| name.ends_with(ext))
                && os.iter().any(|token| contains_token(&name, token))
                && arch.iter().any(|token| contains_token(&name, token))
                && !(target.arch == "x86" && contains_token(&name, "x86_64"))
        }));
    }
    match matches.as_slice() {
        [] => Err(SelectionError::NoMatch),
        [asset] => Ok(asset),
        _ => Err(SelectionError::Ambiguous),
    }
}
