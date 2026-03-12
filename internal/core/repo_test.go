package core

import (
	"testing"
)

func TestParseRepoRef(t *testing.T) {
	tests := []struct {
		input        string
		wantURL      string
		wantRef      string
		wantIsCommit bool
		wantErr      bool
	}{
		// GitHub shorthand
		{
			input:   "owner/repo",
			wantURL: "https://github.com/owner/repo",
			wantRef: "",
		},
		{
			input:   "owner/repo@main",
			wantURL: "https://github.com/owner/repo",
			wantRef: "main",
		},
		{
			input:   "owner/repo@v1.2.0",
			wantURL: "https://github.com/owner/repo",
			wantRef: "v1.2.0",
		},
		{
			input:        "owner/repo@" + "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
			wantURL:      "https://github.com/owner/repo",
			wantRef:      "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
			wantIsCommit: true,
		},
		// Repo with dots and hyphens
		{
			input:   "acme-corp/my.tool@v2",
			wantURL: "https://github.com/acme-corp/my.tool",
			wantRef: "v2",
		},
		// HTTPS URLs
		{
			input:   "https://github.com/owner/repo",
			wantURL: "https://github.com/owner/repo",
			wantRef: "",
		},
		{
			input:   "https://github.com/owner/repo.git",
			wantURL: "https://github.com/owner/repo",
			wantRef: "",
		},
		{
			input:   "https://github.com/owner/repo.git@main",
			wantURL: "https://github.com/owner/repo",
			wantRef: "main",
		},
		{
			input:   "https://github.com/owner/repo@v1.2.0",
			wantURL: "https://github.com/owner/repo",
			wantRef: "v1.2.0",
		},
		// SSH URLs
		{
			input:   "git@github.com:owner/repo.git",
			wantURL: "https://github.com/owner/repo",
			wantRef: "",
		},
		{
			input:   "git@github.com:owner/repo.git@main",
			wantURL: "https://github.com/owner/repo",
			wantRef: "main",
		},
		{
			input:   "git@gitlab.com:group/project.git@develop",
			wantURL: "https://gitlab.com/group/project",
			wantRef: "develop",
		},
		// HTTP normalized to HTTPS
		{
			input:   "http://github.com/owner/repo",
			wantURL: "https://github.com/owner/repo",
			wantRef: "",
		},
		// Errors
		{
			input:   "notarepo",
			wantErr: true,
		},
		{
			input:   "",
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got, err := ParseRepoRef(tt.input)
			if (err != nil) != tt.wantErr {
				t.Fatalf("ParseRepoRef(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
			}
			if tt.wantErr {
				return
			}
			if got.CanonicalURL != tt.wantURL {
				t.Errorf("CanonicalURL = %q, want %q", got.CanonicalURL, tt.wantURL)
			}
			if got.Ref != tt.wantRef {
				t.Errorf("Ref = %q, want %q", got.Ref, tt.wantRef)
			}
			if got.IsCommit != tt.wantIsCommit {
				t.Errorf("IsCommit = %v, want %v", got.IsCommit, tt.wantIsCommit)
			}
		})
	}
}

func TestRepoRefIsMutableRef(t *testing.T) {
	tests := []struct {
		ref  string
		want bool
	}{
		{"", false},   // empty = HEAD, treated as immutable for this purpose
		{"main", true},
		{"v1.0.0", true},
		{"feature/foo", true},
		{"a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2", false}, // full SHA
	}
	for _, tt := range tests {
		r := makeRepoRef("https://github.com/a/b", tt.ref)
		if got := r.IsMutableRef(); got != tt.want {
			t.Errorf("IsMutableRef(%q) = %v, want %v", tt.ref, got, tt.want)
		}
	}
}

func TestURLHash(t *testing.T) {
	// Same URL must produce same hash.
	h1 := URLHash("https://github.com/owner/repo")
	h2 := URLHash("https://github.com/owner/repo")
	if h1 != h2 {
		t.Errorf("URLHash not deterministic: %q vs %q", h1, h2)
	}
	// Different URLs must produce different hashes.
	h3 := URLHash("https://github.com/owner/other")
	if h1 == h3 {
		t.Errorf("URLHash collision between different URLs")
	}
	// Hash should be 16 hex characters (8 bytes).
	if len(h1) != 16 {
		t.Errorf("URLHash length = %d, want 16", len(h1))
	}
}

func TestSplitOnDash(t *testing.T) {
	tests := []struct {
		args        []string
		wantBefore  []string
		wantAfter   []string
	}{
		{
			args:       []string{"a", "b", "--", "c", "d"},
			wantBefore: []string{"a", "b"},
			wantAfter:  []string{"c", "d"},
		},
		{
			args:       []string{"a", "b"},
			wantBefore: []string{"a", "b"},
			wantAfter:  nil,
		},
		{
			args:       []string{"--", "c", "d"},
			wantBefore: []string{},
			wantAfter:  []string{"c", "d"},
		},
		{
			args:       nil,
			wantBefore: nil,
			wantAfter:  nil,
		},
	}
	for _, tt := range tests {
		before, after := SplitOnDash(tt.args)
		if len(before) != len(tt.wantBefore) {
			t.Errorf("SplitOnDash(%v) before = %v, want %v", tt.args, before, tt.wantBefore)
			continue
		}
		for i := range before {
			if before[i] != tt.wantBefore[i] {
				t.Errorf("SplitOnDash before[%d] = %q, want %q", i, before[i], tt.wantBefore[i])
			}
		}
		if len(after) != len(tt.wantAfter) {
			t.Errorf("SplitOnDash(%v) after = %v, want %v", tt.args, after, tt.wantAfter)
		}
	}
}
