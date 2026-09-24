mod render;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use ward_syntax::Severity;

/// Exit codes are part of the CLI contract: tests and CI scripts rely on them.
mod exit {
    pub const DIAGNOSTICS: u8 = 1;
    pub const INTERNAL: u8 = 2;
    /// `ward run`: the program threw, or failed in the runtime.
    pub const RUNTIME: u8 = 3;
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
        #[arg(allow_negative_numbers = true)]
        args: Vec<String>,
        /// Answer `ai fn` calls from a JSON file: `{"fn_name": answer, ...}`
        #[arg(long)]
        mock: Option<PathBuf>,
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
        Command::Build { file, target, out } => build(&file, target, &out),
        Command::Run {
            file,
            function,
            args,
            mock,
        } => run(&file, &function, &args, mock.as_deref()),
    }
}

fn analyze(file: &Path) -> Result<ward_check::Analysis, ExitCode> {
    ward_check::analyze(file, &ward_resolve::RealFs).map_err(|e| {
        eprintln!("error: cannot read `{}`: {}", e.path.display(), e.error);
        ExitCode::from(exit::INTERNAL)
    })
}

/// Checks and lowers a program, reporting diagnostics on stderr.
fn compile(file: &Path, verb: &str) -> Result<ward_ir::Program, ExitCode> {
    let analysis = analyze(file)?;
    let diags = analysis.diagnostics();
    let color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut err = std::io::stderr().lock();
    if let Err(e) = render::human(&analysis.program, diags, color, &mut err) {
        eprintln!("error: failed to write diagnostics: {e}");
        return Err(ExitCode::from(exit::INTERNAL));
    }
    match ward_ir::lower(&analysis) {
        Ok(program) => Ok(program),
        Err(ward_ir::LowerError::HasErrors) => {
            let errors = diags
                .iter()
                .filter(|d| d.diagnostic.severity == Severity::Error)
                .count();
            let _ = writeln!(
                err,
                "error: could not {verb} `{}` due to {}",
                file.display(),
                count(errors, "error")
            );
            Err(ExitCode::from(exit::DIAGNOSTICS))
        }
        Err(e @ ward_ir::LowerError::Internal { .. }) => {
            let _ = writeln!(err, "error: {e}");
            Err(ExitCode::from(exit::INTERNAL))
        }
    }
}

fn write_files<'a>(
    dir: &Path,
    files: impl IntoIterator<Item = (PathBuf, &'a str)>,
) -> std::io::Result<()> {
    for (path, contents) in files {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }
    Ok(())
}

fn build(file: &Path, target: Target, out: &Path) -> ExitCode {
    let Target::Python = target;
    let program = match compile(file, "build") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let files = ward_codegen_py::generate(&program);
    if let Err(e) = write_files(
        out,
        files.iter().map(|f| (f.path.clone(), f.contents.as_str())),
    ) {
        eprintln!("error: cannot write to `{}`: {e}", out.display());
        return ExitCode::from(exit::INTERNAL);
    }
    let entry = files
        .first()
        .map_or_else(PathBuf::new, |f| out.join(&f.path));
    eprintln!("built: {} -> {}", file.display(), entry.display());
    ExitCode::SUCCESS
}

fn run(file: &Path, function: &str, args: &[String], mock: Option<&Path>) -> ExitCode {
    let program = match compile(file, "run") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let Some(runner) = ward_codegen_py::runner(&program, function) else {
        let names: Vec<String> = program
            .modules
            .first()
            .map(|m| m.fns.iter().map(|f| format!("`{}`", f.name)).collect())
            .unwrap_or_default();
        eprintln!(
            "error: `{}` has no function `{function}` (functions: {})",
            file.display(),
            names.join(", ")
        );
        return ExitCode::from(exit::INTERNAL);
    };
    if args.len() != runner.arity {
        eprintln!(
            "error: `{function}` takes {} but {} {} given",
            count(runner.arity, "argument"),
            args.len(),
            if args.len() == 1 { "was" } else { "were" }
        );
        return ExitCode::from(exit::INTERNAL);
    }
    let mock = match mock.map(std::path::absolute).transpose() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: invalid mock path: {e}");
            return ExitCode::from(exit::INTERNAL);
        }
    };

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let dir = std::env::temp_dir().join(format!("ward-run-{}-{nanos}", std::process::id()));
    let generated = ward_codegen_py::generate(&program);
    let files = generated
        .iter()
        .map(|f| (f.path.clone(), f.contents.as_str()))
        .chain(
            ward_runtime::PYTHON_PACKAGE
                .iter()
                .map(|(p, c)| (PathBuf::from(p), *c)),
        )
        .chain([(PathBuf::from("__ward_run__.py"), runner.script.as_str())]);
    if let Err(e) = write_files(&dir, files) {
        eprintln!("error: cannot write to `{}`: {e}", dir.display());
        let _ = std::fs::remove_dir_all(&dir);
        return ExitCode::from(exit::INTERNAL);
    }

    let python = std::env::var_os("WARD_PYTHON").unwrap_or_else(|| "python3".into());
    let mut cmd = std::process::Command::new(&python);
    cmd.arg(dir.join("__ward_run__.py"))
        .args(args)
        .env("PYTHONPATH", &dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("WARD_MOCK");
    if let Some(mock) = &mock {
        cmd.env("WARD_MOCK", mock);
    }
    let status = cmd.status();
    let _ = std::fs::remove_dir_all(&dir);
    match status {
        Ok(s) => match s.code() {
            Some(0) => ExitCode::SUCCESS,
            Some(2) => ExitCode::from(exit::INTERNAL),
            _ => ExitCode::from(exit::RUNTIME),
        },
        Err(e) => {
            eprintln!(
                "error: cannot run `{}`: {e} (set WARD_PYTHON to a Python 3.10+ interpreter)",
                python.to_string_lossy()
            );
            ExitCode::from(exit::INTERNAL)
        }
    }
}

fn count(n: usize, what: &str) -> String {
    match n {
        1 => format!("1 {what}"),
        n => format!("{n} {what}s"),
    }
}

fn check(file: &Path, format: Format) -> ExitCode {
    let path = file.display().to_string();
    let analysis = match analyze(file) {
        Ok(a) => a,
        Err(code) => return code,
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
