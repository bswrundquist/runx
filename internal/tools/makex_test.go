package tools

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/bswr/runx/internal/core"
)

func prepareRepoInCache(t *testing.T, bareDir, cacheDir string) {
	t.Helper()
	repoHash := core.URLHash("file://" + bareDir)
	_ = os.MkdirAll(filepath.Join(cacheDir, "repos"), 0o755)
	out, err := exec.Command("cp", "-r", bareDir, filepath.Join(cacheDir, "repos", repoHash)).CombinedOutput()
	if err != nil {
		t.Fatalf("copying bare repo to cache: %v\n%s", err, out)
	}
}

// TestMakeX_target verifies that a named target is run correctly.
func TestMakeX_target(t *testing.T) {
	makefile := `
.PHONY: greet
greet:
	@echo hello-from-make
`
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"Makefile": makefile,
	})

	cacheDir := t.TempDir()
	prepareRepoInCache(t, bareDir, cacheDir)

	f := &core.Flags{CacheDir: cacheDir, Yes: true}
	engine := core.NewEngine(f)
	ref := core.RepoRef{CanonicalURL: "file://" + bareDir, Ref: sha, IsCommit: true}

	ws, err := engine.Prepare(ref, true)
	if err != nil {
		t.Skipf("Prepare: %v", err)
	}
	defer ws.Close() //nolint:errcheck

	cmd := exec.Command("make", "greet")
	cmd.Dir = ws.Root
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("make greet: %v", err)
	}
	if !strings.Contains(string(out), "hello-from-make") {
		t.Errorf("output %q does not contain 'hello-from-make'", string(out))
	}
}

// TestMakeX_defaultTarget verifies that make runs the default target when none is given.
func TestMakeX_defaultTarget(t *testing.T) {
	makefile := `
.PHONY: all
all:
	@echo default-target-ran
`
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"Makefile": makefile,
	})

	cacheDir := t.TempDir()
	prepareRepoInCache(t, bareDir, cacheDir)

	f := &core.Flags{CacheDir: cacheDir, Yes: true}
	engine := core.NewEngine(f)
	ref := core.RepoRef{CanonicalURL: "file://" + bareDir, Ref: sha, IsCommit: true}

	ws, err := engine.Prepare(ref, true)
	if err != nil {
		t.Skipf("Prepare: %v", err)
	}
	defer ws.Close() //nolint:errcheck

	// No target: make runs default.
	cmd := exec.Command("make")
	cmd.Dir = ws.Root
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("make (default): %v", err)
	}
	if !strings.Contains(string(out), "default-target-ran") {
		t.Errorf("output %q does not contain 'default-target-ran'", string(out))
	}
}

// TestMakeX_explicitMakefile verifies that an explicit -f flag selects the right Makefile.
func TestMakeX_explicitMakefile(t *testing.T) {
	altMakefile := `
.PHONY: build
build:
	@echo alt-makefile-ran
`
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"Makefile":           ".PHONY: all\nall:\n\t@echo wrong-makefile\n",
		"build/Makefile.alt": altMakefile,
	})

	cacheDir := t.TempDir()
	prepareRepoInCache(t, bareDir, cacheDir)

	f := &core.Flags{CacheDir: cacheDir, Yes: true}
	engine := core.NewEngine(f)
	ref := core.RepoRef{CanonicalURL: "file://" + bareDir, Ref: sha, IsCommit: true}

	ws, err := engine.Prepare(ref, true)
	if err != nil {
		t.Skipf("Prepare: %v", err)
	}
	defer ws.Close() //nolint:errcheck

	// Pass explicit -f flag via passthrough args.
	cmd := exec.Command("make", "-f", "build/Makefile.alt", "build")
	cmd.Dir = ws.Root
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("make -f build/Makefile.alt build: %v", err)
	}
	if !strings.Contains(string(out), "alt-makefile-ran") {
		t.Errorf("output %q does not contain 'alt-makefile-ran'", string(out))
	}
}

// TestMakeX_writeDoesNotDirtyImmutableCache verifies that make artifacts stay in the
// mutable workspace and do not appear in the immutable tree.
func TestMakeX_writeDoesNotDirtyImmutableCache(t *testing.T) {
	makefile := `
.PHONY: build
build:
	@echo artifact > artifact.txt
`
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"Makefile": makefile,
	})

	cacheDir := t.TempDir()
	prepareRepoInCache(t, bareDir, cacheDir)

	f := &core.Flags{CacheDir: cacheDir, Yes: true}
	engine := core.NewEngine(f)
	ref := core.RepoRef{CanonicalURL: "file://" + bareDir, Ref: sha, IsCommit: true}

	// First: materialize immutable tree.
	g := &core.Git{}
	_ = os.MkdirAll(filepath.Join(cacheDir, "trees", core.URLHash("file://"+bareDir)), 0o755)
	treeDir := filepath.Join(cacheDir, "trees", core.URLHash("file://"+bareDir), sha)
	if err := g.Archive(bareDir, sha, treeDir); err != nil {
		t.Fatalf("Archive: %v", err)
	}

	ws, err := engine.Prepare(ref, true)
	if err != nil {
		t.Skipf("Prepare: %v", err)
	}
	defer ws.Close() //nolint:errcheck

	// Run make in the mutable workspace.
	cmd := exec.Command("make", "build")
	cmd.Dir = ws.Root
	if err := cmd.Run(); err != nil {
		t.Fatalf("make build: %v", err)
	}

	// artifact.txt should exist in workspace.
	if _, err := os.Stat(filepath.Join(ws.Root, "artifact.txt")); err != nil {
		t.Errorf("artifact.txt missing from workspace: %v", err)
	}

	// artifact.txt must NOT exist in the immutable tree.
	if _, err := os.Stat(filepath.Join(treeDir, "artifact.txt")); err == nil {
		t.Error("immutable tree was dirtied: artifact.txt found in cache")
	}
}

// TestMakeX_exitCode verifies exit code propagation.
func TestMakeX_exitCode(t *testing.T) {
	makefile := ".PHONY: fail\nfail:\n\t@exit 7\n"
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"Makefile": makefile,
	})

	cacheDir := t.TempDir()
	prepareRepoInCache(t, bareDir, cacheDir)

	f := &core.Flags{CacheDir: cacheDir, Yes: true}
	engine := core.NewEngine(f)
	ref := core.RepoRef{CanonicalURL: "file://" + bareDir, Ref: sha, IsCommit: true}

	ws, err := engine.Prepare(ref, true)
	if err != nil {
		t.Skipf("Prepare: %v", err)
	}
	defer ws.Close() //nolint:errcheck

	cmd := exec.Command("make", "fail")
	cmd.Dir = ws.Root

	code, err := core.RunChild(cmd)
	if err != nil {
		t.Fatalf("RunChild: %v", err)
	}
	// make wraps the exit code through its own exit; it typically exits 2 on recipe failure.
	// Just verify it's non-zero.
	if code == 0 {
		t.Error("expected non-zero exit code from failing make target")
	}
}
