use std::fmt::Debug;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

use pulzar_syntax::{Diagnostic, LineIndex, Token, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Success,
    Failure,
    Info,
}

pub struct Reporter {
    color_enabled: bool,
}

impl Reporter {
    pub fn new(color_mode: ColorMode) -> Self {
        let color_enabled = match color_mode {
            ColorMode::Auto => io::stdout().is_terminal() && io::stderr().is_terminal(),
            ColorMode::Always => true,
            ColorMode::Never => false,
        };

        Self { color_enabled }
    }

    pub fn print_status(&self, kind: StatusKind, message: &str) {
        let label = match kind {
            StatusKind::Success => self.paint("ok", Color::Green, true),
            StatusKind::Failure => self.paint("error", Color::Red, true),
            StatusKind::Info => self.paint("info", Color::Blue, true),
        };
        println!("{label} {message}");
    }

    pub fn print_lex_tokens(&self, source: &str, line_index: &LineIndex, tokens: &[Token]) {
        for token in tokens {
            let (line, col) = line_index.line_col(token.span.start());
            let text = &source[token.span.start()..token.span.end()];
            let token_kind = self.paint(&format!("{:?}", token.kind), Color::Cyan, true);
            println!(
                "{line}:{col}\t{token_kind}\t{}",
                format_token_text(token.kind, text)
            );
        }
    }

    pub fn print_parse_summary(&self, statement_count: usize, debug_ast: bool) {
        let message = if debug_ast {
            format!("parsed {statement_count} statement(s), debug AST enabled")
        } else {
            format!("parsed {statement_count} statement(s)")
        };
        self.print_status(StatusKind::Success, &message);
    }

    pub fn print_check_summary(
        &self,
        statement_count: usize,
        parse_diagnostics: usize,
        semantic_diagnostics: usize,
    ) {
        if parse_diagnostics == 0 && semantic_diagnostics == 0 {
            self.print_status(
                StatusKind::Success,
                &format!("check passed for {statement_count} statement(s)"),
            );
        } else {
            self.print_status(
                StatusKind::Failure,
                &format!(
                    "check failed for {statement_count} statement(s): {parse_diagnostics} parse diagnostic(s), {semantic_diagnostics} semantic diagnostic(s)"
                ),
            );
        }
    }

    pub fn print_debug_ast<T: Debug>(&self, label: &str, ast: &T) {
        let header = self.paint(label, Color::Magenta, true);
        println!("{header}");
        println!("{ast:#?}");
    }

    pub fn print_diagnostic_section(
        &self,
        title: &str,
        source_name: &str,
        source: &str,
        diagnostics: &[Diagnostic],
    ) {
        if diagnostics.is_empty() {
            return;
        }

        let section = self.paint(title, Color::Yellow, true);
        eprintln!("{section}");
        let line_index = LineIndex::new(source);
        for diagnostic in diagnostics {
            self.print_diagnostic(source_name, source, &line_index, diagnostic);
        }
    }

    pub fn print_shell_message(&self, message: &str) {
        let prefix = self.paint("pulzar", Color::Blue, true);
        println!("{prefix} {message}");
    }

    pub fn print_prompt(&self, cwd: &Path) -> io::Result<()> {
        let prompt = self.paint(&format!("{} >", cwd.display()), Color::Blue, true);
        print!("{prompt} ");
        io::stdout().flush()
    }

    pub fn print_value(&self, value: &str) {
        println!("{value}");
    }

    fn print_diagnostic(
        &self,
        source_name: &str,
        source: &str,
        line_index: &LineIndex,
        diagnostic: &Diagnostic,
    ) {
        let (line, col) = line_index.line_col(diagnostic.span.start());
        let line_text = source_line(source, line).unwrap_or_default();
        let gutter_width = line.to_string().len().max(1);
        let line_no = line.to_string();
        let severity = self.paint("error", Color::Red, true);
        let kind = self.paint(&format!("{:?}", diagnostic.kind), Color::Yellow, false);
        let arrow = self.paint("-->", Color::Blue, true);
        let pipe = self.paint("|", Color::Blue, true);
        let underline_len = underline_len(&line_text, col, diagnostic.span.range.len());
        let underline = format!(
            "{}{}",
            " ".repeat(col.saturating_sub(1)),
            self.paint(&"^".repeat(underline_len), Color::Red, true)
        );

        eprintln!("{severity}[{kind}]: {}", diagnostic.message);
        eprintln!(
            "{:>width$} {arrow} {source_name}:{line}:{col}",
            "",
            width = gutter_width
        );
        eprintln!("{:>width$} {pipe}", "", width = gutter_width);
        eprintln!("{line_no:>width$} {pipe} {line_text}", width = gutter_width);
        eprintln!("{:>width$} {pipe} {underline}", "", width = gutter_width);
        eprintln!();
    }

    fn paint(&self, text: &str, color: Color, bold: bool) -> String {
        if !self.color_enabled {
            return text.to_string();
        }

        let color_code = match color {
            Color::Red => 31,
            Color::Green => 32,
            Color::Yellow => 33,
            Color::Blue => 34,
            Color::Magenta => 35,
            Color::Cyan => 36,
        };

        if bold {
            format!("\x1b[1;{color_code}m{text}\x1b[0m")
        } else {
            format!("\x1b[{color_code}m{text}\x1b[0m")
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Color {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
}

fn source_line(source: &str, line_number: usize) -> Option<String> {
    source
        .lines()
        .nth(line_number.saturating_sub(1))
        .map(|line| line.trim_end_matches('\r').to_string())
}

fn underline_len(line_text: &str, column: usize, span_len: usize) -> usize {
    let available = line_text
        .chars()
        .count()
        .saturating_sub(column.saturating_sub(1));
    span_len.max(1).min(available.max(1))
}

fn format_token_text(kind: TokenKind, text: &str) -> String {
    if kind == TokenKind::Eof {
        return "<eof>".to_string();
    }

    text.escape_debug().to_string()
}

#[cfg(test)]
mod tests {
    use super::{ColorMode, Reporter, StatusKind};
    use pulzar_syntax::{Diagnostic, DiagnosticKind, SourceId, Span};

    #[test]
    fn reporter_constructs_without_color() {
        let reporter = Reporter::new(ColorMode::Never);
        reporter.print_status(StatusKind::Info, "hello");
    }

    #[test]
    fn diagnostic_snippet_uses_minimum_underline() {
        let reporter = Reporter::new(ColorMode::Never);
        let diag = Diagnostic::new(
            DiagnosticKind::UnexpectedToken,
            Span::new(SourceId(0), 3, 3),
            "problem",
        );
        reporter.print_diagnostic_section("diagnostics", "<expr>", "abc", &[diag]);
    }
}
