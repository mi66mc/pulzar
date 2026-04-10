use std::{fs, path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand};
use pulzar_lexer::lex;
use pulzar_parser::{parse_expr, parse_file};
use pulzar_syntax::{LineIndex, SourceId, TokenKind};

#[derive(Debug, Parser)]
#[command(name = "pulzar")]
#[command(about = "Pulzar language tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Lex(LexArgs),
    Parse(ParseArgs),
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
    match cli.command {
        Command::Lex(args) => {
            let input = load_input(args.expr, args.path)?;
            let file = lex(&input.1, SourceId(0));
            let line_index = LineIndex::new(&input.1);

            for token in &file.tokens {
                let (line, col) = line_index.line_col(token.span.start());
                let text = &input.1[token.span.start()..token.span.end()];
                println!(
                    "{line}:{col}\t{:?}\t{}",
                    token.kind,
                    format_token_text(token.kind, text)
                );
            }

            if !file.diagnostics.is_empty() {
                eprintln!("diagnostics:");
                for diagnostic in &file.diagnostics {
                    let (line, col) = line_index.line_col(diagnostic.span.start());
                    eprintln!(
                        "{line}:{col}\t{:?}\t{}",
                        diagnostic.kind, diagnostic.message
                    );
                }
            }
        }
        Command::Parse(args) => {
            let input = load_input(args.expr, args.path)?;
            let line_index = LineIndex::new(&input.1);

            if input.0 == "<expr>" {
                let parsed = parse_expr(&input.1, SourceId(0));
                println!("{:#?}", parsed.expr);
                if !parsed.diagnostics.is_empty() {
                    eprintln!("diagnostics:");
                    for diagnostic in &parsed.diagnostics {
                        let (line, col) = line_index.line_col(diagnostic.span.start());
                        eprintln!(
                            "{line}:{col}\t{:?}\t{}",
                            diagnostic.kind, diagnostic.message
                        );
                    }
                }
            } else {
                let parsed = parse_file(&input.1, SourceId(0));
                println!("{:#?}", parsed.file);
                if !parsed.diagnostics.is_empty() {
                    eprintln!("diagnostics:");
                    for diagnostic in &parsed.diagnostics {
                        let (line, col) = line_index.line_col(diagnostic.span.start());
                        eprintln!(
                            "{line}:{col}\t{:?}\t{}",
                            diagnostic.kind, diagnostic.message
                        );
                    }
                }
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

fn format_token_text(kind: TokenKind, text: &str) -> String {
    if kind == TokenKind::Eof {
        return "<eof>".to_string();
    }

    text.escape_debug().to_string()
}
