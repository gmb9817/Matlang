//! Semantic analysis crate for workspace, binding, and validation rules.

pub mod binder;
pub mod diagnostics;
pub mod resolution;
pub mod symbols;
pub mod testing;
pub mod workspace;

pub use binder::{
    analyze_compilation_unit, analyze_compilation_unit_with_source_context, AnalysisResult,
};
use matlab_frontend::ast::CompilationUnit;
use matlab_resolver::ResolverContext;
pub use resolution::apply_resolver_context;

pub fn analyze_compilation_unit_with_context(
    unit: &CompilationUnit,
    context: &ResolverContext,
) -> AnalysisResult {
    let mut result =
        analyze_compilation_unit_with_source_context(unit, context.source_file.clone());
    apply_resolver_context(&mut result, context);
    result
}

pub const CRATE_NAME: &str = "matlab-semantics";

pub fn summary() -> &'static str {
    "Owns semantic analysis, workspace rules, binding validation, and semantic diagnostics."
}
