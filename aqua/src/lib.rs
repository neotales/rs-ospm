//! Strict subset of Aqua registry recipes for GitHub release archives.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

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

/// A GitHub release recipe. Unsupported fields are rejected during parsing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    #[serde(rename = "type")]
    kind: String,
    repo_owner: String,
    repo_name: String,
    asset: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    replacements: BTreeMap<String, String>,
    #[serde(default)]
    overrides: Vec<Override>,
    #[serde(default)]
    supported_envs: Option<Vec<String>>,
    #[serde(default)]
    files: Vec<RecipeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    goos: Option<String>,
    goarch: Option<String>,
    envs: Option<Vec<String>>,
    asset: Option<String>,
    format: Option<String>,
    replacements: Option<BTreeMap<String, String>>,
    files: Option<Vec<RecipeFile>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeFile {
    name: String,
    src: Option<String>,
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
    /// GitHub discovery or installation failed.
    Github(neotales_ospm_github::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml(e) => write!(f, "invalid or unsupported registry: {e}"),
            Self::Package => f.write_str("registry package missing or ambiguous"),
            Self::Unsupported(e) => write!(f, "unsupported registry recipe: {e}"),
            Self::Github(e) => write!(f, "GitHub release: {e}"),
        }
    }
}
impl std::error::Error for Error {}

impl Registry {
    /// Parse a registry. Unsupported fields cause an error rather than being
    /// silently ignored, including security and version-constraint fields.
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

fn format_for(name: &str) -> Result<Format, Error> {
    match name {
        "zip" => Ok(Format::Zip),
        "tar" => Ok(Format::Tar),
        "tar.gz" => Ok(Format::TarGz),
        "tar.xz" => Ok(Format::TarXz),
        "tar.bz2" => Ok(Format::TarBz2),
        _ => Err(Error::Unsupported("archive format")),
    }
}

fn render(input: &str, os: &str, arch: &str, version: &str, format: &str) -> Result<String, Error> {
    let output = input
        .replace("{{.OS}}", os)
        .replace("{{.Arch}}", arch)
        .replace("{{.Version}}", version)
        .replace("{{.Format}}", format);
    if output.contains("{{") || output.contains("}}") {
        return Err(Error::Unsupported("template expression"));
    }
    Ok(output)
}

fn env_matches(env: &str, os: &str, arch: &str) -> Result<bool, Error> {
    if env == "all" {
        return Ok(true);
    }
    let known_os = ["linux", "darwin", "windows", "freebsd", "openbsd", "netbsd"];
    let known_arch = ["amd64", "arm64", "386", "arm"];
    if known_os.contains(&env) {
        return Ok(env == os);
    }
    if known_arch.contains(&env) {
        return Ok(env == arch);
    }
    if let Some((candidate_os, candidate_arch)) = env.split_once('/')
        && known_os.contains(&candidate_os)
        && known_arch.contains(&candidate_arch)
    {
        return Ok(candidate_os == os && candidate_arch == arch);
    }
    Err(Error::Unsupported("environment selector"))
}

fn any_env_matches(envs: &[String], os: &str, arch: &str) -> Result<bool, Error> {
    let mut matched = false;
    for env in envs {
        matched |= env_matches(env, os, arch)?;
    }
    Ok(matched)
}

impl Recipe {
    /// Resolve the supported recipe subset for a target and exact release tag.
    ///
    /// The checksum must be supplied from an independently trusted source.
    pub fn plan(
        &self,
        target: &Target,
        version: &str,
        target_dir: &Path,
        sha256: &str,
    ) -> Result<Plan, Error> {
        if self.kind != "github_release" || version.is_empty() {
            return Err(Error::Unsupported("package type or empty version"));
        }
        let os = match target.os.as_str() {
            "macos" => "darwin",
            "linux" => "linux",
            "windows" => "windows",
            _ => return Err(Error::Unsupported("operating system")),
        };
        let arch = match target.arch.as_str() {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            "x86" => "386",
            "arm" => "arm",
            _ => return Err(Error::Unsupported("architecture")),
        };
        if let Some(envs) = &self.supported_envs
            && !any_env_matches(envs, os, arch)?
        {
            return Err(Error::Unsupported("target is not in supported_envs"));
        }
        let mut selected = None;
        for override_ in &self.overrides {
            if override_.goos.is_none() && override_.goarch.is_none() && override_.envs.is_none() {
                return Err(Error::Unsupported("override has no platform selector"));
            }
            let env_matches = match &override_.envs {
                Some(envs) => any_env_matches(envs, os, arch)?,
                None => true,
            };
            if override_.goos.as_deref().is_none_or(|v| v == os)
                && override_.goarch.as_deref().is_none_or(|v| v == arch)
                && env_matches
            {
                selected = Some(override_);
                break;
            }
        }
        let mut replacements = self.replacements.clone();
        if let Some(extra) = selected.and_then(|o| o.replacements.as_ref()) {
            replacements.extend(extra.clone());
        }
        let mapped_os = replacements.get(os).map_or(os, String::as_str);
        let mapped_arch = replacements.get(arch).map_or(arch, String::as_str);
        let template = selected
            .and_then(|o| o.asset.as_deref())
            .unwrap_or(&self.asset);
        let configured_format = selected
            .and_then(|o| o.format.as_deref())
            .unwrap_or(&self.format);
        let format = if configured_format.is_empty() {
            ["tar.gz", "tar.xz", "tar.bz2", "tar", "zip"]
                .into_iter()
                .find(|extension| template.ends_with(&format!(".{extension}")))
                .ok_or(Error::Unsupported("archive format cannot be inferred"))?
        } else {
            configured_format
        };
        let format_enum = format_for(format)?;
        let asset = render(template, mapped_os, mapped_arch, version, format)?;
        let files = selected
            .and_then(|o| o.files.as_ref())
            .unwrap_or(&self.files);
        if files.len() != 1 {
            return Err(Error::Unsupported("exactly one file is required"));
        }
        let file = &files[0];
        let name = render(&file.name, mapped_os, mapped_arch, version, format)?;
        let source = render(
            file.src.as_deref().unwrap_or(&file.name),
            mapped_os,
            mapped_arch,
            version,
            format,
        )?;
        // The default src is the name at the archive root, not any basename.
        let pattern = format!("^{}$", regex::escape(&source));
        let mut install =
            InstallOptions::new(format_enum, Selector::Regex(pattern), target_dir, sha256);
        install.rename = Some(name);
        let _description = &self.description;
        Ok(Plan {
            owner: self.repo_owner.clone(),
            repository: self.repo_name.clone(),
            tag: version.into(),
            asset,
            install,
        })
    }
}

impl Plan {
    /// Fetch the exact release and asset, then verify and install it.
    pub fn install(&self, client: &Github) -> Result<PathBuf, Error> {
        let release = client
            .release(&self.owner, &self.repository, Some(&self.tag))
            .map_err(Error::Github)?;
        let selection = AssetOptions {
            template: Some(self.asset.clone()),
            ..Default::default()
        };
        release
            .install(&Target::current(), &selection, &self.install)
            .map_err(Error::Github)
    }
}
