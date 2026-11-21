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
    /// The API answered with a non-success HTTP status.
    ///
    /// A redirect status appears here when the client refused to follow it,
    /// either because it would leave HTTPS or because the redirect limit was
    /// reached.
    Status(reqwest::StatusCode),
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
            Self::Status(s) => write!(f, "GitLab API returned HTTP {s}"),
            Self::Selection(e) => write!(f, "GitLab release asset: {e}"),
            Self::Install(e) => write!(f, "GitLab release installation: {e}"),
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

/// Reject a project ID or path that cannot address one project.
///
/// `project` is pushed as one encoded segment, so `group/project` is valid,
/// but an empty component or a `.`/`..` component is not.
fn validate_project(project: &str) -> Result<(), Error> {
    if project.is_empty() || project == "." || project == ".." {
        return Err(Error::Invalid("project must not be empty, '.', or '..'"));
    }
    if project
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(Error::Invalid(
            "project path has an empty, '.', or '..' segment",
        ));
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

    /// Fetch the latest release or a release with an exact tag.
    ///
    /// `project` is a numeric ID or a full `namespace/project` path.
    pub fn release(&self, project: &str, tag: Option<&str>) -> Result<Release, Error> {
        let url = self.endpoint(project, tag)?;
        let response = self.client.get(url).send().map_err(Error::Http)?;
        ensure_success(response.status())?;
        response.json().map_err(Error::Http)
    }

    /// Build the API URL for a project release.
    ///
    /// The project is percent-encoded as one path segment, so
    /// `namespace/project` becomes `namespace%2Fproject` as the API expects.
    /// A tag containing `/`, `#`, `%`, or a space consequently cannot alter
    /// the endpoint. `.` and `..` are rejected rather than dropped by the
    /// encoder.
    fn endpoint(&self, project: &str, tag: Option<&str>) -> Result<reqwest::Url, Error> {
        validate_project(project)?;
        if let Some(tag) = tag {
            validate_segment(tag, "tag must not be empty, '.', or '..'")?;
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
        Ok(url)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn client(base: &str) -> Gitlab {
        Gitlab::with_api_base(base).expect("valid test API base")
    }

    // The URL cannot be observed through the public API without a TLS server,
    // so endpoint construction is asserted directly here.
    #[test]
    fn latest_endpoint_is_appended_to_base_with_or_without_trailing_slash() {
        for base in ["https://gitlab.com/api/v4", "https://gitlab.com/api/v4/"] {
            let url = client(base).endpoint("group/project", None).unwrap();
            assert_eq!(
                url.as_str(),
                "https://gitlab.com/api/v4/projects/group%2Fproject/releases/permalink/latest"
            );
        }
    }

    #[test]
    fn numeric_project_id_needs_no_path_encoding() {
        let url = client("https://gitlab.com/api/v4/")
            .endpoint("1234", Some("v1.0.0"))
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://gitlab.com/api/v4/projects/1234/releases/v1.0.0"
        );
    }

    #[test]
    fn tag_becomes_a_single_percent_encoded_segment() {
        let url = client("https://gitlab.com/api/v4/")
            .endpoint("group/project", Some("release/v1 #1"))
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://gitlab.com/api/v4/projects/group%2Fproject/releases/release%2Fv1%20%231"
        );
    }

    #[test]
    fn percent_in_caller_value_is_encoded_once() {
        let url = client("https://gitlab.com/api/v4/")
            .endpoint("group/project%20name", None)
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://gitlab.com/api/v4/projects/group%2Fproject%2520name/releases/permalink/latest"
        );
    }

    #[test]
    fn dot_segments_are_rejected_instead_of_being_dropped() {
        let client = client("https://gitlab.com/api/v4/");
        let cases = [
            ("", None),
            ("project", Some("")),
            (".", None),
            ("..", None),
            ("project", Some(".")),
            ("project", Some("..")),
            ("group/../project", None),
            ("group//project", None),
            ("group/project/", None),
        ];
        for (project, tag) in cases {
            assert!(
                matches!(client.endpoint(project, tag), Err(Error::Invalid(_))),
                "accepted project={project:?} tag={tag:?}"
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
            "GitLab API returned HTTP 404 Not Found"
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
