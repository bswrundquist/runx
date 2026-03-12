package tools

import (
	"flag"
	"fmt"
	"os"
	"os/exec"

	"github.com/bswr/runx/internal/core"
)

const dockerxUsage = `dockerx — run docker commands from a git repository checkout

Usage:
  dockerx [flags] <repo[@ref]> -- <docker args...>

Examples:
  dockerx acme/app@main -- build -t acme/app:dev .
  dockerx acme/app@v1.2.0 -- run --rm app:test
  dockerx acme/app@main -- compose up -d

The '--' separator is required. Everything after it is passed to the docker CLI.
The docker command runs with its working directory set to the repo root.

Flags:
`

// RunDockerX is the entry point for the dockerx binary (and the 'docker' subcommand of runx).
func RunDockerX(args []string) int {
	fs := flag.NewFlagSet("dockerx", flag.ContinueOnError)
	fs.Usage = func() { fmt.Fprint(os.Stderr, dockerxUsage); fs.PrintDefaults() }

	var f core.Flags
	core.AddFlags(fs, &f)

	toolArgs, dockerArgs := core.SplitOnDash(args[1:])
	if err := fs.Parse(toolArgs); err != nil {
		if err == flag.ErrHelp {
			return 0
		}
		return 2
	}

	positional := fs.Args()
	if len(positional) < 1 {
		fmt.Fprintln(os.Stderr, "dockerx: error: expected <repo[@ref]>")
		fs.Usage()
		return 2
	}
	if len(dockerArgs) == 0 {
		fmt.Fprintln(os.Stderr, "dockerx: error: missing docker arguments after '--'")
		fs.Usage()
		return 2
	}
	repoArg := positional[0]

	ref, err := core.ParseRepoRef(repoArg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dockerx: %v\n", err)
		return 2
	}

	engine := core.NewEngine(&f)
	// docker build reads the context but does not write into it; use immutable.
	ws, err := engine.Prepare(ref, false)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dockerx: %v\n", err)
		return 1
	}
	defer ws.Close() //nolint:errcheck

	cmd := exec.Command("docker", dockerArgs...)
	cmd.Dir = ws.Root

	code, err := core.RunChild(cmd)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dockerx: %v\n", err)
		return 1
	}
	return code
}
