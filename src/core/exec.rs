use crate::core::RunxError;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// Put the child in its own process group so we can forward signals cleanly.
fn setup_process_group(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
}

/// Guard that closes the signal handle on drop, ensuring the forwarding
/// thread exits even if the caller returns early via `?`.
struct SignalGuard {
    handle: signal_hook::iterator::backend::Handle,
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        self.handle.close();
    }
}

/// Spawn a signal-forwarding thread and return a guard that cleans it up on drop.
fn forward_signals(child_pid: i32) -> Result<SignalGuard, RunxError> {
    let mut signals =
        signal_hook::iterator::Signals::new([libc::SIGINT, libc::SIGTERM])
            .map_err(RunxError::Io)?;
    let handle = signals.handle();
    std::thread::spawn(move || {
        for sig in signals.forever() {
            unsafe { libc::kill(-child_pid, sig); }
        }
    });
    Ok(SignalGuard { handle })
}

/// Execute cmd, forwarding stdin/stdout/stderr and signals.
/// Returns the child's exit code. Errors only for "cannot start process" failures.
pub fn run_child(cmd: &mut Command) -> Result<i32, RunxError> {
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    setup_process_group(cmd);

    let mut child = cmd.spawn().map_err(RunxError::ProcessStart)?;
    let _guard = forward_signals(child.id() as i32)?;

    let status = child.wait().map_err(RunxError::Io)?;
    Ok(status.code().unwrap_or(1))
}

/// Execute cmd with stderr piped, forwarding signals. Calls `on_stderr_line`
/// for each line read from the child's stderr while also printing it to stderr.
/// Returns the child's exit code.
pub fn run_child_with_stderr<F>(cmd: &mut Command, mut on_stderr_line: F) -> Result<i32, RunxError>
where
    F: FnMut(&str),
{
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::piped());
    setup_process_group(cmd);

    let mut child = cmd.spawn().map_err(RunxError::ProcessStart)?;
    let _guard = forward_signals(child.id() as i32)?;

    // Stream stderr while processing each line.
    if let Some(stderr) = child.stderr.take() {
        use std::io::{BufRead, Write};
        let reader = std::io::BufReader::new(stderr);
        let mut err_out = std::io::stderr();
        for line in reader.lines().map_while(Result::ok) {
            let _ = writeln!(err_out, "{line}");
            on_stderr_line(&line);
        }
    }

    let status = child.wait().map_err(RunxError::Io)?;
    Ok(status.code().unwrap_or(1))
}

/// Ensure path has the owner-execute bit set.
pub fn make_executable(path: &Path) -> Result<(), RunxError> {
    let meta = std::fs::metadata(path)?;
    let mode = meta.permissions().mode() | 0o100;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}
