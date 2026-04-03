use crate::core::{self, Engine, Flags, RepoRef};
use regex::Regex;
use std::process::Command;
use std::sync::LazyLock;

static COMMAND_NOT_FOUND_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:[\w/]*sh):\s*(\S+):\s*(?:command not found|not found)").unwrap()
});

/// Run make targets from a git repository checkout.
pub fn run(
    flags: Flags,
    repo_ref: RepoRef,
    target: Option<&str>,
    passthrough: &[String],
) -> i32 {
    let engine = Engine::new(flags);
    // Always mutable: make writes artifacts into the working directory.
    let ws = match engine.prepare(&repo_ref, true) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("runx: {e}");
            return 1;
        }
    };

    // Build make argument list: [passthrough flags/vars] [target]
    let mut make_args: Vec<String> = passthrough.to_vec();
    if let Some(t) = target {
        make_args.push(t.to_string());
    }

    let mut cmd = Command::new("make");
    cmd.args(&make_args);
    cmd.current_dir(&ws.root);

    // Intercept stderr to detect missing commands while still streaming it.
    let mut missing_tool: Option<String> = None;
    let code = match core::run_child_with_stderr(&mut cmd, |line| {
        if missing_tool.is_none() {
            if let Some(caps) = COMMAND_NOT_FOUND_RE.captures(line) {
                missing_tool = Some(caps[1].to_string());
            }
        }
    }) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("runx: {e}");
            return 1;
        }
    };

    if code != 0 {
        if let Some(ref tool) = missing_tool {
            eprintln!();
            eprintln!("runx: hint: {tool:?} not found on this host");
            eprintln!(
                "       runx runs make inside the repo but requires host tools to be installed."
            );
            eprintln!("       Install it first, e.g.: brew install {tool}");
        }
    }
    code
}
