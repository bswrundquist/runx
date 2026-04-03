use crate::core::repo::is_full_sha;
use crate::core::RunxError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// How long a cached ref→commit mapping is considered fresh.
const REF_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Serialize, Deserialize, Default)]
struct RefCache {
    refs: HashMap<String, RefEntry>,
}

#[derive(Serialize, Deserialize)]
struct RefEntry {
    commit: String,
    /// Seconds since UNIX epoch.
    resolved_at: u64,
}

/// Wraps the system git binary for repository operations.
pub struct Git {
    pub verbose: bool,
}

impl Git {
    /// Ensure the bare repo cache is present and optionally up-to-date.
    ///
    /// Logic:
    ///   - Bare repo absent + offline → error
    ///   - Bare repo absent            → clone
    ///   - Bare repo present + offline → return (use cache as-is)
    ///   - Bare repo present + refresh → fetch
    ///   - Bare repo present + mutable_ref AND TTL expired → fetch
    ///   - Bare repo present + immutable ref → skip fetch
    pub fn clone_or_fetch(
        &self,
        clone_url: &str,
        bare_dir: &Path,
        offline: bool,
        refresh: bool,
        mutable_ref: bool,
        ref_name: &str,
    ) -> Result<(), RunxError> {
        let exists = bare_dir.is_dir();
        if !exists {
            if offline {
                return Err(RunxError::Offline(format!(
                    "repo not in cache and --offline is set: {clone_url}"
                )));
            }
            return self.clone(clone_url, bare_dir);
        }
        if offline {
            return Ok(());
        }
        if refresh {
            return self.fetch(bare_dir);
        }
        // HEAD (empty ref_name) tracks the default branch and is also mutable.
        if mutable_ref || ref_name.is_empty() {
            // Normalize the TTL cache key: resolve_ref/cache_ref use "HEAD" for
            // empty ref_name, so the lookup here must match.
            let cache_key = if ref_name.is_empty() { "HEAD" } else { ref_name };
            if self.cached_ref(bare_dir, cache_key).is_some() {
                if self.verbose {
                    eprintln!("runx: ref {cache_key:?} is fresh (TTL); skipping fetch");
                }
                return Ok(());
            }
            return self.fetch(bare_dir);
        }
        Ok(())
    }

    fn clone(&self, url: &str, bare_dir: &Path) -> Result<(), RunxError> {
        if let Some(parent) = bare_dir.parent() {
            fs::create_dir_all(parent).map_err(RunxError::Cache)?;
        }
        self.run(None, &["clone", "--bare", "--quiet", url, &bare_dir.to_string_lossy()])
    }

    fn fetch(&self, bare_dir: &Path) -> Result<(), RunxError> {
        self.run(Some(bare_dir), &["fetch", "--prune", "--quiet", "origin"])
    }

    /// Resolve ref to a full 40-char commit SHA using the local bare repo.
    /// Uses a TTL-based cache to skip repeated git calls within REF_TTL.
    pub fn resolve_ref(&self, bare_dir: &Path, ref_name: &str) -> Result<String, RunxError> {
        let ref_name = if ref_name.is_empty() { "HEAD" } else { ref_name };

        // Full SHA: verify it exists locally then return immediately.
        if is_full_sha(ref_name) {
            let check = format!("{ref_name}^{{commit}}");
            if self.run(Some(bare_dir), &["cat-file", "-e", &check]).is_ok() {
                return Ok(ref_name.to_string());
            }
            // SHA not present locally — trust the user; we'll get an archive error if wrong.
            return Ok(ref_name.to_string());
        }

        // Check TTL cache.
        if let Some(commit) = self.cached_ref(bare_dir, ref_name) {
            return Ok(commit);
        }

        // Peel to commit: resolves branches, tags, annotated tags, short SHAs.
        let check = format!("{ref_name}^{{commit}}");
        let out = self.output(Some(bare_dir), &["rev-parse", "--verify", &check])?;
        let commit = out.trim().to_string();
        if !is_full_sha(&commit) {
            return Err(RunxError::Git {
                op: "rev-parse",
                detail: format!("unexpected output for ref {ref_name:?}: {commit:?}"),
            });
        }
        self.cache_ref(bare_dir, ref_name, &commit);
        Ok(commit)
    }

    /// Extract the commit tree into dest_dir using git archive piped through tar.
    pub fn archive(&self, bare_dir: &Path, commit: &str, dest_dir: &Path) -> Result<(), RunxError> {
        fs::create_dir_all(dest_dir)?;

        let git_dir_arg = format!("--git-dir={}", bare_dir.display());
        let mut git_cmd = Command::new("git");
        git_cmd.args([&git_dir_arg, "archive", commit]);
        git_cmd.stdout(Stdio::piped());
        if !self.verbose {
            git_cmd.stderr(Stdio::null());
        }

        let mut tar_cmd = Command::new("tar");
        tar_cmd.args(["-C", &dest_dir.to_string_lossy(), "-xf", "-"]);
        if !self.verbose {
            tar_cmd.stderr(Stdio::null());
        }

        let mut git_child = git_cmd.spawn().map_err(|e| RunxError::Git {
            op: "git archive",
            detail: e.to_string(),
        })?;

        // Take stdout so the pipe is moved to tar, not borrowed.
        tar_cmd.stdin(git_child.stdout.take().unwrap());
        let tar_output = tar_cmd.output().map_err(|e| RunxError::Git {
            op: "tar",
            detail: e.to_string(),
        })?;

        // Reap the git process to avoid zombie.
        let git_status = git_child.wait().map_err(|e| RunxError::Git {
            op: "git archive",
            detail: format!("waiting for git archive: {e}"),
        })?;

        if !git_status.success() {
            return Err(RunxError::Git {
                op: "git archive",
                detail: format!("exit code: {:?}", git_status.code()),
            });
        }

        if !tar_output.status.success() {
            return Err(RunxError::Git {
                op: "tar extract",
                detail: format!("exit code: {:?}", tar_output.status.code()),
            });
        }

        Ok(())
    }

    fn run(&self, dir: Option<&Path>, args: &[&str]) -> Result<(), RunxError> {
        let mut cmd = Command::new("git");
        cmd.args(args);
        set_git_timeouts(&mut cmd);
        if let Some(d) = dir {
            cmd.current_dir(d);
        }
        if self.verbose {
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());
            let status = cmd.status().map_err(|e| RunxError::Git {
                op: "git",
                detail: e.to_string(),
            })?;
            if !status.success() {
                return Err(RunxError::Git {
                    op: "git",
                    detail: format!("{args:?} exited with {status}"),
                });
            }
        } else {
            // Capture stderr so we can include it in error messages.
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::piped());
            let output = cmd.output().map_err(|e| RunxError::Git {
                op: "git",
                detail: e.to_string(),
            })?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let detail = if stderr.trim().is_empty() {
                    format!("{args:?} exited with {}", output.status)
                } else {
                    format!("{args:?}: {}", stderr.trim())
                };
                return Err(RunxError::Git { op: "git", detail });
            }
        }
        Ok(())
    }

    fn output(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, RunxError> {
        let mut cmd = Command::new("git");
        cmd.args(args);
        set_git_timeouts(&mut cmd);
        if let Some(d) = dir {
            cmd.current_dir(d);
        }
        if self.verbose {
            cmd.stderr(Stdio::inherit());
        }
        let out = cmd.output().map_err(|e| RunxError::Git {
            op: "git",
            detail: e.to_string(),
        })?;
        if !out.status.success() {
            return Err(RunxError::Git {
                op: "git",
                detail: format!(
                    "cannot resolve ref in {:?}: {}",
                    args,
                    String::from_utf8_lossy(&out.stderr)
                ),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn cached_ref(&self, bare_dir: &Path, ref_name: &str) -> Option<String> {
        let data = fs::read_to_string(ref_cache_path(bare_dir)).ok()?;
        let rc: RefCache = serde_json::from_str(&data).ok()?;
        let entry = rc.refs.get(ref_name)?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(entry.resolved_at) > REF_TTL.as_secs() {
            return None;
        }
        Some(entry.commit.clone())
    }

    fn cache_ref(&self, bare_dir: &Path, ref_name: &str, commit: &str) {
        let path = ref_cache_path(bare_dir);
        let mut rc: RefCache = fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        rc.refs.insert(
            ref_name.to_string(),
            RefEntry {
                commit: commit.to_string(),
                resolved_at: now,
            },
        );
        if let Ok(data) = serde_json::to_string_pretty(&rc) {
            let _ = fs::write(path, data);
        }
    }
}

/// Set sensible network timeouts so git doesn't hang indefinitely on flaky connections.
/// Abort if transfer speed stays below 1 KB/s for 30 seconds.
fn set_git_timeouts(cmd: &mut Command) {
    cmd.env("GIT_HTTP_LOW_SPEED_LIMIT", "1000");
    cmd.env("GIT_HTTP_LOW_SPEED_TIME", "30");
}

fn ref_cache_path(bare_dir: &Path) -> PathBuf {
    bare_dir.join("runx-refs.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cache::Cache;
    use crate::core::workspace::Workspace;

    /// Create a local bare git repo for testing. Returns (bare_dir, commit_sha).
    fn make_local_repo(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf, String) {
        let work_dir = tempfile::tempdir().unwrap();
        let work = work_dir.path();

        run_git(work, &["init", "-b", "main"]);
        run_git(work, &["config", "user.email", "test@example.com"]);
        run_git(work, &["config", "user.name", "Test"]);

        for (name, content) in files {
            let full = work.join(name);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, content).unwrap();
        }

        run_git(work, &["add", "."]);
        run_git(work, &["commit", "-m", "initial"]);

        let out = Command::new("git")
            .args(["-C", &work.to_string_lossy(), "rev-parse", "HEAD"])
            .output()
            .unwrap();
        let sha = String::from_utf8(out.stdout).unwrap().trim().to_string();

        let bare_dir = tempfile::tempdir().unwrap();
        let bare_path = bare_dir.path().join("bare.git");
        Command::new("git")
            .args([
                "clone",
                "--bare",
                "--quiet",
                &work.to_string_lossy(),
                &bare_path.to_string_lossy(),
            ])
            .status()
            .unwrap();

        (bare_dir, bare_path, sha)
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn resolve_ref_full_sha() {
        let (_td, bare, sha) = make_local_repo(&[("hello.txt", "hello world\n")]);
        let g = Git { verbose: false };
        let got = g.resolve_ref(&bare, &sha).unwrap();
        assert_eq!(got, sha);
    }

    #[test]
    fn resolve_ref_branch() {
        let (_td, bare, sha) = make_local_repo(&[("hello.txt", "hello world\n")]);
        let g = Git { verbose: false };
        let got = g.resolve_ref(&bare, "main").unwrap();
        assert_eq!(got, sha);
    }

    #[test]
    fn resolve_ref_ttl_cache() {
        let (_td, bare, sha) = make_local_repo(&[("hello.txt", "hello world\n")]);
        let g = Git { verbose: false };

        let got1 = g.resolve_ref(&bare, "main").unwrap();
        let got2 = g.resolve_ref(&bare, "main").unwrap();
        assert_eq!(got1, sha);
        assert_eq!(got2, sha);

        // Verify cache file exists.
        assert!(bare.join("runx-refs.json").exists());
    }

    #[test]
    fn archive_extracts_files() {
        let (_td, bare, sha) = make_local_repo(&[
            ("hello.txt", "hello world\n"),
            ("scripts/run.sh", "#!/bin/sh\necho hi\n"),
        ]);

        let dest = tempfile::tempdir().unwrap();
        let g = Git { verbose: false };
        g.archive(&bare, &sha, dest.path()).unwrap();

        let content = fs::read_to_string(dest.path().join("hello.txt")).unwrap();
        assert_eq!(content, "hello world\n");
        assert!(dest.path().join("scripts/run.sh").exists());
    }

    #[test]
    fn archive_immutable_cache_protection() {
        let (_td, bare, sha) = make_local_repo(&[("data.txt", "original\n")]);

        let immutable = tempfile::tempdir().unwrap();
        let g = Git { verbose: false };
        g.archive(&bare, &sha, immutable.path()).unwrap();

        let ws = Workspace::mutable_copy(immutable.path(), sha).unwrap();
        fs::write(ws.root.join("data.txt"), "mutated\n").unwrap();

        let content = fs::read_to_string(immutable.path().join("data.txt")).unwrap();
        assert_eq!(content, "original\n");
    }

    #[test]
    fn cache_hit_miss() {
        let (_td, bare, sha) = make_local_repo(&[("file.txt", "content\n")]);

        let cache_dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(cache_dir.path().to_path_buf());
        cache.ensure_roots().unwrap();

        let canonical = format!("file://{}", bare.display());
        assert!(!cache.tree_exists(&canonical, &sha));

        let tree_dir = cache.tree_dir(&canonical, &sha);
        let g = Git { verbose: false };
        g.archive(&bare, &sha, &tree_dir).unwrap();

        assert!(cache.tree_exists(&canonical, &sha));
    }
}
