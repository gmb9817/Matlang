//! Frontend diagnostics for lexing, parsing, and early semantic feedback.

use crate::source::SourceSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: SourceSpan,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
        }
    }

    pub fn warning(code: &'static str, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, Severity};
    use crate::source::{SourceFileId, SourcePosition, SourceSpan};

    #[test]
    fn constructs_error_diagnostic() {
        let span = SourceSpan::new(
            SourceFileId(1),
            SourcePosition::new(0, 1, 1),
            SourcePosition::new(3, 1, 4),
        );

        let diagnostic = Diagnostic::error("LEX001", "unexpected token", span);

        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.code, "LEX001");
        assert_eq!(diagnostic.message, "unexpected token");
    }
}
