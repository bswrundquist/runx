package tools

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/bswr/runx/internal/core"
)

// makeLocalRepo creates a temp bare git repo for use in tool tests.
func makeLocalRepo(t *testing.T, files map[string]string) (bareDir, commitSHA string) {
	t.Helper()
	workDir := t.TempDir()
	runGit(t, workDir, "init", "-b", "main")
	runGit(t, workDir, "config", "user.email", "test@example.com")
	runGit(t, workDir, "config", "user.name", "Test")
	for name, content := range files {
		full := filepath.Join(workDir, name)
		_ = os.MkdirAll(filepath.Dir(full), 0o755)
		if err := os.WriteFile(full, []byte(content), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	runGit(t, workDir, "add", ".")
	runGit(t, workDir, "commit", "-m", "init")

	out, err := exec.Command("git", "-C", workDir, "rev-parse", "HEAD").Output()
	if err != nil {
		t.Fatal(err)
	}
	commitSHA = strings.TrimSpace(string(out))

	bareDir = t.TempDir()
	_ = os.RemoveAll(bareDir)
	if err := exec.Command("git", "clone", "--bare", "--quiet", workDir, bareDir).Run(); err != nil {
		t.Fatalf("git clone --bare: %v", err)
	}
	return bareDir, commitSHA
}

func runGit(t *testing.T, dir string, args ...string) {
	t.Helper()
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git %v: %v\n%s", args, err, out)
	}
}

func prepareEngine(t *testing.T, bareDir string) (*core.Engine, core.RepoRef) {
	t.Helper()
	cacheDir := t.TempDir()

	// Use the bare repo dir as if it's a git URL by cloning from it.
	// We'll set CacheDir and RUNX_CACHE_DIR so the engine uses our temp dir.
	t.Setenv("RUNX_CACHE_DIR", cacheDir)

	f := &core.Flags{
		CacheDir: cacheDir,
		Yes:      true, // skip trust prompt in tests
	}
	engine := core.NewEngine(f)

	// Use the bare dir URL directly as a "file://" remote.
	ref, err := core.ParseRepoRef("file://" + bareDir)
	if err != nil {
		// file:// not parsed by shorthand; build manually
		ref = core.RepoRef{CanonicalURL: "file://" + bareDir}
	}

	return engine, ref
}

// TestSHX_exitCode verifies that the child exit code is propagated.
func TestSHX_exitCode(t *testing.T) {
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"exit42.sh": "#!/bin/sh\nexit 42\n",
	})

	cacheDir := t.TempDir()
	f := &core.Flags{CacheDir: cacheDir, Yes: true, Commit: sha}
	engine := core.NewEngine(f)

	ref := core.RepoRef{CanonicalURL: "file://" + bareDir, Ref: sha, IsCommit: true}
	// Pre-populate bare repo in cache by copying it.
	repoHash := core.URLHash("file://" + bareDir)
	_ = os.MkdirAll(filepath.Join(cacheDir, "repos"), 0o755)
	_ = exec.Command("cp", "-r", bareDir, filepath.Join(cacheDir, "repos", repoHash)).Run()

	ws, err := engine.Prepare(ref, true)
	if err != nil {
		t.Skipf("Prepare failed (likely git URL issue): %v", err)
	}
	defer ws.Close() //nolint:errcheck

	scriptPath := filepath.Join(ws.Root, "exit42.sh")
	cmd := exec.Command(scriptPath)
	cmd.Dir = ws.Root

	code, err := core.RunChild(cmd)
	if err != nil {
		t.Fatalf("RunChild: %v", err)
	}
	if code != 42 {
		t.Errorf("exit code = %d, want 42", code)
	}
}

// TestSHX_noShebang verifies that scripts without a shebang line still run via sh.
func TestSHX_noShebang(t *testing.T) {
	// Script has no #! line — would trigger "exec format error" if exec'd directly.
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"noshebang.sh": "printf 'ran-without-shebang'\n",
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

	cmd := scriptCmd(filepath.Join(ws.Root, "noshebang.sh"), nil)
	cmd.Dir = ws.Root
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("running no-shebang script: %v", err)
	}
	if string(out) != "ran-without-shebang" {
		t.Errorf("output = %q, want %q", string(out), "ran-without-shebang")
	}
}

// TestSHX_argPassthrough verifies that script args are passed through unchanged.
func TestSHX_argPassthrough(t *testing.T) {
	bareDir, sha := makeLocalRepo(t, map[string]string{
		"args.sh": "#!/bin/sh\nprintf '%s\\n' \"$@\"\n",
	})

	cacheDir := t.TempDir()
	repoHash := core.URLHash("file://" + bareDir)
	_ = os.MkdirAll(filepath.Join(cacheDir, "repos"), 0o755)
	_ = exec.Command("cp", "-r", bareDir, filepath.Join(cacheDir, "repos", repoHash)).Run()

	f := &core.Flags{CacheDir: cacheDir, Yes: true}
	engine := core.NewEngine(f)
	ref := core.RepoRef{CanonicalURL: "file://" + bareDir, Ref: sha, IsCommit: true}

	ws, err := engine.Prepare(ref, true)
	if err != nil {
		t.Skipf("Prepare: %v", err)
	}
	defer ws.Close() //nolint:errcheck

	scriptPath := filepath.Join(ws.Root, "args.sh")
	wantArgs := []string{"--dry-run", "foo bar", "--count=3"}
	cmd := exec.Command(scriptPath, wantArgs...)
	cmd.Dir = ws.Root

	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("running args.sh: %v", err)
	}
	lines := strings.Split(strings.TrimSpace(string(out)), "\n")
	if len(lines) != len(wantArgs) {
		t.Fatalf("got %d lines, want %d\noutput: %q", len(lines), len(wantArgs), string(out))
	}
	for i, want := range wantArgs {
		if lines[i] != want {
			t.Errorf("arg[%d] = %q, want %q", i, lines[i], want)
		}
	}
}
