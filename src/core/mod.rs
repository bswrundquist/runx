pub mod cache;
pub mod engine;
pub mod error;
pub mod exec;
pub mod git;
pub mod repo;
pub mod trust;
pub mod workspace;

pub use cache::Cache;
pub use engine::Engine;
pub use error::RunxError;
pub use exec::{make_executable, run_child, run_child_with_stderr};
pub use git::Git;
pub use repo::RepoRef;
pub use trust::TrustStore;
pub use workspace::Workspace;

use std::path::PathBuf;

/// Shared flags for all runx tools.
pub struct Flags {
    pub refresh: bool,
    pub offline: bool,
    pub cache_dir: PathBuf,
    pub yes: bool,
    pub trust: bool,
    pub pin: bool,
    pub verbose: bool,
    pub commit: Option<String>,
}

impl Flags {
    /// Returns the platform default cache directory.
    /// Overridable via the RUNX_CACHE_DIR environment variable.
    pub fn default_cache_dir() -> PathBuf {
        if let Ok(d) = std::env::var("RUNX_CACHE_DIR") {
            return PathBuf::from(d);
        }
        if let Some(home) = dirs_home() {
            return home.join(".cache").join("runx");
        }
        std::env::temp_dir().join("runx-cache")
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
