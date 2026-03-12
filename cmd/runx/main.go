// runx is the umbrella binary for the runx tool family.
// Subcommands: sh, docker, compose, make
// Thin wrappers shx, dockerx, dcx, makex are also provided as separate binaries.
package main

import (
	"fmt"
	"os"

	"github.com/bswr/runx/internal/tools"
)

const runxUsage = `runx — execute scripts, docker, compose, and make from git repositories

Usage:
  runx <subcommand> [flags] <repo[@ref]> ...

Subcommands:
  sh       run a shell script       (alias: shx)
  docker   run docker commands      (alias: dockerx)
  compose  run docker compose       (alias: dcx)
  make     run make targets         (alias: makex)

Run 'runx <subcommand> --help' for subcommand usage.

Examples:
  runx sh acme/tools@main scripts/release.sh
  runx docker acme/app@main -- build -t acme/app:dev .
  runx compose acme/platform@main -- up -d
  runx make acme/infra@main bootstrap
`

func main() {
	if len(os.Args) < 2 {
		fmt.Fprint(os.Stderr, runxUsage)
		os.Exit(2)
	}

	sub := os.Args[1]
	// Rewrite os.Args so that each tool sees its own name as args[0].
	rest := os.Args[2:]

	switch sub {
	case "sh", "shx":
		os.Exit(tools.RunSHX(append([]string{"shx"}, rest...)))
	case "docker", "dockerx":
		os.Exit(tools.RunDockerX(append([]string{"dockerx"}, rest...)))
	case "compose", "dcx", "dc":
		os.Exit(tools.RunDCX(append([]string{"dcx"}, rest...)))
	case "make", "makex":
		os.Exit(tools.RunMakeX(append([]string{"makex"}, rest...)))
	case "-h", "--help", "help":
		fmt.Fprint(os.Stderr, runxUsage)
		os.Exit(0)
	default:
		fmt.Fprintf(os.Stderr, "runx: unknown subcommand %q\n\n%s", sub, runxUsage)
		os.Exit(2)
	}
}
