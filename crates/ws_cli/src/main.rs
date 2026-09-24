mod render;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use ws_syntax::Severity;

/// Exit codes are part of the CLI contract: tests and CI scripts rely on them.
mod exit {
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
    match cli.command {
        Command::Check { file, format } => check(&file, format),
        Command::Build { .. } => not_implemented("build", "M3"),
        Command::Run { .. } => not_implemented("run", "M3"),
    }
}

fn not_implemented(name: &str, milestone: &str) -> ExitCode {
    eprintln!("error: `ward {name}` is not implemented yet (planned for {milestone})");
    ExitCode::from(exit::INTERNAL)
}

fn check(file: &std::path::Path, format: Format) -> ExitCode {
    let path = file.display().to_string();
    let src = match std::fs::read_to_string(file) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("error: cannot read `{path}`: {e}");
            return ExitCode::from(exit::INTERNAL);
        }
    };

    // Name resolution and type checking join the pipeline in M2.
    let diags = ws_syntax::parse(&src).diagnostics;

    let written = match format {
        Format::Human => {
            let color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
            let mut err = std::io::stderr().lock();
            render::human(&path, &src, &diags, color, &mut err).and_then(|()| match diags.len() {
                0 => writeln!(err, "ok: {path}"),
                1 => writeln!(err, "error: could not check `{path}` due to 1 error"),
                n => writeln!(err, "error: could not check `{path}` due to {n} errors"),
            })
        }
        Format::Json => {
            let value = render::json(&path, &src, &diags);
            let mut out = std::io::stdout().lock();
            serde_json::to_writer_pretty(&mut out, &value)
                .map_err(std::io::Error::from)
                .and_then(|()| writeln!(out))
        }
    };
    if let Err(e) = written {
        eprintln!("error: failed to write diagnostics: {e}");
        return ExitCode::from(exit::INTERNAL);
    }

    if diags.iter().any(|d| d.severity == Severity::Error) {
        ExitCode::from(exit::DIAGNOSTICS)
    } else {
        ExitCode::SUCCESS
    }
}
