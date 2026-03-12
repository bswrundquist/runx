package core

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCacheLayout(t *testing.T) {
	root := t.TempDir()
	c := NewCache(root)

	const canonical = "https://github.com/owner/repo"
	const commit = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"

	// EnsureRoots creates repos/ and trees/ subdirectories.
	if err := c.EnsureRoots(); err != nil {
		t.Fatalf("EnsureRoots: %v", err)
	}
	for _, sub := range []string{"repos", "trees"} {
		if _, err := os.Stat(filepath.Join(root, sub)); err != nil {
			t.Errorf("missing subdir %q after EnsureRoots: %v", sub, err)
		}
	}

	// BareRepoDir uses URLHash.
	bareDir := c.BareRepoDir(canonical)
	if bareDir == "" {
		t.Error("BareRepoDir returned empty string")
	}
	if !filepath.IsAbs(bareDir) {
		t.Errorf("BareRepoDir not absolute: %q", bareDir)
	}

	// Two calls for same URL produce same path.
	if c.BareRepoDir(canonical) != bareDir {
		t.Error("BareRepoDir not deterministic")
	}

	// Different URLs produce different paths.
	other := c.BareRepoDir("https://github.com/owner/other")
	if other == bareDir {
		t.Error("BareRepoDir collision between different URLs")
	}

	// TreeDir includes commit SHA.
	treeDir := c.TreeDir(canonical, commit)
	if treeDir == "" {
		t.Error("TreeDir returned empty string")
	}
	if filepath.Base(treeDir) != commit {
		t.Errorf("TreeDir base = %q, want commit SHA %q", filepath.Base(treeDir), commit)
	}

	// TreeExists false before materialization.
	if c.TreeExists(canonical, commit) {
		t.Error("TreeExists returned true before materialization")
	}

	// TreeExists true after creating directory.
	if err := os.MkdirAll(treeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if !c.TreeExists(canonical, commit) {
		t.Error("TreeExists returned false after creating tree dir")
	}

	// TrustFile is within the cache root.
	trustFile := c.TrustFile()
	if filepath.Dir(trustFile) != root {
		t.Errorf("TrustFile not in cache root: %q", trustFile)
	}
}

func TestCacheHitMiss(t *testing.T) {
	root := t.TempDir()
	c := NewCache(root)
	_ = c.EnsureRoots()

	const canonical = "https://github.com/owner/repo"

	bareDir, sha := makeLocalRepo(t, map[string]string{
		"file.txt": "content\n",
	})

	// Cache miss: tree does not exist yet.
	if c.TreeExists(canonical, sha) {
		t.Fatal("expected cache miss before archive")
	}

	// Materialize tree.
	treeDir := c.TreeDir(canonical, sha)
	g := &Git{}
	if err := g.Archive(bareDir, sha, treeDir); err != nil {
		t.Fatalf("Archive: %v", err)
	}

	// Cache hit: tree now exists.
	if !c.TreeExists(canonical, sha) {
		t.Error("expected cache hit after archive")
	}
}
