use std::{
    fs::File,
    io::{self, BufWriter, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use lib_respect::{
    CollectOptions, DependencyReport, GhClient, collect_dependencies, collect_stars,
};
use serde::Serialize;

mod library_cli;
mod self_update;
mod tui;

#[derive(Parser)]
#[command(
    version,
    about = "Discover Cargo/CMake dependency repositories and your GitHub star status"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Compare the build-time commit with the remote main branch.
    Check,
    /// Run self-update to fetch the latest version.
    Update,
    /// Browse your public GitHub projects and their declared dependencies.
    Tui,
    /// List direct dependencies as JSON (no recursive traversal).
    Deps(DepsArgs),
    /// Group deps JSON by GitHub repository and query your star status using gh.
    Stars(StarsArgs),
    /// Inspect the shared library-to-repository cache as JSON, independent of cwd.
    Libraries {
        /// Enrich dependencies of all cached projects; refresh expired library metadata.
        #[arg(long)]
        refresh: bool,
    },
}

#[derive(Args)]
struct DepsArgs {
    /// Project directory or Cargo.toml (defaults to the current directory).
    path: Option<PathBuf>,
    /// Include only this workspace package; may be repeated.
    #[arg(short, long = "package")]
    packages: Vec<String>,
    /// Activate all Cargo features, including optional dependencies.
    #[arg(long)]
    all_features: bool,
    /// Disable Cargo's default features.
    #[arg(long)]
    no_default_features: bool,
    /// Cargo features to activate (comma-separated or repeated).
    #[arg(long, value_delimiter = ',')]
    features: Vec<String>,
    /// Restrict resolved dependencies to a target triple.
    #[arg(long)]
    filter_platform: Option<String>,
    /// Require an existing, up-to-date Cargo.lock without modifying it.
    #[arg(long)]
    locked: bool,
    /// Prevent Cargo from accessing the network.
    #[arg(long)]
    offline: bool,
}

#[derive(Args)]
struct StarsArgs {
    /// One or more deps JSON files; '-' reads stdin (default).
    #[arg(default_value = "-")]
    inputs: Vec<PathBuf>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(2),
        Err(error) => {
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<io::Error>()
                    .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
            }) {
                return ExitCode::SUCCESS;
            }
            eprintln!("lib-respect: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<bool> {
    match cli.command {
        Some(Commands::Check) => {
            self_update::run_check()?;
            Ok(false)
        }
        Some(Commands::Update) => {
            self_update::run_self_update()?;
            Ok(false)
        }
        Some(Commands::Libraries { refresh }) => {
            let store = lib_respect::browser::Store::local()?;
            if refresh {
                library_cli::refresh(&store)?;
            }
            write_json(&store.libraries()?)?;
            Ok(false)
        }
        None | Some(Commands::Tui) => {
            tui::run()?;
            Ok(false)
        }
        Some(Commands::Deps(args)) => {
            let report = collect_dependencies(&CollectOptions {
                manifest_path: args.path,
                packages: args.packages,
                all_features: args.all_features,
                no_default_features: args.no_default_features,
                features: args.features,
                filter_platform: args.filter_platform,
                locked: args.locked,
                offline: args.offline,
            })?;
            write_json(&report)?;
            Ok(false)
        }
        Some(Commands::Stars(args)) => {
            if args
                .inputs
                .iter()
                .filter(|path| path.as_os_str() == "-")
                .count()
                > 1
            {
                bail!("stdin ('-') can only be read once");
            }
            let reports = args
                .inputs
                .iter()
                .map(read_report)
                .collect::<Result<Vec<_>>>()?;
            let report = collect_stars(&reports, &GhClient)?;
            let failed = report
                .repositories
                .iter()
                .filter(|repo| repo.error.is_some())
                .count();
            write_json(&report)?;
            if failed > 0 {
                eprintln!("lib-respect: {failed} repository check(s) failed; see errors in JSON");
            }
            Ok(failed > 0)
        }
    }
}

fn read_report(path: &PathBuf) -> Result<DependencyReport> {
    let mut json = String::new();
    if path.as_os_str() == "-" {
        io::stdin()
            .read_to_string(&mut json)
            .context("cannot read stdin")?;
    } else {
        File::open(path)
            .with_context(|| format!("cannot open {}", path.display()))?
            .read_to_string(&mut json)
            .with_context(|| format!("cannot read {}", path.display()))?;
    }
    serde_json::from_str(json.trim_start_matches('\u{feff}'))
        .with_context(|| format!("invalid deps JSON in {}", path.display()))
}

fn write_json(report: &impl Serialize) -> Result<()> {
    let mut output = BufWriter::new(io::stdout().lock());
    serde_json::to_writer_pretty(&mut output, report).context("cannot write JSON")?;
    output.write_all(b"\n").context("cannot write newline")?;
    output.flush().context("cannot flush stdout")
}
