//! Shared vocabulary for the compiler: diagnostics and source positions.
//!
//! This crate holds no pipeline logic — only types every phase agrees on.

/// A byte offset into a source file.
pub type SourcePosition = usize;

/// A half-open span of source text `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl SourceSpan {
    pub fn new(start: SourcePosition, end: SourcePosition) -> Self {
        SourceSpan { start, end }
    }
}

/// A compiler diagnostic. We fail honestly: when a construct is not yet
/// supported, a phase returns a `Diagnostic` rather than emitting wrong bytes.
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
