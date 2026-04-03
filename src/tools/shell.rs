use crate::core::{self, Engine, Flags, RepoRef};
use std::path::Path;
use std::process::Command;

/// Run a shell script from a git repository.
pub fn run(flags: Flags, repo_ref: RepoRef, script_path: &str, args: &[String]) -> i32 {
    let engine = Engine::new(flags);
    let ws = match engine.prepare(&repo_ref, true) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("runx: {e}");
            return 1;
        }
    };

    let full_script = ws.root.join(script_path);

    let mut cmd = script_cmd(&full_script, args);
    cmd.current_dir(&ws.root);

    match core::run_child(&mut cmd) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("runx: {e}");
            1
        }
    }
}

/// Build a Command to run the script.
/// If the script has a shebang (#!) the kernel will select the interpreter.
/// If not, we invoke it via "sh" so scripts without shebangs work.
fn script_cmd(script: &Path, args: &[String]) -> Command {
    if has_shebang(script) {
        let _ = core::make_executable(script);
        let mut cmd = Command::new(script);
        cmd.args(args);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg(script);
        cmd.args(args);
        cmd
    }
}

/// Whether the file starts with "#!".
fn has_shebang(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 2];
    f.read_exact(&mut buf).is_ok() && buf == *b"#!"
}
