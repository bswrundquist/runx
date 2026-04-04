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

    /// Mode (sh/docker/compose/make/bin) or target (deploy.sh, Dockerfile, etc.)
    #[arg()]
    second: Option<String>,

    /// Target (when second arg is a mode keyword)
    #[arg()]
    third: Option<String>,

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
            eprintln!("Usage: runx [flags] <repo[@ref]> [mode|target] [target] [-- args...]");
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

    match args.second.as_deref() {
        // Explicit mode keywords
        Some("sh" | "shx") => {
            let script = match args.third.as_deref() {
                Some(s) => s,
                None => {
                    eprintln!("runx: sh mode requires a script path");
                    return 2;
                }
            };
            tools::shell::run(flags, repo_ref, script, &args.passthrough)
        }
        Some("docker" | "dockerx") => {
            tools::docker::run(flags, repo_ref, &args.passthrough)
        }
        Some("compose" | "dc" | "dcx") => {
            tools::compose::run(flags, repo_ref, &args.passthrough)
        }
        Some("make" | "makex") => {
            tools::make::run(flags, repo_ref, args.third.as_deref(), &args.passthrough)
        }
        Some("bin") => {
            let asset = match args.third.as_deref() {
                Some(s) => s,
                None => {
                    eprintln!("runx: bin mode requires an asset name");
                    return 2;
                }
            };
            tools::release::run(flags, repo_ref, asset, &args.passthrough)
        }

        // Auto-detect from file pattern
        Some(target) => {
            if let Some(mode) = tools::detect_mode(Some(target)) {
                if flags.verbose {
                    eprintln!("runx: auto-detected mode: {mode}");
                }
                match mode {
                    tools::ToolMode::Shell => {
                        tools::shell::run(flags, repo_ref, target, &args.passthrough)
                    }
                    tools::ToolMode::Docker => {
                        tools::docker::run(flags, repo_ref, &args.passthrough)
                    }
                    tools::ToolMode::Compose => {
                        tools::compose::run(flags, repo_ref, &args.passthrough)
                    }
                    tools::ToolMode::Release => {
                        tools::release::run(flags, repo_ref, target, &args.passthrough)
                    }
                }
            } else {
                no_mode_error(Some(target))
            }
        }

        // No second arg at all
        None => no_mode_error(None),
    }
}

fn no_mode_error(target: Option<&str>) -> i32 {
    if let Some(t) = target {
        eprintln!("runx: error: {t:?} does not match a known file pattern");
    } else {
        eprintln!("runx: error: no mode or target specified");
    }
    eprintln!();
    eprintln!("  Use an explicit mode:");
    eprintln!("    runx <repo> sh <script>         run a shell script");
    eprintln!("    runx <repo> make [target]        run a make target");
    eprintln!("    runx <repo> docker [-- args]     run docker");
    eprintln!("    runx <repo> compose [-- args]    run docker compose");
    eprintln!("    runx <repo> bin <asset>          download a release binary");
    eprintln!();
    eprintln!("  Or use a recognizable file name (*.sh, Dockerfile*, compose*.yml)");
    2
}
