//! VCS host detection and release API abstraction.
//!
//! Supports GitHub, GitLab, and a generic fallback for other hosts
//! (Bitbucket, Gitea, self-hosted, etc.).

/// The kind of VCS host, used to select the right release API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HostKind {
    GitHub,
    GitLab,
    Generic,
}

/// Detect the host kind from a hostname.
///
/// Checks `RUNX_HOST_TYPE` env var first (values: "github", "gitlab", "generic")
/// for self-hosted instances with non-obvious hostnames.
pub fn detect(host: &str) -> HostKind {
    if let Ok(override_val) = std::env::var("RUNX_HOST_TYPE") {
        match override_val.to_lowercase().as_str() {
            "github" => return HostKind::GitHub,
            "gitlab" => return HostKind::GitLab,
            "generic" => return HostKind::Generic,
            _ => {} // ignore invalid values, fall through to detection
        }
    }

    let lower = host.to_lowercase();
    if lower == "github.com" || lower.contains("github") {
        HostKind::GitHub
    } else if lower == "gitlab.com" || lower.contains("gitlab") {
        HostKind::GitLab
    } else {
        HostKind::Generic
    }
}

/// Build the release API URL for fetching release metadata.
///
/// Returns `None` for `Generic` hosts (they use direct download instead).
pub fn release_api_url(kind: HostKind, host: &str, path: &str, tag: &str) -> Option<String> {
    match kind {
        HostKind::GitHub => {
            // path is "owner/repo"
            Some(format!("https://api.github.com/repos/{path}/releases/tags/{tag}"))
        }
        HostKind::GitLab => {
            // path may be "group/project" or "group/sub/project".
            // GitLab API uses URL-encoded project path.
            let encoded_path = path.replace('/', "%2F");
            Some(format!("https://{host}/api/v4/projects/{encoded_path}/releases/{tag}"))
        }
        HostKind::Generic => None,
    }
}

/// Build curl headers for the release API request.
pub fn release_headers(kind: HostKind) -> Vec<String> {
    let mut headers = Vec::new();

    if let HostKind::GitHub = kind {
        headers.push("Accept: application/vnd.github+json".to_string());
    }

    headers
}

/// Extract the download URL for a named asset from release API JSON.
pub fn extract_asset_url(
    kind: HostKind,
    json: &serde_json::Value,
    asset_name: &str,
) -> Option<String> {
    match kind {
        HostKind::GitHub => {
            // GitHub: assets[].name / assets[].browser_download_url
            json["assets"].as_array()?.iter()
                .find(|a| a["name"].as_str() == Some(asset_name))
                .and_then(|a| a["browser_download_url"].as_str())
                .map(|s| s.to_string())
        }
        HostKind::GitLab => {
            // GitLab: assets.links[].name / assets.links[].direct_asset_url
            // Also check url field as fallback.
            json["assets"]["links"].as_array()?.iter()
                .find(|a| a["name"].as_str() == Some(asset_name))
                .and_then(|a| {
                    a["direct_asset_url"].as_str()
                        .or_else(|| a["url"].as_str())
                })
                .map(|s| s.to_string())
        }
        HostKind::Generic => None,
    }
}

/// Build a direct download URL for hosts without a release API.
///
/// Uses the GitHub/Gitea-style convention: {canonical_url}/releases/download/{tag}/{asset}
pub fn direct_download_url(canonical_url: &str, tag: &str, asset_name: &str) -> String {
    format!("{canonical_url}/releases/download/{tag}/{asset_name}")
}

impl std::fmt::Display for HostKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitHub => write!(f, "github"),
            Self::GitLab => write!(f, "gitlab"),
            Self::Generic => write!(f, "generic"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_github() {
        assert_eq!(detect("github.com"), HostKind::GitHub);
        assert_eq!(detect("GitHub.com"), HostKind::GitHub);
        assert_eq!(detect("github.corp.com"), HostKind::GitHub);
    }

    #[test]
    fn detect_gitlab() {
        assert_eq!(detect("gitlab.com"), HostKind::GitLab);
        assert_eq!(detect("gitlab.example.org"), HostKind::GitLab);
    }

    #[test]
    fn detect_generic() {
        assert_eq!(detect("bitbucket.org"), HostKind::Generic);
        assert_eq!(detect("git.sr.ht"), HostKind::Generic);
        assert_eq!(detect("gitea.example.com"), HostKind::Generic);
    }

    #[test]
    fn github_release_url() {
        let url = release_api_url(HostKind::GitHub, "github.com", "owner/repo", "v1.0");
        assert_eq!(
            url.unwrap(),
            "https://api.github.com/repos/owner/repo/releases/tags/v1.0"
        );
    }

    #[test]
    fn gitlab_release_url() {
        let url = release_api_url(HostKind::GitLab, "gitlab.com", "group/project", "v2.0");
        assert_eq!(
            url.unwrap(),
            "https://gitlab.com/api/v4/projects/group%2Fproject/releases/v2.0"
        );
    }

    #[test]
    fn gitlab_release_url_subgroup() {
        let url = release_api_url(HostKind::GitLab, "gitlab.com", "group/sub/project", "v1");
        assert_eq!(
            url.unwrap(),
            "https://gitlab.com/api/v4/projects/group%2Fsub%2Fproject/releases/v1"
        );
    }

    #[test]
    fn generic_release_url_is_none() {
        assert!(release_api_url(HostKind::Generic, "bitbucket.org", "team/repo", "v1").is_none());
    }

    #[test]
    fn extract_github_asset() {
        let json = serde_json::json!({
            "assets": [
                {
                    "name": "tool-linux-amd64",
                    "browser_download_url": "https://github.com/owner/repo/releases/download/v1/tool-linux-amd64"
                },
                {
                    "name": "tool-darwin-arm64",
                    "browser_download_url": "https://github.com/owner/repo/releases/download/v1/tool-darwin-arm64"
                }
            ]
        });
        let url = extract_asset_url(HostKind::GitHub, &json, "tool-darwin-arm64");
        assert_eq!(
            url.unwrap(),
            "https://github.com/owner/repo/releases/download/v1/tool-darwin-arm64"
        );
    }

    #[test]
    fn extract_github_asset_not_found() {
        let json = serde_json::json!({"assets": []});
        assert!(extract_asset_url(HostKind::GitHub, &json, "missing").is_none());
    }

    #[test]
    fn extract_gitlab_asset() {
        let json = serde_json::json!({
            "assets": {
                "links": [
                    {
                        "name": "tool-linux-amd64",
                        "direct_asset_url": "https://gitlab.com/group/project/-/releases/v1/downloads/tool-linux-amd64"
                    }
                ]
            }
        });
        let url = extract_asset_url(HostKind::GitLab, &json, "tool-linux-amd64");
        assert_eq!(
            url.unwrap(),
            "https://gitlab.com/group/project/-/releases/v1/downloads/tool-linux-amd64"
        );
    }

    #[test]
    fn direct_download_url_format() {
        let url = direct_download_url("https://bitbucket.org/team/repo", "v1.0", "binary-linux");
        assert_eq!(
            url,
            "https://bitbucket.org/team/repo/releases/download/v1.0/binary-linux"
        );
    }
}
