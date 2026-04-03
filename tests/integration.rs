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

// ---------------------------------------------------------------------------
// Make tests
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

    let out = Command::new(runx_bin())
        .args([
            "make",
            "--yes",
            &format!("--cache-dir={}", cache_dir.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "greet",
        ])
        .output()
        .unwrap();

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

    let out = Command::new(runx_bin())
        .args([
            "make",
            "--yes",
            &format!("--cache-dir={}", cache_dir.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
        ])
        .output()
        .unwrap();

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

    let status = Command::new(runx_bin())
        .args([
            "make",
            "--yes",
            &format!("--cache-dir={}", cache_dir.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "fail",
        ])
        .status()
        .unwrap();

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

    let status = Command::new(runx_bin())
        .args([
            "make",
            "--yes",
            &format!("--cache-dir={}", cache_dir.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "build",
        ])
        .status()
        .unwrap();
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
// Shell tests
// ---------------------------------------------------------------------------

#[test]
fn sh_exit_code() {
    let (_td, bare, sha) = make_local_repo(&[("exit42.sh", "#!/bin/sh\nexit 42\n")]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let status = Command::new(runx_bin())
        .args([
            "sh",
            "--yes",
            &format!("--cache-dir={}", cache_dir.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "exit42.sh",
        ])
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(42));
}

#[test]
fn sh_no_shebang() {
    let (_td, bare, sha) =
        make_local_repo(&[("noshebang.sh", "printf 'ran-without-shebang'\n")]);

    let cache_dir = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache_dir.path(), &canonical);

    let out = Command::new(runx_bin())
        .args([
            "sh",
            "--yes",
            &format!("--cache-dir={}", cache_dir.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "noshebang.sh",
        ])
        .output()
        .unwrap();

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
            "sh",
            "--yes",
            &format!("--cache-dir={}", cache_dir.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
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
// Auto-detection tests (via CLI)
// ---------------------------------------------------------------------------

/// Helper: run runx with auto-detection and return (stdout, stderr, exit_code).
fn run_auto(bare: &Path, sha: &str, cache_dir: &Path, target: Option<&str>) -> (String, String, i32) {
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(bare, cache_dir, &canonical);

    let mut args = vec![
        "--yes".to_string(),
        format!("--cache-dir={}", cache_dir.display()),
        format!("--commit={sha}"),
        format!("file://{}", bare.display()),
    ];
    if let Some(t) = target {
        args.push(t.to_string());
    }

    let out = Command::new(runx_bin())
        .args(&args)
        .output()
        .unwrap();

    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

// --- Shell auto-detection ---

#[test]
fn auto_detect_shell_sh() {
    let (_td, bare, sha) = make_local_repo(&[("deploy.sh", "#!/bin/sh\necho auto-sh\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("deploy.sh"));
    assert_eq!(code, 0);
    assert!(stdout.contains("auto-sh"), "expected 'auto-sh', got: {stdout}");
}

#[test]
fn auto_detect_shell_bash_ext() {
    let (_td, bare, sha) = make_local_repo(&[("setup.bash", "#!/bin/bash\necho auto-bash\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("setup.bash"));
    assert_eq!(code, 0);
    assert!(stdout.contains("auto-bash"), "expected 'auto-bash', got: {stdout}");
}

#[test]
fn auto_detect_shell_zsh_ext() {
    // .zsh extension should auto-detect as shell mode.
    // We run it via sh since zsh may not be installed.
    let (_td, bare, sha) = make_local_repo(&[("init.zsh", "echo auto-zsh\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("init.zsh"));
    assert_eq!(code, 0);
    assert!(stdout.contains("auto-zsh"), "expected 'auto-zsh', got: {stdout}");
}

#[test]
fn auto_detect_shell_scripts_prefix() {
    // Anything under scripts/ should auto-detect as shell.
    let (_td, bare, sha) = make_local_repo(&[(
        "scripts/bootstrap",
        "#!/bin/sh\necho scripts-prefix\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("scripts/bootstrap"));
    assert_eq!(code, 0);
    assert!(stdout.contains("scripts-prefix"), "expected 'scripts-prefix', got: {stdout}");
}

#[test]
fn auto_detect_shell_uppercase_ext() {
    // Case-insensitive: .SH should also detect as shell.
    let (_td, bare, sha) = make_local_repo(&[("RUN.SH", "#!/bin/sh\necho upper-sh\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("RUN.SH"));
    assert_eq!(code, 0);
    assert!(stdout.contains("upper-sh"), "expected 'upper-sh', got: {stdout}");
}

// --- Make auto-detection ---

#[test]
fn auto_detect_make_bare_word() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: build\nbuild:\n\t@echo auto-make\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("build"));
    assert_eq!(code, 0);
    assert!(stdout.contains("auto-make"), "expected 'auto-make', got: {stdout}");
}

#[test]
fn auto_detect_make_no_target() {
    // No target at all → make mode with default target.
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: all\nall:\n\t@echo default-make\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), None);
    assert_eq!(code, 0);
    assert!(stdout.contains("default-make"), "expected 'default-make', got: {stdout}");
}

#[test]
fn auto_detect_make_hyphenated_target() {
    // Hyphenated target names should be treated as make targets, not shell.
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: run-tests\nrun-tests:\n\t@echo hyphen-make\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("run-tests"));
    assert_eq!(code, 0);
    assert!(stdout.contains("hyphen-make"), "expected 'hyphen-make', got: {stdout}");
}

#[test]
fn auto_detect_make_with_passthrough() {
    // Make target with passthrough args after --.
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

// --- Explicit subcommand overrides auto-detection ---

#[test]
fn explicit_sh_overrides_make_detection() {
    // A bare word like "run" would auto-detect as make, but `runx sh` forces shell mode.
    let (_td, bare, sha) = make_local_repo(&[("run", "#!/bin/sh\necho forced-shell\n")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let out = Command::new(runx_bin())
        .args([
            "sh",
            "--yes",
            &format!("--cache-dir={}", cache.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
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
fn explicit_make_overrides_shell_detection() {
    // "deploy.sh" would auto-detect as shell, but `runx make` forces make mode.
    // The make target is literally named "deploy.sh".
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: deploy.sh\ndeploy.sh:\n\t@echo forced-make\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let out = Command::new(runx_bin())
        .args([
            "make",
            "--yes",
            &format!("--cache-dir={}", cache.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
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

// --- Exit code propagation through auto-detect ---

#[test]
fn auto_detect_shell_exit_code() {
    let (_td, bare, sha) = make_local_repo(&[("fail.sh", "#!/bin/sh\nexit 13\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (_, _, code) = run_auto(&bare, &sha, cache.path(), Some("fail.sh"));
    assert_eq!(code, 13, "expected exit code 13, got {code}");
}

#[test]
fn auto_detect_make_exit_code() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: boom\nboom:\n\t@exit 42\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (_, _, code) = run_auto(&bare, &sha, cache.path(), Some("boom"));
    // make wraps non-zero exits as exit code 2
    assert_ne!(code, 0, "expected non-zero exit code");
}
