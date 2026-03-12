package core

import (
	"os"
	"path/filepath"
	"testing"
)

func TestGitResolveRef_fullSHA(t *testing.T) {
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"hello.txt": "hello world\n",
	})

	g := &Git{}
	got, err := g.ResolveRef(bareDir, sha)
	if err != nil {
		t.Fatalf("ResolveRef(full SHA): %v", err)
	}
	if got != sha {
		t.Errorf("got %q, want %q", got, sha)
	}
}

func TestGitResolveRef_branch(t *testing.T) {
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"hello.txt": "hello world\n",
	})

	g := &Git{}
	got, err := g.ResolveRef(bareDir, "main")
	if err != nil {
		t.Fatalf("ResolveRef(main): %v", err)
	}
	if got != sha {
		t.Errorf("ResolveRef(main) = %q, want %q", got, sha)
	}
}

func TestGitResolveRef_TTLCache(t *testing.T) {
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"hello.txt": "hello world\n",
	})

	g := &Git{}
	// First call: populates TTL cache.
	got1, err := g.ResolveRef(bareDir, "main")
	if err != nil {
		t.Fatalf("first ResolveRef: %v", err)
	}

	// Second call: should hit TTL cache (same result).
	got2, err := g.ResolveRef(bareDir, "main")
	if err != nil {
		t.Fatalf("second ResolveRef: %v", err)
	}
	if got1 != sha || got2 != sha {
		t.Errorf("ResolveRef inconsistent: %q, %q, want %q", got1, got2, sha)
	}
	// Verify cache file exists.
	if _, err := os.Stat(filepath.Join(bareDir, "runx-refs.json")); err != nil {
		t.Errorf("TTL cache file missing: %v", err)
	}
}

func TestGitArchive(t *testing.T) {
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"hello.txt":      "hello world\n",
		"scripts/run.sh": "#!/bin/sh\necho hi\n",
	})

	destDir := t.TempDir()
	g := &Git{}
	if err := g.Archive(bareDir, sha, destDir); err != nil {
		t.Fatalf("Archive: %v", err)
	}

	// Verify files were extracted.
	content, err := os.ReadFile(filepath.Join(destDir, "hello.txt"))
	if err != nil {
		t.Fatalf("reading hello.txt: %v", err)
	}
	if string(content) != "hello world\n" {
		t.Errorf("hello.txt content = %q, want %q", string(content), "hello world\n")
	}

	if _, err := os.Stat(filepath.Join(destDir, "scripts/run.sh")); err != nil {
		t.Errorf("scripts/run.sh missing: %v", err)
	}
}

func TestGitArchive_immutableCacheProtection(t *testing.T) {
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"data.txt": "original\n",
	})

	// Materialize immutable tree.
	immutableDir := t.TempDir()
	g := &Git{}
	if err := g.Archive(bareDir, sha, immutableDir); err != nil {
		t.Fatalf("Archive: %v", err)
	}

	// Create mutable workspace from immutable tree.
	ws, err := MutableWorkspace(immutableDir, sha)
	if err != nil {
		t.Fatalf("MutableWorkspace: %v", err)
	}
	defer ws.Close() //nolint:errcheck

	// Write to the mutable workspace.
	if err := os.WriteFile(filepath.Join(ws.Root, "data.txt"), []byte("mutated\n"), 0o644); err != nil {
		t.Fatalf("writing to workspace: %v", err)
	}

	// Verify immutable tree is untouched.
	content, err := os.ReadFile(filepath.Join(immutableDir, "data.txt"))
	if err != nil {
		t.Fatalf("reading immutable data.txt: %v", err)
	}
	if string(content) != "original\n" {
		t.Errorf("immutable cache was mutated! got %q, want %q", string(content), "original\n")
	}
}
