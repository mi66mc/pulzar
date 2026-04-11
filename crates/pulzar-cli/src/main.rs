use std::{
    fs,
    io::{self, BufRead},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use pulzar_lexer::lex;
use pulzar_output::{ColorMode, Reporter};
use pulzar_parser::parse_file;
use pulzar_runtime::{
    Session, ShellContext, install_interrupt_handler, run_file, run_file_in_session, take_interrupt,
};
use pulzar_sema::analyze_file;
use pulzar_syntax::SourceId;

#[derive(Debug, Parser)]
#[command(name = "pulzar")]
#[command(about = "Pulzar language tooling")]
struct Cli {
    #[arg(long, global = true, value_enum, default_value = "auto")]
    color: CliColor,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Lex(LexArgs),
    Parse(ParseArgs),
    Check(CheckArgs),
    Run(RunArgs),
}

#[derive(Debug, Args)]
struct LexArgs {
    #[arg(long, conflicts_with = "path")]
    expr: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ParseArgs {
    #[arg(long, conflicts_with = "path")]
    expr: Option<String>,
    path: Option<PathBuf>,
    #[arg(long)]
    debug_ast: bool,
}

#[derive(Debug, Args)]
struct CheckArgs {
    #[arg(long, conflicts_with = "path")]
    expr: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long, conflicts_with = "path")]
    expr: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliColor {
    Auto,
    Always,
    Never,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    install_interrupt_handler()?;
    let cli = Cli::parse();
    let reporter = Reporter::new(match cli.color {
        CliColor::Auto => ColorMode::Auto,
        CliColor::Always => ColorMode::Always,
        CliColor::Never => ColorMode::Never,
    });

    match cli.command {
        None => run_repl(&reporter)?,
        Some(Command::Lex(args)) => {
            let input = load_input(args.expr, args.path)?;
            let file = lex(&input.1, SourceId(0));
            reporter.print_status(pulzar_output::StatusKind::Info, "lexed input");
            reporter.print_lex_tokens(
                &input.1,
                &pulzar_syntax::LineIndex::new(&input.1),
                &file.tokens,
            );
            reporter.print_diagnostic_section(
                "lexer diagnostics",
                &input.0,
                &input.1,
                &file.diagnostics,
            );
        }
        Some(Command::Parse(args)) => {
            let input = load_input(args.expr, args.path)?;
            let parsed = parse_file(&input.1, SourceId(0));
            if parsed.diagnostics.is_empty() {
                reporter.print_parse_summary(parsed.file.statements.len(), args.debug_ast);
            } else {
                reporter.print_status(pulzar_output::StatusKind::Failure, "parse failed");
            }
            reporter.print_diagnostic_section(
                "parse diagnostics",
                &input.0,
                &input.1,
                &parsed.diagnostics,
            );
            if args.debug_ast {
                reporter.print_debug_ast("ast", &parsed.file);
            }
        }
        Some(Command::Check(args)) => {
            let input = load_input(args.expr, args.path)?;
            let parsed = parse_file(&input.1, SourceId(0));
            let sema = analyze_file(&parsed.file);
            reporter.print_check_summary(
                parsed.file.statements.len(),
                parsed.diagnostics.len(),
                sema.diagnostics.len(),
            );
            reporter.print_diagnostic_section(
                "parse diagnostics",
                &input.0,
                &input.1,
                &parsed.diagnostics,
            );
            reporter.print_diagnostic_section(
                "semantic diagnostics",
                &input.0,
                &input.1,
                &sema.diagnostics,
            );
        }
        Some(Command::Run(args)) => {
            let input = load_input(args.expr, args.path)?;
            execute_program(&input.0, &input.1, &reporter, None, true);
        }
    }

    Ok(())
}

fn run_repl(reporter: &Reporter) -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut shell = ShellContext::default();
    shell.interactive = true;
    let mut session = Session::new(shell);

    loop {
        reporter
            .print_prompt(session.cwd())
            .map_err(|err| format!("failed to write prompt: {err}"))?;

        let mut line = String::new();
        let bytes = match stdin.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                let _ = take_interrupt();
                println!();
                continue;
            }
            Err(err) => return Err(format!("failed to read input: {err}")),
        };
        if bytes == 0 {
            if take_interrupt() {
                println!();
                continue;
            }
            println!();
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if matches!(input, "exit" | "quit") {
            break;
        }

        execute_program("<repl>", input, reporter, Some(&mut session), false);
    }

    Ok(())
}

fn load_input(expr: Option<String>, path: Option<PathBuf>) -> Result<(String, String), String> {
    match (expr, path) {
        (Some(expr), None) => Ok(("<expr>".to_string(), expr)),
        (None, Some(path)) => {
            let display = path.display().to_string();
            let contents = fs::read_to_string(&path)
                .map_err(|err| format!("failed to read `{display}`: {err}"))?;
            Ok((display, contents))
        }
        (None, None) => Err("expected either a source path or --expr".to_string()),
        (Some(_), Some(_)) => Err("use either a source path or --expr, not both".to_string()),
    }
}

fn execute_program(
    source_name: &str,
    source: &str,
    reporter: &Reporter,
    session: Option<&mut Session>,
    print_success_status: bool,
) {
    let parsed = parse_file(source, SourceId(0));
    let sema = analyze_file(&parsed.file);

    reporter.print_diagnostic_section(
        "parse diagnostics",
        source_name,
        source,
        &parsed.diagnostics,
    );
    reporter.print_diagnostic_section(
        "semantic diagnostics",
        source_name,
        source,
        &sema.diagnostics,
    );

    if !parsed.diagnostics.is_empty() || !sema.diagnostics.is_empty() {
        if print_success_status {
            reporter.print_status(
                pulzar_output::StatusKind::Failure,
                "run aborted due to parse/semantic diagnostics",
            );
        }
        return;
    }

    let runtime = match session {
        Some(session) => run_file_in_session(&parsed.file, session),
        None => {
            let mut shell = ShellContext::default();
            run_file(&parsed.file, &mut shell)
        }
    };

    reporter.print_diagnostic_section(
        "runtime diagnostics",
        source_name,
        source,
        &runtime.diagnostics,
    );

    if runtime.diagnostics.is_empty() {
        if print_success_status {
            reporter.print_status(
                pulzar_output::StatusKind::Success,
                &format!("ran {} statement(s)", parsed.file.statements.len()),
            );
        }
        if let Some(value) = runtime.value {
            if !matches!(value, pulzar_runtime::Value::Null) {
                reporter.print_runtime_value(&value);
            }
        }
    } else if print_success_status {
        reporter.print_status(
            pulzar_output::StatusKind::Failure,
            "runtime execution failed",
        );
    }
}
