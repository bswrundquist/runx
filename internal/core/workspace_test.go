package core

import (
	"os"
	"path/filepath"
	"testing"
)

func TestMutableWorkspace_isolation(t *testing.T) {
	// Create immutable tree.
	immutableDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(immutableDir, "file.txt"), []byte("original\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	const sha = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
	ws, err := MutableWorkspace(immutableDir, sha)
	if err != nil {
		t.Fatalf("MutableWorkspace: %v", err)
	}
	defer ws.Close() //nolint:errcheck

	if !ws.Mutable {
		t.Error("expected Mutable = true")
	}
	if ws.Commit != sha {
		t.Errorf("Commit = %q, want %q", ws.Commit, sha)
	}

	// Mutate the workspace.
	if err := os.WriteFile(filepath.Join(ws.Root, "file.txt"), []byte("mutated\n"), 0o644); err != nil {
		t.Fatalf("writing to workspace: %v", err)
	}

	// Original immutable tree must be unchanged.
	content, err := os.ReadFile(filepath.Join(immutableDir, "file.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if string(content) != "original\n" {
		t.Errorf("immutable tree mutated: got %q", string(content))
	}
}

func TestMutableWorkspace_cleanup(t *testing.T) {
	immutableDir := t.TempDir()
	_ = os.WriteFile(filepath.Join(immutableDir, "x"), []byte("x"), 0o644)

	ws, err := MutableWorkspace(immutableDir, "sha")
	if err != nil {
		t.Fatal(err)
	}

	wsRoot := ws.Root
	if _, err := os.Stat(wsRoot); err != nil {
		t.Fatalf("workspace root missing before Close: %v", err)
	}

	if err := ws.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}

	if _, err := os.Stat(wsRoot); err == nil {
		t.Error("workspace root still exists after Close")
	}
}

func TestImmutableWorkspace_noCleanup(t *testing.T) {
	dir := t.TempDir()
	ws := ImmutableWorkspace(dir, "sha")
	if ws.Mutable {
		t.Error("expected Mutable = false")
	}
	// Close should not remove the directory.
	if err := ws.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
	if _, err := os.Stat(dir); err != nil {
		t.Error("immutable workspace removed on Close")
	}
}

func TestCopyDir_preservesSymlinks(t *testing.T) {
	src := t.TempDir()
	// Create a regular file.
	_ = os.WriteFile(filepath.Join(src, "real.txt"), []byte("real\n"), 0o644)
	// Create a symlink.
	_ = os.Symlink("real.txt", filepath.Join(src, "link.txt"))

	dst := t.TempDir()
	if err := copyDir(src, dst); err != nil {
		t.Fatalf("copyDir: %v", err)
	}

	// Symlink should be preserved (not dereferenced).
	info, err := os.Lstat(filepath.Join(dst, "link.txt"))
	if err != nil {
		t.Fatalf("Lstat link.txt: %v", err)
	}
	if info.Mode()&os.ModeSymlink == 0 {
		t.Error("link.txt is not a symlink in destination")
	}

	// Target should still be readable through the symlink.
	content, err := os.ReadFile(filepath.Join(dst, "link.txt"))
	if err != nil {
		t.Fatalf("reading through symlink: %v", err)
	}
	if string(content) != "real\n" {
		t.Errorf("symlink content = %q, want %q", string(content), "real\n")
	}
}
