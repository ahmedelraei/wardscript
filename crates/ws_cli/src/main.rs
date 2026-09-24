use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

/// Exit codes are part of the CLI contract: tests and CI scripts rely on them.
mod exit {
    #[expect(dead_code, reason = "returned once `check` reports diagnostics (M2)")]
    pub const DIAGNOSTICS: u8 = 1;
    pub const INTERNAL: u8 = 2;
}

#[derive(Parser)]
#[command(
    name = "ward",
    version,
    about = "Compiler for Wardscript, a typed language for trustworthy AI functions and agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Type-check a program and report diagnostics
    Check {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
    },
    /// Compile a program to a target language
    Build {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = Target::Python)]
        target: Target,
        /// Output directory
        #[arg(short, long, default_value = "build")]
        out: PathBuf,
    },
    /// Build a program and call one of its functions
    Run {
        file: PathBuf,
        function: String,
        /// Arguments passed to the function, as JSON values
        args: Vec<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Human,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum Target {
    Python,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (name, milestone) = match cli.command {
        Command::Check { .. } => ("check", "M2"),
        Command::Build { .. } => ("build", "M3"),
        Command::Run { .. } => ("run", "M3"),
    };
    eprintln!("error: `ward {name}` is not implemented yet (planned for {milestone})");
    ExitCode::from(exit::INTERNAL)
}
