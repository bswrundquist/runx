use crate::core::RunxError;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A materialized checkout ready for command execution.
pub struct Workspace {
    /// Absolute path to the checkout directory.
    pub root: PathBuf,
    /// Full 40-char SHA that was checked out.
    #[allow(dead_code)] // part of the public API; used in tests and by downstream callers
    pub commit: String,
    /// Whether this is a temporary copy (not the immutable cache).
    /// Used by callers to decide whether writes are safe.
    #[allow(dead_code)] // part of the public API; used in tests and by downstream callers
    pub mutable: bool,
    /// Set for mutable workspaces; the directory to remove on drop.
    temp_dir: Option<PathBuf>,
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if let Some(ref dir) = self.temp_dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

impl Workspace {
    /// Wrap an already-materialized immutable tree.
    /// The caller must not write into the returned root.
    pub fn immutable(tree_dir: PathBuf, commit: String) -> Self {
        Self {
            root: tree_dir,
            commit,
            mutable: false,
            temp_dir: None,
        }
    }

    /// Create a temporary writable copy of tree_dir.
    /// Cleanup happens automatically on drop.
    pub fn mutable_copy(tree_dir: &Path, commit: String) -> Result<Self, RunxError> {
        let tmp = tempfile::Builder::new()
            .prefix("runx-")
            .tempdir()
            .map_err(RunxError::Io)?;
        let tmp_path = tmp.keep(); // prevent auto-cleanup; we manage it via Drop
        copy_dir(tree_dir, &tmp_path)?;
        Ok(Self {
            root: tmp_path.clone(),
            commit,
            mutable: true,
            temp_dir: Some(tmp_path),
        })
    }
}

/// Recursively copy src into dst, preserving file modes and symlinks.
/// dst must already exist.
fn copy_dir(src: &Path, dst: &Path) -> Result<(), RunxError> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if file_type.is_symlink() {
            let link = fs::read_link(&src_path)?;
            std::os::unix::fs::symlink(&link, &dst_path)?;
        } else if file_type.is_dir() {
            let meta = fs::metadata(&src_path)?;
            fs::create_dir(&dst_path)?;
            fs::set_permissions(&dst_path, meta.permissions())?;
            copy_dir(&src_path, &dst_path)?;
        } else {
            copy_file(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn copy_file(src: &Path, dst: &Path) -> Result<(), RunxError> {
    let meta = fs::metadata(src)?;
    let mut reader = fs::File::open(src)?;
    let mut writer = fs::File::create(dst)?;
    io::copy(&mut reader, &mut writer)?;
    fs::set_permissions(dst, fs::Permissions::from_mode(meta.permissions().mode()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutable_workspace_isolation() {
        let immutable = tempfile::tempdir().unwrap();
        fs::write(immutable.path().join("file.txt"), "original\n").unwrap();

        let sha = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string();
        let ws = Workspace::mutable_copy(immutable.path(), sha.clone()).unwrap();
        assert!(ws.mutable);
        assert_eq!(ws.commit, sha);

        // Mutate workspace.
        fs::write(ws.root.join("file.txt"), "mutated\n").unwrap();

        // Original must be unchanged.
        let content = fs::read_to_string(immutable.path().join("file.txt")).unwrap();
        assert_eq!(content, "original\n");
    }

    #[test]
    fn mutable_workspace_cleanup() {
        let immutable = tempfile::tempdir().unwrap();
        fs::write(immutable.path().join("x"), "x").unwrap();

        let ws = Workspace::mutable_copy(immutable.path(), "sha".to_string()).unwrap();
        let root = ws.root.clone();
        assert!(root.exists());

        drop(ws);
        assert!(!root.exists());
    }

    #[test]
    fn immutable_workspace_no_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();
        let ws = Workspace::immutable(dir_path.clone(), "sha".to_string());
        assert!(!ws.mutable);

        drop(ws);
        assert!(dir_path.exists());
    }

    #[test]
    fn copy_dir_preserves_symlinks() {
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("real.txt"), "real\n").unwrap();
        std::os::unix::fs::symlink("real.txt", src.path().join("link.txt")).unwrap();

        let dst = tempfile::tempdir().unwrap();
        copy_dir(src.path(), dst.path()).unwrap();

        let info = fs::symlink_metadata(dst.path().join("link.txt")).unwrap();
        assert!(info.file_type().is_symlink());

        let content = fs::read_to_string(dst.path().join("link.txt")).unwrap();
        assert_eq!(content, "real\n");
    }
}
