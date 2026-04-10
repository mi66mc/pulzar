use crate::{Diagnostic, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Identifier,
    Integer,
    Float,
    String,
    Comment,
    StatementEnd,
    Let,
    Fn,
    Return,
    True,
    False,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Comma,
    Colon,
    Dot,
    At,
    Dollar,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Power,
    Assign,
    FatArrow,
    PipeForward,
    Greater,
    Less,
    GreaterEqual,
    LessEqual,
    EqualEqual,
    BangEqual,
    Bang,
    AmpersandAmpersand,
    PipePipe,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    ShiftLeft,
    ShiftRight,
    Eof,
}

impl TokenKind {
    pub const fn is_trivia(self) -> bool {
        matches!(self, Self::Comment)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone)]
pub struct LexedFile {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

impl LexedFile {
    pub fn new(tokens: Vec<Token>, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            tokens,
            diagnostics,
        }
    }
}
