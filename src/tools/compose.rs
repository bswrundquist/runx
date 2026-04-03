use crate::core::{self, Engine, Flags, RepoRef};
use std::process::Command;

/// Run docker compose commands from a git repository checkout.
pub fn run(flags: Flags, repo_ref: RepoRef, compose_args: &[String]) -> i32 {
    if compose_args.is_empty() {
        eprintln!("runx: error: missing compose arguments after '--'");
        return 2;
    }

    let engine = Engine::new(flags);
    // Always mutable: compose creates artifacts, bind mounts, override files.
    let ws = match engine.prepare(&repo_ref, true) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("runx: {e}");
            return 1;
        }
    };

    let mut args = vec!["compose".to_string()];
    args.extend_from_slice(compose_args);

    let mut cmd = Command::new("docker");
    cmd.args(&args);
    cmd.current_dir(&ws.root);

    match core::run_child(&mut cmd) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("runx: {e}");
            1
        }
    }
}
