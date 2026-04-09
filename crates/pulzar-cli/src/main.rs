use std::{fs, path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand};
use pulzar_lexer::lex;
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
}

#[derive(Debug, Args)]
struct LexArgs {
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
    let input = match cli.command {
        Command::Lex(args) => match (args.expr, args.path) {
            (Some(expr), None) => ("<expr>".to_string(), expr),
            (None, Some(path)) => {
                let display = path.display().to_string();
                let contents = fs::read_to_string(&path)
                    .map_err(|err| format!("failed to read `{display}`: {err}"))?;
                (display, contents)
            }
            (None, None) => return Err("expected either a source path or --expr".to_string()),
            (Some(_), Some(_)) => {
                return Err("use either a source path or --expr, not both".to_string());
            }
        },
    };

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

    Ok(())
}

fn format_token_text(kind: TokenKind, text: &str) -> String {
    if kind == TokenKind::Eof {
        return "<eof>".to_string();
    }

    text.escape_debug().to_string()
}
