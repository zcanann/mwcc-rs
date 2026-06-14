//! Source positions and spans.

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
