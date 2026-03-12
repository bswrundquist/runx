// Package tools contains the thin execution logic for each runx tool.
package tools

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"

	"github.com/bswr/runx/internal/core"
)

// scriptCmd returns a Cmd to run the script.
// If the script has a shebang (#!) the kernel will select the interpreter.
// If not, we invoke it via "sh" so scripts without shebangs work regardless.
func scriptCmd(script string, args []string) *exec.Cmd {
	if hasShebang(script) {
		// Make executable so the kernel can run it directly.
		_ = core.MakeExecutable(script)
		return exec.Command(script, args...)
	}
	// No shebang: invoke via sh. Don't need exec bit in this path.
	return exec.Command("sh", append([]string{script}, args...)...)
}

// hasShebang reports whether the file starts with "#!".
func hasShebang(path string) bool {
	f, err := os.Open(path)
	if err != nil {
		return false
	}
	defer f.Close()
	var buf [2]byte
	n, _ := f.Read(buf[:])
	return n == 2 && buf[0] == '#' && buf[1] == '!'
}

const shxUsage = `shx — run a shell script from a git repository

Usage:
  shx [flags] <repo[@ref]> <script-path> [-- [script args...]]

Examples:
  shx acme/tools@main scripts/release.sh
  shx acme/tools@1a2b3c4 scripts/bootstrap.sh -- --dry-run
  shx https://github.com/acme/tools@v1.2.0 scripts/test.sh

Flags:
`

// RunSHX is the entry point for the shx binary (and the 'sh' subcommand of runx).
// args should be os.Args (including the program name as args[0]).
func RunSHX(args []string) int {
	fs := flag.NewFlagSet("shx", flag.ContinueOnError)
	fs.Usage = func() { fmt.Fprint(os.Stderr, shxUsage); fs.PrintDefaults() }

	var f core.Flags
	core.AddFlags(fs, &f)

	// Split on "--" before flag parsing so script args don't confuse the parser.
	toolArgs, passthroughArgs := core.SplitOnDash(args[1:])
	if err := fs.Parse(toolArgs); err != nil {
		if err == flag.ErrHelp {
			return 0
		}
		return 2
	}

	positional := fs.Args()
	if len(positional) < 2 {
		fmt.Fprintln(os.Stderr, "shx: error: expected <repo[@ref]> <script-path>")
		fs.Usage()
		return 2
	}
	repoArg, scriptPath := positional[0], positional[1]

	ref, err := core.ParseRepoRef(repoArg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "shx: %v\n", err)
		return 2
	}

	engine := core.NewEngine(&f)
	ws, err := engine.Prepare(ref, true) // always mutable: scripts may write files
	if err != nil {
		fmt.Fprintf(os.Stderr, "shx: %v\n", err)
		return 1
	}
	defer ws.Close() //nolint:errcheck

	fullScript := filepath.Join(ws.Root, filepath.Clean(scriptPath))

	// Build the command. If the script has a shebang the kernel handles interpreter
	// selection; if not (exec format error would occur), fall back to sh.
	cmd := scriptCmd(fullScript, passthroughArgs)
	cmd.Dir = ws.Root

	code, err := core.RunChild(cmd)
	if err != nil {
		fmt.Fprintf(os.Stderr, "shx: %v\n", err)
		return 1
	}
	return code
}
