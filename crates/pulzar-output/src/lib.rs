use std::collections::BTreeSet;
use std::fmt::Debug;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

use pulzar_runtime::Value;
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

    pub fn print_runtime_value(&self, value: &Value) {
        println!("{}", self.format_runtime_value(value));
    }

    pub fn format_runtime_value(&self, value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            Value::List(items) => format_list(items),
            Value::Object(fields) => {
                let rows = fields
                    .iter()
                    .map(|(key, value)| vec![key.clone(), compact_value(value)])
                    .collect::<Vec<_>>();
                render_table(&["key".to_string(), "value".to_string()], &rows)
            }
            other => other.to_string(),
        }
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

fn format_list(items: &[Value]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }

    if items.iter().all(|item| matches!(item, Value::Object(_))) {
        return format_object_rows(items);
    }

    let rows = items
        .iter()
        .enumerate()
        .map(|(idx, value)| vec![idx.to_string(), compact_value(value)])
        .collect::<Vec<_>>();
    render_table(&["#".to_string(), "value".to_string()], &rows)
}

fn format_object_rows(items: &[Value]) -> String {
    let mut columns = BTreeSet::new();
    for item in items {
        let Value::Object(object) = item else {
            return format_list(items);
        };
        for key in object.keys() {
            columns.insert(key.clone());
        }
    }

    let headers = columns.into_iter().collect::<Vec<_>>();
    let rows = items
        .iter()
        .map(|item| {
            let Value::Object(object) = item else {
                unreachable!("checked all items are objects");
            };
            headers
                .iter()
                .map(|key| {
                    object
                        .get(key)
                        .map(compact_value)
                        .unwrap_or_else(String::new)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    render_table(&headers, &rows)
}

fn render_table(headers: &[String], rows: &[Vec<String>]) -> String {
    let mut widths = headers
        .iter()
        .map(|header| header.chars().count())
        .collect::<Vec<_>>();
    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            if idx >= widths.len() {
                widths.push(0);
            }
            widths[idx] = widths[idx].max(cell.chars().count());
        }
    }

    let mut out = String::new();
    out.push_str(&render_row(headers, &widths));
    out.push('\n');
    out.push_str(&render_separator(&widths));
    for row in rows {
        out.push('\n');
        out.push_str(&render_row(row, &widths));
    }
    out
}

fn render_row(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(idx, cell)| format!("{:<width$}", cell, width = widths[idx]))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn render_separator(widths: &[usize]) -> String {
    widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>()
        .join("-+-")
}

fn compact_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(text) => sanitize_cell(text),
        Value::List(items) => format!("[{} item(s)]", items.len()),
        Value::Object(fields) => {
            if fields.is_empty() {
                "{}".to_string()
            } else {
                format!("{{{} field(s)}}", fields.len())
            }
        }
        Value::Function(_) => "<function>".to_string(),
    }
}

fn sanitize_cell(text: &str) -> String {
    const MAX_CELL_WIDTH: usize = 60;

    let mut text = text.replace("\r\n", "\\n");
    text = text.replace('\n', "\\n");
    text = text.replace('\t', "\\t");

    let char_count = text.chars().count();
    if char_count <= MAX_CELL_WIDTH {
        return text;
    }

    let truncated = text.chars().take(MAX_CELL_WIDTH - 1).collect::<String>();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::{ColorMode, Reporter, StatusKind};
    use pulzar_runtime::Value;
    use pulzar_syntax::{Diagnostic, DiagnosticKind, SourceId, Span};
    use std::collections::BTreeMap;

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

    #[test]
    fn formats_object_as_key_value_table() {
        let reporter = Reporter::new(ColorMode::Never);
        let mut object = BTreeMap::new();
        object.insert("name".to_string(), Value::String("alice".to_string()));
        object.insert("age".to_string(), Value::Int(18));

        let text = reporter.format_runtime_value(&Value::Object(object));
        assert!(text.contains("key"));
        assert!(text.contains("value"));
        assert!(text.contains("name"));
        assert!(text.contains("alice"));
    }

    #[test]
    fn formats_scalar_list_as_indexed_table() {
        let reporter = Reporter::new(ColorMode::Never);
        let text = reporter.format_runtime_value(&Value::List(vec![
            Value::String("a".to_string()),
            Value::Int(2),
        ]));

        assert!(text.contains("#"));
        assert!(text.contains("value"));
        assert!(text.contains("0"));
        assert!(text.contains("a"));
    }

    #[test]
    fn formats_object_list_as_table() {
        let reporter = Reporter::new(ColorMode::Never);
        let mut left = BTreeMap::new();
        left.insert("name".to_string(), Value::String("alice".to_string()));
        left.insert("age".to_string(), Value::Int(18));

        let mut right = BTreeMap::new();
        right.insert("name".to_string(), Value::String("bob".to_string()));
        right.insert("age".to_string(), Value::Int(22));

        let text = reporter.format_runtime_value(&Value::List(vec![
            Value::Object(left),
            Value::Object(right),
        ]));

        assert!(text.contains("name"));
        assert!(text.contains("age"));
        assert!(text.contains("alice"));
        assert!(text.contains("bob"));
    }
}
