use crate::source::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    UnexpectedCharacter,
    UnterminatedString,
    InvalidNumber,
    UnexpectedToken,
    ExpectedExpression,
    ExpectedStatement,
    InvalidAssignmentTarget,
    MissingDelimiter,
    InvalidLambdaParameterList,
    UnexpectedStatementEnd,
    AssignmentToUndeclaredName,
    DuplicateBinding,
    InvalidReturnContext,
    RuntimeError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn new(kind: DiagnosticKind, span: Span, message: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            message: message.into(),
        }
    }
}
