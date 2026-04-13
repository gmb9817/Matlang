//! Semantic diagnostics emitted by the binder and later analysis passes.

use matlab_frontend::source::SourceSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDiagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: SourceSpan,
}

impl SemanticDiagnostic {
    pub fn error(code: &'static str, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
        }
    }
}
