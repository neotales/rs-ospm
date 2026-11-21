//! Strict subset of Aqua registry recipes for GitHub release archives.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod expr;
mod template;
mod version;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use neotales_ospm_archive::{Format, InstallOptions, Selector};
use neotales_ospm_github::Github;
use neotales_ospm_release::{AssetOptions, Target};
use serde::Deserialize;

/// A parsed registry containing the supported recipe subset.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    /// Registry packages.
    pub packages: Vec<Recipe>,
}

/// A GitHub release recipe.
///
/// Metadata is retained, version selection is evaluated, and security features
/// that this crate cannot enforce are rejected during parsing rather than being
/// silently ignored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    repo_owner: String,
    #[serde(default)]
    repo_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    asset: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    link: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    search_words: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    aliases: Vec<Alias>,
    #[serde(default)]
    replacements: BTreeMap<String, String>,
    #[serde(default)]
    overrides: Vec<Override>,
    #[serde(default)]
    format_overrides: Vec<FormatOverride>,
    #[serde(default)]
    supported_envs: Option<Vec<String>>,
    #[serde(default)]
    files: Vec<RecipeFile>,
    #[serde(default)]
    version_constraint: Option<String>,
    #[serde(default)]
    version_overrides: Vec<VersionOverride>,
    #[serde(default)]
    version_prefix: Option<String>,
    #[serde(default)]
    version_filter: Option<String>,
    #[serde(default)]
    version_source: Option<String>,
    #[serde(default)]
    checksum: Option<Checksum>,
    #[serde(default)]
    append_ext: Option<bool>,
    #[serde(default)]
    rosetta2: bool,
    #[serde(default)]
    windows_arm_emulation: bool,
    #[serde(default)]
    complete_windows_ext: Option<bool>,
    #[serde(default)]
    windows_ext: Option<String>,
    #[serde(default)]
    no_asset: bool,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(rename = "crate", default)]
    crate_name: Option<String>,
}

/// An alternative package name. It is metadata only.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Alias {
    name: String,
}

/// A platform-specific override.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    #[serde(default)]
    goos: Option<String>,
    #[serde(default)]
    goarch: Option<String>,
    #[serde(default)]
    envs: Option<Vec<String>>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    asset: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    replacements: Option<BTreeMap<String, String>>,
    #[serde(default)]
    files: Option<Vec<RecipeFile>>,
    #[serde(default)]
    checksum: Option<Checksum>,
    #[serde(default)]
    append_ext: Option<bool>,
    #[serde(default)]
    complete_windows_ext: Option<bool>,
    #[serde(default)]
    windows_ext: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(rename = "crate", default)]
    crate_name: Option<String>,
    #[serde(default)]
    variants: Vec<Variant>,
}

/// An additional platform condition, such as a C library variant.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Variant {
    key: String,
    value: String,
}

/// An operating-system format override.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FormatOverride {
    goos: String,
    format: String,
}

/// A version-specific recipe override.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionOverride {
    #[serde(default)]
    version_constraint: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    repo_owner: Option<String>,
    #[serde(default)]
    repo_name: Option<String>,
    #[serde(default)]
    asset: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    files: Option<Vec<RecipeFile>>,
    #[serde(default)]
    replacements: Option<BTreeMap<String, String>>,
    #[serde(default)]
    overrides: Option<Vec<Override>>,
    #[serde(default)]
    format_overrides: Option<Vec<FormatOverride>>,
    #[serde(default)]
    supported_envs: Option<Vec<String>>,
    #[serde(default)]
    checksum: Option<Checksum>,
    #[serde(default)]
    version_prefix: Option<String>,
    #[serde(default)]
    version_filter: Option<String>,
    #[serde(default)]
    version_source: Option<String>,
    #[serde(default)]
    append_ext: Option<bool>,
    #[serde(default)]
    rosetta2: Option<bool>,
    #[serde(default)]
    windows_arm_emulation: Option<bool>,
    #[serde(default)]
    complete_windows_ext: Option<bool>,
    #[serde(default)]
    windows_ext: Option<String>,
    #[serde(default)]
    no_asset: Option<bool>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(rename = "crate", default)]
    crate_name: Option<String>,
}

/// A file selected from the release archive.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeFile {
    name: String,
    #[serde(default)]
    src: Option<String>,
    /// Used only by Go package recipes, so it is ignored here.
    #[serde(default)]
    #[allow(dead_code)]
    dir: Option<String>,
    /// Symlink aliases are not implemented.
    #[serde(default)]
    link: Option<String>,
    /// Hardlink aliases are not implemented.
    #[serde(default)]
    hard: Option<bool>,
}

/// An upstream checksum description. Signing fields are intentionally absent so
/// `deny_unknown_fields` rejects them.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checksum {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    asset: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    file_format: Option<String>,
    #[serde(default)]
    algorithm: Option<String>,
    #[serde(default)]
    pattern: Option<ChecksumPattern>,
    #[serde(default)]
    replacements: Option<BTreeMap<String, String>>,
}

/// Regular expressions that extract a digest and file name.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChecksumPattern {
    checksum: String,
    #[serde(default)]
    file: Option<String>,
}

/// Effective configuration after applying a matching version override.
#[derive(Debug)]
struct Effective {
    kind: String,
    repo_owner: String,
    repo_name: String,
    package_name: Option<String>,
    asset: String,
    format: String,
    files: Vec<RecipeFile>,
    replacements: BTreeMap<String, String>,
    overrides: Vec<Override>,
    format_overrides: Vec<FormatOverride>,
    supported_envs: Option<Vec<String>>,
    checksum: Option<Checksum>,
    version_prefix: String,
    version_filter: Option<String>,
    version_source: Option<String>,
    append_ext: bool,
    rosetta2: bool,
    windows_arm_emulation: bool,
    complete_windows_ext: Option<bool>,
    windows_ext: Option<String>,
    no_asset: bool,
    error_message: Option<String>,
    url: Option<String>,
    path: Option<String>,
    crate_name: Option<String>,
}

/// Where a recipe says its release publishes checksums.
///
/// This value is descriptive only. A checksum file downloaded from the same
/// release as the asset is not an independent trust source, so installation
/// never uses it. [`Recipe::plan`] still requires a trusted SHA-256 supplied by
/// the caller, and `algorithm` may name a digest this crate does not compute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpstreamChecksum {
    /// Source kind: `github_release` or `http`.
    pub kind: String,
    /// Rendered checksum asset name for `github_release`.
    pub asset: Option<String>,
    /// Rendered checksum URL for `http`.
    pub url: Option<String>,
    /// `raw`, `regexp`, or empty for the default line-oriented format.
    pub file_format: String,
    /// Algorithm named by the recipe; empty means the upstream default.
    pub algorithm: String,
    /// Regular expression that captures the digest, when `file_format` is `regexp`.
    pub checksum_pattern: Option<String>,
    /// Regular expression that captures the file name, when present.
    pub file_pattern: Option<String>,
}

/// A resolved single-file installation plan.
#[derive(Debug)]
pub struct Plan {
    /// GitHub repository owner.
    pub owner: String,
    /// GitHub repository name.
    pub repository: String,
    /// Exact release tag.
    pub tag: String,
    /// Exact asset name.
    pub asset: String,
    /// Archive installation options.
    pub install: InstallOptions,
    /// Target used to resolve the asset and installed file.
    pub target: Target,
    /// Upstream checksum description from the recipe, if enabled.
    ///
    /// This is not a trust source and is not used by [`Plan::install`].
    pub upstream_checksum: Option<UpstreamChecksum>,
}

/// Failure parsing, planning, or installing a recipe.
#[derive(Debug)]
pub enum Error {
    /// Registry YAML is malformed or contains unsupported fields.
    Yaml(String),
    /// Requested package was not found or was duplicated.
    Package,
    /// The recipe cannot be represented by this implementation.
    Unsupported(&'static str),
    /// The plan's target cannot be installed on this host.
    Target(&'static str),
    /// GitHub discovery or installation failed.
    Github(neotales_ospm_github::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml(e) => write!(f, "invalid or unsupported registry: {e}"),
            Self::Package => f.write_str("registry package missing or ambiguous"),
            Self::Unsupported(e) => write!(f, "unsupported registry recipe: {e}"),
            Self::Target(e) => write!(f, "unsupported installation target: {e}"),
            Self::Github(e) => write!(f, "GitHub release: {e}"),
        }
    }
}
impl std::error::Error for Error {}

impl Registry {
    /// Parse a registry.
    ///
    /// Unsupported fields cause an error rather than being silently ignored.
    /// In particular, `cosign`, `slsa_provenance`, `minisign`, `packslip`,
    /// `github_artifact_attestations`, and `github_immutable_release` are
    /// rejected here, including when nested in a checksum block.
    pub fn parse(yaml: &str) -> Result<Self, Error> {
        serde_saphyr::from_str(yaml).map_err(|e| Error::Yaml(e.to_string()))
    }

    /// Select one recipe by its `owner/repository` identifier.
    pub fn package(&self, id: &str) -> Result<&Recipe, Error> {
        let mut found = self
            .packages
            .iter()
            .filter(|p| format!("{}/{}", p.repo_owner, p.repo_name) == id);
        let recipe = found.next().ok_or(Error::Package)?;
        if found.next().is_some() {
            return Err(Error::Package);
        }
        Ok(recipe)
    }
}

/// Map the effective recipe onto a target and exact version.
impl Recipe {
    fn effective(&self, version: &str) -> Result<Effective, Error> {
        let base_prefix = self.version_prefix.clone().unwrap_or_default();
        let mut effective = Effective {
            kind: self.kind.clone(),
            repo_owner: self.repo_owner.clone(),
            repo_name: self.repo_name.clone(),
            package_name: self.name.clone(),
            asset: self.asset.clone(),
            format: self.format.clone(),
            files: self.files.clone(),
            replacements: self.replacements.clone(),
            overrides: self.overrides.clone(),
            format_overrides: self.format_overrides.clone(),
            supported_envs: self.supported_envs.clone(),
            checksum: self.checksum.clone(),
            version_prefix: base_prefix.clone(),
            version_filter: self.version_filter.clone(),
            version_source: self.version_source.clone(),
            append_ext: self.append_ext.unwrap_or(true),
            rosetta2: self.rosetta2,
            windows_arm_emulation: self.windows_arm_emulation,
            complete_windows_ext: self.complete_windows_ext,
            windows_ext: self.windows_ext.clone(),
            no_asset: self.no_asset,
            error_message: self.error_message.clone(),
            url: self.url.clone(),
            path: self.path.clone(),
            crate_name: self.crate_name.clone(),
        };
        let constraint = self.version_constraint.as_deref().unwrap_or("").trim();
        if !constraint.is_empty() {
            let prefix_matches = base_prefix.is_empty() || version.starts_with(&base_prefix);
            let semver = trim_prefix(version, &base_prefix);
            let matched = prefix_matches && evaluate_constraint(constraint, version, semver, true)?;
            if !matched {
                let mut found = false;
                for candidate in &self.version_overrides {
                    let Some(candidate_constraint) = candidate
                        .version_constraint
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                    else {
                        continue;
                    };
                    let prefix = candidate
                        .version_prefix
                        .as_deref()
                        .unwrap_or(base_prefix.as_str());
                    if !version.starts_with(prefix) {
                        continue;
                    }
                    let candidate_semver = &version[prefix.len()..];
                    if evaluate_constraint(candidate_constraint, version, candidate_semver, true)? {
                        apply_version_override(&mut effective, candidate)?;
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(Error::Unsupported("no matching version constraint"));
                }
            }
        }
        if let Some(filter) = effective
            .version_filter
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let context = expr::Context {
                version,
                semver: version,
                allow_semver_variable: false,
            };
            match expr::evaluate(filter, &context) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(Error::Unsupported("version_filter excludes this version"));
                }
                Err(()) => return Err(Error::Unsupported("version_filter expression")),
            }
        }
        Ok(effective)
    }

    /// Resolve the supported recipe subset for a target and exact release tag.
    ///
    /// Version constraints, version overrides, format overrides, platform
    /// overrides, and supported environments are applied in Aqua's order. The
    /// checksum must be a 64-character SHA-256 supplied from an independently
    /// trusted source; a checksum named by the recipe is not used.
    pub fn plan(
        &self,
        target: &Target,
        version: &str,
        target_dir: &Path,
        sha256: &str,
    ) -> Result<Plan, Error> {
        if version.is_empty() {
            return Err(Error::Unsupported("empty version"));
        }
        validate_sha256(sha256)?;
        let mut effective = self.effective(version)?;
        if effective.kind != "github_release" {
            return Err(Error::Unsupported("package type"));
        }
        if effective.repo_owner.is_empty() || effective.repo_name.is_empty() {
            return Err(Error::Unsupported("repository"));
        }
        if effective
            .version_source
            .as_deref()
            .is_some_and(|value| !value.is_empty() && value != "github_tag")
        {
            return Err(Error::Unsupported("version source"));
        }
        if [&effective.url, &effective.path, &effective.crate_name]
            .into_iter()
            .any(|value| value.as_deref().is_some_and(|value| !value.is_empty()))
        {
            return Err(Error::Unsupported("non-release package field"));
        }
        if effective.no_asset
            || effective
                .error_message
                .as_deref()
                .is_some_and(|message| !message.is_empty())
        {
            return Err(Error::Unsupported("version has no release asset"));
        }
        let (os, arch) = go_target(target)?;

        // Format overrides are applied before platform overrides, which can
        // still replace the format.
        let mut format = effective.format.clone();
        if let Some(override_) = effective
            .format_overrides
            .iter()
            .find(|override_| override_.goos == os)
        {
            format = override_.format.clone();
        }
        if let Some(override_) = effective
            .overrides
            .iter()
            .find(|override_| override_matches(override_, os, arch))
        {
            if let Some(kind) = override_.kind.as_deref().filter(|value| !value.is_empty())
                && kind != "github_release"
            {
                return Err(Error::Unsupported("package type"));
            }
            if !override_.variants.is_empty() {
                return Err(Error::Unsupported("platform variant"));
            }
            if [&override_.url, &override_.path, &override_.crate_name]
                .into_iter()
                .any(|value| value.as_deref().is_some_and(|value| !value.is_empty()))
            {
                return Err(Error::Unsupported("non-release package field"));
            }
            if let Some(value) = override_.asset.as_deref().filter(|value| !value.is_empty()) {
                effective.asset = value.to_owned();
            }
            if let Some(value) = override_
                .format
                .as_deref()
                .filter(|value| !value.is_empty())
            {
                format = value.to_owned();
            }
            if let Some(files) = &override_.files {
                effective.files = files.clone();
            }
            if let Some(replacements) = &override_.replacements {
                effective.replacements.extend(replacements.clone());
            }
            if let Some(checksum) = &override_.checksum {
                effective.checksum = Some(checksum.clone());
            }
            if let Some(value) = override_.append_ext {
                effective.append_ext = value;
            }
            if let Some(value) = override_.complete_windows_ext {
                effective.complete_windows_ext = Some(value);
            }
            if let Some(value) = &override_.windows_ext {
                effective.windows_ext = Some(value.clone());
            }
        }
        if let Some(envs) = &effective.supported_envs
            && !env_matches(
                envs,
                os,
                arch,
                effective.rosetta2,
                effective.windows_arm_emulation,
            )
        {
            return Err(Error::Unsupported("target is not in supported_envs"));
        }

        let emulate_amd64 = (effective.rosetta2 && os == "darwin" && arch == "arm64")
            || (effective.windows_arm_emulation && os == "windows" && arch == "arm64");
        let arch_for_template = if emulate_amd64 { "amd64" } else { arch };
        let mapped_os = mapped(&effective.replacements, os).to_owned();
        let mapped_arch = mapped(&effective.replacements, arch_for_template).to_owned();
        let semver = trim_prefix(version, &effective.version_prefix);
        let windows_ext = effective
            .windows_ext
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(".exe");
        let complete_windows_ext = effective.complete_windows_ext.unwrap_or(true);

        let mut asset = render(
            &effective.asset,
            RenderValues {
                os: &mapped_os,
                arch: &mapped_arch,
                goos: os,
                goarch: arch,
                version,
                semver,
                format: &format,
            },
            None,
            None,
            None,
        )?;
        if effective.append_ext {
            asset = append_extension(&asset, &format);
        }
        if os == "windows" {
            asset = complete_windows_asset(&asset, &format, complete_windows_ext, windows_ext);
        }
        if asset.is_empty() {
            return Err(Error::Unsupported("empty asset name"));
        }

        let files = if effective.files.is_empty() {
            vec![RecipeFile {
                name: default_file_name(effective.package_name.as_deref(), &effective.repo_name),
                src: None,
                dir: None,
                link: None,
                hard: None,
            }]
        } else {
            effective.files.clone()
        };
        if files.len() != 1 {
            return Err(Error::Unsupported("exactly one file is required"));
        }
        let file = &files[0];
        if file.link.is_some() || file.hard == Some(true) {
            return Err(Error::Unsupported("file link aliases"));
        }
        let asset_without_ext = remove_extension(&asset);
        let mut source = match file.src.as_deref().filter(|value| !value.is_empty()) {
            Some(source) => render(
                source,
                RenderValues {
                    os: &mapped_os,
                    arch: &mapped_arch,
                    goos: os,
                    goarch: arch,
                    version,
                    semver,
                    format: &format,
                },
                Some(&asset),
                Some(asset_without_ext),
                Some(&file.name),
            )?,
            None => file.name.clone(),
        };
        let mut destination = file.name.clone();
        if os == "windows" {
            if !has_extension(&source, version) {
                source.push_str(windows_ext);
            }
            if !has_extension(&destination, version) {
                destination.push_str(windows_ext);
            }
        }
        validate_file_name(&destination)?;
        let pattern = format!("^{}$", regex::escape(&source));

        let format_name = if format.is_empty() {
            inferred_format(&asset)?
        } else {
            format.as_str()
        };
        let format_enum = format_for(format_name)?;

        let upstream_checksum = match &effective.checksum {
            Some(checksum) if checksum.enabled.unwrap_or(true) => Some(build_checksum(
                checksum,
                &effective,
                &asset,
                RenderValues {
                    os: &mapped_os,
                    arch: &mapped_arch,
                    goos: os,
                    goarch: arch,
                    version,
                    semver,
                    format: &format,
                },
                arch_for_template,
            )?),
            _ => None,
        };

        let mut install =
            InstallOptions::new(format_enum, Selector::Regex(pattern), target_dir, sha256);
        install.rename = Some(destination);
        // The plan already resolved the exact source path and applied any
        // target-specific extension completion. Archive normalization would
        // rewrite an exact `.exe` source on non-Windows hosts and could select
        // a different member.
        install.normalize_exe_suffix = false;
        let _ = &self.description;
        Ok(Plan {
            owner: effective.repo_owner,
            repository: effective.repo_name,
            tag: version.into(),
            asset,
            install,
            target: target.clone(),
            upstream_checksum,
        })
    }
}

impl Plan {
    /// Fetch the exact release and asset, then verify and install it.
    ///
    /// A plan for a different operating system than the host is rejected
    /// before any request. The archive layer normalizes executable extensions
    /// for the host, so a cross-target download could select the wrong member.
    pub fn install(&self, client: &Github) -> Result<PathBuf, Error> {
        if !target_matches_host(&self.target.os, Target::current().os.as_str()) {
            return Err(Error::Target("cross-target installation"));
        }
        let release = client
            .release(&self.owner, &self.repository, Some(&self.tag))
            .map_err(Error::Github)?;
        let selection = AssetOptions {
            template: Some(self.asset.clone()),
            ..Default::default()
        };
        release
            .install(&self.target, &selection, &self.install)
            .map_err(Error::Github)
    }
}

/// Test whether a plan target can be installed on this host.
///
/// The comparison is deliberately two-way: extension handling differs between
/// hosts, so neither Windows-to-other nor other-to-Windows is safe.
fn target_matches_host(target_os: &str, host_os: &str) -> bool {
    target_os == host_os
}

/// Values used to render the supported template subset.
struct RenderValues<'a> {
    os: &'a str,
    arch: &'a str,
    goos: &'a str,
    goarch: &'a str,
    version: &'a str,
    semver: &'a str,
    format: &'a str,
}

/// Render one template string with the supported variables.
fn render(
    input: &str,
    values: RenderValues<'_>,
    asset: Option<&str>,
    asset_without_ext: Option<&str>,
    file_name: Option<&str>,
) -> Result<String, Error> {
    template::render(
        input,
        &template::Context {
            os: values.os,
            arch: values.arch,
            goos: values.goos,
            goarch: values.goarch,
            version: values.version,
            semver: values.semver,
            format: values.format,
            asset,
            asset_without_ext,
            file_name,
        },
    )
    .map_err(|()| Error::Unsupported("template expression"))
}

/// Evaluate one version expression.
fn evaluate_constraint(
    source: &str,
    version: &str,
    semver: &str,
    allow_semver_variable: bool,
) -> Result<bool, Error> {
    expr::evaluate(
        source,
        &expr::Context {
            version,
            semver,
            allow_semver_variable,
        },
    )
    .map_err(|()| Error::Unsupported("version constraint expression"))
}

/// Apply a matching version override.
fn apply_version_override(
    effective: &mut Effective,
    candidate: &VersionOverride,
) -> Result<(), Error> {
    if let Some(value) = candidate.kind.as_deref().filter(|value| !value.is_empty()) {
        effective.kind = value.to_owned();
    }
    if let Some(value) = candidate
        .repo_owner
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        effective.repo_owner = value.to_owned();
    }
    if let Some(value) = candidate
        .repo_name
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        effective.repo_name = value.to_owned();
    }
    if let Some(value) = candidate.asset.as_deref().filter(|value| !value.is_empty()) {
        effective.asset = value.to_owned();
    }
    if let Some(value) = candidate
        .format
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        effective.format = value.to_owned();
    }
    if let Some(value) = &candidate.files {
        effective.files = value.clone();
    }
    if let Some(value) = &candidate.replacements {
        effective.replacements = value.clone();
    }
    if let Some(value) = &candidate.overrides {
        effective.overrides = value.clone();
    }
    if let Some(value) = &candidate.format_overrides {
        effective.format_overrides = value.clone();
    }
    if let Some(value) = &candidate.supported_envs {
        effective.supported_envs = Some(value.clone());
    }
    if let Some(value) = &candidate.checksum {
        effective.checksum = Some(value.clone());
    }
    if let Some(value) = &candidate.version_prefix {
        effective.version_prefix = value.clone();
    }
    if let Some(value) = &candidate.version_filter {
        effective.version_filter = Some(value.clone());
    }
    if let Some(value) = candidate
        .version_source
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        effective.version_source = Some(value.to_owned());
    }
    if let Some(value) = candidate.append_ext {
        effective.append_ext = value;
    }
    if let Some(value) = candidate.rosetta2 {
        effective.rosetta2 = value;
    }
    if let Some(value) = candidate.windows_arm_emulation {
        effective.windows_arm_emulation = value;
    }
    if let Some(value) = candidate.complete_windows_ext {
        effective.complete_windows_ext = Some(value);
    }
    if let Some(value) = &candidate.windows_ext {
        effective.windows_ext = Some(value.clone());
    }
    if let Some(value) = candidate.no_asset {
        effective.no_asset = value;
    }
    if let Some(value) = &candidate.error_message {
        effective.error_message = Some(value.clone());
    }
    for value in [&candidate.url, &candidate.path, &candidate.crate_name] {
        if value.as_deref().is_some_and(|value| !value.is_empty()) {
            return Err(Error::Unsupported("non-release package field"));
        }
    }
    Ok(())
}

/// Describe an enabled upstream checksum without treating it as trusted.
fn build_checksum(
    checksum: &Checksum,
    effective: &Effective,
    asset: &str,
    values: RenderValues<'_>,
    go_arch: &str,
) -> Result<UpstreamChecksum, Error> {
    let kind = checksum
        .kind
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(Error::Unsupported("checksum type"))?;
    let replacements = match &checksum.replacements {
        Some(map) if map.is_empty() => BTreeMap::new(),
        Some(map) => {
            let mut merged = effective.replacements.clone();
            merged.extend(map.clone());
            merged
        }
        None => effective.replacements.clone(),
    };
    let checksum_os = mapped(&replacements, values.goos).to_owned();
    let checksum_arch = mapped(&replacements, go_arch).to_owned();
    let render_values = RenderValues {
        os: &checksum_os,
        arch: &checksum_arch,
        ..values
    };
    let (asset_name, url) = match kind {
        "github_release" => {
            let template = checksum
                .asset
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or(Error::Unsupported("checksum asset"))?;
            (
                Some(render(
                    template,
                    render_values,
                    Some(asset),
                    Some(remove_extension(asset)),
                    None,
                )?),
                None,
            )
        }
        "http" => {
            let template = checksum
                .url
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or(Error::Unsupported("checksum URL"))?;
            (
                None,
                Some(render(
                    template,
                    render_values,
                    Some(asset),
                    Some(remove_extension(asset)),
                    None,
                )?),
            )
        }
        _ => return Err(Error::Unsupported("checksum type")),
    };
    let file_format = checksum.file_format.clone().unwrap_or_default();
    if !matches!(file_format.as_str(), "" | "raw" | "regexp") {
        return Err(Error::Unsupported("checksum file format"));
    }
    let algorithm = checksum.algorithm.clone().unwrap_or_default();
    if !matches!(
        algorithm.as_str(),
        "" | "md5" | "sha1" | "sha256" | "sha512"
    ) {
        return Err(Error::Unsupported("checksum algorithm"));
    }
    let (checksum_pattern, file_pattern) = if file_format == "regexp" {
        let pattern = checksum
            .pattern
            .as_ref()
            .ok_or(Error::Unsupported("checksum pattern"))?;
        if pattern.checksum.is_empty() {
            return Err(Error::Unsupported("checksum pattern"));
        }
        regex::Regex::new(&pattern.checksum).map_err(|_| Error::Unsupported("checksum pattern"))?;
        if let Some(file) = pattern.file.as_deref().filter(|value| !value.is_empty()) {
            regex::Regex::new(file).map_err(|_| Error::Unsupported("checksum pattern"))?;
        }
        (Some(pattern.checksum.clone()), pattern.file.clone())
    } else {
        (None, None)
    };
    Ok(UpstreamChecksum {
        kind: kind.to_owned(),
        asset: asset_name,
        url,
        file_format,
        algorithm,
        checksum_pattern,
        file_pattern,
    })
}

/// Map the target onto Go's operating-system and architecture names.
fn go_target(target: &Target) -> Result<(&'static str, &'static str), Error> {
    let os = match target.os.as_str() {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        "freebsd" => "freebsd",
        "openbsd" => "openbsd",
        "netbsd" => "netbsd",
        _ => return Err(Error::Unsupported("operating system")),
    };
    let arch = match target.arch.as_str() {
        "x86_64" | "amd64" => "amd64",
        "aarch64" | "arm64" => "arm64",
        "x86" | "386" => "386",
        "arm" => "arm",
        _ => return Err(Error::Unsupported("architecture")),
    };
    Ok((os, arch))
}

/// Test a supported environment list against a target.
fn env_matches(envs: &[String], os: &str, arch: &str, rosetta2: bool, windows_arm: bool) -> bool {
    let combined = format!("{os}/{arch}");
    if envs
        .iter()
        .any(|env| env == "all" || env == os || env == arch || env == &combined)
    {
        return true;
    }
    if os == "darwin"
        && arch == "arm64"
        && rosetta2
        && envs
            .iter()
            .any(|env| env == "amd64" || env == "darwin/amd64")
    {
        return true;
    }
    if os == "windows"
        && arch == "arm64"
        && windows_arm
        && envs
            .iter()
            .any(|env| env == "amd64" || env == "windows/amd64")
    {
        return true;
    }
    false
}

/// Test a platform override. An override without selectors matches everything,
/// as it does in Aqua.
fn override_matches(override_: &Override, os: &str, arch: &str) -> bool {
    override_.goos.as_deref().is_none_or(|value| value == os)
        && override_
            .goarch
            .as_deref()
            .is_none_or(|value| value == arch)
        && override_
            .envs
            .as_deref()
            .is_none_or(|envs| env_matches(envs, os, arch, false, false))
}

/// Apply a replacement, treating an empty value as no replacement.
fn mapped<'a>(replacements: &'a BTreeMap<String, String>, key: &'a str) -> &'a str {
    match replacements.get(key) {
        Some(value) if !value.is_empty() => value,
        _ => key,
    }
}

/// Trim the recipe's version prefix.
fn trim_prefix<'a>(version: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        version
    } else {
        version.strip_prefix(prefix).unwrap_or(version)
    }
}

/// The default command name from the package name or repository name.
fn default_file_name(package_name: Option<&str>, repo_name: &str) -> String {
    package_name
        .and_then(|name| name.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .unwrap_or(repo_name)
        .to_owned()
}

/// Reject a destination that is not a single filename.
fn validate_file_name(name: &str) -> Result<(), Error> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None)
            if !name.contains(['/', '\\', ':'])
                && name != "."
                && name != ".."
                && !name.contains('\0') =>
        {
            Ok(())
        }
        _ => Err(Error::Unsupported("file name")),
    }
}

/// Require a trusted SHA-256 digest.
fn validate_sha256(digest: &str) -> Result<(), Error> {
    if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::Unsupported("trusted SHA-256"))
    }
}

/// Known archive extensions, longest first so `tar.gz` wins over `gz`.
const EXTENSIONS: &[&str] = &[
    "tar.br", "tar.bz2", "tar.gz", "tar.lz4", "tar.sz", "tar.xz", "tar.zst", "tbr", "tbz", "tbz2",
    "tgz", "tlz4", "tsz", "txz", "zip", "gz", "bz2", "lz4", "sz", "xz", "zst", "dmg", "pkg", "rar",
    "tar",
];

/// Find a known archive extension at the end of a name.
fn asset_extension(asset: &str) -> Option<&'static str> {
    EXTENSIONS
        .iter()
        .copied()
        .find(|extension| asset.ends_with(&format!(".{extension}")))
}

/// Remove a known archive extension, or return the name unchanged.
fn remove_extension(asset: &str) -> &str {
    match asset_extension(asset) {
        Some(extension) => &asset[..asset.len() - extension.len() - 1],
        None => asset,
    }
}

/// Append the configured format when the asset has no known extension.
fn append_extension(asset: &str, format: &str) -> String {
    if asset_extension(asset).is_some() || format.is_empty() || format == "raw" {
        return asset.to_owned();
    }
    if asset.ends_with(&format!(".{format}")) {
        return asset.to_owned();
    }
    format!("{asset}.{format}")
}

/// Complete a Windows executable extension on an asset.
fn complete_windows_asset(asset: &str, format: &str, complete: bool, extension: &str) -> String {
    if asset.ends_with(".exe") || asset.ends_with(".jar") {
        return asset.to_owned();
    }
    let needs_extension = match format {
        "raw" => true,
        "" => asset_extension(asset).is_none(),
        _ => false,
    };
    if needs_extension && complete {
        format!("{asset}{extension}")
    } else {
        asset.to_owned()
    }
}

/// Test whether a value already has a file extension, ignoring the version.
fn has_extension(value: &str, version: &str) -> bool {
    let version = version.strip_prefix('v').unwrap_or(version);
    let without_version = if version.is_empty() {
        value.to_owned()
    } else {
        value.replace(version, "")
    };
    Path::new(&without_version).extension().is_some()
}

/// Infer an archive format from an asset's extension.
fn inferred_format(asset: &str) -> Result<&'static str, Error> {
    match asset_extension(asset) {
        Some("zip") => Ok("zip"),
        Some("tar") => Ok("tar"),
        Some("tar.gz" | "tgz") => Ok("tar.gz"),
        Some("tar.xz" | "txz") => Ok("tar.xz"),
        Some("tar.bz2" | "tbz" | "tbz2") => Ok("tar.bz2"),
        Some(_) | None => Err(Error::Unsupported("archive format")),
    }
}

/// Map a recipe format onto an archive format the installer supports.
fn format_for(name: &str) -> Result<Format, Error> {
    match name {
        "zip" => Ok(Format::Zip),
        "tar" => Ok(Format::Tar),
        "tar.gz" | "tgz" => Ok(Format::TarGz),
        "tar.xz" | "txz" => Ok(Format::TarXz),
        "tar.bz2" | "tbz" | "tbz2" => Ok(Format::TarBz2),
        _ => Err(Error::Unsupported("archive format")),
    }
}

#[cfg(test)]
mod tests {
    use super::target_matches_host;

    #[test]
    fn cross_target_checks_both_directions() {
        assert!(target_matches_host("linux", "linux"));
        assert!(target_matches_host("windows", "windows"));
        assert!(!target_matches_host("windows", "linux"));
        assert!(!target_matches_host("linux", "windows"));
        assert!(!target_matches_host("macos", "linux"));
    }
}
