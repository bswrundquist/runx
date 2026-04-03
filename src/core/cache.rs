use crate::core::RunxError;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Manages the on-disk cache layout.
///
/// Layout:
///   <root>/
///     repos/<urlhash>/         bare git repos
///     trees/<urlhash>/<sha>/   immutable materialized checkouts
///     releases/<urlhash>/<tag>/<asset>/  downloaded release binaries
///     trust.json               trusted repo registry
pub struct Cache {
    pub root: PathBuf,
}

impl Cache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Short, stable hash of the canonical URL for directory names.
    /// First 8 bytes of SHA-256 (16 hex characters).
    pub fn url_hash(canonical: &str) -> String {
        let hash = Sha256::digest(canonical.as_bytes());
        hex::encode(&hash[..8])
    }

    /// Path for the bare git repository of a canonical URL.
    pub fn bare_repo_dir(&self, canonical: &str) -> PathBuf {
        self.root.join("repos").join(Self::url_hash(canonical))
    }

    /// Path for the immutable materialized tree of a specific commit.
    pub fn tree_dir(&self, canonical: &str, commit: &str) -> PathBuf {
        self.root
            .join("trees")
            .join(Self::url_hash(canonical))
            .join(commit)
    }

    /// Path for a cached release binary asset.
    pub fn release_dir(&self, canonical: &str, tag: &str, asset: &str) -> PathBuf {
        self.root
            .join("releases")
            .join(Self::url_hash(canonical))
            .join(tag)
            .join(asset)
    }

    /// Path to the trust store JSON file.
    pub fn trust_file(&self) -> PathBuf {
        self.root.join("trust.json")
    }

    /// Create the top-level cache directories.
    pub fn ensure_roots(&self) -> Result<(), RunxError> {
        for sub in &["repos", "trees", "releases"] {
            fs::create_dir_all(self.root.join(sub)).map_err(RunxError::Cache)?;
        }
        Ok(())
    }

    /// Whether the immutable tree for commit has been materialized.
    pub fn tree_exists(&self, canonical: &str, commit: &str) -> bool {
        dir_exists(&self.tree_dir(canonical, commit))
    }
}

fn dir_exists(path: &Path) -> bool {
    path.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_hash_deterministic() {
        let h1 = Cache::url_hash("https://github.com/owner/repo");
        let h2 = Cache::url_hash("https://github.com/owner/repo");
        assert_eq!(h1, h2);
    }

    #[test]
    fn url_hash_no_collision() {
        let h1 = Cache::url_hash("https://github.com/owner/repo");
        let h2 = Cache::url_hash("https://github.com/owner/other");
        assert_ne!(h1, h2);
    }

    #[test]
    fn url_hash_length() {
        let h = Cache::url_hash("https://github.com/owner/repo");
        assert_eq!(h.len(), 16);
    }

    #[test]
    fn cache_layout() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::new(dir.path().to_path_buf());
        let canonical = "https://github.com/owner/repo";
        let commit = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";

        c.ensure_roots().unwrap();
        assert!(dir.path().join("repos").is_dir());
        assert!(dir.path().join("trees").is_dir());
        assert!(dir.path().join("releases").is_dir());

        let bare = c.bare_repo_dir(canonical);
        assert!(bare.is_absolute());
        assert_eq!(c.bare_repo_dir(canonical), bare); // deterministic

        let other = c.bare_repo_dir("https://github.com/owner/other");
        assert_ne!(other, bare);

        let tree = c.tree_dir(canonical, commit);
        assert_eq!(tree.file_name().unwrap().to_str().unwrap(), commit);

        assert!(!c.tree_exists(canonical, commit));
        fs::create_dir_all(&tree).unwrap();
        assert!(c.tree_exists(canonical, commit));

        let tf = c.trust_file();
        assert_eq!(tf.parent().unwrap(), dir.path());
    }
}
