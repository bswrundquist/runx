package tools

import (
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"regexp"

	"github.com/bswr/runx/internal/core"
)

// commandNotFoundRe matches shell "command not found" lines.
// Covers bash, sh, zsh, and absolute shell paths like /bin/bash.
var commandNotFoundRe = regexp.MustCompile(`(?:[\w/]*sh):\s*(\S+):\s*(?:command not found|not found)`)

// stderrDetector streams stderr to w while detecting missing-tool errors.
type stderrDetector struct {
	w           io.Writer
	missingTool string // populated on first "command not found" match
}

func (d *stderrDetector) Write(p []byte) (n int, err error) {
	n, err = d.w.Write(p)
	if d.missingTool == "" {
		if m := commandNotFoundRe.FindStringSubmatch(string(p)); m != nil {
			d.missingTool = m[1]
		}
	}
	return
}

const makexUsage = `makex — run make targets from a git repository checkout

Usage:
  makex [flags] <repo[@ref]> [target] [-- [make args...]]

Examples:
  makex acme/infra@main bootstrap
  makex acme/infra@v1.2.0 test -- -j4
  makex acme/infra@main -- -f build/Makefile deploy
  makex acme/monorepo@main          (runs default make target)

Target is optional. When omitted, make runs its default target.
Flags and variables after '--' are passed directly to make.

makex always uses a mutable workspace because make writes build artifacts,
lockfiles, caches, compiled output, and generated sources.

Flags:
`

// RunMakeX is the entry point for the makex binary (and the 'make' subcommand of runx).
func RunMakeX(args []string) int {
	fs := flag.NewFlagSet("makex", flag.ContinueOnError)
	fs.Usage = func() { fmt.Fprint(os.Stderr, makexUsage); fs.PrintDefaults() }

	var f core.Flags
	core.AddFlags(fs, &f)

	toolArgs, makePassthrough := core.SplitOnDash(args[1:])
	if err := fs.Parse(toolArgs); err != nil {
		if err == flag.ErrHelp {
			return 0
		}
		return 2
	}

	positional := fs.Args()
	if len(positional) < 1 {
		fmt.Fprintln(os.Stderr, "makex: error: expected <repo[@ref]>")
		fs.Usage()
		return 2
	}
	repoArg := positional[0]
	var target string
	if len(positional) >= 2 {
		target = positional[1]
	}

	ref, err := core.ParseRepoRef(repoArg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "makex: %v\n", err)
		return 2
	}

	engine := core.NewEngine(&f)
	// Always mutable: make writes artifacts into the working directory.
	ws, err := engine.Prepare(ref, true)
	if err != nil {
		fmt.Fprintf(os.Stderr, "makex: %v\n", err)
		return 1
	}
	defer ws.Close() //nolint:errcheck

	// Build make argument list.
	// Order: [passthrough flags/vars] [target]
	// This allows -f Makefile.custom via the passthrough, and target last.
	var makeArgs []string
	makeArgs = append(makeArgs, makePassthrough...)
	if target != "" {
		makeArgs = append(makeArgs, target)
	}

	det := &stderrDetector{w: os.Stderr}

	cmd := exec.Command("make", makeArgs...)
	cmd.Dir = ws.Root
	cmd.Stderr = det // intercept stderr while still streaming it

	code, err := core.RunChild(cmd)
	if err != nil {
		fmt.Fprintf(os.Stderr, "makex: %v\n", err)
		return 1
	}
	if code != 0 && det.missingTool != "" {
		fmt.Fprintf(os.Stderr, "\nmakex: hint: %q not found on this host\n", det.missingTool)
		fmt.Fprintf(os.Stderr, "       makex runs make inside the repo but requires host tools to be installed.\n")
		fmt.Fprintf(os.Stderr, "       Install it first, e.g.: brew install %s\n", det.missingTool)
	}
	return code
}
