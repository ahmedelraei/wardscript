mod render;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use ward_syntax::Severity;

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
    let analysis = match ward_check::analyze(file, &ward_resolve::RealFs) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: cannot read `{}`: {}", e.path.display(), e.error);
            return ExitCode::from(exit::INTERNAL);
        }
    };
    let diags = analysis.diagnostics();
    let errors = diags
        .iter()
        .filter(|d| d.diagnostic.severity == Severity::Error)
        .count();
    let warnings = diags.len() - errors;

    let written = match format {
        Format::Human => {
            let color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
            let mut err = std::io::stderr().lock();
            render::human(&analysis.program, diags, color, &mut err).and_then(|()| {
                let count = |n: usize, what: &str| match n {
                    1 => format!("1 {what}"),
                    n => format!("{n} {what}s"),
                };
                match (errors, warnings) {
                    (0, 0) => writeln!(err, "ok: {path}"),
                    (0, w) => writeln!(err, "ok: {path} ({})", count(w, "warning")),
                    (e, _) => writeln!(
                        err,
                        "error: could not check `{path}` due to {}",
                        count(e, "error")
                    ),
                }
            })
        }
        Format::Json => {
            let value = render::json(&analysis.program, diags);
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

    if errors > 0 {
        ExitCode::from(exit::DIAGNOSTICS)
    } else {
        ExitCode::SUCCESS
    }
}
