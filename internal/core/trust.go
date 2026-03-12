package core

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"
)

// TrustStore tracks permanently trusted repositories.
type TrustStore struct {
	path string
}

type trustFile struct {
	Version int                  `json:"version"`
	Repos   map[string]trustEntry `json:"repos"`
}

type trustEntry struct {
	URL       string    `json:"url"`
	TrustedAt time.Time `json:"trusted_at"`
}

// NewTrustStore creates a TrustStore backed by path.
func NewTrustStore(path string) *TrustStore {
	return &TrustStore{path: path}
}

// IsTrusted reports whether the canonical URL has been permanently trusted.
func (ts *TrustStore) IsTrusted(canonical string) bool {
	tf, err := ts.load()
	if err != nil {
		return false
	}
	_, ok := tf.Repos[canonical]
	return ok
}

// Trust permanently trusts canonical and saves to disk.
func (ts *TrustStore) Trust(canonical string) {
	tf, _ := ts.load()
	if tf.Repos == nil {
		tf.Repos = make(map[string]trustEntry)
	}
	tf.Repos[canonical] = trustEntry{URL: canonical, TrustedAt: time.Now()}
	_ = ts.save(tf)
}

// Check verifies trust for ref/commit and prompts the user if needed.
// Returns an error (and does not execute) if the user declines.
func (ts *TrustStore) Check(ref RepoRef, commit string, f *Flags) error {
	// --trust implies --yes and permanently records trust.
	if f.Trust {
		ts.Trust(ref.CanonicalURL)
		return nil
	}
	// --yes: skip prompt but do not permanently trust.
	if f.Yes {
		return nil
	}
	// Already trusted.
	if ts.IsTrusted(ref.CanonicalURL) {
		return nil
	}

	// First-run prompt.
	short := commit
	if len(short) > 12 {
		short = short[:12]
	}

	fmt.Fprintln(os.Stderr)
	fmt.Fprintln(os.Stderr, "runx: ⚠️  executing code from an untrusted repository")
	fmt.Fprintln(os.Stderr)
	fmt.Fprintf(os.Stderr, "  Repository: %s\n", ref.CanonicalURL)
	fmt.Fprintf(os.Stderr, "  Ref:        %s → %s\n", ref.DisplayRef(), short)
	if ref.IsMutableRef() {
		fmt.Fprintln(os.Stderr)
		fmt.Fprintf(os.Stderr, "  WARNING: '%s' is a mutable ref. Pin to a commit SHA for reproducibility:\n", ref.Ref)
		fmt.Fprintf(os.Stderr, "           %s@%s\n", ref.CanonicalURL, commit)
	}
	fmt.Fprintln(os.Stderr)
	fmt.Fprint(os.Stderr, "  Type 'yes' to proceed once, 'trust' to remember, or Ctrl-C to abort: ")

	scanner := bufio.NewScanner(os.Stdin)
	if !scanner.Scan() {
		return fmt.Errorf("aborted")
	}
	answer := strings.ToLower(strings.TrimSpace(scanner.Text()))
	switch answer {
	case "yes", "y":
		return nil
	case "trust", "t":
		ts.Trust(ref.CanonicalURL)
		fmt.Fprintf(os.Stderr, "\n  Trusted. Future runs will skip this prompt.\n\n")
		return nil
	default:
		return fmt.Errorf("aborted by user")
	}
}

func (ts *TrustStore) load() (trustFile, error) {
	data, err := os.ReadFile(ts.path)
	if err != nil {
		return trustFile{Version: 1, Repos: make(map[string]trustEntry)}, nil
	}
	var tf trustFile
	if err := json.Unmarshal(data, &tf); err != nil {
		return trustFile{Version: 1, Repos: make(map[string]trustEntry)}, nil
	}
	if tf.Repos == nil {
		tf.Repos = make(map[string]trustEntry)
	}
	return tf, nil
}

func (ts *TrustStore) save(tf trustFile) error {
	tf.Version = 1
	data, err := json.MarshalIndent(tf, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(ts.path, data, 0o600)
}
