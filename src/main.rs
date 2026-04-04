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
    run: RunArgs,
}

#[derive(Args)]
struct RunArgs {
    #[command(flatten)]
    flags: SharedFlags,

    /// Repository reference: owner/repo[@ref] or full git URL
    #[arg()]
    repo: Option<String>,

    /// Mode: sh, docker, compose, make, bin
    #[arg()]
    mode: Option<String>,

    /// Target (script path, make target, asset name)
    #[arg()]
    target: Option<String>,

    /// Arguments passed through to the underlying tool (after --)
    #[arg(last = true)]
    passthrough: Vec<String>,
}

#[derive(Subcommand)]
enum SubCmd {
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
        Some(SubCmd::Update(args)) => {
            tools::update::run(args.check, args.force, args.verbose)
        }
        None => run(cli.run),
    };

    std::process::exit(exit_code);
}

fn run(args: RunArgs) -> i32 {
    let repo_str = match &args.repo {
        Some(r) => r.as_str(),
        None => {
            eprintln!("runx: error: expected <repo[@ref]>");
            eprintln!();
            eprintln!("Usage: runx <repo[@ref]> <mode> [target] [-- args...]");
            eprintln!();
            eprintln!("Modes: sh, docker, compose, make, bin");
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

    let flags = args.flags.into_flags();

    match args.mode.as_deref() {
        Some("sh") => {
            let script = match args.target.as_deref() {
                Some(s) => s,
                None => {
                    eprintln!("runx: sh mode requires a script path");
                    return 2;
                }
            };
            tools::shell::run(flags, repo_ref, script, &args.passthrough)
        }
        Some("docker") => {
            tools::docker::run(flags, repo_ref, &args.passthrough)
        }
        Some("compose") => {
            tools::compose::run(flags, repo_ref, &args.passthrough)
        }
        Some("make") => {
            tools::make::run(flags, repo_ref, args.target.as_deref(), &args.passthrough)
        }
        Some("bin") => {
            let asset = match args.target.as_deref() {
                Some(s) => s,
                None => {
                    eprintln!("runx: bin mode requires an asset name");
                    return 2;
                }
            };
            tools::release::run(flags, repo_ref, asset, &args.passthrough)
        }
        Some(unknown) => {
            eprintln!("runx: error: unknown mode {unknown:?}");
            eprintln!();
            eprintln!("  Available modes:");
            eprintln!("    runx <repo> sh <script>         run a shell script");
            eprintln!("    runx <repo> make [target]        run a make target");
            eprintln!("    runx <repo> docker [-- args]     run docker");
            eprintln!("    runx <repo> compose [-- args]    run docker compose");
            eprintln!("    runx <repo> bin <asset> [-- args] run a release binary");
            2
        }
        None => {
            eprintln!("runx: error: no mode specified");
            eprintln!();
            eprintln!("  Usage: runx <repo[@ref]> <mode> [target] [-- args...]");
            eprintln!();
            eprintln!("  Available modes:");
            eprintln!("    runx <repo> sh <script>         run a shell script");
            eprintln!("    runx <repo> make [target]        run a make target");
            eprintln!("    runx <repo> docker [-- args]     run docker");
            eprintln!("    runx <repo> compose [-- args]    run docker compose");
            eprintln!("    runx <repo> bin <asset> [-- args] run a release binary");
            2
        }
    }
}

