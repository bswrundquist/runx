use crate::core::RunxError;
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Default GitHub repository for self-update.
/// Override at runtime with the RUNX_REPO environment variable.
const DEFAULT_REPO: &str = "bswrundquist/runx";

/// Current version from Cargo.toml.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check for updates and optionally install the latest release.
pub fn run(check: bool, force: bool, verbose: bool) -> i32 {
    let repo = std::env::var("RUNX_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let (owner, repo_name) = match repo.split_once('/') {
        Some(pair) => pair,
        None => {
            eprintln!("runx update: invalid RUNX_REPO: {repo:?} (expected owner/repo)");
            return 2;
        }
    };

    if verbose {
        eprintln!("runx update: current version v{CURRENT_VERSION}");
        eprintln!("runx update: checking {owner}/{repo_name} for latest release");
    }

    let json = match fetch_latest_release(owner, repo_name) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("runx: {e}");
            return 1;
        }
    };

    let release: serde_json::Value = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("runx: update: failed to parse release metadata: {e}");
            return 1;
        }
    };

    let tag = match release["tag_name"].as_str() {
        Some(t) => t,
        None => {
            eprintln!("runx: update: no releases found for {owner}/{repo_name}");
            return 1;
        }
    };

    let latest_version = tag.strip_prefix('v').unwrap_or(tag);

    let up_to_date = compare_versions(CURRENT_VERSION, latest_version) != Ordering::Less;

    if up_to_date && !force {
        eprintln!("runx: already up to date (v{CURRENT_VERSION})");
        return 0;
    }

    if check {
        if !up_to_date {
            eprintln!("runx: update available: v{CURRENT_VERSION} -> v{latest_version}");
            eprintln!("runx: run 'runx update' to install");
        } else {
            eprintln!("runx: already up to date (v{CURRENT_VERSION})");
        }
        return 0;
    }

    if up_to_date && force {
        eprintln!("runx: reinstalling v{CURRENT_VERSION}");
    } else {
        eprintln!("runx: updating v{CURRENT_VERSION} -> v{latest_version}");
    }

    let asset_base = match platform_asset_name() {
        Some(n) => n,
        None => {
            eprintln!(
                "runx: update: unsupported platform ({}/{})",
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            return 1;
        }
    };

    if verbose {
        eprintln!("runx update: looking for asset matching {asset_base:?}");
    }

    let (download_url, actual_asset) = match find_asset_url(&release, &asset_base) {
        Some(pair) => pair,
        None => {
            eprintln!("runx: update: no matching asset for this platform in release {tag}");
            if let Some(assets) = release["assets"].as_array() {
                eprintln!("runx: update: available assets:");
                for a in assets {
                    if let Some(name) = a["name"].as_str() {
                        eprintln!("  {name}");
                    }
                }
            }
            return 1;
        }
    };

    if verbose {
        eprintln!("runx update: downloading {actual_asset}");
    }

    let current_exe = match std::env::current_exe().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("runx: update: cannot determine binary path: {e}");
            return 1;
        }
    };

    let tmp_path = current_exe.with_extension("update-tmp");

    if let Err(e) = download_binary(&download_url, &actual_asset, &tmp_path) {
        eprintln!("runx: {e}");
        let _ = fs::remove_file(&tmp_path);
        return 1;
    }

    if let Err(e) = crate::core::make_executable(&tmp_path) {
        eprintln!("runx: {e}");
        let _ = fs::remove_file(&tmp_path);
        return 1;
    }

    // Verify the downloaded binary is functional before replacing.
    match Command::new(&tmp_path).args(["--version"]).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if !stdout.contains("runx") {
                let _ = fs::remove_file(&tmp_path);
                eprintln!("runx: update: downloaded binary failed verification (unexpected --version output)");
                return 1;
            }
        }
        _ => {
            let _ = fs::remove_file(&tmp_path);
            eprintln!("runx: update: downloaded binary failed verification (cannot execute)");
            return 1;
        }
    }

    if let Err(e) = fs::rename(&tmp_path, &current_exe) {
        let _ = fs::remove_file(&tmp_path);
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            eprintln!("runx: update: permission denied replacing {}", current_exe.display());
            eprintln!("runx: try: sudo runx update");
        } else {
            eprintln!("runx: update: failed to replace binary: {e}");
        }
        return 1;
    }

    eprintln!("runx: updated to v{latest_version}");
    0
}

/// Build the expected asset name for the current OS and architecture.
fn platform_asset_name() -> Option<String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        _ => return None,
    };
    Some(format!("runx-{os}-{arch}"))
}

/// Search release assets for one matching the platform asset name.
/// Tries exact, then .tar.gz, then .tgz, then .zip.
/// Returns (download_url, actual_asset_name).
fn find_asset_url(release: &serde_json::Value, base_name: &str) -> Option<(String, String)> {
    let assets = release["assets"].as_array()?;
    let candidates = [
        base_name.to_string(),
        format!("{base_name}.tar.gz"),
        format!("{base_name}.tgz"),
        format!("{base_name}.zip"),
    ];
    for candidate in &candidates {
        if let Some(asset) = assets.iter().find(|a| a["name"].as_str() == Some(candidate)) {
            let url = asset["browser_download_url"].as_str()?;
            return Some((url.to_string(), candidate.clone()));
        }
    }
    None
}

/// Simple numeric version comparison for x.y.z strings.
fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.').filter_map(|p| p.parse::<u64>().ok()).collect()
    };
    parse(a).cmp(&parse(b))
}

// ---------------------------------------------------------------------------
// GitHub API
// ---------------------------------------------------------------------------

fn fetch_latest_release(owner: &str, repo: &str) -> Result<String, RunxError> {
    try_gh_latest(owner, repo).or_else(|_| try_curl_latest(owner, repo))
}

fn try_gh_latest(owner: &str, repo: &str) -> Result<String, RunxError> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{owner}/{repo}/releases/latest")])
        .output()
        .map_err(|e| RunxError::Update(format!("gh: {e}")))?;
    if !output.status.success() {
        return Err(RunxError::Update("gh api failed".to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn try_curl_latest(owner: &str, repo: &str) -> Result<String, RunxError> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let output = Command::new("curl")
        .args(["-sfL", "-H", "Accept: application/vnd.github+json", &url])
        .output()
        .map_err(|e| RunxError::Update(format!("curl: {e}")))?;
    if !output.status.success() {
        return Err(RunxError::Update(
            "no releases found or GitHub API request failed".to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// Download helpers
// ---------------------------------------------------------------------------

/// Download a binary asset, handling bare binaries and archives.
fn download_binary(url: &str, asset_name: &str, dest: &Path) -> Result<(), RunxError> {
    if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".tgz") {
        let tmp_dir = tempfile::tempdir().map_err(RunxError::Io)?;
        download_and_extract_tar(url, tmp_dir.path())?;
        let bin = find_binary_in_dir(tmp_dir.path(), "runx")?;
        fs::copy(&bin, dest)?;
    } else if asset_name.ends_with(".zip") {
        let tmp_dir = tempfile::tempdir().map_err(RunxError::Io)?;
        download_and_extract_zip(url, tmp_dir.path())?;
        let bin = find_binary_in_dir(tmp_dir.path(), "runx")?;
        fs::copy(&bin, dest)?;
    } else {
        download_file(url, dest)?;
    }
    Ok(())
}

/// Find a binary by name in a directory, checking top level and one level deep.
fn find_binary_in_dir(dir: &Path, name: &str) -> Result<PathBuf, RunxError> {
    let direct = dir.join(name);
    if direct.is_file() {
        return Ok(direct);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            let nested = entry.path().join(name);
            if nested.is_file() {
                return Ok(nested);
            }
        }
    }
    Err(RunxError::Update(format!(
        "cannot find {name:?} binary in downloaded archive"
    )))
}

fn download_and_extract_tar(url: &str, dest: &Path) -> Result<(), RunxError> {
    let mut curl = Command::new("curl")
        .args(["-sfL", url])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| RunxError::Update(format!("curl: {e}")))?;

    let tar = Command::new("tar")
        .args(["-xzf", "-", "-C", &dest.to_string_lossy()])
        .stdin(curl.stdout.take().unwrap())
        .output()
        .map_err(|e| RunxError::Update(format!("tar: {e}")))?;

    let curl_status = curl
        .wait()
        .map_err(|e| RunxError::Update(format!("curl: {e}")))?;
    if !curl_status.success() {
        return Err(RunxError::Update(format!("download failed: {url}")));
    }
    if !tar.status.success() {
        return Err(RunxError::Update("tar extraction failed".to_string()));
    }
    Ok(())
}

fn download_and_extract_zip(url: &str, dest: &Path) -> Result<(), RunxError> {
    let tmp = dest.join("_download.zip");
    download_file(url, &tmp)?;
    let output = Command::new("unzip")
        .args(["-o", &tmp.to_string_lossy(), "-d", &dest.to_string_lossy()])
        .output()
        .map_err(|e| RunxError::Update(format!("unzip: {e}")))?;
    let _ = fs::remove_file(&tmp);
    if !output.status.success() {
        return Err(RunxError::Update("unzip failed".to_string()));
    }
    Ok(())
}

fn download_file(url: &str, dest: &Path) -> Result<(), RunxError> {
    let output = Command::new("curl")
        .args(["-sfL", "-o", &dest.to_string_lossy(), url])
        .output()
        .map_err(|e| RunxError::Update(format!("curl: {e}")))?;
    if !output.status.success() {
        return Err(RunxError::Update(format!("download failed: {url}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert_eq!(compare_versions("0.1.0", "0.1.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.0", "0.2.0"), Ordering::Less);
        assert_eq!(compare_versions("0.2.0", "0.1.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "0.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.0", "0.1.1"), Ordering::Less);
        assert_eq!(compare_versions("1.10.0", "1.9.0"), Ordering::Greater);
    }

    #[test]
    fn platform_asset_name_returns_value() {
        // Should return Some on macOS/Linux x86_64/aarch64.
        let name = platform_asset_name();
        if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
            assert!(name.is_some());
            let name = name.unwrap();
            assert!(name.starts_with("runx-"));
        }
    }

    #[test]
    fn find_asset_url_exact_match() {
        let release = serde_json::json!({
            "assets": [
                {
                    "name": "runx-darwin-arm64",
                    "browser_download_url": "https://example.com/runx-darwin-arm64"
                }
            ]
        });
        let result = find_asset_url(&release, "runx-darwin-arm64");
        assert!(result.is_some());
        let (url, name) = result.unwrap();
        assert_eq!(url, "https://example.com/runx-darwin-arm64");
        assert_eq!(name, "runx-darwin-arm64");
    }

    #[test]
    fn find_asset_url_tar_gz_fallback() {
        let release = serde_json::json!({
            "assets": [
                {
                    "name": "runx-linux-amd64.tar.gz",
                    "browser_download_url": "https://example.com/runx-linux-amd64.tar.gz"
                }
            ]
        });
        let result = find_asset_url(&release, "runx-linux-amd64");
        assert!(result.is_some());
        let (_, name) = result.unwrap();
        assert_eq!(name, "runx-linux-amd64.tar.gz");
    }

    #[test]
    fn find_asset_url_no_match() {
        let release = serde_json::json!({
            "assets": [
                {
                    "name": "runx-windows-amd64.exe",
                    "browser_download_url": "https://example.com/runx-windows-amd64.exe"
                }
            ]
        });
        assert!(find_asset_url(&release, "runx-darwin-arm64").is_none());
    }
}
