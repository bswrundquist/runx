use crate::core::{Cache, Flags, RepoRef, RunxError, TrustStore};
use crate::core::host::{self, HostKind};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Download and run a release binary from a git host.
pub fn run(flags: Flags, repo_ref: RepoRef, asset_name: &str, args: &[String]) -> i32 {
    if repo_ref.ref_name.is_empty() {
        eprintln!("runx: release mode requires a tag/ref, e.g. owner/repo@v1.0");
        return 2;
    }

    // Reject names that could escape the cache directory.
    if asset_name.contains("..") || asset_name.contains('/') || asset_name.contains('\\') {
        eprintln!("runx: invalid asset name: {asset_name:?}");
        return 2;
    }
    if repo_ref.ref_name.contains("..") {
        eprintln!("runx: invalid ref: {:?}", repo_ref.ref_name);
        return 2;
    }

    let cache = Cache::new(flags.cache_dir.clone());
    let release_dir = cache.release_dir(
        &repo_ref.canonical_url,
        &repo_ref.ref_name,
        asset_name,
    );

    // Trust check.
    let trust = TrustStore::new(cache.trust_file());
    if let Err(e) = trust.check(&repo_ref, &repo_ref.ref_name, &flags) {
        eprintln!("runx: {e}");
        return 1;
    }

    // Check cache first.
    if !release_dir.exists() || flags.refresh {
        if flags.offline {
            eprintln!("runx: release not cached and --offline is set");
            return 1;
        }

        if let Err(e) = cache.ensure_roots() {
            eprintln!("runx: {e}");
            return 1;
        }

        if let Err(e) = download_release(&repo_ref, asset_name, &release_dir, &flags) {
            eprintln!("runx: {e}");
            return 1;
        }
    }

    // Find the executable in the release dir.
    let executable = match find_executable(&release_dir, asset_name) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("runx: {e}");
            return 1;
        }
    };

    if let Err(e) = crate::core::make_executable(&executable) {
        eprintln!("runx: {e}");
        return 1;
    }

    let mut cmd = Command::new(&executable);
    cmd.args(args);

    match crate::core::run_child(&mut cmd) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("runx: {e}");
            1
        }
    }
}

/// Download a release asset from a git host (GitHub, GitLab, or generic).
fn download_release(
    repo_ref: &RepoRef,
    asset_name: &str,
    release_dir: &Path,
    flags: &Flags,
) -> Result<(), RunxError> {
    let (host_name, path) = parse_host_and_path(&repo_ref.canonical_url)?;
    let kind = host::detect(&host_name);
    let tag = &repo_ref.ref_name;

    let download_url = match kind {
        HostKind::GitHub | HostKind::GitLab => {
            let api_url = host::release_api_url(kind, &host_name, &path, tag)
                .expect("GitHub/GitLab always returns Some");
            let json = fetch_release_json(&api_url, kind)?;
            let release: serde_json::Value = serde_json::from_str(&json)
                .map_err(|e| RunxError::Release(format!("parsing release JSON: {e}")))?;
            host::extract_asset_url(kind, &release, asset_name).ok_or_else(|| {
                RunxError::Release(format!("asset {asset_name:?} not found in release {tag}"))
            })?
        }
        HostKind::Generic => {
            host::direct_download_url(&repo_ref.canonical_url, tag, asset_name)
        }
    };

    if flags.verbose {
        eprintln!("runx: downloading {download_url}");
    }

    fs::create_dir_all(release_dir)?;

    if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".tgz") {
        download_and_extract_tar(&download_url, release_dir)?;
    } else if asset_name.ends_with(".zip") {
        download_and_extract_zip(&download_url, release_dir)?;
    } else {
        // Bare binary.
        download_file(&download_url, &release_dir.join(asset_name))?;
    }

    Ok(())
}

/// Extract (host, path) from a canonical URL like "https://github.com/owner/repo".
fn parse_host_and_path(canonical_url: &str) -> Result<(String, String), RunxError> {
    let rest = canonical_url
        .strip_prefix("https://")
        .ok_or_else(|| RunxError::Release(format!("cannot parse host from {canonical_url}")))?;

    let (host, path) = rest.split_once('/').ok_or_else(|| {
        RunxError::Release(format!("cannot parse host from {canonical_url}"))
    })?;

    if host.is_empty() || path.is_empty() {
        return Err(RunxError::Release(format!(
            "cannot parse host/path from {canonical_url}"
        )));
    }

    Ok((host.to_string(), path.to_string()))
}

/// Fetch release metadata JSON from a host API using curl.
fn fetch_release_json(api_url: &str, kind: HostKind) -> Result<String, RunxError> {
    let headers = host::release_headers(kind);
    let mut args = vec!["-sfL".to_string()];
    for h in &headers {
        args.push("-H".to_string());
        args.push(h.clone());
    }
    args.push(api_url.to_string());

    let output = Command::new("curl")
        .args(&args)
        .output()
        .map_err(|e| RunxError::Release(format!("curl: {e}")))?;
    if !output.status.success() {
        return Err(RunxError::Release(format!(
            "release API request failed for {api_url}"
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn download_and_extract_tar(url: &str, dest: &Path) -> Result<(), RunxError> {
    let mut curl = Command::new("curl")
        .args(["-sfL", url])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| RunxError::Release(format!("curl: {e}")))?;

    let tar = Command::new("tar")
        .args(["-xzf", "-", "-C", &dest.to_string_lossy()])
        .stdin(curl.stdout.take().unwrap())
        .output()
        .map_err(|e| RunxError::Release(format!("tar: {e}")))?;

    // Reap curl to avoid zombie process.
    let curl_status = curl.wait()
        .map_err(|e| RunxError::Release(format!("curl: {e}")))?;
    if !curl_status.success() {
        return Err(RunxError::Release(format!("download failed: {url}")));
    }

    if !tar.status.success() {
        return Err(RunxError::Release("tar extraction failed".to_string()));
    }
    Ok(())
}

fn download_and_extract_zip(url: &str, dest: &Path) -> Result<(), RunxError> {
    let tmp = dest.join("_download.zip");
    download_file(url, &tmp)?;

    let output = Command::new("unzip")
        .args(["-o", &tmp.to_string_lossy(), "-d", &dest.to_string_lossy()])
        .output()
        .map_err(|e| RunxError::Release(format!("unzip: {e}")))?;

    let _ = fs::remove_file(&tmp);

    if !output.status.success() {
        return Err(RunxError::Release("unzip failed".to_string()));
    }
    Ok(())
}

fn download_file(url: &str, dest: &Path) -> Result<(), RunxError> {
    let output = Command::new("curl")
        .args(["-sfL", "-o", &dest.to_string_lossy(), url])
        .output()
        .map_err(|e| RunxError::Release(format!("curl: {e}")))?;
    if !output.status.success() {
        return Err(RunxError::Release(format!("download failed: {url}")));
    }
    Ok(())
}

/// Find the executable binary inside a release directory.
/// For archives, look for a single binary or one matching the asset name stem.
/// For bare binaries, the file is named after the asset.
fn find_executable(release_dir: &Path, asset_name: &str) -> Result<std::path::PathBuf, RunxError> {
    use std::os::unix::fs::PermissionsExt;

    // Strip archive extension to get the stem.
    let stem = asset_name
        .strip_suffix(".tar.gz")
        .or_else(|| asset_name.strip_suffix(".tgz"))
        .or_else(|| asset_name.strip_suffix(".zip"))
        .unwrap_or(asset_name);

    // Direct match: bare binary named exactly as the asset.
    let direct = release_dir.join(asset_name);
    if direct.is_file() {
        return Ok(direct);
    }

    // Match by stem name.
    let stem_path = release_dir.join(stem);
    if stem_path.is_file() {
        return Ok(stem_path);
    }

    // Check one level deep (archives often have a top-level dir like bin/).
    for entry in fs::read_dir(release_dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            let nested_stem = entry.path().join(stem);
            if nested_stem.is_file() {
                return Ok(nested_stem);
            }
            // Also try without the version/platform suffix — many tools
            // name the binary simply (e.g. "glab" inside "glab_1.0_os_arch/bin/").
            // Look for an extensionless executable in the subdirectory.
            if let Ok(sub_entries) = fs::read_dir(entry.path()) {
                for se in sub_entries.filter_map(|e| e.ok()) {
                    if !se.path().is_file() {
                        continue;
                    }
                    let name = se.file_name();
                    let name = name.to_string_lossy();
                    if !name.contains('.') && !name.starts_with('_') && !name.starts_with('.') {
                        if let Ok(meta) = se.metadata() {
                            let mode = meta.permissions().mode();
                            if mode & 0o111 != 0 {
                                return Ok(se.path());
                            }
                        }
                    }
                }
            }
        }
    }

    // Collect all regular files in the directory.
    let entries: Vec<_> = fs::read_dir(release_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();

    // Single file in the directory — must be the binary.
    if entries.len() == 1 {
        return Ok(entries[0].path());
    }

    // Prefer files that have the executable bit set and no extension
    // (skip non-executables like README, LICENSE, etc).
    let is_likely_binary = |entry: &fs::DirEntry| -> bool {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('_') || name.starts_with('.') {
            return false;
        }
        // Check executable bit.
        if let Ok(meta) = entry.metadata() {
            let mode = meta.permissions().mode();
            if mode & 0o111 != 0 && !name.contains('.') {
                return true;
            }
        }
        false
    };

    let candidates: Vec<_> = entries.iter().filter(|e| is_likely_binary(e)).collect();
    if candidates.len() == 1 {
        return Ok(candidates[0].path());
    }

    // Fallback: extensionless file without executable bit (tar may not preserve it).
    for entry in &entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.contains('.') && !name.starts_with('_') && !name.starts_with('.') {
            // Skip well-known non-binary files.
            let lower = name.to_lowercase();
            if matches!(
                lower.as_str(),
                "readme" | "license" | "licence" | "changelog" | "authors" | "notice" | "makefile"
            ) {
                continue;
            }
            return Ok(entry.path());
        }
    }

    Err(RunxError::Release(format!(
        "cannot find executable in release directory: {}",
        release_dir.display()
    )))
}
