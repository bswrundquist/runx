use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Create a local bare git repo for integration testing.
/// Returns (tempdir_handle, bare_path, commit_sha).
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
        // Write with executable bit for scripts.
        fs::write(&full, content).unwrap();
        #[cfg(unix)]
        if name.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&full, fs::Permissions::from_mode(0o755)).unwrap();
        }
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

    // work_dir is a full clone source; bare_dir is independent.
    // Drop work_dir now — the bare clone doesn't reference it.
    drop(work_dir);

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

/// Copy bare repo into a cache directory so the engine can find it.
fn prepare_repo_in_cache(bare_path: &Path, cache_dir: &Path, canonical: &str) {
    let hash = sha256_prefix(canonical);
    let repos_dir = cache_dir.join("repos");
    fs::create_dir_all(&repos_dir).unwrap();
    let dest = repos_dir.join(&hash);
    copy_dir_recursive(bare_path, &dest);
}

fn sha256_prefix(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(s.as_bytes());
    hex::encode(&hash[..8])
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    Command::new("cp")
        .args(["-r", &src.to_string_lossy(), &dst.to_string_lossy()])
        .status()
        .unwrap();
}

/// Find the runx binary (built by cargo test).
fn runx_bin() -> PathBuf {
    // cargo test puts the binary in target/debug or target/release
    let mut path = std::env::current_exe().unwrap();
    // Go up from the test binary to the deps dir, then to the target dir.
    path.pop(); // remove test binary name
    path.pop(); // remove deps/
    path.push("runx");
    if path.exists() {
        return path;
    }
    // Fallback: try bin/runx
    PathBuf::from("bin/runx")
}

/// Helper: build common args for a repo (flags before repo, mode/target after).
fn base_args(cache_dir: &Path, sha: &str, bare: &Path) -> Vec<String> {
    vec![
        "--yes".to_string(),
        format!("--cache-dir={}", cache_dir.display()),
        format!("--commit={sha}"),
        format!("file://{}", bare.display()),
    ]
}

// ---------------------------------------------------------------------------
// Make tests (explicit mode: runx <repo> make [target])
// ---------------------------------------------------------------------------

#[test]
fn make_target() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: greet\ngreet:\n\t@echo hello-from-make\n",
    )]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let mut args = base_args(cache_dir.path(), &sha, &bare);
    args.push("make".to_string());
    args.push("greet".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("hello-from-make"),
        "expected 'hello-from-make' in output, got: {stdout}"
    );
}

#[test]
fn make_default_target() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: all\nall:\n\t@echo default-target-ran\n",
    )]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let mut args = base_args(cache_dir.path(), &sha, &bare);
    args.push("make".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("default-target-ran"),
        "expected 'default-target-ran' in output, got: {stdout}"
    );
}

#[test]
fn make_exit_code() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: fail\nfail:\n\t@exit 7\n",
    )]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let mut args = base_args(cache_dir.path(), &sha, &bare);
    args.push("make".to_string());
    args.push("fail".to_string());

    let status = Command::new(runx_bin()).args(&args).status().unwrap();

    assert!(!status.success(), "expected non-zero exit code");
}

#[test]
fn make_write_does_not_dirty_immutable_cache() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: build\nbuild:\n\t@echo artifact > artifact.txt\n",
    )]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let mut args = base_args(cache_dir.path(), &sha, &bare);
    args.push("make".to_string());
    args.push("build".to_string());

    let status = Command::new(runx_bin()).args(&args).status().unwrap();
    assert!(status.success());

    // Check that the immutable tree does NOT have artifact.txt.
    let hash = sha256_prefix(&canonical);
    let tree_dir = cache_dir.path().join("trees").join(&hash).join(&sha);
    assert!(
        !tree_dir.join("artifact.txt").exists(),
        "immutable tree was dirtied: artifact.txt found in cache"
    );
}

// ---------------------------------------------------------------------------
// Shell tests (explicit mode: runx <repo> sh <script>)
// ---------------------------------------------------------------------------

#[test]
fn sh_exit_code() {
    let (_td, bare, sha) = make_local_repo(&[("exit42.sh", "#!/bin/sh\nexit 42\n")]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let mut args = base_args(cache_dir.path(), &sha, &bare);
    args.push("sh".to_string());
    args.push("exit42.sh".to_string());

    let status = Command::new(runx_bin()).args(&args).status().unwrap();

    assert_eq!(status.code(), Some(42));
}

#[test]
fn sh_no_shebang() {
    let (_td, bare, sha) =
        make_local_repo(&[("noshebang.sh", "printf 'ran-without-shebang'\n")]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let mut args = base_args(cache_dir.path(), &sha, &bare);
    args.push("sh".to_string());
    args.push("noshebang.sh".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "ran-without-shebang");
}

#[test]
fn sh_arg_passthrough() {
    let (_td, bare, sha) = make_local_repo(&[(
        "args.sh",
        "#!/bin/sh\nprintf '%s\\n' \"$@\"\n",
    )]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let out = Command::new(runx_bin())
        .args([
            "--yes",
            &format!("--cache-dir={}", cache_dir.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "sh",
            "args.sh",
            "--",
            "--dry-run",
            "foo bar",
            "--count=3",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines, vec!["--dry-run", "foo bar", "--count=3"]);
}

// ---------------------------------------------------------------------------
// Explicit mode: sh with bare script name (no extension)
// ---------------------------------------------------------------------------

#[test]
fn sh_bare_script_name() {
    let (_td, bare, sha) = make_local_repo(&[("run", "#!/bin/sh\necho forced-shell\n")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let out = Command::new(runx_bin())
        .args([
            "--yes",
            &format!("--cache-dir={}", cache.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "sh",
            "run",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("forced-shell"),
        "expected 'forced-shell', got: {stdout}"
    );
}

#[test]
fn make_target_with_dot() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: deploy.sh\ndeploy.sh:\n\t@echo forced-make\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let out = Command::new(runx_bin())
        .args([
            "--yes",
            &format!("--cache-dir={}", cache.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "make",
            "deploy.sh",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("forced-make"),
        "expected 'forced-make', got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error paths: missing required args
// ---------------------------------------------------------------------------

#[test]
fn sh_without_script_errors() {
    let (_td, bare, sha) = make_local_repo(&[("dummy.txt", "x")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let mut args = base_args(cache.path(), &sha, &bare);
    args.push("sh".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("script path"), "expected script path error, got: {stderr}");
}

#[test]
fn bin_without_asset_errors() {
    let (_td, bare, sha) = make_local_repo(&[("dummy.txt", "x")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let mut args = base_args(cache.path(), &sha, &bare);
    args.push("bin".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("asset name"), "expected asset name error, got: {stderr}");
}

#[test]
fn no_repo_errors() {
    let out = Command::new(runx_bin())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("expected"), "expected usage error, got: {stderr}");
}

// ---------------------------------------------------------------------------
// Error: unknown mode
// ---------------------------------------------------------------------------

#[test]
fn unknown_mode_errors() {
    let (_td, bare, sha) = make_local_repo(&[("dummy.txt", "x")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let mut args = base_args(cache.path(), &sha, &bare);
    args.push("something".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown mode"), "expected unknown mode error, got: {stderr}");
}

#[test]
fn no_mode_errors() {
    let (_td, bare, sha) = make_local_repo(&[("dummy.txt", "x")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let args = base_args(cache.path(), &sha, &bare);

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no mode specified"), "expected no mode error, got: {stderr}");
}

// ---------------------------------------------------------------------------
// Make with passthrough args
// ---------------------------------------------------------------------------

#[test]
fn make_passthrough_args() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: greet\ngreet:\n\t@echo hello-passthrough\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let out = Command::new(runx_bin())
        .args([
            "--yes",
            &format!("--cache-dir={}", cache.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "make",
            "greet",
            "--",
            "VERBOSE=1",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("hello-passthrough"),
        "expected 'hello-passthrough', got: {stdout}"
    );
}
