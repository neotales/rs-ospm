//! GitLab release discovery and verified asset installation.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use neotales_ospm_archive::{InstallError, InstallOptions, install_https};
use neotales_ospm_release::{Asset, AssetOptions, SelectionError, Target, select_asset};
use serde::Deserialize;

/// A GitLab release.
#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    /// Release tag.
    pub tag_name: String,
    /// Release asset collection.
    pub assets: ReleaseAssets,
}

/// Named links on a release. Generated source archives are not selected.
#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseAssets {
    /// Named downloadable asset links.
    pub links: Vec<ReleaseLink>,
}

/// A named release link.
#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseLink {
    /// Asset name.
    pub name: String,
    /// Asset URL, which must be HTTPS when downloaded.
    pub url: String,
}

/// Failure discovering, selecting, or installing a release.
#[derive(Debug)]
pub enum Error {
    /// Invalid endpoint or project input.
    Invalid(&'static str),
    /// API request or JSON decoding failed.
    Http(reqwest::Error),
    /// No unique asset could be selected.
    Selection(SelectionError),
    /// Download or installation failed.
    Install(InstallError),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => write!(f, "GitLab provider: {e}"),
            Self::Http(e) => write!(f, "GitLab API: {e}"),
            Self::Selection(e) => write!(f, "GitLab release asset: {e}"),
            Self::Install(e) => write!(f, "GitLab release installation: {e}"),
        }
    }
}
impl std::error::Error for Error {}

/// Blocking GitLab API client.
#[derive(Clone, Debug)]
pub struct Gitlab {
    api: reqwest::Url,
    client: reqwest::blocking::Client,
}
impl Gitlab {
    /// Use the public GitLab API.
    pub fn new() -> Result<Self, Error> {
        Self::with_api_base("https://gitlab.com/api/v4/")
    }

    /// Configure an HTTPS API base, including `/api/v4/`.
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
            .user_agent("neotales-ospm-gitlab")
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

    /// Fetch the latest release or a release with an exact tag.
    ///
    /// `project` is a numeric ID or a full `namespace/project` path.
    pub fn release(&self, project: &str, tag: Option<&str>) -> Result<Release, Error> {
        if project.is_empty() || tag == Some("") {
            return Err(Error::Invalid("project and tag must not be empty"));
        }
        let mut url = self.api.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| Error::Invalid("API URL cannot be a base"))?;
            path.pop_if_empty()
                .extend(["projects", project, "releases"]);
            match tag {
                Some(tag) => {
                    path.push(tag);
                }
                None => {
                    path.extend(["permalink", "latest"]);
                }
            }
        }
        let response = self.client.get(url).send().map_err(Error::Http)?;
        if !response.status().is_success() {
            return Err(Error::Invalid("API returned a non-success response"));
        }
        response.json().map_err(Error::Http)
    }
}

impl Release {
    /// Select an asset using the release tag as the template version.
    pub fn select(&self, target: &Target, options: &AssetOptions) -> Result<&ReleaseLink, Error> {
        let assets: Vec<Asset> = self
            .assets
            .links
            .iter()
            .map(|a| Asset {
                name: a.name.clone(),
                url: a.url.clone(),
            })
            .collect();
        let selected =
            select_asset(&assets, target, &self.tag_name, options).map_err(Error::Selection)?;
        let index = assets
            .iter()
            .position(|a| std::ptr::eq(a, selected))
            .ok_or(Error::Invalid("selected asset missing"))?;
        Ok(&self.assets.links[index])
    }

    /// Select, download, verify, and install an asset using a trusted SHA-256.
    pub fn install(
        &self,
        target: &Target,
        selection: &AssetOptions,
        install: &InstallOptions,
    ) -> Result<PathBuf, Error> {
        let asset = self.select(target, selection)?;
        install_https(&asset.url, install).map_err(Error::Install)
    }
}
