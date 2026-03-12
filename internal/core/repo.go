// Package core is the shared engine for all runx tools.
package core

import (
	"fmt"
	"net/url"
	"regexp"
	"strings"
)

// RepoRef is a parsed repository reference.
type RepoRef struct {
	// CanonicalURL is the normalized HTTPS URL, no .git suffix, no trailing slash.
	CanonicalURL string
	// Ref is the branch, tag, or commit SHA. Empty means HEAD.
	Ref string
	// IsCommit is true when Ref is a full 40-char hex SHA.
	IsCommit bool
}

var (
	// shorthandRe matches owner/repo or owner/repo@ref.
	// Deliberately restrictive: no dots in owner, dots OK in repo.
	shorthandRe = regexp.MustCompile(`^([a-zA-Z0-9][a-zA-Z0-9_-]*)/([a-zA-Z0-9][a-zA-Z0-9_.-]*)(?:@(.+))?$`)
	// fullSHARe matches a 40-character hex commit SHA.
	fullSHARe = regexp.MustCompile(`^[0-9a-f]{40}$`)
)

// ParseRepoRef parses a repo reference string into a RepoRef.
//
// Supported formats:
//
//	owner/repo[@ref]                  GitHub shorthand
//	https://host/path[.git][@ref]     Full HTTPS URL
//	git@host:path[.git][@ref]         SSH URL
func ParseRepoRef(s string) (RepoRef, error) {
	switch {
	case strings.HasPrefix(s, "git@"):
		return parseSSHRepoRef(s)
	case strings.HasPrefix(s, "https://") || strings.HasPrefix(s, "http://"):
		return parseHTTPSRepoRef(s)
	default:
		return parseShorthandRepoRef(s)
	}
}

func parseShorthandRepoRef(s string) (RepoRef, error) {
	m := shorthandRe.FindStringSubmatch(s)
	if m == nil {
		return RepoRef{}, fmt.Errorf("invalid repo reference %q: expected owner/repo[@ref] or a full git URL", s)
	}
	canonical := fmt.Sprintf("https://github.com/%s/%s", m[1], m[2])
	return makeRepoRef(canonical, m[3]), nil
}

func parseHTTPSRepoRef(s string) (RepoRef, error) {
	// Extract @ref from the URL path: https://host/owner/repo@ref
	var ref string
	u, err := url.Parse(s)
	if err != nil {
		return RepoRef{}, fmt.Errorf("invalid URL %q: %w", s, err)
	}
	path := u.Path
	if i := strings.Index(path, "@"); i >= 0 {
		ref = path[i+1:]
		path = path[:i]
	}
	path = strings.TrimSuffix(path, ".git")
	path = strings.TrimRight(path, "/")
	u.Scheme = "https"
	u.Path = path
	u.Fragment = ""
	u.RawQuery = ""
	return makeRepoRef(u.String(), ref), nil
}

func parseSSHRepoRef(s string) (RepoRef, error) {
	// git@host:owner/repo[.git][@ref]
	rest := strings.TrimPrefix(s, "git@")
	colonIdx := strings.Index(rest, ":")
	if colonIdx < 0 {
		return RepoRef{}, fmt.Errorf("invalid SSH URL %q: missing colon separator", s)
	}
	host := rest[:colonIdx]
	pathRef := rest[colonIdx+1:]

	var ref string
	// @ref suffix follows the last @ in the path portion.
	if i := strings.LastIndex(pathRef, "@"); i >= 0 {
		ref = pathRef[i+1:]
		pathRef = pathRef[:i]
	}
	path := strings.TrimSuffix(pathRef, ".git")
	canonical := fmt.Sprintf("https://%s/%s", host, path)
	return makeRepoRef(canonical, ref), nil
}

func makeRepoRef(canonical, ref string) RepoRef {
	return RepoRef{
		CanonicalURL: canonical,
		Ref:          ref,
		IsCommit:     fullSHARe.MatchString(ref),
	}
}

// IsMutableRef returns true when the ref is a branch or tag (not a pinned commit).
func (r RepoRef) IsMutableRef() bool {
	return r.Ref != "" && !r.IsCommit
}

// DisplayRef returns the ref for display, defaulting to "HEAD" when empty.
func (r RepoRef) DisplayRef() string {
	if r.Ref == "" {
		return "HEAD"
	}
	return r.Ref
}

// String returns a human-readable form.
func (r RepoRef) String() string {
	if r.Ref == "" {
		return r.CanonicalURL
	}
	return r.CanonicalURL + "@" + r.Ref
}
