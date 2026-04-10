use pulzar_lexer::lex;
use pulzar_syntax::{
    BinaryOp, Block, Diagnostic, DiagnosticKind, Expr, ExprKind, File, FnBody, LambdaBody,
    LexedFile, ObjectField, Param, SourceId, Span, Stmt, StmtKind, Token, TokenKind, UnaryOp,
};

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub file: File,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct ParsedExpr {
    pub expr: Option<Expr>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_file(source: &str, source_id: SourceId) -> ParsedFile {
    let lexed = lex(source, source_id);
    Parser::new(source, lexed).parse_file()
}

pub fn parse_expr(source: &str, source_id: SourceId) -> ParsedExpr {
    let lexed = lex(source, source_id);
    Parser::new(source, lexed).parse_expr_entry()
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, lexed: LexedFile) -> Self {
        let tokens = lexed
            .tokens
            .into_iter()
            .filter(|token| token.kind != TokenKind::Comment)
            .collect();

        Self {
            source,
            tokens,
            diagnostics: lexed.diagnostics,
            index: 0,
        }
    }

    fn parse_file(mut self) -> ParsedFile {
        let start = self.current_span();
        let mut statements = Vec::new();

        self.skip_statement_ends();
        while !self.at(TokenKind::Eof) {
            statements.push(self.parse_statement());
            self.skip_statement_ends();
        }

        ParsedFile {
            file: File {
                statements,
                span: start.cover(self.current_span()),
            },
            diagnostics: self.diagnostics,
        }
    }

    fn parse_expr_entry(mut self) -> ParsedExpr {
        self.skip_statement_ends();
        let expr = if self.at(TokenKind::Eof) {
            None
        } else {
            Some(self.parse_expression())
        };

        if !self.at(TokenKind::Eof) {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::UnexpectedToken,
                self.current_span(),
                "unexpected token after expression",
            ));
        }

        ParsedExpr {
            expr,
            diagnostics: self.diagnostics,
        }
    }

    fn parse_statement(&mut self) -> Stmt {
        self.skip_statement_ends();
        let start = self.current_span();

        match self.current_kind() {
            TokenKind::Let => self.parse_let_statement(start),
            TokenKind::Fn => self.parse_fn_decl(start),
            TokenKind::Return => self.parse_return_statement(start),
            TokenKind::StatementEnd => {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::UnexpectedStatementEnd,
                    start,
                    "unexpected statement boundary",
                ));
                self.bump();
                Stmt {
                    kind: StmtKind::Expr(self.error_expr(start)),
                    span: start,
                }
            }
            _ => self.parse_expr_or_assign_statement(),
        }
    }

    fn parse_let_statement(&mut self, start: Span) -> Stmt {
        self.bump();
        let name = self.expect_identifier("expected identifier after `let`");
        self.expect(TokenKind::Assign, "expected `=` after let binding");
        let value = self.parse_expression();

        Stmt {
            kind: StmtKind::Let {
                name: name.0,
                value,
            },
            span: start.cover(self.prev_span()),
        }
    }

    fn parse_fn_decl(&mut self, start: Span) -> Stmt {
        self.bump();
        let name = self.expect_identifier("expected function name after `fn`");
        let params = self.parse_parenthesized_params();
        let body = if self.at(TokenKind::LeftBrace) {
            FnBody::Block(self.parse_block())
        } else if self.match_kind(TokenKind::FatArrow) {
            FnBody::Expr(Box::new(self.parse_expression()))
        } else {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::ExpectedStatement,
                self.current_span(),
                "expected function body",
            ));
            FnBody::Expr(Box::new(self.error_expr(self.current_span())))
        };

        Stmt {
            kind: StmtKind::FnDecl {
                name: name.0,
                params,
                body,
            },
            span: start.cover(self.prev_span()),
        }
    }

    fn parse_return_statement(&mut self, start: Span) -> Stmt {
        self.bump();
        let value = if self.is_statement_terminator() {
            None
        } else {
            Some(self.parse_expression())
        };

        Stmt {
            kind: StmtKind::Return { value },
            span: start.cover(self.prev_span()),
        }
    }

    fn parse_expr_or_assign_statement(&mut self) -> Stmt {
        let expr = self.parse_expression();
        let start = expr.span;

        if self.match_kind(TokenKind::Assign) {
            let rhs = self.parse_expression();
            if !matches!(expr.kind, ExprKind::Identifier(_)) {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::InvalidAssignmentTarget,
                    expr.span,
                    "invalid assignment target",
                ));
            }

            Stmt {
                kind: StmtKind::Assign {
                    target: expr,
                    value: rhs,
                },
                span: start.cover(self.prev_span()),
            }
        } else {
            Stmt {
                span: expr.span,
                kind: StmtKind::Expr(expr),
            }
        }
    }

    fn parse_expression(&mut self) -> Expr {
        self.parse_pipeline(true)
    }

    fn parse_pipeline(&mut self, allow_juxtaposition: bool) -> Expr {
        let mut expr = self.parse_lambda(allow_juxtaposition);

        while self.match_kind(TokenKind::PipeForward) {
            let right = self.parse_lambda(true);
            let span = expr.span.cover(right.span);
            expr = Expr {
                kind: ExprKind::Pipeline {
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            };
        }

        expr
    }

    fn parse_lambda(&mut self, allow_juxtaposition: bool) -> Expr {
        if self.is_lambda_start() {
            let start = self.current_span();
            let params = if self.at(TokenKind::Identifier) {
                let (name, span) = self.expect_identifier("expected lambda parameter");
                vec![Param { name, span }]
            } else {
                self.parse_parenthesized_params()
            };

            self.expect(TokenKind::FatArrow, "expected `=>` after lambda parameters");
            let body = if self.at(TokenKind::LeftBrace) {
                LambdaBody::Block(self.parse_block())
            } else {
                LambdaBody::Expr(Box::new(self.parse_pipeline(true)))
            };

            return Expr {
                kind: ExprKind::Lambda { params, body },
                span: start.cover(self.prev_span()),
            };
        }

        self.parse_binary(allow_juxtaposition, 0)
    }

    fn parse_binary(&mut self, allow_juxtaposition: bool, min_prec: u8) -> Expr {
        let mut left = self.parse_prefix(allow_juxtaposition);

        while let Some((op, prec, right_assoc)) = self.current_binary_op() {
            if prec < min_prec {
                break;
            }

            self.bump();
            let next_min = if right_assoc { prec } else { prec + 1 };
            let right = self.parse_binary(allow_juxtaposition, next_min);
            let span = left.span.cover(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }

        left
    }

    fn parse_prefix(&mut self, allow_juxtaposition: bool) -> Expr {
        let start = self.current_span();
        match self.current_kind() {
            TokenKind::Minus => {
                self.bump();
                let expr = self.parse_prefix(allow_juxtaposition);
                Expr {
                    span: start.cover(expr.span),
                    kind: ExprKind::Unary {
                        op: UnaryOp::Negate,
                        expr: Box::new(expr),
                    },
                }
            }
            TokenKind::Bang => {
                self.bump();
                let expr = self.parse_prefix(allow_juxtaposition);
                Expr {
                    span: start.cover(expr.span),
                    kind: ExprKind::Unary {
                        op: UnaryOp::Not,
                        expr: Box::new(expr),
                    },
                }
            }
            TokenKind::Tilde => {
                self.bump();
                let expr = self.parse_prefix(allow_juxtaposition);
                Expr {
                    span: start.cover(expr.span),
                    kind: ExprKind::Unary {
                        op: UnaryOp::BitNot,
                        expr: Box::new(expr),
                    },
                }
            }
            _ => self.parse_postfix(allow_juxtaposition),
        }
    }

    fn parse_postfix(&mut self, allow_juxtaposition: bool) -> Expr {
        let mut expr = self.parse_atom();

        loop {
            if self.at(TokenKind::LeftParen) {
                let args = self.parse_call_args();
                expr = Self::push_call_args(expr, args);
                continue;
            }

            if allow_juxtaposition && self.can_start_application_arg() {
                let arg = self.parse_application_arg();
                expr = Self::push_call_args(expr, vec![arg]);
                continue;
            }

            break;
        }

        expr
    }

    fn parse_atom(&mut self) -> Expr {
        self.skip_statement_ends();
        let start = self.current_span();
        match self.current_kind() {
            TokenKind::Identifier => self.parse_identifier_or_path(),
            TokenKind::Integer => {
                let text = self.current_text().to_string();
                self.bump();
                Expr {
                    kind: ExprKind::Integer(text),
                    span: start,
                }
            }
            TokenKind::Float => {
                let text = self.current_text().to_string();
                self.bump();
                Expr {
                    kind: ExprKind::Float(text),
                    span: start,
                }
            }
            TokenKind::String => {
                let text = self.current_text().to_string();
                self.bump();
                Expr {
                    kind: ExprKind::String(text),
                    span: start,
                }
            }
            TokenKind::True => {
                self.bump();
                Expr {
                    kind: ExprKind::Bool(true),
                    span: start,
                }
            }
            TokenKind::False => {
                self.bump();
                Expr {
                    kind: ExprKind::Bool(false),
                    span: start,
                }
            }
            TokenKind::LeftParen => self.parse_grouped(),
            TokenKind::LeftBracket => self.parse_list(),
            TokenKind::At => self.parse_at_expr(),
            _ => {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::ExpectedExpression,
                    start,
                    "expected expression",
                ));
                self.bump();
                self.error_expr(start)
            }
        }
    }

    fn parse_identifier_or_path(&mut self) -> Expr {
        let start = self.current_span();
        let mut segments = vec![self.current_text().to_string()];
        self.bump();

        while self.at(TokenKind::Dot) && self.peek_kind(1) == TokenKind::Identifier {
            self.bump();
            segments.push(self.current_text().to_string());
            self.bump();
        }

        let span = start.cover(self.prev_span());
        let kind = if segments.len() == 1 {
            ExprKind::Identifier(segments.remove(0))
        } else {
            ExprKind::Path(segments)
        };

        Expr { kind, span }
    }

    fn parse_grouped(&mut self) -> Expr {
        let start = self.expect(TokenKind::LeftParen, "expected `(`");
        self.skip_statement_ends();
        let expr = self.parse_expression();
        self.skip_statement_ends();
        let end = self.expect(TokenKind::RightParen, "expected `)`");
        Expr {
            kind: ExprKind::Grouped(Box::new(expr)),
            span: start.cover(end),
        }
    }

    fn parse_list(&mut self) -> Expr {
        let start = self.expect(TokenKind::LeftBracket, "expected `[`");
        let mut items = Vec::new();

        self.skip_statement_ends();
        while !self.at(TokenKind::RightBracket) && !self.at(TokenKind::Eof) {
            items.push(self.parse_expression());
            self.skip_statement_ends();
            if self.match_kind(TokenKind::Comma) {
                self.skip_statement_ends();
            } else if !self.at(TokenKind::RightBracket) {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::MissingDelimiter,
                    self.current_span(),
                    "expected `,` or `]` in list literal",
                ));
                break;
            }
        }

        let end = self.expect(TokenKind::RightBracket, "expected `]`");
        Expr {
            kind: ExprKind::List(items),
            span: start.cover(end),
        }
    }

    fn parse_at_expr(&mut self) -> Expr {
        let start = self.expect(TokenKind::At, "expected `@`");
        if self.at(TokenKind::LeftBrace) {
            return self.parse_object_literal(start);
        }

        let object = self.parse_atom_without_path_chain();
        let mut fields = Vec::new();

        while self.match_kind(TokenKind::Dot) {
            if self.at(TokenKind::Identifier) {
                fields.push(self.current_text().to_string());
                self.bump();
            } else {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::UnexpectedToken,
                    self.current_span(),
                    "expected field name after `.`",
                ));
                break;
            }
        }

        if fields.is_empty() {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::ExpectedExpression,
                start,
                "expected member chain after `@`",
            ));
        }

        Expr {
            kind: ExprKind::Member {
                object: Box::new(object),
                fields,
            },
            span: start.cover(self.prev_span()),
        }
    }

    fn parse_atom_without_path_chain(&mut self) -> Expr {
        self.skip_statement_ends();
        let start = self.current_span();
        match self.current_kind() {
            TokenKind::Identifier => {
                let name = self.current_text().to_string();
                self.bump();
                Expr {
                    kind: ExprKind::Identifier(name),
                    span: start,
                }
            }
            TokenKind::LeftParen => self.parse_grouped(),
            TokenKind::Integer => {
                let text = self.current_text().to_string();
                self.bump();
                Expr {
                    kind: ExprKind::Integer(text),
                    span: start,
                }
            }
            TokenKind::Float => {
                let text = self.current_text().to_string();
                self.bump();
                Expr {
                    kind: ExprKind::Float(text),
                    span: start,
                }
            }
            TokenKind::String => {
                let text = self.current_text().to_string();
                self.bump();
                Expr {
                    kind: ExprKind::String(text),
                    span: start,
                }
            }
            TokenKind::True => {
                self.bump();
                Expr {
                    kind: ExprKind::Bool(true),
                    span: start,
                }
            }
            TokenKind::False => {
                self.bump();
                Expr {
                    kind: ExprKind::Bool(false),
                    span: start,
                }
            }
            TokenKind::LeftBracket => self.parse_list(),
            TokenKind::At => self.parse_at_expr(),
            _ => {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::ExpectedExpression,
                    start,
                    "expected expression after `@`",
                ));
                self.bump();
                self.error_expr(start)
            }
        }
    }

    fn parse_object_literal(&mut self, at_span: Span) -> Expr {
        let _ = self.expect(TokenKind::LeftBrace, "expected `{` after `@`");
        let mut fields = Vec::new();

        self.skip_statement_ends();
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::Eof) {
            let field_start = self.current_span();
            let (name, _) = self.expect_identifier("expected object field name");
            self.skip_statement_ends();
            self.expect(TokenKind::Colon, "expected `:` after object field name");
            self.skip_statement_ends();
            let value = self.parse_expression();
            fields.push(ObjectField {
                name,
                span: field_start.cover(value.span),
                value,
            });
            self.skip_statement_ends();

            if self.match_kind(TokenKind::Comma) {
                self.skip_statement_ends();
            } else if !self.at(TokenKind::RightBrace) {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::MissingDelimiter,
                    self.current_span(),
                    "expected `,` or `}` in object literal",
                ));
                break;
            }
        }

        let end = self.expect(TokenKind::RightBrace, "expected `}`");
        Expr {
            kind: ExprKind::Object(fields),
            span: at_span.cover(end),
        }
    }

    fn parse_block(&mut self) -> Block {
        let start = self.expect(TokenKind::LeftBrace, "expected `{`");
        let mut statements = Vec::new();

        self.skip_statement_ends();
        while !self.at(TokenKind::RightBrace) && !self.at(TokenKind::Eof) {
            statements.push(self.parse_statement());
            self.skip_statement_ends();
        }

        let end = self.expect(TokenKind::RightBrace, "expected `}`");
        Block {
            statements,
            span: start.cover(end),
        }
    }

    fn parse_call_args(&mut self) -> Vec<Expr> {
        self.expect(TokenKind::LeftParen, "expected `(`");
        let mut args = Vec::new();

        self.skip_statement_ends();
        while !self.at(TokenKind::RightParen) && !self.at(TokenKind::Eof) {
            args.push(self.parse_expression());
            self.skip_statement_ends();
            if self.match_kind(TokenKind::Comma) {
                self.skip_statement_ends();
            } else if !self.at(TokenKind::RightParen) {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::MissingDelimiter,
                    self.current_span(),
                    "expected `,` or `)` in argument list",
                ));
                break;
            }
        }

        self.expect(TokenKind::RightParen, "expected `)`");
        args
    }

    fn parse_application_arg(&mut self) -> Expr {
        self.parse_lambda(false)
    }

    fn parse_parenthesized_params(&mut self) -> Vec<Param> {
        self.expect(TokenKind::LeftParen, "expected `(`");
        let mut params = Vec::new();
        self.skip_statement_ends();

        while !self.at(TokenKind::RightParen) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Identifier) {
                let span = self.current_span();
                let name = self.current_text().to_string();
                self.bump();
                params.push(Param { name, span });
            } else {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::InvalidLambdaParameterList,
                    self.current_span(),
                    "expected identifier in parameter list",
                ));
                self.bump();
            }

            self.skip_statement_ends();
            if self.match_kind(TokenKind::Comma) {
                self.skip_statement_ends();
            } else if !self.at(TokenKind::RightParen) {
                self.diagnostics.push(Diagnostic::new(
                    DiagnosticKind::MissingDelimiter,
                    self.current_span(),
                    "expected `,` or `)` in parameter list",
                ));
                break;
            }
        }

        self.expect(TokenKind::RightParen, "expected `)`");
        params
    }

    fn is_lambda_start(&self) -> bool {
        if self.at(TokenKind::Identifier) && self.peek_kind(1) == TokenKind::FatArrow {
            return true;
        }

        if !self.at(TokenKind::LeftParen) {
            return false;
        }

        let mut index = self.index + 1;
        let mut expect_ident = true;

        while let Some(token) = self.tokens.get(index) {
            match token.kind {
                TokenKind::StatementEnd => index += 1,
                TokenKind::RightParen => {
                    return self
                        .tokens
                        .get(index + 1)
                        .map(|token| token.kind == TokenKind::FatArrow)
                        .unwrap_or(false);
                }
                TokenKind::Identifier if expect_ident => {
                    expect_ident = false;
                    index += 1;
                }
                TokenKind::Comma if !expect_ident => {
                    expect_ident = true;
                    index += 1;
                }
                _ => return false,
            }
        }

        false
    }

    fn can_start_application_arg(&self) -> bool {
        matches!(
            self.current_kind(),
            TokenKind::Identifier
                | TokenKind::Integer
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::True
                | TokenKind::False
                | TokenKind::LeftParen
                | TokenKind::LeftBracket
                | TokenKind::At
        ) && !self.at(TokenKind::StatementEnd)
    }

    fn is_statement_terminator(&self) -> bool {
        matches!(
            self.current_kind(),
            TokenKind::StatementEnd | TokenKind::RightBrace | TokenKind::Eof
        )
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, u8, bool)> {
        let item = match self.current_kind() {
            TokenKind::Star => (BinaryOp::Multiply, 11, false),
            TokenKind::Slash => (BinaryOp::Divide, 11, false),
            TokenKind::Percent => (BinaryOp::Modulo, 11, false),
            TokenKind::Power => (BinaryOp::Power, 12, true),
            TokenKind::Plus => (BinaryOp::Add, 10, false),
            TokenKind::Minus => (BinaryOp::Subtract, 10, false),
            TokenKind::ShiftLeft => (BinaryOp::ShiftLeft, 9, false),
            TokenKind::ShiftRight => (BinaryOp::ShiftRight, 9, false),
            TokenKind::Less => (BinaryOp::Less, 8, false),
            TokenKind::LessEqual => (BinaryOp::LessEqual, 8, false),
            TokenKind::Greater => (BinaryOp::Greater, 8, false),
            TokenKind::GreaterEqual => (BinaryOp::GreaterEqual, 8, false),
            TokenKind::EqualEqual => (BinaryOp::Equal, 7, false),
            TokenKind::BangEqual => (BinaryOp::NotEqual, 7, false),
            TokenKind::Ampersand => (BinaryOp::BitAnd, 6, false),
            TokenKind::Caret => (BinaryOp::BitXor, 5, false),
            TokenKind::Pipe => (BinaryOp::BitOr, 4, false),
            TokenKind::AmpersandAmpersand => (BinaryOp::LogicalAnd, 3, false),
            TokenKind::PipePipe => (BinaryOp::LogicalOr, 2, false),
            _ => return None,
        };

        Some(item)
    }

    fn push_call_args(callee: Expr, mut args: Vec<Expr>) -> Expr {
        let arg_end = args.last().map(|expr| expr.span).unwrap_or(callee.span);
        let span = callee.span.cover(arg_end);
        match callee {
            Expr {
                kind:
                    ExprKind::Call {
                        callee: inner_callee,
                        args: mut existing_args,
                    },
                ..
            } => {
                existing_args.append(&mut args);
                Expr {
                    kind: ExprKind::Call {
                        callee: inner_callee,
                        args: existing_args,
                    },
                    span,
                }
            }
            other => Expr {
                kind: ExprKind::Call {
                    callee: Box::new(other),
                    args,
                },
                span,
            },
        }
    }

    fn expect_identifier(&mut self, message: &str) -> (String, Span) {
        if self.at(TokenKind::Identifier) {
            let span = self.current_span();
            let name = self.current_text().to_string();
            self.bump();
            (name, span)
        } else {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::UnexpectedToken,
                self.current_span(),
                message,
            ));
            let span = self.current_span();
            if !self.at(TokenKind::Eof) {
                self.bump();
            }
            ("<error>".to_string(), span)
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Span {
        if self.at(kind) {
            let span = self.current_span();
            self.bump();
            span
        } else {
            let span = self.current_span();
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::MissingDelimiter,
                span,
                message,
            ));
            span
        }
    }

    fn skip_statement_ends(&mut self) {
        while self.at(TokenKind::StatementEnd) {
            self.bump();
        }
    }

    fn match_kind(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current_kind() == kind
    }

    fn current_kind(&self) -> TokenKind {
        self.tokens
            .get(self.index)
            .map(|token| token.kind)
            .unwrap_or(TokenKind::Eof)
    }

    fn peek_kind(&self, offset: usize) -> TokenKind {
        self.tokens
            .get(self.index + offset)
            .map(|token| token.kind)
            .unwrap_or(TokenKind::Eof)
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.index)
            .map(|token| token.span)
            .unwrap_or_else(|| self.prev_span())
    }

    fn prev_span(&self) -> Span {
        self.tokens
            .get(self.index.saturating_sub(1))
            .map(|token| token.span)
            .unwrap_or(Span::default())
    }

    fn current_text(&self) -> &str {
        let span = self.current_span();
        &self.source[span.start()..span.end()]
    }

    fn bump(&mut self) {
        if !self.at(TokenKind::Eof) {
            self.index += 1;
        }
    }

    fn error_expr(&self, span: Span) -> Expr {
        Expr {
            kind: ExprKind::Error,
            span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_expr, parse_file};
    use pulzar_syntax::{ExprKind, SourceId, StmtKind};

    #[test]
    fn parses_pipeline_call_chain() {
        let parsed = parse_file("let users = cat users.json |> decode", SourceId(0));
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert_eq!(parsed.file.statements.len(), 1);
        match &parsed.file.statements[0].kind {
            StmtKind::Let { value, .. } => assert!(matches!(value.kind, ExprKind::Pipeline { .. })),
            other => panic!("unexpected statement: {other:?}"),
        }
    }

    #[test]
    fn parses_lambda_with_return_block() {
        let parsed = parse_expr("u => { return @u.age >= 18 }", SourceId(0));
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert!(matches!(
            parsed.expr.expect("expr").kind,
            ExprKind::Lambda { .. }
        ));
    }

    #[test]
    fn parses_function_expr_body() {
        let parsed = parse_file("fn score(u) => @u.points * 2", SourceId(0));
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert!(matches!(
            parsed.file.statements[0].kind,
            StmtKind::FnDecl { .. }
        ));
    }

    #[test]
    fn parses_member_chain_and_object_literal() {
        let member = parse_expr("@user.profile.name", SourceId(0));
        assert!(member.diagnostics.is_empty(), "{:?}", member.diagnostics);
        assert!(matches!(
            member.expr.expect("expr").kind,
            ExprKind::Member { .. }
        ));

        let object = parse_expr("@{name: \"a\", age: 18}", SourceId(0));
        assert!(object.diagnostics.is_empty(), "{:?}", object.diagnostics);
        assert!(matches!(
            object.expr.expect("expr").kind,
            ExprKind::Object(_)
        ));
    }

    #[test]
    fn rejects_assignment_expression() {
        let parsed = parse_file("foo(x = 10)", SourceId(0));
        assert!(!parsed.diagnostics.is_empty());
    }

    #[test]
    fn diagnoses_invalid_assignment_target() {
        let parsed = parse_file("a + b = 1", SourceId(0));
        assert!(!parsed.diagnostics.is_empty());
    }

    #[test]
    fn ignores_statement_end_inside_object_and_list() {
        let object = parse_expr("@{\nname: \"a\",\nage: 18\n}", SourceId(0));
        assert!(object.diagnostics.is_empty(), "{:?}", object.diagnostics);

        let list = parse_expr("[\n1,\n2\n]", SourceId(0));
        assert!(list.diagnostics.is_empty(), "{:?}", list.diagnostics);
    }
}
