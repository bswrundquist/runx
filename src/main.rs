mod core;
mod tools;

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// runx — execute scripts, docker, compose, make, and release binaries from git repositories
#[derive(Parser)]
#[command(name = "runx", version, about, disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<SubCmd>,

    #[command(flatten)]
    auto: AutoArgs,
}

#[derive(Args)]
struct AutoArgs {
    #[command(flatten)]
    flags: SharedFlags,

    /// Repository reference: owner/repo[@ref] or full git URL
    #[arg()]
    repo: Option<String>,

    /// Script path, target name, or asset name
    #[arg()]
    target: Option<String>,

    /// Arguments passed through to the underlying tool (after --)
    #[arg(last = true)]
    passthrough: Vec<String>,
}

#[derive(Subcommand)]
enum SubCmd {
    /// Run a shell script from a git repository
    #[command(alias = "shx")]
    Sh(ToolArgs),

    /// Run docker commands from a git repository checkout
    #[command(alias = "dockerx")]
    Docker(ToolArgs),

    /// Run docker compose commands from a git repository checkout
    #[command(aliases = ["dcx", "dc"])]
    Compose(ToolArgs),

    /// Run make targets from a git repository checkout
    #[command(alias = "makex")]
    Make(ToolArgs),

    /// Download and run a release binary from GitHub
    Bin(ToolArgs),

    /// Update runx to the latest release
    Update(UpdateArgs),
}

#[derive(Args)]
struct UpdateArgs {
    /// Only check if an update is available; do not download
    #[arg(long)]
    check: bool,

    /// Force update even if already on the latest version
    #[arg(long)]
    force: bool,

    /// Print verbose output
    #[arg(long)]
    verbose: bool,
}

#[derive(Args)]
struct ToolArgs {
    #[command(flatten)]
    flags: SharedFlags,

    /// Repository reference: owner/repo[@ref] or full git URL
    repo: String,

    /// Script path, make target, or asset name
    target: Option<String>,

    /// Arguments passed through to the underlying tool (after --)
    #[arg(last = true)]
    passthrough: Vec<String>,
}

#[derive(Args, Clone)]
struct SharedFlags {
    /// Re-fetch from remote even if ref is cached
    #[arg(long)]
    refresh: bool,

    /// Disable network access; fail if ref is not cached
    #[arg(long)]
    offline: bool,

    /// Cache root directory
    #[arg(long, env = "RUNX_CACHE_DIR")]
    cache_dir: Option<PathBuf>,

    /// Skip interactive trust prompt (this run only)
    #[arg(long)]
    yes: bool,

    /// Permanently trust this repo (implies --yes)
    #[arg(long)]
    trust: bool,

    /// Print pinned-commit command after resolving mutable refs
    #[arg(long)]
    pin: bool,

    /// Print verbose output
    #[arg(long)]
    verbose: bool,

    /// Override ref resolution with a specific commit SHA
    #[arg(long)]
    commit: Option<String>,
}

impl SharedFlags {
    fn into_flags(self) -> core::Flags {
        core::Flags {
            refresh: self.refresh,
            offline: self.offline,
            cache_dir: self.cache_dir.unwrap_or_else(core::Flags::default_cache_dir),
            yes: self.yes,
            trust: self.trust,
            pin: self.pin,
            verbose: self.verbose,
            commit: self.commit,
        }
    }
}

fn main() {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Some(cmd) => run_subcommand(cmd),
        None => run_auto(cli.auto),
    };

    std::process::exit(exit_code);
}

fn run_subcommand(cmd: SubCmd) -> i32 {
    match cmd {
        SubCmd::Sh(args) => {
            let (flags, repo_ref) = match parse_tool_args(&args) {
                Ok(v) => v,
                Err(code) => return code,
            };
            let script = match &args.target {
                Some(s) => s.as_str(),
                None => {
                    eprintln!("runx sh: error: expected <script-path>");
                    return 2;
                }
            };
            tools::shell::run(flags, repo_ref, script, &args.passthrough)
        }
        SubCmd::Docker(args) => {
            let (flags, repo_ref) = match parse_tool_args(&args) {
                Ok(v) => v,
                Err(code) => return code,
            };
            tools::docker::run(flags, repo_ref, &args.passthrough)
        }
        SubCmd::Compose(args) => {
            let (flags, repo_ref) = match parse_tool_args(&args) {
                Ok(v) => v,
                Err(code) => return code,
            };
            tools::compose::run(flags, repo_ref, &args.passthrough)
        }
        SubCmd::Make(args) => {
            let (flags, repo_ref) = match parse_tool_args(&args) {
                Ok(v) => v,
                Err(code) => return code,
            };
            tools::make::run(flags, repo_ref, args.target.as_deref(), &args.passthrough)
        }
        SubCmd::Bin(args) => {
            let (flags, repo_ref) = match parse_tool_args(&args) {
                Ok(v) => v,
                Err(code) => return code,
            };
            let asset = match &args.target {
                Some(s) => s.as_str(),
                None => {
                    eprintln!("runx bin: error: expected <asset-name>");
                    return 2;
                }
            };
            tools::release::run(flags, repo_ref, asset, &args.passthrough)
        }
        SubCmd::Update(args) => {
            tools::update::run(args.check, args.force, args.verbose)
        }
    }
}

fn run_auto(auto: AutoArgs) -> i32 {
    let repo_str = match &auto.repo {
        Some(r) => r.as_str(),
        None => {
            eprintln!("runx: error: expected <repo[@ref]>");
            eprintln!();
            eprintln!("Usage: runx [flags] <repo[@ref]> [target] [-- args...]");
            eprintln!();
            eprintln!("Subcommands: sh, docker, compose, make, bin");
            eprintln!("Run 'runx --help' for more information.");
            return 2;
        }
    };

    let repo_ref = match core::RepoRef::parse(repo_str) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runx: {e}");
            return 2;
        }
    };

    let flags = auto.flags.into_flags();
    let mode = tools::detect_mode(auto.target.as_deref());

    if flags.verbose {
        eprintln!("runx: auto-detected mode: {mode}");
    }

    match mode {
        tools::ToolMode::Shell => {
            let Some(script) = auto.target.as_deref() else {
                eprintln!("runx: shell mode requires a script path");
                return 2;
            };
            tools::shell::run(flags, repo_ref, script, &auto.passthrough)
        }
        tools::ToolMode::Docker => {
            tools::docker::run(flags, repo_ref, &auto.passthrough)
        }
        tools::ToolMode::Compose => {
            tools::compose::run(flags, repo_ref, &auto.passthrough)
        }
        tools::ToolMode::Make => {
            tools::make::run(flags, repo_ref, auto.target.as_deref(), &auto.passthrough)
        }
        tools::ToolMode::Release => {
            let Some(asset) = auto.target.as_deref() else {
                eprintln!("runx: release mode requires an asset name");
                return 2;
            };
            tools::release::run(flags, repo_ref, asset, &auto.passthrough)
        }
    }
}

fn parse_tool_args(args: &ToolArgs) -> Result<(core::Flags, core::RepoRef), i32> {
    let repo_ref = match core::RepoRef::parse(&args.repo) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runx: {e}");
            return Err(2);
        }
    };
    Ok((args.flags.clone().into_flags(), repo_ref))
}
