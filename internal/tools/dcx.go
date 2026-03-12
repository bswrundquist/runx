package tools

import (
	"flag"
	"fmt"
	"os"
	"os/exec"

	"github.com/bswr/runx/internal/core"
)

const dcxUsage = `dcx — run docker compose commands from a git repository checkout

Usage:
  dcx [flags] <repo[@ref]> -- <compose args...>

Examples:
  dcx acme/platform@main -- up -d
  dcx acme/platform@9f8e7d6 -- logs api
  dcx acme/platform@main -- -f docker-compose.prod.yml up

The '--' separator is required. Everything after it is passed to 'docker compose'.
The compose command runs from the repo root where compose files are auto-detected.

Auto-detected compose files (in order):
  compose.yml, compose.yaml, docker-compose.yml, docker-compose.yaml

dcx always uses a mutable workspace because compose workflows create local
artifacts, bind mounts, env files, and override files.

Flags:
`

// RunDCX is the entry point for the dcx binary (and the 'compose' subcommand of runx).
func RunDCX(args []string) int {
	fs := flag.NewFlagSet("dcx", flag.ContinueOnError)
	fs.Usage = func() { fmt.Fprint(os.Stderr, dcxUsage); fs.PrintDefaults() }

	var f core.Flags
	core.AddFlags(fs, &f)

	toolArgs, composeArgs := core.SplitOnDash(args[1:])
	if err := fs.Parse(toolArgs); err != nil {
		if err == flag.ErrHelp {
			return 0
		}
		return 2
	}

	positional := fs.Args()
	if len(positional) < 1 {
		fmt.Fprintln(os.Stderr, "dcx: error: expected <repo[@ref]>")
		fs.Usage()
		return 2
	}
	if len(composeArgs) == 0 {
		fmt.Fprintln(os.Stderr, "dcx: error: missing compose arguments after '--'")
		fs.Usage()
		return 2
	}
	repoArg := positional[0]

	ref, err := core.ParseRepoRef(repoArg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dcx: %v\n", err)
		return 2
	}

	engine := core.NewEngine(&f)
	// Always mutable: compose creates artifacts, bind mounts, override files.
	ws, err := engine.Prepare(ref, true)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dcx: %v\n", err)
		return 1
	}
	defer ws.Close() //nolint:errcheck

	cmd := exec.Command("docker", append([]string{"compose"}, composeArgs...)...)
	cmd.Dir = ws.Root

	code, err := core.RunChild(cmd)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dcx: %v\n", err)
		return 1
	}
	return code
}
