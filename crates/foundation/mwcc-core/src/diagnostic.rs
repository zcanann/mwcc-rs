//! Diagnostics and the `Compilation` result type. We fail honestly: when a
//! construct is not yet supported, a phase returns a `Diagnostic` rather than
//! emitting wrong bytes.

use crate::span::SourceSpan;

/// A compiler diagnostic.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub message: String,
    pub span: Option<SourceSpan>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Diagnostic { message: message.into(), span: None }
    }

    pub fn at(message: impl Into<String>, span: SourceSpan) -> Self {
        Diagnostic { message: message.into(), span: Some(span) }
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.span {
            Some(span) => write!(formatter, "{} (at {}..{})", self.message, span.start, span.end),
            None => write!(formatter, "{}", self.message),
        }
    }
}

impl std::error::Error for Diagnostic {}

/// Every phase returns this: a value, or a diagnostic explaining the stop.
pub type Compilation<T> = Result<T, Diagnostic>;
