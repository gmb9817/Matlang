//! Resolver-backed finalization of semantic reference classifications.

use matlab_resolver::{resolve_function, ResolvedFunctionKind, ResolverContext};

use crate::{
    binder::AnalysisResult,
    symbols::{
        FinalReferenceResolution, PathResolutionKind, ReferenceResolution, ReferenceRole,
        ResolvedReference,
    },
};

pub fn apply_resolver_context(result: &mut AnalysisResult, context: &ResolverContext) {
    result.resolved_references = result
        .references
        .iter()
        .filter(|reference| {
            matches!(
                reference.role,
                ReferenceRole::CallTarget | ReferenceRole::FunctionHandleTarget
            )
        })
        .map(|reference| ResolvedReference {
            name: reference.name.clone(),
            span: reference.span,
            role: reference.role,
            semantic_resolution: reference.resolution.clone(),
            final_resolution: resolve_final_reference(reference, context),
        })
        .collect();
}

fn resolve_final_reference(
    reference: &crate::symbols::SymbolReference,
    context: &ResolverContext,
) -> FinalReferenceResolution {
    match &reference.resolution {
        ReferenceResolution::WorkspaceValue => FinalReferenceResolution::WorkspaceValue,
        ReferenceResolution::BuiltinValue => FinalReferenceResolution::BuiltinValue,
        ReferenceResolution::NestedFunction => FinalReferenceResolution::NestedFunction,
        ReferenceResolution::FileFunction => FinalReferenceResolution::FileFunction,
        ReferenceResolution::BuiltinFunction => {
            if let Some(resolved) = resolve_function(&reference.name, context) {
                FinalReferenceResolution::ResolvedPath {
                    kind: map_path_kind(resolved.kind),
                    path: resolved.path,
                    package: resolved.package,
                    shadowed_builtin: true,
                }
            } else {
                FinalReferenceResolution::BuiltinFunction
            }
        }
        ReferenceResolution::ExternalFunctionCandidate => {
            if let Some(resolved) = resolve_function(&reference.name, context) {
                FinalReferenceResolution::ResolvedPath {
                    kind: map_path_kind(resolved.kind),
                    path: resolved.path,
                    package: resolved.package,
                    shadowed_builtin: false,
                }
            } else {
                FinalReferenceResolution::UnresolvedExternal
            }
        }
        ReferenceResolution::UnresolvedValue => FinalReferenceResolution::UnresolvedValue,
    }
}

fn map_path_kind(kind: ResolvedFunctionKind) -> PathResolutionKind {
    match kind {
        ResolvedFunctionKind::PrivateDirectory => PathResolutionKind::PrivateDirectory,
        ResolvedFunctionKind::CurrentDirectory => PathResolutionKind::CurrentDirectory,
        ResolvedFunctionKind::SearchPath => PathResolutionKind::SearchPath,
        ResolvedFunctionKind::PackageDirectory => PathResolutionKind::PackageDirectory,
    }
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
    use matlab_interop::write_mat_file;
    use matlab_resolver::ResolverContext;
    use matlab_runtime::{Value, Workspace};

    use crate::{
        analyze_compilation_unit_with_context,
        symbols::{FinalReferenceResolution, PathResolutionKind, ReferenceRole},
    };

    #[test]
    fn builtin_reference_is_upgraded_to_current_directory_function_when_shadowed() {
        let workspace = temp_test_dir();
        write_file(&workspace.join("main.m"), "y = zeros(1, 2);\n");
        write_file(
            &workspace.join("zeros.m"),
            "function y = zeros(varargin)\ny = 1;\nend\n",
        );

        let parsed = parse_source("y = zeros(1, 2);\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(workspace.join("main.m")),
        );

        let resolved = analysis
            .resolved_references
            .iter()
            .find(|reference| reference.name == "zeros")
            .expect("zeros final resolution");

        assert_eq!(resolved.role, ReferenceRole::CallTarget);
        assert_eq!(
            resolved.final_resolution,
            FinalReferenceResolution::ResolvedPath {
                kind: PathResolutionKind::CurrentDirectory,
                path: workspace.join("zeros.m"),
                package: None,
                shadowed_builtin: true,
            }
        );
        cleanup(&workspace);
    }

    #[test]
    fn external_reference_is_upgraded_to_private_function() {
        let workspace = temp_test_dir();
        fs::create_dir_all(workspace.join("private")).expect("create private dir");
        write_file(&workspace.join("main.m"), "y = helper(1);\n");
        write_file(
            &workspace.join("private").join("helper.m"),
            "function y = helper(x)\ny = x;\nend\n",
        );

        let parsed = parse_source("y = helper(1);\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(workspace.join("main.m")),
        );

        let resolved = analysis
            .resolved_references
            .iter()
            .find(|reference| reference.name == "helper")
            .expect("helper final resolution");

        assert_eq!(
            resolved.final_resolution,
            FinalReferenceResolution::ResolvedPath {
                kind: PathResolutionKind::PrivateDirectory,
                path: workspace.join("private").join("helper.m"),
                package: None,
                shadowed_builtin: false,
            }
        );
        cleanup(&workspace);
    }

    #[test]
    fn package_qualified_reference_is_upgraded_to_package_function() {
        let workspace = temp_test_dir();
        let source = workspace.join("src");
        let root = workspace.join("packages");
        fs::create_dir_all(&source).expect("create source dir");
        fs::create_dir_all(root.join("+pkg")).expect("create package dir");
        write_file(&source.join("main.m"), "y = pkg.helper(1);\n");
        write_file(
            &root.join("+pkg").join("helper.m"),
            "function y = helper(x)\ny = x;\nend\n",
        );

        let parsed = parse_source("y = pkg.helper(1);\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::new(Some(source.join("main.m")), vec![root.clone()]),
        );

        let resolved = analysis
            .resolved_references
            .iter()
            .find(|reference| reference.name == "pkg.helper")
            .expect("package final resolution");

        assert_eq!(
            resolved.final_resolution,
            FinalReferenceResolution::ResolvedPath {
                kind: PathResolutionKind::PackageDirectory,
                path: root.join("+pkg").join("helper.m"),
                package: Some("pkg".to_string()),
                shadowed_builtin: false,
            }
        );
        cleanup(&workspace);
    }

    #[test]
    fn load_side_effect_declarations_allow_later_same_script_references() {
        let workspace = temp_test_dir();
        let main = workspace.join("main.m");
        write_file(&main, "load('vars.mat');\ny = x;\n");
        let mut mat_workspace = Workspace::new();
        mat_workspace.insert("x".to_string(), Value::Scalar(7.0));
        write_mat_file(&workspace.join("vars.mat"), &mat_workspace).expect("write mat");

        let parsed = parse_source(
            "load('vars.mat');\ny = x;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis =
            analyze_compilation_unit_with_context(&unit, &ResolverContext::from_source_file(main));

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis
            .references
            .iter()
            .any(|reference| { reference.name == "x" && reference.role == ReferenceRole::Value }));
        cleanup(&workspace);
    }

    #[test]
    fn load_regexp_side_effect_declarations_allow_later_same_script_references() {
        let workspace = temp_test_dir();
        let main = workspace.join("main.m");
        write_file(
            &main,
            "load('vars.mat', '-regexp', '^alpha');\ny = alphabet;\n",
        );
        let mut mat_workspace = Workspace::new();
        mat_workspace.insert("alpha".to_string(), Value::Scalar(7.0));
        mat_workspace.insert("alphabet".to_string(), Value::Scalar(9.0));
        mat_workspace.insert("beta".to_string(), Value::Scalar(11.0));
        write_mat_file(&workspace.join("vars.mat"), &mat_workspace).expect("write mat");

        let parsed = parse_source(
            "load('vars.mat', '-regexp', '^alpha');\ny = alphabet;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis =
            analyze_compilation_unit_with_context(&unit, &ResolverContext::from_source_file(main));

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "alphabet" && reference.role == ReferenceRole::Value
        }));
        cleanup(&workspace);
    }

    #[test]
    fn command_form_load_side_effect_declarations_allow_later_same_script_references() {
        let workspace = temp_test_dir();
        let main = workspace.join("main.m");
        write_file(&main, "load vars.mat x\ny = x;\n");
        let mut mat_workspace = Workspace::new();
        mat_workspace.insert("x".to_string(), Value::Scalar(7.0));
        write_mat_file(&workspace.join("vars.mat"), &mat_workspace).expect("write mat");

        let parsed = parse_source(
            "load vars.mat x\ny = x;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis =
            analyze_compilation_unit_with_context(&unit, &ResolverContext::from_source_file(main));

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis
            .references
            .iter()
            .any(|reference| { reference.name == "x" && reference.role == ReferenceRole::Value }));
        cleanup(&workspace);
    }

    #[test]
    fn command_form_load_regexp_side_effect_declarations_allow_later_same_script_references() {
        let workspace = temp_test_dir();
        let main = workspace.join("main.m");
        write_file(&main, "load vars.mat -regexp ^alpha\ny = alphabet;\n");
        let mut mat_workspace = Workspace::new();
        mat_workspace.insert("alpha".to_string(), Value::Scalar(7.0));
        mat_workspace.insert("alphabet".to_string(), Value::Scalar(9.0));
        mat_workspace.insert("beta".to_string(), Value::Scalar(11.0));
        write_mat_file(&workspace.join("vars.mat"), &mat_workspace).expect("write mat");

        let parsed = parse_source(
            "load vars.mat -regexp ^alpha\ny = alphabet;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis =
            analyze_compilation_unit_with_context(&unit, &ResolverContext::from_source_file(main));

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "alphabet" && reference.role == ReferenceRole::Value
        }));
        cleanup(&workspace);
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_test_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix time")
            .as_nanos();
        path.push(format!(
            "matlab_semantics_resolution_test_{}_{}",
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
