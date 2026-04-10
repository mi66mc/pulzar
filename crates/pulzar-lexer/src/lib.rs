use pulzar_syntax::{Diagnostic, DiagnosticKind, LexedFile, SourceId, Span, Token, TokenKind};

pub fn lex(source: &str, source_id: SourceId) -> LexedFile {
    Lexer::new(source, source_id).lex_all()
}

struct Lexer<'a> {
    source: &'a str,
    source_id: SourceId,
    offset: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, source_id: SourceId) -> Self {
        Self {
            source,
            source_id,
            offset: 0,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn lex_all(mut self) -> LexedFile {
        while let Some(ch) = self.peek_char() {
            let start = self.offset;
            match ch {
                ' ' | '\t' => self.skip_whitespace(),
                '\r' | '\n' => self.lex_statement_end(start),
                '#' => self.lex_comment(start),
                '/' if self.peek_next_char() == Some('/') => self.lex_comment(start),
                '"' => self.lex_string(start),
                '0'..='9' => self.lex_number(start),
                'a'..='z' | 'A'..='Z' | '_' => self.lex_identifier(start),
                '(' => self.bump_single(TokenKind::LeftParen),
                ')' => self.bump_single(TokenKind::RightParen),
                '{' => self.bump_single(TokenKind::LeftBrace),
                '}' => self.bump_single(TokenKind::RightBrace),
                '[' => self.bump_single(TokenKind::LeftBracket),
                ']' => self.bump_single(TokenKind::RightBracket),
                ',' => self.bump_single(TokenKind::Comma),
                ':' => self.bump_single(TokenKind::Colon),
                '.' => self.bump_single(TokenKind::Dot),
                '@' => self.bump_single(TokenKind::At),
                ';' => self.bump_single(TokenKind::StatementEnd),
                '+' => self.bump_single(TokenKind::Plus),
                '-' => self.bump_single(TokenKind::Minus),
                '%' => self.bump_single(TokenKind::Percent),
                '~' => self.bump_single(TokenKind::Tilde),
                '*' => {
                    self.bump_char();
                    if self.peek_char() == Some('*') {
                        self.bump_char();
                        self.push_token(TokenKind::Power, start, self.offset);
                    } else {
                        self.push_token(TokenKind::Star, start, self.offset);
                    }
                }
                '|' => {
                    self.bump_char();
                    match self.peek_char() {
                        Some('>') => {
                            self.bump_char();
                            self.push_token(TokenKind::PipeForward, start, self.offset);
                        }
                        Some('|') => {
                            self.bump_char();
                            self.push_token(TokenKind::PipePipe, start, self.offset);
                        }
                        _ => self.push_token(TokenKind::Pipe, start, self.offset),
                    }
                }
                '&' => {
                    self.bump_char();
                    if self.peek_char() == Some('&') {
                        self.bump_char();
                        self.push_token(TokenKind::AmpersandAmpersand, start, self.offset);
                    } else {
                        self.push_token(TokenKind::Ampersand, start, self.offset);
                    }
                }
                '=' => {
                    self.bump_char();
                    match self.peek_char() {
                        Some('>') => {
                            self.bump_char();
                            self.push_token(TokenKind::FatArrow, start, self.offset);
                        }
                        Some('=') => {
                            self.bump_char();
                            self.push_token(TokenKind::EqualEqual, start, self.offset);
                        }
                        _ => self.push_token(TokenKind::Assign, start, self.offset),
                    }
                }
                '!' => {
                    self.bump_char();
                    if self.peek_char() == Some('=') {
                        self.bump_char();
                        self.push_token(TokenKind::BangEqual, start, self.offset);
                    } else {
                        self.push_token(TokenKind::Bang, start, self.offset);
                    }
                }
                '>' => {
                    self.bump_char();
                    match self.peek_char() {
                        Some('=') => {
                            self.bump_char();
                            self.push_token(TokenKind::GreaterEqual, start, self.offset);
                        }
                        Some('>') => {
                            self.bump_char();
                            self.push_token(TokenKind::ShiftRight, start, self.offset);
                        }
                        _ => self.push_token(TokenKind::Greater, start, self.offset),
                    }
                }
                '<' => {
                    self.bump_char();
                    match self.peek_char() {
                        Some('=') => {
                            self.bump_char();
                            self.push_token(TokenKind::LessEqual, start, self.offset);
                        }
                        Some('<') => {
                            self.bump_char();
                            self.push_token(TokenKind::ShiftLeft, start, self.offset);
                        }
                        _ => self.push_token(TokenKind::Less, start, self.offset),
                    }
                }
                '^' => self.bump_single(TokenKind::Caret),
                '/' => self.bump_single(TokenKind::Slash),
                _ => {
                    self.bump_char();
                    let span = Span::new(self.source_id, start, self.offset);
                    self.diagnostics.push(Diagnostic::new(
                        DiagnosticKind::UnexpectedCharacter,
                        span,
                        format!("unexpected character `{ch}`"),
                    ));
                }
            }
        }

        self.tokens.push(Token::new(
            TokenKind::Eof,
            Span::new(self.source_id, self.offset, self.offset),
        ));
        LexedFile::new(self.tokens, self.diagnostics)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_char(), Some(' ' | '\t')) {
            self.bump_char();
        }
    }

    fn lex_statement_end(&mut self, start: usize) {
        if self.peek_char() == Some('\r') {
            self.bump_char();
        }
        if self.peek_char() == Some('\n') {
            self.bump_char();
        }
        self.push_token(TokenKind::StatementEnd, start, self.offset);
    }

    fn lex_comment(&mut self, start: usize) {
        if self.peek_char() == Some('/') && self.peek_next_char() == Some('/') {
            self.bump_char();
            self.bump_char();
        } else {
            self.bump_char();
        }

        while let Some(ch) = self.peek_char() {
            if matches!(ch, '\r' | '\n') {
                break;
            }
            self.bump_char();
        }
        self.push_token(TokenKind::Comment, start, self.offset);
    }

    fn lex_string(&mut self, start: usize) {
        self.bump_char();
        let mut terminated = false;

        while let Some(ch) = self.peek_char() {
            match ch {
                '"' => {
                    self.bump_char();
                    terminated = true;
                    break;
                }
                '\\' => {
                    self.bump_char();
                    if self.peek_char().is_some() {
                        self.bump_char();
                    }
                }
                '\r' | '\n' => break,
                _ => {
                    self.bump_char();
                }
            }
        }

        if !terminated {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::UnterminatedString,
                Span::new(self.source_id, start, self.offset),
                "unterminated string literal",
            ));
        }

        self.push_token(TokenKind::String, start, self.offset);
    }

    fn lex_number(&mut self, start: usize) {
        while matches!(self.peek_char(), Some('0'..='9')) {
            self.bump_char();
        }

        let mut kind = TokenKind::Integer;
        if self.peek_char() == Some('.') && matches!(self.peek_next_char(), Some('0'..='9')) {
            kind = TokenKind::Float;
            self.bump_char();

            let frac_start = self.offset;
            while matches!(self.peek_char(), Some('0'..='9')) {
                self.bump_char();
            }

            if self.offset == frac_start {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::InvalidNumber,
                    Span::new(self.source_id, start, self.offset),
                    "expected digits after decimal point",
                ));
            }
        }

        if matches!(self.peek_char(), Some('a'..='z' | 'A'..='Z' | '_')) {
            while matches!(
                self.peek_char(),
                Some('a'..='z' | 'A'..='Z' | '_' | '0'..='9')
            ) {
                self.bump_char();
            }
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::InvalidNumber,
                Span::new(self.source_id, start, self.offset),
                "invalid number literal",
            ));
        }

        self.push_token(kind, start, self.offset);
    }

    fn lex_identifier(&mut self, start: usize) {
        while matches!(
            self.peek_char(),
            Some('a'..='z' | 'A'..='Z' | '_' | '0'..='9')
        ) {
            self.bump_char();
        }

        let kind = match &self.source[start..self.offset] {
            "let" => TokenKind::Let,
            "fn" => TokenKind::Fn,
            "return" => TokenKind::Return,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            _ => TokenKind::Identifier,
        };
        self.push_token(kind, start, self.offset);
    }

    fn bump_single(&mut self, kind: TokenKind) {
        let start = self.offset;
        self.bump_char();
        self.push_token(kind, start, self.offset);
    }

    fn push_token(&mut self, kind: TokenKind, start: usize, end: usize) {
        self.tokens
            .push(Token::new(kind, Span::new(self.source_id, start, end)));
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }

    fn peek_next_char(&self) -> Option<char> {
        let mut chars = self.source[self.offset..].chars();
        chars.next()?;
        chars.next()
    }

    fn bump_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }
}

#[cfg(test)]
mod tests {
    use super::lex;
    use pulzar_syntax::{DiagnosticKind, SourceId, TokenKind};

    fn kinds(source: &str) -> Vec<TokenKind> {
        lex(source, SourceId(0))
            .tokens
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    #[test]
    fn lexes_simple_pipeline() {
        let kinds = kinds("ps |> filter p => @p.cpu > 10");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Identifier,
                TokenKind::PipeForward,
                TokenKind::Identifier,
                TokenKind::Identifier,
                TokenKind::FatArrow,
                TokenKind::At,
                TokenKind::Identifier,
                TokenKind::Dot,
                TokenKind::Identifier,
                TokenKind::Greater,
                TokenKind::Integer,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_newline_and_semicolon_statements() {
        let kinds = kinds("ls; pwd\nps");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Identifier,
                TokenKind::StatementEnd,
                TokenKind::Identifier,
                TokenKind::StatementEnd,
                TokenKind::Identifier,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn distinguishes_integer_float_and_property_access() {
        let kinds = kinds("1 2.5 @p.cpu");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Integer,
                TokenKind::Float,
                TokenKind::At,
                TokenKind::Identifier,
                TokenKind::Dot,
                TokenKind::Identifier,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_fn_list_object_and_types() {
        let kinds =
            kinds("fn isAdult(u) { [1, 2]; map p => { return @{ name: \"a\", age: 18 } } }");
        assert!(kinds.contains(&TokenKind::Fn));
        assert!(kinds.contains(&TokenKind::Return));
        assert!(kinds.contains(&TokenKind::At));
        assert!(kinds.contains(&TokenKind::LeftBracket));
        assert!(kinds.contains(&TokenKind::LeftBrace));
        assert!(kinds.contains(&TokenKind::Colon));
        assert!(kinds.contains(&TokenKind::FatArrow));
    }

    #[test]
    fn lexes_full_operator_set() {
        let kinds = kinds(
            "a ** b ^ c << d >> e | f || g & h && !i == j != k >= l <= m + n - o * p / q % r ~s",
        );
        assert!(kinds.contains(&TokenKind::Power));
        assert!(kinds.contains(&TokenKind::Caret));
        assert!(kinds.contains(&TokenKind::ShiftLeft));
        assert!(kinds.contains(&TokenKind::ShiftRight));
        assert!(kinds.contains(&TokenKind::Pipe));
        assert!(kinds.contains(&TokenKind::PipePipe));
        assert!(kinds.contains(&TokenKind::Ampersand));
        assert!(kinds.contains(&TokenKind::AmpersandAmpersand));
        assert!(kinds.contains(&TokenKind::Bang));
        assert!(kinds.contains(&TokenKind::EqualEqual));
        assert!(kinds.contains(&TokenKind::BangEqual));
        assert!(kinds.contains(&TokenKind::GreaterEqual));
        assert!(kinds.contains(&TokenKind::LessEqual));
        assert!(kinds.contains(&TokenKind::Percent));
        assert!(kinds.contains(&TokenKind::Tilde));
    }

    #[test]
    fn recovers_from_unterminated_string() {
        let file = lex("\"hello", SourceId(0));
        assert_eq!(file.diagnostics.len(), 1);
        assert_eq!(file.diagnostics[0].kind, DiagnosticKind::UnterminatedString);
        assert_eq!(
            file.tokens.last().map(|token| token.kind),
            Some(TokenKind::Eof)
        );
    }

    #[test]
    fn reports_invalid_number_suffix() {
        let file = lex("10abc", SourceId(0));
        assert_eq!(file.diagnostics.len(), 1);
        assert_eq!(file.diagnostics[0].kind, DiagnosticKind::InvalidNumber);
    }

    #[test]
    fn lexes_comments() {
        let kinds = kinds("ls // comment\n# next\nps");
        assert!(kinds.contains(&TokenKind::Comment));
        assert!(kinds.contains(&TokenKind::StatementEnd));
    }
}
