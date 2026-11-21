//! GitHub release discovery and verified asset installation.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use neotales_ospm_archive::{InstallError, InstallOptions, install_https};
use neotales_ospm_release::{Asset, AssetOptions, SelectionError, Target, select_asset};
use serde::Deserialize;

/// A release and its downloadable assets.
#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    /// Release tag.
    pub tag_name: String,
    /// Uploaded release assets.
    pub assets: Vec<ReleaseAsset>,
}

/// A GitHub release asset.
#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseAsset {
    /// Asset filename.
    pub name: String,
    /// HTTPS asset download URL.
    pub browser_download_url: String,
    /// Provider-reported digest, when supplied; it is not independently trusted.
    pub digest: Option<String>,
}

/// Failure discovering, selecting, or installing a release.
#[derive(Debug)]
pub enum Error {
    /// Invalid API endpoint or repository input.
    Invalid(&'static str),
    /// GitHub API request or JSON decoding failed.
    Http(reqwest::Error),
    /// No unique asset could be selected.
    Selection(SelectionError),
    /// The download or installation failed.
    Install(InstallError),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => write!(f, "GitHub provider: {e}"),
            Self::Http(e) => write!(f, "GitHub API: {e}"),
            Self::Selection(e) => write!(f, "GitHub release asset: {e}"),
            Self::Install(e) => write!(f, "GitHub release installation: {e}"),
        }
    }
}
impl std::error::Error for Error {}

/// Blocking GitHub release client.
#[derive(Clone, Debug)]
pub struct Github {
    api: reqwest::Url,
    client: reqwest::blocking::Client,
}
impl Github {
    /// Create a client for the public GitHub API.
    pub fn new() -> Result<Self, Error> {
        Self::with_api_base("https://api.github.com/")
    }

    /// Configure an HTTPS API base, such as a GitHub Enterprise API endpoint.
    pub fn with_api_base(api: &str) -> Result<Self, Error> {
        let api = reqwest::Url::parse(api).map_err(|_| Error::Invalid("invalid API URL"))?;
        if api.scheme() != "https"
            || api.cannot_be_a_base()
            || api.query().is_some()
            || api.fragment().is_some()
            || !api.username().is_empty()
            || api.password().is_some()
        {
            return Err(Error::Invalid(
                "API URL must be an HTTPS base without credentials, query, or fragment",
            ));
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent("neotales-ospm-github")
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 || attempt.url().scheme() != "https" {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(Error::Http)?;
        Ok(Self { api, client })
    }

    /// Fetch the latest published release, or a release for an exact tag.
    pub fn release(
        &self,
        owner: &str,
        repository: &str,
        tag: Option<&str>,
    ) -> Result<Release, Error> {
        if owner.is_empty() || repository.is_empty() || tag == Some("") {
            return Err(Error::Invalid(
                "owner, repository, and tag must not be empty",
            ));
        }
        let mut url = self.api.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| Error::Invalid("API URL cannot be a base"))?;
            path.pop_if_empty()
                .extend(["repos", owner, repository, "releases"]);
            match tag {
                Some(tag) => {
                    path.extend(["tags", tag]);
                }
                None => {
                    path.push("latest");
                }
            }
        }
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .map_err(Error::Http)?;
        if !response.status().is_success() {
            return Err(Error::Invalid("API returned a non-success response"));
        }
        response.json().map_err(Error::Http)
    }
}

impl Release {
    /// Select an asset using the release tag as the template version.
    pub fn select(&self, target: &Target, options: &AssetOptions) -> Result<&ReleaseAsset, Error> {
        let assets: Vec<Asset> = self
            .assets
            .iter()
            .map(|a| Asset {
                name: a.name.clone(),
                url: a.browser_download_url.clone(),
            })
            .collect();
        let selected =
            select_asset(&assets, target, &self.tag_name, options).map_err(Error::Selection)?;
        let index = assets
            .iter()
            .position(|a| std::ptr::eq(a, selected))
            .ok_or(Error::Invalid("selected asset missing"))?;
        Ok(&self.assets[index])
    }

    /// Select, download, verify, and install an asset using a trusted SHA-256.
    ///
    /// `install.sha256` must come from a trusted source. The provider-reported
    /// digest is informational and is not automatically treated as trusted.
    pub fn install(
        &self,
        target: &Target,
        selection: &AssetOptions,
        install: &InstallOptions,
    ) -> Result<PathBuf, Error> {
        let asset = self.select(target, selection)?;
        install_https(&asset.browser_download_url, install).map_err(Error::Install)
    }
}
