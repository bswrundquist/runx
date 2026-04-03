use crate::core::{Flags, RepoRef, RunxError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
struct TrustFile {
    version: u32,
    repos: HashMap<String, TrustEntry>,
}

#[derive(Serialize, Deserialize)]
struct TrustEntry {
    url: String,
    trusted_at: u64,
}

/// Tracks permanently trusted repositories.
pub struct TrustStore {
    path: PathBuf,
}

impl TrustStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Whether the canonical URL has been permanently trusted.
    pub fn is_trusted(&self, canonical: &str) -> bool {
        self.load().repos.contains_key(canonical)
    }

    /// Permanently trust canonical and save to disk.
    pub fn trust(&self, canonical: &str) {
        let mut tf = self.load();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        tf.repos.insert(
            canonical.to_string(),
            TrustEntry {
                url: canonical.to_string(),
                trusted_at: now,
            },
        );
        let _ = self.save(&tf);
    }

    /// Verify trust for ref/commit and prompt the user if needed.
    pub fn check(&self, repo_ref: &RepoRef, commit: &str, flags: &Flags) -> Result<(), RunxError> {
        // --trust implies --yes and permanently records trust.
        if flags.trust {
            self.trust(&repo_ref.canonical_url);
            return Ok(());
        }
        // --yes: skip prompt but do not permanently trust.
        if flags.yes {
            return Ok(());
        }
        // Already trusted.
        if self.is_trusted(&repo_ref.canonical_url) {
            return Ok(());
        }

        // First-run prompt.
        let is_sha = crate::core::repo::is_full_sha(commit);
        let short_commit = if is_sha && commit.len() > 12 {
            &commit[..12]
        } else {
            commit
        };

        let stderr = io::stderr();
        let mut err = stderr.lock();
        writeln!(err).ok();
        writeln!(err, "runx: \u{26a0}\u{fe0f}  executing code from an untrusted repository").ok();
        writeln!(err).ok();
        writeln!(err, "  Repository: {}", repo_ref.canonical_url).ok();
        if is_sha {
            writeln!(err, "  Ref:        {} \u{2192} {}", repo_ref.display_ref(), short_commit).ok();
        } else {
            writeln!(err, "  Version:    {commit}").ok();
        }
        if repo_ref.is_mutable_ref() {
            writeln!(err).ok();
            writeln!(
                err,
                "  WARNING: '{}' is a mutable ref. Pin to a commit SHA for reproducibility:",
                repo_ref.ref_name
            ).ok();
            writeln!(err, "           {}@{}", repo_ref.canonical_url, commit).ok();
        }
        writeln!(err).ok();
        write!(
            err,
            "  Type 'yes' to proceed once, 'trust' to remember, or Ctrl-C to abort: "
        ).ok();
        err.flush().ok();

        let stdin = io::stdin();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_err() || line.is_empty() {
            return Err(RunxError::TrustDeclined);
        }
        match line.trim().to_lowercase().as_str() {
            "yes" | "y" => Ok(()),
            "trust" | "t" => {
                self.trust(&repo_ref.canonical_url);
                writeln!(err, "\n  Trusted. Future runs will skip this prompt.\n").ok();
                Ok(())
            }
            _ => Err(RunxError::TrustDeclined),
        }
    }

    fn load(&self) -> TrustFile {
        let data = match fs::read_to_string(&self.path) {
            Ok(d) => d,
            Err(_) => {
                return TrustFile {
                    version: 1,
                    repos: HashMap::new(),
                }
            }
        };
        serde_json::from_str(&data).unwrap_or(TrustFile {
            version: 1,
            repos: HashMap::new(),
        })
    }

    fn save(&self, tf: &TrustFile) -> Result<(), RunxError> {
        let data = serde_json::to_string_pretty(tf)
            .map_err(|e| RunxError::Io(io::Error::other(e)))?;
        fs::write(&self.path, data)?;
        Ok(())
    }
}
