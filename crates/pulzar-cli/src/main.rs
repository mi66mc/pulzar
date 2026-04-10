use std::{fs, path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand, ValueEnum};
use pulzar_lexer::lex;
use pulzar_output::{ColorMode, Reporter};
use pulzar_parser::parse_file;
use pulzar_runtime::{ShellContext, run_file};
use pulzar_sema::analyze_file;
use pulzar_syntax::SourceId;

#[derive(Debug, Parser)]
#[command(name = "pulzar")]
#[command(about = "Pulzar language tooling")]
struct Cli {
    #[arg(long, global = true, value_enum, default_value = "auto")]
    color: CliColor,
    #[command(subcommand)]
    command: Command,
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
    let cli = Cli::parse();
    let reporter = Reporter::new(match cli.color {
        CliColor::Auto => ColorMode::Auto,
        CliColor::Always => ColorMode::Always,
        CliColor::Never => ColorMode::Never,
    });

    match cli.command {
        Command::Lex(args) => {
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
        Command::Parse(args) => {
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
        Command::Check(args) => {
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
        Command::Run(args) => {
            let input = load_input(args.expr, args.path)?;
            let parsed = parse_file(&input.1, SourceId(0));
            let sema = analyze_file(&parsed.file);

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

            if parsed.diagnostics.is_empty() && sema.diagnostics.is_empty() {
                let mut shell = ShellContext::default();
                let runtime = run_file(&parsed.file, &mut shell);
                reporter.print_diagnostic_section(
                    "runtime diagnostics",
                    &input.0,
                    &input.1,
                    &runtime.diagnostics,
                );

                if runtime.diagnostics.is_empty() {
                    reporter.print_status(
                        pulzar_output::StatusKind::Success,
                        &format!("ran {} statement(s)", parsed.file.statements.len()),
                    );
                    if let Some(value) = runtime.value {
                        reporter.print_value(&value.to_string());
                    }
                } else {
                    reporter.print_status(
                        pulzar_output::StatusKind::Failure,
                        "runtime execution failed",
                    );
                }
            } else {
                reporter.print_status(
                    pulzar_output::StatusKind::Failure,
                    "run aborted due to parse/semantic diagnostics",
                );
            }
        }
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
