package core

import (
	"fmt"
	"os"
)

// Engine is the shared core that all runx tools use.
// It orchestrates ref resolution, caching, trust, and materialization.
type Engine struct {
	Flags *Flags
	cache *Cache
	git   *Git
	trust *TrustStore
}

// NewEngine constructs an Engine from flags.
func NewEngine(f *Flags) *Engine {
	return &Engine{
		Flags: f,
		cache: NewCache(f.CacheDir),
		git:   &Git{Verbose: f.Verbose},
		trust: NewTrustStore(NewCache(f.CacheDir).TrustFile()),
	}
}

// Prepare resolves ref, ensures the repo is cached, checks trust, materializes
// a checkout, and returns a ready Workspace.
//
// If mutable is true, the workspace is a writable temporary copy; the caller
// must call ws.Close() when done. Immutable workspaces need not be closed.
func (e *Engine) Prepare(ref RepoRef, mutable bool) (*Workspace, error) {
	// Override ref with --commit flag.
	if e.Flags.Commit != "" {
		ref = makeRepoRef(ref.CanonicalURL, e.Flags.Commit)
	}

	if err := e.cache.EnsureRoots(); err != nil {
		return nil, fmt.Errorf("cache setup: %w", err)
	}

	bareDir := e.cache.BareRepoDir(ref.CanonicalURL)

	// Clone or fetch.
	if err := e.git.CloneOrFetch(
		ref.CanonicalURL, bareDir,
		e.Flags.Offline, e.Flags.Refresh,
		ref.IsMutableRef(), ref.Ref,
	); err != nil {
		return nil, err
	}

	// Resolve ref → commit.
	commit, err := e.git.ResolveRef(bareDir, ref.Ref)
	if err != nil {
		return nil, err
	}

	// Announce the resolved commit (always visible; helps with reproducibility).
	short := commit
	if len(short) > 12 {
		short = short[:12]
	}
	fmt.Fprintf(os.Stderr, "runx: %s → %s\n", ref.DisplayRef(), short)

	// Print pinned command when requested.
	if e.Flags.Pin && ref.IsMutableRef() {
		fmt.Fprintf(os.Stderr, "runx: pin → %s@%s\n", ref.CanonicalURL, commit)
	}

	// Trust check (may prompt the user interactively).
	if err := e.trust.Check(ref, commit, e.Flags); err != nil {
		return nil, err
	}

	// Materialize immutable tree if not already cached.
	treeDir := e.cache.TreeDir(ref.CanonicalURL, commit)
	if !e.cache.TreeExists(ref.CanonicalURL, commit) {
		if e.Flags.Offline {
			return nil, fmt.Errorf("commit %s is not cached and --offline is set", short)
		}
		if err := e.git.Archive(bareDir, commit, treeDir); err != nil {
			// Clean up partial tree on failure.
			_ = os.RemoveAll(treeDir)
			return nil, fmt.Errorf("materializing tree: %w", err)
		}
	}

	if !mutable {
		return ImmutableWorkspace(treeDir, commit), nil
	}
	return MutableWorkspace(treeDir, commit)
}
