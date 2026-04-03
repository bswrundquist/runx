use crate::core::{Cache, Flags, Git, RepoRef, RunxError, TrustStore, Workspace};
use std::fs;

/// The shared core that all runx tools use.
/// Orchestrates ref resolution, caching, trust, and materialization.
pub struct Engine {
    pub flags: Flags,
    cache: Cache,
    git: Git,
    trust: TrustStore,
}

impl Engine {
    pub fn new(flags: Flags) -> Self {
        let cache = Cache::new(flags.cache_dir.clone());
        let trust_path = cache.trust_file();
        Self {
            git: Git {
                verbose: flags.verbose,
            },
            trust: TrustStore::new(trust_path),
            cache,
            flags,
        }
    }

    /// Resolve ref, ensure repo is cached, check trust, materialize a checkout,
    /// and return a ready Workspace.
    ///
    /// If mutable is true, the workspace is a writable temporary copy.
    pub fn prepare(&self, repo_ref: &RepoRef, mutable: bool) -> Result<Workspace, RunxError> {
        // Override ref with --commit flag.
        let repo_ref = if let Some(ref commit) = self.flags.commit {
            RepoRef {
                canonical_url: repo_ref.canonical_url.clone(),
                clone_url: repo_ref.clone_url.clone(),
                ref_name: commit.clone(),
                is_commit: crate::core::repo::is_full_sha(commit),
            }
        } else {
            repo_ref.clone()
        };

        self.cache.ensure_roots()?;

        let bare_dir = self.cache.bare_repo_dir(&repo_ref.canonical_url);

        // Clone or fetch using the original clone URL (preserves SSH, file://, etc).
        self.git.clone_or_fetch(
            &repo_ref.clone_url,
            &bare_dir,
            self.flags.offline,
            self.flags.refresh,
            repo_ref.is_mutable_ref(),
            &repo_ref.ref_name,
        )?;

        // Resolve ref → commit.
        let commit = self.git.resolve_ref(&bare_dir, &repo_ref.ref_name)?;

        // Announce the resolved commit.
        let short = if commit.len() > 12 {
            &commit[..12]
        } else {
            &commit
        };
        eprintln!("runx: {} \u{2192} {}", repo_ref.display_ref(), short);

        // Print pinned command when requested.
        if self.flags.pin && repo_ref.is_mutable_ref() {
            eprintln!("runx: pin \u{2192} {}@{}", repo_ref.canonical_url, commit);
        }

        // Trust check (may prompt the user interactively).
        self.trust.check(&repo_ref, &commit, &self.flags)?;

        // Materialize immutable tree if not already cached.
        // git archive operates on the local bare repo — no network needed.
        // Archive into a temp dir then rename atomically to prevent TOCTOU
        // races when multiple runx processes target the same commit.
        let tree_dir = self.cache.tree_dir(&repo_ref.canonical_url, &commit);
        if !self.cache.tree_exists(&repo_ref.canonical_url, &commit) {
            let tmp_dir = tree_dir.with_extension(format!("tmp.{}", std::process::id()));
            if let Err(e) = self.git.archive(&bare_dir, &commit, &tmp_dir) {
                let _ = fs::remove_dir_all(&tmp_dir);
                return Err(RunxError::Git {
                    op: "materializing tree",
                    detail: e.to_string(),
                });
            }
            // Atomic move into place; if another process won the race, discard ours.
            if fs::rename(&tmp_dir, &tree_dir).is_err() {
                let _ = fs::remove_dir_all(&tmp_dir);
            }
        }

        if !mutable {
            return Ok(Workspace::immutable(tree_dir, commit));
        }
        Workspace::mutable_copy(&tree_dir, commit)
    }
}
