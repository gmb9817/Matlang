//! Semantic analysis crate for workspace, binding, and validation rules.

pub mod binder;
pub mod diagnostics;
pub mod resolution;
pub mod symbols;
pub mod testing;
pub mod workspace;

use std::collections::HashMap;

pub use binder::{
    analyze_compilation_unit, analyze_compilation_unit_with_source_context, builtin_function_names,
    is_builtin_function_name, AnalysisResult,
};
use matlab_frontend::{
    ast::{CompilationUnit, Expression, ExpressionKind, FunctionHandleTarget, IndexArgument},
    source::SourceSpan,
};
use matlab_resolver::{resolve_class_definition, ResolverContext};
pub use resolution::apply_resolver_context;
use symbols::{
    FinalReferenceResolution, ReferenceRole, ResolvedReference, SymbolKind, SymbolReference,
};

pub fn analyze_compilation_unit_with_context(
    unit: &CompilationUnit,
    context: &ResolverContext,
) -> AnalysisResult {
    let mut result =
        analyze_compilation_unit_with_source_context(unit, context.source_file.clone());
    apply_resolver_context(&mut result, context);
    for class in &mut result.classes {
        if let Some(superclass_name) = &class.superclass_name {
            if !superclass_name.eq_ignore_ascii_case("handle") {
                class.superclass_path = resolve_class_definition(superclass_name, context)
                    .map(|resolved| resolved.path);
            }
        }
    }
    validate_class_property_defaults(&mut result);
    result
}

fn validate_class_property_defaults(result: &mut AnalysisResult) {
    let references = result
        .references
        .iter()
        .map(|reference| ((reference.span, reference.role), reference))
        .collect::<HashMap<_, _>>();
    let resolved = result
        .resolved_references
        .iter()
        .map(|reference| ((reference.span, reference.role), reference))
        .collect::<HashMap<_, _>>();
    let mut diagnostics = Vec::new();

    for class in &result.classes {
        for property in &class.properties {
            if let Some(default) = &property.default {
                validate_class_property_default_expression(
                    default,
                    &property.name,
                    &references,
                    &resolved,
                    &mut diagnostics,
                );
            }
        }
    }

    result.diagnostics.extend(diagnostics);
}

fn validate_class_property_default_expression(
    expression: &Expression,
    property_name: &str,
    references: &HashMap<(SourceSpan, ReferenceRole), &SymbolReference>,
    resolved: &HashMap<(SourceSpan, ReferenceRole), &ResolvedReference>,
    diagnostics: &mut Vec<crate::diagnostics::SemanticDiagnostic>,
) {
    match &expression.kind {
        ExpressionKind::NumberLiteral(_)
        | ExpressionKind::StringLiteral(_)
        | ExpressionKind::CharLiteral(_)
        | ExpressionKind::EndKeyword => {}
        ExpressionKind::Identifier(_) => validate_property_default_reference(
            expression.span,
            ReferenceRole::Value,
            property_name,
            references,
            resolved,
            diagnostics,
        ),
        ExpressionKind::MatrixLiteral(rows) | ExpressionKind::CellLiteral(rows) => {
            for row in rows {
                for expression in row {
                    validate_class_property_default_expression(
                        expression,
                        property_name,
                        references,
                        resolved,
                        diagnostics,
                    );
                }
            }
        }
        ExpressionKind::FunctionHandle(target) => match target {
            FunctionHandleTarget::Name(name) => validate_property_default_reference(
                name.span,
                ReferenceRole::FunctionHandleTarget,
                property_name,
                references,
                resolved,
                diagnostics,
            ),
            FunctionHandleTarget::Expression(expression) => {
                validate_class_property_default_expression(
                    expression,
                    property_name,
                    references,
                    resolved,
                    diagnostics,
                )
            }
        },
        ExpressionKind::Unary { rhs, .. } => validate_class_property_default_expression(
            rhs,
            property_name,
            references,
            resolved,
            diagnostics,
        ),
        ExpressionKind::Binary { lhs, rhs, .. } => {
            validate_class_property_default_expression(
                lhs,
                property_name,
                references,
                resolved,
                diagnostics,
            );
            validate_class_property_default_expression(
                rhs,
                property_name,
                references,
                resolved,
                diagnostics,
            );
        }
        ExpressionKind::Range { start, step, end } => {
            validate_class_property_default_expression(
                start,
                property_name,
                references,
                resolved,
                diagnostics,
            );
            if let Some(step) = step {
                validate_class_property_default_expression(
                    step,
                    property_name,
                    references,
                    resolved,
                    diagnostics,
                );
            }
            validate_class_property_default_expression(
                end,
                property_name,
                references,
                resolved,
                diagnostics,
            );
        }
        ExpressionKind::ParenApply { target, indices } => {
            if references.contains_key(&(target.span, ReferenceRole::CallTarget)) {
                validate_property_default_reference(
                    target.span,
                    ReferenceRole::CallTarget,
                    property_name,
                    references,
                    resolved,
                    diagnostics,
                );
            } else {
                validate_class_property_default_expression(
                    target,
                    property_name,
                    references,
                    resolved,
                    diagnostics,
                );
            }
            validate_index_arguments(indices, property_name, references, resolved, diagnostics);
        }
        ExpressionKind::CellIndex { target, indices } => {
            validate_class_property_default_expression(
                target,
                property_name,
                references,
                resolved,
                diagnostics,
            );
            validate_index_arguments(indices, property_name, references, resolved, diagnostics);
        }
        ExpressionKind::FieldAccess { target, .. } => validate_class_property_default_expression(
            target,
            property_name,
            references,
            resolved,
            diagnostics,
        ),
        ExpressionKind::AnonymousFunction { .. } => {}
    }
}

fn validate_index_arguments(
    indices: &[IndexArgument],
    property_name: &str,
    references: &HashMap<(SourceSpan, ReferenceRole), &SymbolReference>,
    resolved: &HashMap<(SourceSpan, ReferenceRole), &ResolvedReference>,
    diagnostics: &mut Vec<crate::diagnostics::SemanticDiagnostic>,
) {
    for argument in indices {
        if let IndexArgument::Expression(expression) = argument {
            validate_class_property_default_expression(
                expression,
                property_name,
                references,
                resolved,
                diagnostics,
            );
        }
    }
}

fn validate_property_default_reference(
    span: SourceSpan,
    role: ReferenceRole,
    property_name: &str,
    references: &HashMap<(SourceSpan, ReferenceRole), &SymbolReference>,
    resolved: &HashMap<(SourceSpan, ReferenceRole), &ResolvedReference>,
    diagnostics: &mut Vec<crate::diagnostics::SemanticDiagnostic>,
) {
    let Some(reference) = references.get(&(span, role)).copied() else {
        return;
    };

    let allowed = match role {
        ReferenceRole::Value => matches!(
            reference.resolution,
            symbols::ReferenceResolution::BuiltinValue
        ),
        ReferenceRole::CallTarget | ReferenceRole::FunctionHandleTarget => {
            if let Some(resolved_reference) = resolved.get(&(span, role)).copied() {
                matches!(
                    resolved_reference.final_resolution,
                    FinalReferenceResolution::BuiltinFunction
                )
            } else {
                matches!(
                    reference.resolution,
                    symbols::ReferenceResolution::BuiltinFunction
                )
            }
        }
    };
    if allowed {
        return;
    }

    let detail = match role {
        ReferenceRole::Value => match reference.resolved_kind {
            Some(SymbolKind::Property) => format!("property `{}`", reference.name),
            Some(SymbolKind::Parameter) => format!("parameter `{}`", reference.name),
            Some(SymbolKind::Output) => format!("output `{}`", reference.name),
            Some(SymbolKind::Variable) => format!("variable `{}`", reference.name),
            _ => format!("identifier `{}`", reference.name),
        },
        ReferenceRole::CallTarget => format!("call target `{}`", reference.name),
        ReferenceRole::FunctionHandleTarget => {
            format!("function handle target `{}`", reference.name)
        }
    };
    diagnostics.push(crate::diagnostics::SemanticDiagnostic::error(
        "SEM010",
        format!(
            "class property default for `{property_name}` supports only builtin values and builtin function calls; {detail} is not supported"
        ),
        reference.span,
    ));
}

pub const CRATE_NAME: &str = "matlab-semantics";

pub fn summary() -> &'static str {
    "Owns semantic analysis, workspace rules, binding validation, and semantic diagnostics."
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use matlab_frontend::{
        parser::{parse_source, ParseMode},
        source::SourceFileId,
    };

    use super::analyze_compilation_unit_with_context;
    use matlab_resolver::ResolverContext;

    #[test]
    fn class_property_defaults_reject_sibling_property_references() {
        let source = "classdef Pair\nproperties\nleft = 1;\nright = left;\nend\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(PathBuf::from("Pair.m")),
        );

        assert!(analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(
            analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "SEM010" && diagnostic.message.contains("property `left`")
            }),
            "{:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn class_property_defaults_reject_external_function_calls() {
        let workspace = temp_test_dir();
        let class_path = workspace.join("Pair.m");
        write_file(
            &class_path,
            "classdef Pair\nproperties\nleft = helper();\nend\nend\n",
        );
        write_file(
            &workspace.join("helper.m"),
            "function out = helper()\nout = 1;\nend\n",
        );

        let parsed = parse_source(
            "classdef Pair\nproperties\nleft = helper();\nend\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(class_path),
        );

        assert!(analysis.has_errors());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "SEM010" && diagnostic.message.contains("call target `helper`")
        }));
        cleanup(&workspace);
    }

    #[test]
    fn class_property_defaults_reject_shadowed_builtin_calls() {
        let workspace = temp_test_dir();
        let class_path = workspace.join("Pair.m");
        write_file(
            &class_path,
            "classdef Pair\nproperties\nleft = zeros(1, 1);\nend\nend\n",
        );
        write_file(
            &workspace.join("zeros.m"),
            "function out = zeros(varargin)\nout = 99;\nend\n",
        );

        let parsed = parse_source(
            "classdef Pair\nproperties\nleft = zeros(1, 1);\nend\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(class_path),
        );

        assert!(analysis.has_errors());
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "SEM010" && diagnostic.message.contains("call target `zeros`")
        }));
        cleanup(&workspace);
    }

    #[test]
    fn class_property_defaults_allow_builtin_values_and_calls() {
        let source = "classdef Pair\nproperties\nleft = zeros(1, 1);\nright = pi;\nend\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(PathBuf::from("Pair.m")),
        );

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_test_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix time")
            .as_nanos();
        path.push(format!(
            "matlab_semantics_classdef_defaults_{}_{}",
            nanos,
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp test dir");
        path
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(path, contents).expect("write test file");
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }
}
