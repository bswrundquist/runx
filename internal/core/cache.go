package core

import (
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"
)

// Cache manages the on-disk cache layout.
//
// Layout:
//
//	<cacheDir>/
//	  repos/<urlhash>/      bare git repos
//	  trees/<urlhash>/<sha>/  immutable materialized checkouts
//	  trust.json            trusted repo registry
type Cache struct {
	Root string
}

// NewCache creates a Cache rooted at dir.
func NewCache(dir string) *Cache {
	return &Cache{Root: dir}
}

// URLHash returns a short, stable hash of the canonical URL for directory names.
// Uses the first 8 bytes of SHA-256 (16 hex characters).
func URLHash(canonical string) string {
	h := sha256.Sum256([]byte(canonical))
	return fmt.Sprintf("%x", h[:8])
}

// BareRepoDir returns the path for the bare git repository of a canonical URL.
func (c *Cache) BareRepoDir(canonical string) string {
	return filepath.Join(c.Root, "repos", URLHash(canonical))
}

// TreeDir returns the path for the immutable materialized tree of a specific commit.
func (c *Cache) TreeDir(canonical, commit string) string {
	return filepath.Join(c.Root, "trees", URLHash(canonical), commit)
}

// TrustFile returns the path to the trust store JSON file.
func (c *Cache) TrustFile() string {
	return filepath.Join(c.Root, "trust.json")
}

// EnsureRoots creates the top-level cache directories.
func (c *Cache) EnsureRoots() error {
	for _, sub := range []string{"repos", "trees"} {
		if err := os.MkdirAll(filepath.Join(c.Root, sub), 0o755); err != nil {
			return err
		}
	}
	return nil
}

// TreeExists reports whether the immutable tree for commit has been materialized.
func (c *Cache) TreeExists(canonical, commit string) bool {
	return dirExists(c.TreeDir(canonical, commit))
}

// dirExists reports whether path is an existing directory.
func dirExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}
