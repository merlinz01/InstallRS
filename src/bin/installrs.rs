#[path = "../build/mod.rs"]
mod build;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "installrs",
    about = "Build self-contained installer executables"
)]
struct Cli {
    /// Directory containing the installer source crate
    #[arg(long, default_value = ".")]
    target: PathBuf,

    /// Output installer file path
    #[arg(long, short, default_value = "./installer")]
    output: PathBuf,

    /// Compression method: lzma, gzip, bzip2, or none
    #[arg(long, default_value = "lzma")]
    compression: String,

    /// Comma-separated glob patterns to ignore when including directories
    #[arg(long, default_value = ".git,.svn,node_modules")]
    ignore: String,

    /// Rust target triple for cross-compilation (e.g. x86_64-pc-windows-gnu)
    #[arg(long)]
    target_triple: Option<String>,

    /// Enable debug output
    #[arg(long, short)]
    verbose: bool,

    /// Suppress non-error output
    #[arg(long, short)]
    quiet: bool,

    /// Suppress all output
    #[arg(long, short)]
    silent: bool,
}

fn main() {
    let cli = Cli::parse();

    let log_level = if cli.silent {
        "off"
    } else if cli.quiet {
        "error"
    } else if cli.verbose {
        "debug"
    } else {
        "info"
    };

    env_logger::Builder::new()
        .filter_level(log_level.parse().unwrap())
        .format_timestamp(None)
        .format_target(false)
        .init();

    if let Err(e) = run(cli) {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let target = cli
        .target
        .canonicalize()
        .unwrap_or_else(|_| cli.target.clone());

    let build_dir = target.join("build");

    let ignore_patterns: Vec<String> = cli
        .ignore
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let params = build::builder::BuildParams {
        target_dir: target,
        build_dir,
        output_file: cli.output,
        compression: cli.compression,
        ignore_patterns,
        target_triple: cli.target_triple,
    };

    build::builder::build(&params)
}
