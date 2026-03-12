package core

import (
	"flag"
	"os"
	"path/filepath"
)

// Flags holds the shared CLI flags for all runx tools.
type Flags struct {
	Refresh  bool   // re-fetch even if a cached ref exists
	Offline  bool   // do not access the network
	CacheDir string // cache root directory
	Yes      bool   // skip trust prompt
	Trust    bool   // permanently trust this repo (implies Yes)
	Pin      bool   // print pinned-commit command after mutable-ref resolution
	Verbose  bool   // print verbose output
	Commit   string // override ref resolution with this commit SHA
}

// DefaultCacheDir returns the platform default cache directory.
// Overridable via the RUNX_CACHE_DIR environment variable.
func DefaultCacheDir() string {
	if d := os.Getenv("RUNX_CACHE_DIR"); d != "" {
		return d
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return filepath.Join(os.TempDir(), "runx-cache")
	}
	return filepath.Join(home, ".cache", "runx")
}

// AddFlags registers the shared flags on fs and stores values into f.
func AddFlags(fs *flag.FlagSet, f *Flags) {
	fs.BoolVar(&f.Refresh, "refresh", false, "re-fetch from remote even if ref is cached")
	fs.BoolVar(&f.Offline, "offline", false, "disable network access; fail if ref is not cached")
	fs.StringVar(&f.CacheDir, "cache-dir", DefaultCacheDir(), "cache root directory")
	fs.BoolVar(&f.Yes, "yes", false, "skip interactive trust prompt")
	fs.BoolVar(&f.Trust, "trust", false, "permanently trust this repo (implies --yes)")
	fs.BoolVar(&f.Pin, "pin", false, "print pinned-commit command after resolving mutable refs")
	fs.BoolVar(&f.Verbose, "verbose", false, "print verbose output")
	fs.StringVar(&f.Commit, "commit", "", "override ref resolution with a specific commit SHA")
}

// SplitOnDash splits args on the first "--" separator, returning (before, after).
// If "--" is not present, returns (args, nil).
func SplitOnDash(args []string) (before, after []string) {
	for i, a := range args {
		if a == "--" {
			return args[:i], args[i+1:]
		}
	}
	return args, nil
}
