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
// Auto-detection tests (via pattern matching)
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
    let (_td, bare, sha) = make_local_repo(&[("init.zsh", "echo auto-zsh\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("init.zsh"));
    assert_eq!(code, 0);
    assert!(stdout.contains("auto-zsh"), "expected 'auto-zsh', got: {stdout}");
}

#[test]
fn auto_detect_shell_scripts_prefix() {
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
    let (_td, bare, sha) = make_local_repo(&[("RUN.SH", "#!/bin/sh\necho upper-sh\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (stdout, _, code) = run_auto(&bare, &sha, cache.path(), Some("RUN.SH"));
    assert_eq!(code, 0);
    assert!(stdout.contains("upper-sh"), "expected 'upper-sh', got: {stdout}");
}

// --- No-match: bare words and absent target produce error ---

#[test]
fn no_match_bare_word_errors() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: build\nbuild:\n\t@echo auto-make\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (_, stderr, code) = run_auto(&bare, &sha, cache.path(), Some("build"));
    assert_eq!(code, 2, "expected exit code 2, got {code}");
    assert!(stderr.contains("does not match"), "expected error message, got: {stderr}");
}

#[test]
fn no_match_no_target_errors() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: all\nall:\n\t@echo default-make\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (_, stderr, code) = run_auto(&bare, &sha, cache.path(), None);
    assert_eq!(code, 2, "expected exit code 2, got {code}");
    assert!(stderr.contains("no mode or target"), "expected error message, got: {stderr}");
}

#[test]
fn no_match_hyphenated_target_errors() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: run-tests\nrun-tests:\n\t@echo hyphen-make\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (_, stderr, code) = run_auto(&bare, &sha, cache.path(), Some("run-tests"));
    assert_eq!(code, 2, "expected exit code 2, got {code}");
    assert!(stderr.contains("does not match"), "expected error message, got: {stderr}");
}

// --- Explicit mode overrides auto-detection ---

#[test]
fn explicit_sh_overrides_detection() {
    // A bare word like "run" would not auto-detect, but `runx <repo> sh run` forces shell mode.
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
fn explicit_make_overrides_detection() {
    // "deploy.sh" would auto-detect as shell, but `runx <repo> make deploy.sh` forces make mode.
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

// --- Exit code propagation through auto-detect ---

#[test]
fn auto_detect_shell_exit_code() {
    let (_td, bare, sha) = make_local_repo(&[("fail.sh", "#!/bin/sh\nexit 13\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (_, _, code) = run_auto(&bare, &sha, cache.path(), Some("fail.sh"));
    assert_eq!(code, 13, "expected exit code 13, got {code}");
}

// ---------------------------------------------------------------------------
// Mode keyword aliases
// ---------------------------------------------------------------------------

#[test]
fn alias_shx() {
    let (_td, bare, sha) = make_local_repo(&[("hello.sh", "#!/bin/sh\necho alias-shx\n")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let mut args = base_args(cache.path(), &sha, &bare);
    args.push("shx".to_string());
    args.push("hello.sh".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("alias-shx"), "expected 'alias-shx', got: {stdout}");
}

#[test]
fn alias_makex() {
    let (_td, bare, sha) = make_local_repo(&[(
        "Makefile",
        ".PHONY: hi\nhi:\n\t@echo alias-makex\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let mut args = base_args(cache.path(), &sha, &bare);
    args.push("makex".to_string());
    args.push("hi".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("alias-makex"), "expected 'alias-makex', got: {stdout}");
}

#[test]
fn alias_dc() {
    // "dc" should be accepted as a compose alias (will fail to run without docker,
    // but we verify the mode is selected correctly by checking stderr for compose-related output).
    let (_td, bare, sha) = make_local_repo(&[(
        "compose.yml",
        "services:\n  app:\n    image: alpine\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let mut args = base_args(cache.path(), &sha, &bare);
    args.push("dc".to_string());
    args.push("--".to_string());
    args.push("config".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Should not error with "does not match" — mode was recognized.
    assert!(!stderr.contains("does not match"), "dc alias not recognized: {stderr}");
}

#[test]
fn alias_dcx() {
    let (_td, bare, sha) = make_local_repo(&[(
        "compose.yml",
        "services:\n  app:\n    image: alpine\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let mut args = base_args(cache.path(), &sha, &bare);
    args.push("dcx".to_string());
    args.push("--".to_string());
    args.push("config".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("does not match"), "dcx alias not recognized: {stderr}");
}

#[test]
fn alias_dockerx() {
    let (_td, bare, sha) = make_local_repo(&[("Dockerfile", "FROM alpine\n")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let mut args = base_args(cache.path(), &sha, &bare);
    args.push("dockerx".to_string());
    args.push("--".to_string());
    args.push("version".to_string());

    let out = Command::new(runx_bin()).args(&args).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("does not match"), "dockerx alias not recognized: {stderr}");
}

// ---------------------------------------------------------------------------
// Auto-detection: --verbose flag and compose/docker pattern detection
// ---------------------------------------------------------------------------

#[test]
fn auto_detect_verbose_flag() {
    let (_td, bare, sha) = make_local_repo(&[("deploy.sh", "#!/bin/sh\necho verbose-test\n")]);
    let cache = tempfile::tempdir().unwrap();
    let canonical = format!("file://{}", bare.display());
    prepare_repo_in_cache(&bare, cache.path(), &canonical);

    let out = Command::new(runx_bin())
        .args([
            "--verbose",
            "--yes",
            &format!("--cache-dir={}", cache.path().display()),
            &format!("--commit={sha}"),
            &format!("file://{}", bare.display()),
            "deploy.sh",
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("auto-detected mode: shell"),
        "expected verbose auto-detect message, got: {stderr}"
    );
}

#[test]
fn auto_detect_compose_pattern() {
    // compose.yml triggers compose mode. The tool itself may fail (no docker),
    // but we verify the mode was detected, not a "does not match" error.
    let (_td, bare, sha) = make_local_repo(&[(
        "compose.yml",
        "services:\n  app:\n    image: alpine\n",
    )]);
    let cache = tempfile::tempdir().unwrap();
    let (_, stderr, _) = run_auto(&bare, &sha, cache.path(), Some("compose.yml"));
    assert!(!stderr.contains("does not match"), "compose.yml not detected: {stderr}");
}

#[test]
fn auto_detect_dockerfile_pattern() {
    // Dockerfile triggers docker mode. The tool itself may fail (no docker),
    // but we verify the mode was detected, not a "does not match" error.
    let (_td, bare, sha) = make_local_repo(&[("Dockerfile", "FROM alpine\n")]);
    let cache = tempfile::tempdir().unwrap();
    let (_, stderr, _) = run_auto(&bare, &sha, cache.path(), Some("Dockerfile"));
    assert!(!stderr.contains("does not match"), "Dockerfile not detected: {stderr}");
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
// Error message content
// ---------------------------------------------------------------------------

#[test]
fn no_match_error_shows_hint() {
    let (_td, bare, sha) = make_local_repo(&[("dummy.txt", "x")]);
    let cache = tempfile::tempdir().unwrap();
    let (_, stderr, code) = run_auto(&bare, &sha, cache.path(), Some("something"));
    assert_eq!(code, 2);
    assert!(stderr.contains("explicit mode"), "expected hint about explicit mode, got: {stderr}");
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
