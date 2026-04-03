use crate::core::{self, Engine, Flags, RepoRef};
use std::process::Command;

/// Run docker commands from a git repository checkout.
pub fn run(flags: Flags, repo_ref: RepoRef, docker_args: &[String]) -> i32 {
    if docker_args.is_empty() {
        eprintln!("runx: error: missing docker arguments after '--'");
        return 2;
    }

    let engine = Engine::new(flags);
    // Docker build reads the context but does not write into it; use immutable.
    let ws = match engine.prepare(&repo_ref, false) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("runx: {e}");
            return 1;
        }
    };

    let mut cmd = Command::new("docker");
    cmd.args(docker_args);
    cmd.current_dir(&ws.root);

    match core::run_child(&mut cmd) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("runx: {e}");
            1
        }
    }
}
