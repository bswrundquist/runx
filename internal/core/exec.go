package core

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"syscall"
)

// RunChild executes cmd, forwarding stdin/stdout/stderr and signals.
// It returns the child's exit code (not an error for non-zero exit).
// A non-nil error means the process could not be started.
func RunChild(cmd *exec.Cmd) (int, error) {
	// Attach terminal streams.
	if cmd.Stdin == nil {
		cmd.Stdin = os.Stdin
	}
	if cmd.Stdout == nil {
		cmd.Stdout = os.Stdout
	}
	if cmd.Stderr == nil {
		cmd.Stderr = os.Stderr
	}

	// Put the child in its own process group so we can forward signals cleanly.
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}

	if err := cmd.Start(); err != nil {
		return 1, fmt.Errorf("starting process: %w", err)
	}

	// Forward SIGINT and SIGTERM to the child's process group.
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)
	go func() {
		for sig := range sigCh {
			_ = syscall.Kill(-cmd.Process.Pid, sig.(syscall.Signal))
		}
	}()

	err := cmd.Wait()
	signal.Stop(sigCh)
	close(sigCh)

	if err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			return exitErr.ExitCode(), nil
		}
		return 1, err
	}
	return 0, nil
}

// MakeExecutable ensures path has the owner-execute bit set.
func MakeExecutable(path string) error {
	info, err := os.Stat(path)
	if err != nil {
		return err
	}
	return os.Chmod(path, info.Mode()|0o100)
}
