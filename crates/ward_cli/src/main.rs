mod lock;
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
    /// `ward test`: a test failed.
    pub const TESTS_FAILED: u8 = 4;
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
        /// Generate `async def` functions, for asyncio hosts
        #[arg(long = "async")]
        asyncio: bool,
    },
    /// Build a program and call one of its functions
    Run {
        file: PathBuf,
        function: String,
        /// Arguments passed to the function, as JSON values
        #[arg(allow_negative_numbers = true)]
        args: Vec<String>,
        /// Answer `ai fn` calls from a JSON file: `{"fn_name": answer, ...}`
        #[arg(long, conflicts_with = "model")]
        mock: Option<PathBuf>,
        /// Answer `ai fn` calls with a real model: `anthropic`, `anthropic:<model>` or
        /// `openai:<model>`. Needs the provider's SDK and API key. Repeat as
        /// `alias=<provider>:<model>` for the aliases in `model {...}` clauses; aliases
        /// left out use the plain `--model`.
        #[arg(long)]
        model: Vec<String>,
        /// Where to write the run's audit trace
        #[arg(long, default_value = ".ward/traces")]
        trace_dir: PathBuf,
        /// Don't write an audit trace
        #[arg(long, conflicts_with = "trace_dir")]
        no_trace: bool,
    },
    /// Pin the tool schemas of the MCP servers in `mcp.json` to `ward.lock`
    Lock {
        /// The MCP config listing the servers
        #[arg(default_value = "mcp.json")]
        config: PathBuf,
        /// Fail if `ward.lock` is out of date instead of writing it (for CI)
        #[arg(long)]
        check: bool,
        /// Seconds to wait for each server's answer
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Run the program's `test` blocks against their recorded model answers
    Test {
        file: PathBuf,
        /// Only run tests whose names contain one of these
        filters: Vec<String>,
        /// Run against a model and write the recordings, instead of replaying them
        #[arg(long)]
        record: bool,
        /// With `--record`: answer from a JSON file, `{"fn_name": answer, ...}`
        #[arg(long, requires = "record", conflicts_with = "model")]
        mock: Option<PathBuf>,
        /// With `--record`: the model to ask, as for `ward run --model`
        #[arg(long, requires = "record")]
        model: Vec<String>,
        /// The recordings file (default: `<file>.recordings.json` next to the program)
        #[arg(long)]
        recordings: Option<PathBuf>,
        /// Where to write each test's audit trace
        #[arg(long)]
        trace_dir: Option<PathBuf>,
    },
    /// Run the language server (LSP over stdio), for editors
    Lsp,
    /// Read the audit traces `ward run` and the runtime write
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },
}

#[derive(Subcommand)]
enum TraceCommand {
    /// Show a run's events, and where each value that reached a tool came from
    Show {
        /// A run id, or a unique prefix of one; the latest run if left out
        run: Option<String>,
        #[arg(long, env = "WARD_TRACE_DIR", default_value = ".ward/traces")]
        dir: PathBuf,
    },
    /// Print a run's trace in another format
    Export {
        run: Option<String>,
        #[arg(long, env = "WARD_TRACE_DIR", default_value = ".ward/traces")]
        dir: PathBuf,
        #[arg(long, value_enum, default_value_t = TraceFormat::Otlp)]
        format: TraceFormat,
        /// Send the spans to an OTLP/HTTP collector (e.g. `http://localhost:4318`) instead
        /// of printing them
        #[arg(long, conflicts_with = "format")]
        endpoint: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum TraceFormat {
    /// OpenTelemetry spans, OTLP/JSON
    Otlp,
    /// The trace as written: one JSON object per line
    Jsonl,
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
        Command::Build {
            file,
            target,
            out,
            asyncio,
        } => build(&file, target, &out, asyncio),
        Command::Run {
            file,
            function,
            args,
            mock,
            model,
            trace_dir,
            no_trace,
        } => run(
            &file,
            &function,
            &args,
            &RunOptions {
                mock,
                model,
                trace_dir: (!no_trace).then_some(trace_dir),
            },
        ),
        Command::Lock {
            config,
            check,
            timeout,
        } => lock_command(&config, check, timeout),
        Command::Test {
            file,
            filters,
            record,
            mock,
            model,
            recordings,
            trace_dir,
        } => test(
            &file,
            &TestOptions {
                filters,
                record,
                mock,
                model,
                recordings,
                trace_dir,
            },
        ),
        Command::Lsp => match ward_lsp::serve(std::io::stdin().lock(), std::io::stdout().lock()) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(exit::INTERNAL)
            }
        },
        Command::Trace { command } => trace(command),
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

fn build(file: &Path, target: Target, out: &Path, asyncio: bool) -> ExitCode {
    let Target::Python = target;
    let program = match compile(file, "build") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let files = ward_codegen_py::generate_with(
        &program,
        ward_codegen_py::Options {
            asyncio,
            tests: false,
        },
    );
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

struct RunOptions {
    mock: Option<PathBuf>,
    model: Vec<String>,
    trace_dir: Option<PathBuf>,
}

fn run(file: &Path, function: &str, args: &[String], opts: &RunOptions) -> ExitCode {
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
    let mock = match opts.mock.as_deref().map(std::path::absolute).transpose() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: invalid mock path: {e}");
            return ExitCode::from(exit::INTERNAL);
        }
    };
    let trace_dir = match opts
        .trace_dir
        .as_deref()
        .map(std::path::absolute)
        .transpose()
    {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: invalid trace directory: {e}");
            return ExitCode::from(exit::INTERNAL);
        }
    };

    let mut env: Vec<(&str, std::ffi::OsString)> = Vec::new();
    if let Some(mock) = &mock {
        env.push(("WARD_MOCK", mock.into()));
    }
    if !opts.model.is_empty() {
        env.push(("WARD_MODEL", model_specs(&opts.model).into()));
    }
    if let Some(dir) = &trace_dir {
        env.push(("WARD_TRACE_DIR", dir.into()));
    }
    let generated = ward_codegen_py::generate(&program);
    match run_python(file, &generated, &runner.script, args, env) {
        Ok(Some(0)) => ExitCode::SUCCESS,
        Ok(Some(2)) => ExitCode::from(exit::INTERNAL),
        Ok(_) => ExitCode::from(exit::RUNTIME),
        Err(code) => code,
    }
}

/// `--model` flags as `{"": default, "alias": spec}`, for the runner scripts.
fn model_specs(models: &[String]) -> String {
    let specs: serde_json::Map<String, serde_json::Value> = models
        .iter()
        .map(|m| match m.split_once('=') {
            Some((alias, spec)) => (alias.to_owned(), spec.into()),
            None => (String::new(), m.as_str().into()),
        })
        .collect();
    serde_json::Value::Object(specs).to_string()
}

/// Writes the generated modules, the runtime and `script` to a temporary directory and
/// runs the script with Python; returns its exit code.
fn run_python(
    file: &Path,
    generated: &[ward_codegen_py::OutputFile],
    script: &str,
    args: &[String],
    env: Vec<(&str, std::ffi::OsString)>,
) -> Result<Option<i32>, ExitCode> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let dir = std::env::temp_dir().join(format!("ward-run-{}-{nanos}", std::process::id()));
    let files = generated
        .iter()
        .map(|f| (f.path.clone(), f.contents.as_str()))
        .chain(
            ward_runtime::PYTHON_PACKAGE
                .iter()
                .map(|(p, c)| (PathBuf::from(p), *c)),
        )
        .chain([(PathBuf::from("__ward_run__.py"), script)]);
    if let Err(e) = write_files(&dir, files) {
        eprintln!("error: cannot write to `{}`: {e}", dir.display());
        let _ = std::fs::remove_dir_all(&dir);
        return Err(ExitCode::from(exit::INTERNAL));
    }

    let python = std::env::var_os("WARD_PYTHON").unwrap_or_else(|| "python3".into());
    let mut cmd = std::process::Command::new(&python);
    cmd.arg(dir.join("__ward_run__.py"))
        .args(args)
        .env("PYTHONPATH", &dir)
        .env("PYTHONDONTWRITEBYTECODE", "1");
    for var in [
        "WARD_MOCK",
        "WARD_MODEL",
        "WARD_TRACE_DIR",
        "WARD_MCP_CONFIG",
        "WARD_RECORD",
        "WARD_RECORDINGS",
        "WARD_FILTERS",
    ] {
        cmd.env_remove(var);
    }
    if let Some(config) = find_mcp_config(file) {
        cmd.env("WARD_MCP_CONFIG", config);
    }
    cmd.envs(env);
    let status = cmd.status();
    let _ = std::fs::remove_dir_all(&dir);
    match status {
        Ok(s) => Ok(s.code()),
        Err(e) => {
            eprintln!(
                "error: cannot run `{}`: {e} (set WARD_PYTHON to a Python 3.10+ interpreter)",
                python.to_string_lossy()
            );
            Err(ExitCode::from(exit::INTERNAL))
        }
    }
}

struct TestOptions {
    filters: Vec<String>,
    record: bool,
    mock: Option<PathBuf>,
    model: Vec<String>,
    recordings: Option<PathBuf>,
    trace_dir: Option<PathBuf>,
}

/// `ward test`: runs the file's `test` blocks against their recordings, or records them.
fn test(file: &Path, opts: &TestOptions) -> ExitCode {
    if opts.record && opts.mock.is_none() && opts.model.is_empty() {
        eprintln!(
            "error: `--record` needs a model to ask: `--model <provider>` or `--mock <answers.json>`"
        );
        return ExitCode::from(exit::INTERNAL);
    }
    let program = match compile(file, "test") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let Some(script) = ward_codegen_py::test_runner(&program) else {
        return ExitCode::from(exit::INTERNAL);
    };
    let recordings = opts
        .recordings
        .clone()
        .unwrap_or_else(|| file.with_extension("recordings.json"));
    let absolute = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_owned());
    let mut env: Vec<(&str, std::ffi::OsString)> = vec![
        ("WARD_RECORDINGS", absolute(&recordings).into()),
        (
            "WARD_FILTERS",
            serde_json::Value::from(opts.filters.clone())
                .to_string()
                .into(),
        ),
    ];
    if opts.record {
        env.push(("WARD_RECORD", "1".into()));
        if let Some(mock) = &opts.mock {
            env.push(("WARD_MOCK", absolute(mock).into()));
        }
        if !opts.model.is_empty() {
            env.push(("WARD_MODEL", model_specs(&opts.model).into()));
        }
    }
    if let Some(dir) = &opts.trace_dir {
        env.push(("WARD_TRACE_DIR", absolute(dir).into()));
    }
    let generated = ward_codegen_py::generate_with(
        &program,
        ward_codegen_py::Options {
            asyncio: false,
            tests: true,
        },
    );
    match run_python(file, &generated, &script, &[], env) {
        Ok(Some(0)) => {
            if opts.record {
                eprintln!("recorded to `{}`", recordings.display());
            }
            ExitCode::SUCCESS
        }
        Ok(Some(4)) => ExitCode::from(exit::TESTS_FAILED),
        Ok(Some(2)) => ExitCode::from(exit::INTERNAL),
        Ok(_) => ExitCode::from(exit::RUNTIME),
        Err(code) => code,
    }
}

fn lock_command(config: &Path, check: bool, timeout: u64) -> ExitCode {
    match lock::lock(config, check, std::time::Duration::from_secs(timeout)) {
        Ok(out) => {
            for (name, tools) in &out.servers {
                eprintln!("  {name}: {}", count(*tools, "tool"));
            }
            let path = out.path.display();
            if out.unchanged {
                eprintln!("ok: `{path}` is up to date");
                ExitCode::SUCCESS
            } else if check {
                eprintln!("error: `{path}` is out of date; run `ward lock`");
                ExitCode::from(exit::DIAGNOSTICS)
            } else {
                eprintln!("wrote `{path}`");
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(exit::INTERNAL)
        }
    }
}

/// `mcp.json` in the program's directory or the nearest parent that has one, so `ward
/// run` connects its tool imports to those servers.
fn find_mcp_config(file: &Path) -> Option<PathBuf> {
    let file = std::path::absolute(file).ok()?;
    file.ancestors()
        .skip(1)
        .map(|d| d.join("mcp.json"))
        .find(|p| p.is_file())
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

fn trace(command: TraceCommand) -> ExitCode {
    let (run, dir) = match &command {
        TraceCommand::Show { run, dir } | TraceCommand::Export { run, dir, .. } => (run, dir),
    };
    let records = match ward_runtime::trace::find(dir, run.as_deref().unwrap_or(""))
        .and_then(|path| ward_runtime::trace::read(&path))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(exit::INTERNAL);
        }
    };
    if let TraceCommand::Export {
        endpoint: Some(endpoint),
        ..
    } = &command
    {
        return send_otlp(endpoint, &ward_runtime::otlp::export(&records));
    }
    let text = match command {
        TraceCommand::Show { .. } => ward_runtime::show::render(&records),
        TraceCommand::Export {
            format: TraceFormat::Otlp,
            ..
        } => format!(
            "{}\n",
            serde_json::to_string_pretty(&ward_runtime::otlp::export(&records)).unwrap_or_default()
        ),
        TraceCommand::Export {
            format: TraceFormat::Jsonl,
            ..
        } => records
            .iter()
            .map(|r| serde_json::to_string(r).unwrap_or_default() + "\n")
            .collect(),
    };
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(exit::INTERNAL)
        }
    }
}

/// POSTs spans to `<endpoint>/v1/traces`, as OTLP/HTTP with the JSON encoding.
fn send_otlp(endpoint: &str, spans: &serde_json::Value) -> ExitCode {
    let url = otlp_url(endpoint);
    match ureq::post(&url)
        .header("Content-Type", "application/json")
        .send(spans.to_string())
    {
        Ok(_) => {
            eprintln!("sent to {url}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: sending the trace to {url}: {e}");
            ExitCode::from(exit::INTERNAL)
        }
    }
}

fn otlp_url(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.ends_with("/v1/traces") {
        base.to_owned()
    } else {
        format!("{base}/v1/traces")
    }
}
