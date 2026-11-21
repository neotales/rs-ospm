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
    /// The API answered with a non-success HTTP status.
    ///
    /// A redirect status appears here when the client refused to follow it,
    /// either because it would leave HTTPS or because the redirect limit was
    /// reached.
    Status(reqwest::StatusCode),
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
            Self::Status(s) => write!(f, "GitHub API returned HTTP {s}"),
            Self::Selection(e) => write!(f, "GitHub release asset: {e}"),
            Self::Install(e) => write!(f, "GitHub release installation: {e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e),
            Self::Selection(e) => Some(e),
            Self::Install(e) => Some(e),
            Self::Invalid(_) | Self::Status(_) => None,
        }
    }
}

/// Reject values that the URL segment encoder would silently drop.
///
/// `Url::path_segments_mut` omits `.` and `..` segments, so accepting them
/// would request a different endpoint than the caller named.
fn validate_segment(value: &str, message: &'static str) -> Result<(), Error> {
    if value.is_empty() || value == "." || value == ".." {
        return Err(Error::Invalid(message));
    }
    Ok(())
}

/// Maximum number of redirects followed before returning the redirect.
const MAX_REDIRECTS: usize = 10;

/// Follow a redirect only while it stays on HTTPS and within the limit.
///
/// `previous` counts the initial request plus every redirect already
/// followed, matching `reqwest::redirect::Attempt::previous`.
fn should_follow_redirect(previous: usize, next_scheme: &str) -> bool {
    next_scheme == "https" && previous <= MAX_REDIRECTS
}

/// Convert a non-success API status into an error.
fn ensure_success(status: reqwest::StatusCode) -> Result<(), Error> {
    if status.is_success() {
        Ok(())
    } else {
        Err(Error::Status(status))
    }
}

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
                if should_follow_redirect(attempt.previous().len(), attempt.url().scheme()) {
                    attempt.follow()
                } else {
                    attempt.stop()
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
        let url = self.endpoint(owner, repository, tag)?;
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .map_err(Error::Http)?;
        ensure_success(response.status())?;
        response.json().map_err(Error::Http)
    }

    /// Build the API URL for a repository release.
    ///
    /// Each caller-supplied value becomes exactly one percent-encoded path
    /// segment, so a tag containing `/`, `#`, `%`, or a space cannot alter the
    /// endpoint. `.` and `..` are rejected rather than dropped by the
    /// encoder.
    fn endpoint(
        &self,
        owner: &str,
        repository: &str,
        tag: Option<&str>,
    ) -> Result<reqwest::Url, Error> {
        validate_segment(owner, "owner must not be empty, '.', or '..'")?;
        validate_segment(repository, "repository must not be empty, '.', or '..'")?;
        if let Some(tag) = tag {
            validate_segment(tag, "tag must not be empty, '.', or '..'")?;
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
        Ok(url)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn client(base: &str) -> Github {
        Github::with_api_base(base).expect("valid test API base")
    }

    // The URL cannot be observed through the public API without a TLS server,
    // so endpoint construction is asserted directly here.
    #[test]
    fn latest_endpoint_is_appended_to_base_with_or_without_trailing_slash() {
        for base in ["https://api.github.com", "https://api.github.com/"] {
            let url = client(base).endpoint("owner", "repo", None).unwrap();
            assert_eq!(
                url.as_str(),
                "https://api.github.com/repos/owner/repo/releases/latest"
            );
        }
    }

    #[test]
    fn enterprise_base_keeps_its_path_prefix() {
        let url = client("https://ghe.example/api/v3/")
            .endpoint("owner", "repo", Some("v1.0.0"))
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://ghe.example/api/v3/repos/owner/repo/releases/tags/v1.0.0"
        );
    }

    #[test]
    fn caller_values_become_single_percent_encoded_segments() {
        let url = client("https://api.github.com/")
            .endpoint("weird owner", "repo/name", Some("release/v1 #1"))
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.github.com/repos/weird%20owner/repo%2Fname/releases/tags/release%2Fv1%20%231"
        );
    }

    #[test]
    fn percent_in_caller_value_is_encoded_once() {
        let url = client("https://api.github.com/")
            .endpoint("owner", "repo", Some("v1%2F"))
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.github.com/repos/owner/repo/releases/tags/v1%252F"
        );
    }

    #[test]
    fn dot_segments_are_rejected_instead_of_being_dropped() {
        let client = client("https://api.github.com/");
        let cases = [
            ("", "repo", None),
            ("owner", "", None),
            ("owner", "repo", Some("")),
            (".", "repo", None),
            ("..", "repo", None),
            ("owner", ".", None),
            ("owner", "..", None),
            ("owner", "repo", Some(".")),
            ("owner", "repo", Some("..")),
        ];
        for (owner, repository, tag) in cases {
            assert!(
                matches!(
                    client.endpoint(owner, repository, tag),
                    Err(Error::Invalid(_))
                ),
                "accepted owner={owner:?} repository={repository:?} tag={tag:?}"
            );
        }
    }

    #[test]
    fn non_success_statuses_are_status_errors_not_input_errors() {
        for status in [
            reqwest::StatusCode::FOUND,
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::NOT_FOUND,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            match ensure_success(status) {
                Err(Error::Status(actual)) => assert_eq!(actual, status),
                other => panic!("expected a status error for {status}, got {other:?}"),
            }
        }
        assert!(ensure_success(reqwest::StatusCode::OK).is_ok());
        assert_eq!(
            ensure_success(reqwest::StatusCode::NOT_FOUND)
                .unwrap_err()
                .to_string(),
            "GitHub API returned HTTP 404 Not Found"
        );
    }

    #[test]
    fn redirects_must_stay_on_https_and_stop_at_the_limit() {
        for previous in 0..=MAX_REDIRECTS {
            assert!(should_follow_redirect(previous, "https"));
        }
        assert!(!should_follow_redirect(MAX_REDIRECTS + 1, "https"));
        for scheme in ["http", "ftp", "file", ""] {
            assert!(!should_follow_redirect(0, scheme), "followed {scheme}");
        }
    }

    #[test]
    fn selection_errors_expose_a_source() {
        let error = Error::Selection(SelectionError::Ambiguous);
        assert!(std::error::Error::source(&error).is_some());
    }
}
