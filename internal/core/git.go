package core

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

const (
	// refTTL is how long a cached ref→commit mapping is considered fresh.
	refTTL = 5 * time.Minute
)

// refCache is stored as JSON inside each bare repo directory.
type refCache struct {
	Refs map[string]refEntry `json:"refs"`
}

type refEntry struct {
	Commit     string    `json:"commit"`
	ResolvedAt time.Time `json:"resolved_at"`
}

// Git wraps the system git binary for repository operations.
type Git struct {
	Verbose bool
	Stderr  io.Writer // where to send git stderr; defaults to os.Stderr when Verbose
}

func (g *Git) stderr() io.Writer {
	if g.Stderr != nil {
		return g.Stderr
	}
	return os.Stderr
}

// CloneOrFetch ensures the bare repo cache is present and optionally up-to-date.
//
// Logic:
//   - Bare repo absent + offline → error
//   - Bare repo absent            → clone
//   - Bare repo present + offline → return (use cache as-is)
//   - Bare repo present + refresh → fetch
//   - Bare repo present + mutableRef AND TTL expired → fetch
//   - Bare repo present + immutable ref → skip fetch
func (g *Git) CloneOrFetch(canonical, bareDir string, offline, refresh bool, mutableRef bool, ref string) error {
	exists := dirExists(bareDir)
	if !exists {
		if offline {
			return fmt.Errorf("repo not in cache and --offline is set: %s", canonical)
		}
		return g.clone(canonical, bareDir)
	}
	if offline {
		return nil
	}
	if refresh {
		return g.fetch(bareDir)
	}
	if mutableRef {
		// Skip fetch if we resolved this ref recently.
		if _, fresh := g.cachedRef(bareDir, ref); fresh {
			if g.Verbose {
				fmt.Fprintf(g.stderr(), "runx: ref %q is fresh (TTL); skipping fetch\n", ref)
			}
			return nil
		}
		return g.fetch(bareDir)
	}
	// Immutable ref (full commit SHA): no fetch needed.
	return nil
}

func (g *Git) clone(url, bareDir string) error {
	if err := os.MkdirAll(filepath.Dir(bareDir), 0o755); err != nil {
		return err
	}
	args := []string{"clone", "--bare", "--quiet", url, bareDir}
	return g.run("", args...)
}

func (g *Git) fetch(bareDir string) error {
	return g.run(bareDir, "fetch", "--prune", "--quiet", "origin")
}

// ResolveRef resolves ref to a full 40-char commit SHA using the local bare repo.
// Uses a TTL-based cache to skip repeated git calls within refTTL.
func (g *Git) ResolveRef(bareDir, ref string) (string, error) {
	if ref == "" {
		ref = "HEAD"
	}
	// Full SHA: verify it exists locally then return immediately.
	if fullSHARe.MatchString(ref) {
		if err := g.run(bareDir, "cat-file", "-e", ref+"^{commit}"); err == nil {
			return ref, nil
		}
		// SHA not present locally (shallow clone or partial fetch).
		return ref, nil // trust the user; we'll get an archive error if wrong
	}

	// Check TTL cache.
	if commit, fresh := g.cachedRef(bareDir, ref); fresh {
		return commit, nil
	}

	// Peel to commit: resolves branches, tags, annotated tags, short SHAs.
	out, err := g.output(bareDir, "rev-parse", "--verify", ref+"^{commit}")
	if err != nil {
		return "", fmt.Errorf("cannot resolve ref %q in %s: %w", ref, bareDir, err)
	}
	commit := strings.TrimSpace(string(out))
	if !fullSHARe.MatchString(commit) {
		return "", fmt.Errorf("unexpected rev-parse output for ref %q: %q", ref, commit)
	}
	g.cacheRef(bareDir, ref, commit)
	return commit, nil
}

// Archive extracts the commit tree into destDir using git archive piped through tar.
func (g *Git) Archive(bareDir, commit, destDir string) error {
	if err := os.MkdirAll(destDir, 0o755); err != nil {
		return err
	}
	gitCmd := exec.Command("git", "--git-dir="+bareDir, "archive", commit)
	tarCmd := exec.Command("tar", "-C", destDir, "-xf", "-")
	if g.Verbose {
		gitCmd.Stderr = g.stderr()
		tarCmd.Stderr = g.stderr()
	}

	pipe, err := gitCmd.StdoutPipe()
	if err != nil {
		return err
	}
	tarCmd.Stdin = pipe

	if err := gitCmd.Start(); err != nil {
		return fmt.Errorf("git archive: %w", err)
	}
	if err := tarCmd.Start(); err != nil {
		_ = gitCmd.Wait()
		return fmt.Errorf("tar: %w", err)
	}
	if err := gitCmd.Wait(); err != nil {
		_ = tarCmd.Wait()
		return fmt.Errorf("git archive: %w", err)
	}
	if err := tarCmd.Wait(); err != nil {
		return fmt.Errorf("tar extract: %w", err)
	}
	return nil
}

func (g *Git) run(dir string, args ...string) error {
	cmd := exec.Command("git", args...)
	if dir != "" {
		cmd.Dir = dir
	}
	if g.Verbose {
		cmd.Stdout = g.stderr()
		cmd.Stderr = g.stderr()
	}
	return cmd.Run()
}

func (g *Git) output(dir string, args ...string) ([]byte, error) {
	cmd := exec.Command("git", args...)
	if dir != "" {
		cmd.Dir = dir
	}
	if g.Verbose {
		cmd.Stderr = g.stderr()
	}
	return cmd.Output()
}

// refCachePath returns the TTL cache file path for a bare repo.
func refCachePath(bareDir string) string {
	return filepath.Join(bareDir, "runx-refs.json")
}

func (g *Git) cachedRef(bareDir, ref string) (string, bool) {
	data, err := os.ReadFile(refCachePath(bareDir))
	if err != nil {
		return "", false
	}
	var rc refCache
	if err := json.Unmarshal(data, &rc); err != nil {
		return "", false
	}
	e, ok := rc.Refs[ref]
	if !ok {
		return "", false
	}
	if time.Since(e.ResolvedAt) > refTTL {
		return "", false
	}
	return e.Commit, true
}

func (g *Git) cacheRef(bareDir, ref, commit string) {
	path := refCachePath(bareDir)
	rc := refCache{Refs: make(map[string]refEntry)}
	if data, err := os.ReadFile(path); err == nil {
		_ = json.Unmarshal(data, &rc)
	}
	if rc.Refs == nil {
		rc.Refs = make(map[string]refEntry)
	}
	rc.Refs[ref] = refEntry{Commit: commit, ResolvedAt: time.Now()}
	data, _ := json.MarshalIndent(rc, "", "  ")
	_ = os.WriteFile(path, data, 0o644)
}
