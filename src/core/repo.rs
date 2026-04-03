use crate::core::RunxError;
use regex::Regex;
use std::sync::LazyLock;

/// Parsed repository reference.
#[derive(Debug, Clone)]
pub struct RepoRef {
    /// Normalized HTTPS URL, no .git suffix, no trailing slash.
    /// Used as the stable cache key.
    pub canonical_url: String,
    /// URL to use for `git clone`. Preserves the original scheme (SSH, http, file).
    /// Falls back to canonical_url when not explicitly set.
    pub clone_url: String,
    /// Branch, tag, or commit SHA. Empty means HEAD.
    pub ref_name: String,
    /// True when ref_name is a full 40-char hex SHA.
    pub is_commit: bool,
}

// owner/repo or owner/repo@ref — no dots in owner, dots OK in repo.
static SHORTHAND_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([a-zA-Z0-9][a-zA-Z0-9_-]*)/([a-zA-Z0-9][a-zA-Z0-9_.-]*)(?:@(.+))?$").unwrap()
});

static FULL_SHA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-f]{40}$").unwrap());

impl RepoRef {
    /// Parse a repo reference string.
    ///
    /// Supported formats:
    ///   owner/repo[@ref]               GitHub shorthand
    ///   https://host/path[.git][@ref]  Full HTTPS URL
    ///   git@host:path[.git][@ref]      SSH URL
    pub fn parse(s: &str) -> Result<Self, RunxError> {
        if s.starts_with("git@") {
            Self::parse_ssh(s)
        } else if s.starts_with("https://") || s.starts_with("http://") {
            Self::parse_https(s)
        } else if s.starts_with("file://") {
            Self::parse_file_url(s)
        } else {
            Self::parse_shorthand(s)
        }
    }

    fn parse_shorthand(s: &str) -> Result<Self, RunxError> {
        let caps = SHORTHAND_RE.captures(s).ok_or_else(|| {
            RunxError::InvalidRepoRef(format!(
                "{s:?}: expected owner/repo[@ref] or a full git URL"
            ))
        })?;
        let canonical = format!(
            "https://github.com/{}/{}",
            &caps[1], &caps[2]
        );
        let ref_name = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        Ok(Self::make(canonical, ref_name))
    }

    fn parse_https(s: &str) -> Result<Self, RunxError> {
        // Detect original scheme so we can preserve it for cloning.
        let (original_scheme, without_scheme) = if let Some(rest) = s.strip_prefix("https://") {
            ("https", rest)
        } else if let Some(rest) = s.strip_prefix("http://") {
            ("http", rest)
        } else {
            return Err(RunxError::InvalidRepoRef(format!("invalid URL {s:?}")));
        };

        // Split host from path at first '/'.
        let (host, mut path) = match without_scheme.find('/') {
            Some(i) => (&without_scheme[..i], without_scheme[i..].to_string()),
            None => {
                return Err(RunxError::InvalidRepoRef(format!("invalid URL {s:?}: no path")));
            }
        };

        // Extract @ref from path.
        let ref_name = if let Some(i) = path.find('@') {
            let r = path[i + 1..].to_string();
            path.truncate(i);
            r
        } else {
            String::new()
        };

        // Normalize: strip .git suffix and trailing slashes.
        if path.ends_with(".git") {
            path.truncate(path.len() - 4);
        }
        let path = path.trim_end_matches('/');

        // Canonical URL always uses https for stable cache keys.
        let canonical = format!("https://{host}{path}");
        // Clone URL preserves the original scheme (http vs https).
        let clone_url = format!("{original_scheme}://{host}{path}");

        if canonical == clone_url {
            Ok(Self::make(canonical, &ref_name))
        } else {
            Ok(Self::make_with_clone_url(canonical, clone_url, &ref_name))
        }
    }

    fn parse_file_url(s: &str) -> Result<Self, RunxError> {
        // file:///path/to/repo[@ref]
        let without_scheme = s.strip_prefix("file://").unwrap();
        let (path, ref_name) = if let Some(i) = without_scheme.rfind('@') {
            (&without_scheme[..i], &without_scheme[i + 1..])
        } else {
            (without_scheme, "")
        };
        let canonical = format!("file://{path}");
        Ok(Self::make(canonical, ref_name))
    }

    fn parse_ssh(s: &str) -> Result<Self, RunxError> {
        let rest = s.strip_prefix("git@").unwrap();
        let colon_idx = rest.find(':').ok_or_else(|| {
            RunxError::InvalidRepoRef(format!("invalid SSH URL {s:?}: missing colon separator"))
        })?;
        let host = &rest[..colon_idx];
        let path_ref = &rest[colon_idx + 1..];

        // @ref suffix follows the last @ in the path portion.
        let (path_part, ref_name) = if let Some(i) = path_ref.rfind('@') {
            (&path_ref[..i], &path_ref[i + 1..])
        } else {
            (path_ref, "")
        };
        let path = path_part.strip_suffix(".git").unwrap_or(path_part);
        let canonical = format!("https://{host}/{path}");
        // Preserve the original SSH URL (without @ref) for cloning so that
        // private repos with SSH keys work correctly.
        let clone_url = format!("git@{host}:{path_part}");
        Ok(Self::make_with_clone_url(canonical, clone_url, ref_name))
    }

    fn make(canonical_url: String, ref_name: &str) -> Self {
        Self {
            clone_url: canonical_url.clone(),
            canonical_url,
            is_commit: is_full_sha(ref_name),
            ref_name: ref_name.to_string(),
        }
    }

    fn make_with_clone_url(canonical_url: String, clone_url: String, ref_name: &str) -> Self {
        Self {
            canonical_url,
            clone_url,
            is_commit: is_full_sha(ref_name),
            ref_name: ref_name.to_string(),
        }
    }

    /// True when the ref is a branch or tag (not a pinned commit).
    pub fn is_mutable_ref(&self) -> bool {
        !self.ref_name.is_empty() && !self.is_commit
    }

    /// The ref for display, defaulting to "HEAD" when empty.
    pub fn display_ref(&self) -> &str {
        if self.ref_name.is_empty() {
            "HEAD"
        } else {
            &self.ref_name
        }
    }
}

impl std::fmt::Display for RepoRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ref_name.is_empty() {
            write!(f, "{}", self.canonical_url)
        } else {
            write!(f, "{}@{}", self.canonical_url, self.ref_name)
        }
    }
}

/// Check if a string is a 40-char hex SHA.
pub fn is_full_sha(s: &str) -> bool {
    FULL_SHA_RE.is_match(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shorthand() {
        let r = RepoRef::parse("owner/repo").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
        assert_eq!(r.ref_name, "");
        assert!(!r.is_commit);
    }

    #[test]
    fn parse_shorthand_with_ref() {
        let r = RepoRef::parse("owner/repo@main").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
        assert_eq!(r.ref_name, "main");
    }

    #[test]
    fn parse_shorthand_with_tag() {
        let r = RepoRef::parse("owner/repo@v1.2.0").unwrap();
        assert_eq!(r.ref_name, "v1.2.0");
    }

    #[test]
    fn parse_shorthand_full_sha() {
        let sha = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let r = RepoRef::parse(&format!("owner/repo@{sha}")).unwrap();
        assert_eq!(r.ref_name, sha);
        assert!(r.is_commit);
    }

    #[test]
    fn parse_shorthand_dots_hyphens() {
        let r = RepoRef::parse("acme-corp/my.tool@v2").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/acme-corp/my.tool");
        assert_eq!(r.ref_name, "v2");
    }

    #[test]
    fn parse_https() {
        let r = RepoRef::parse("https://github.com/owner/repo").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
        assert_eq!(r.ref_name, "");
    }

    #[test]
    fn parse_https_dotgit() {
        let r = RepoRef::parse("https://github.com/owner/repo.git").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
    }

    #[test]
    fn parse_https_dotgit_ref() {
        let r = RepoRef::parse("https://github.com/owner/repo.git@main").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
        assert_eq!(r.ref_name, "main");
    }

    #[test]
    fn parse_https_ref() {
        let r = RepoRef::parse("https://github.com/owner/repo@v1.2.0").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
        assert_eq!(r.ref_name, "v1.2.0");
    }

    #[test]
    fn parse_ssh() {
        let r = RepoRef::parse("git@github.com:owner/repo.git").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
        assert_eq!(r.clone_url, "git@github.com:owner/repo.git");
        assert_eq!(r.ref_name, "");
    }

    #[test]
    fn parse_ssh_ref() {
        let r = RepoRef::parse("git@github.com:owner/repo.git@main").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
        assert_eq!(r.clone_url, "git@github.com:owner/repo.git");
        assert_eq!(r.ref_name, "main");
    }

    #[test]
    fn parse_ssh_gitlab() {
        let r = RepoRef::parse("git@gitlab.com:group/project.git@develop").unwrap();
        assert_eq!(r.canonical_url, "https://gitlab.com/group/project");
        assert_eq!(r.clone_url, "git@gitlab.com:group/project.git");
        assert_eq!(r.ref_name, "develop");
    }

    #[test]
    fn shorthand_clone_url_matches_canonical() {
        let r = RepoRef::parse("owner/repo@main").unwrap();
        assert_eq!(r.clone_url, r.canonical_url);
    }

    #[test]
    fn https_clone_url_matches_canonical() {
        let r = RepoRef::parse("https://github.com/owner/repo@v1").unwrap();
        assert_eq!(r.clone_url, r.canonical_url);
    }

    #[test]
    fn parse_http_normalized() {
        let r = RepoRef::parse("http://github.com/owner/repo").unwrap();
        assert_eq!(r.canonical_url, "https://github.com/owner/repo");
        assert_eq!(r.clone_url, "http://github.com/owner/repo");
    }

    #[test]
    fn parse_errors() {
        assert!(RepoRef::parse("notarepo").is_err());
        assert!(RepoRef::parse("").is_err());
    }

    #[test]
    fn is_mutable_ref_cases() {
        assert!(!RepoRef::parse("owner/repo").unwrap().is_mutable_ref());
        assert!(RepoRef::parse("owner/repo@main").unwrap().is_mutable_ref());
        assert!(RepoRef::parse("owner/repo@v1.0.0").unwrap().is_mutable_ref());
        let sha = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        assert!(!RepoRef::parse(&format!("owner/repo@{sha}")).unwrap().is_mutable_ref());
    }
}
