package core

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

// makeLocalRepo creates a temporary bare git repository for testing.
// It returns the path to the bare repo and the full SHA of the single commit created.
func makeLocalRepo(t *testing.T, files map[string]string) (bareDir string, commitSHA string) {
	t.Helper()

	// Create a regular (non-bare) work repo.
	workDir := t.TempDir()
	runGit(t, workDir, "init", "-b", "main")
	runGit(t, workDir, "config", "user.email", "test@example.com")
	runGit(t, workDir, "config", "user.name", "Test")

	// Write files.
	for name, content := range files {
		full := filepath.Join(workDir, name)
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(full, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	runGit(t, workDir, "add", ".")
	runGit(t, workDir, "commit", "-m", "initial")

	// Get the commit SHA.
	out, err := exec.Command("git", "-C", workDir, "rev-parse", "HEAD").Output()
	if err != nil {
		t.Fatalf("rev-parse HEAD: %v", err)
	}
	commitSHA = string(out)
	if len(commitSHA) > 0 && commitSHA[len(commitSHA)-1] == '\n' {
		commitSHA = commitSHA[:len(commitSHA)-1]
	}

	// Clone to bare repo.
	bareDir = t.TempDir()
	if err := os.RemoveAll(bareDir); err != nil {
		t.Fatal(err)
	}
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
