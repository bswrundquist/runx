package core

import (
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
)

// Workspace is a materialized checkout ready for command execution.
type Workspace struct {
	// Root is the absolute path to the checkout directory.
	Root string
	// Commit is the full 40-char SHA that was checked out.
	Commit string
	// Mutable indicates this is a temporary copy (not the immutable cache).
	Mutable bool

	cleanup func() error
}

// Close removes the workspace if it is mutable. No-op for immutable workspaces.
func (w *Workspace) Close() error {
	if w.cleanup != nil {
		return w.cleanup()
	}
	return nil
}

// ImmutableWorkspace wraps an already-materialized immutable tree.
// The caller must not write into the returned Root.
func ImmutableWorkspace(treeDir, commit string) *Workspace {
	return &Workspace{Root: treeDir, Commit: commit, Mutable: false}
}

// MutableWorkspace creates a temporary writable copy of treeDir.
// The caller must call Close() when done.
func MutableWorkspace(treeDir, commit string) (*Workspace, error) {
	tmp, err := os.MkdirTemp("", "runx-*")
	if err != nil {
		return nil, fmt.Errorf("creating workspace: %w", err)
	}
	if err := copyDir(treeDir, tmp); err != nil {
		_ = os.RemoveAll(tmp)
		return nil, fmt.Errorf("copying tree to workspace: %w", err)
	}
	return &Workspace{
		Root:    tmp,
		Commit:  commit,
		Mutable: true,
		cleanup: func() error { return os.RemoveAll(tmp) },
	}, nil
}

// copyDir recursively copies src into dst, preserving file modes and symlinks.
// dst must already exist.
func copyDir(src, dst string) error {
	return filepath.WalkDir(src, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(src, path)
		if err != nil {
			return err
		}
		target := filepath.Join(dst, rel)

		// Symlink: recreate without following.
		if d.Type()&fs.ModeSymlink != 0 {
			link, err := os.Readlink(path)
			if err != nil {
				return err
			}
			return os.Symlink(link, target)
		}

		// Directory: create.
		if d.IsDir() {
			if path == src {
				return nil // dst already exists
			}
			info, err := d.Info()
			if err != nil {
				return err
			}
			return os.Mkdir(target, info.Mode())
		}

		// Regular file: copy.
		return copyFile(path, target)
	})
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()

	info, err := in.Stat()
	if err != nil {
		return err
	}

	out, err := os.OpenFile(dst, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, info.Mode())
	if err != nil {
		return err
	}
	defer out.Close()

	_, err = io.Copy(out, in)
	return err
}
