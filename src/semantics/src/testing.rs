//! Test-oriented renderers for semantic analysis fixtures.

use crate::{
    binder::AnalysisResult,
    symbols::{
        BindingStorage, CaptureAccess, FinalReferenceResolution, PathResolutionKind,
        ReferenceResolution, ReferenceRole, SymbolKind,
    },
};

pub fn render_analysis(result: &AnalysisResult) -> String {
    let mut out = String::new();

    out.push_str("workspaces\n");
    for workspace in &result.workspaces {
        out.push_str(&format!(
            "  {:?} id={} parent={:?} name={:?}\n",
            workspace.kind,
            workspace.id.0,
            workspace.parent.map(|id| id.0),
            workspace.name
        ));
    }

    out.push_str("scopes\n");
    for scope in &result.scopes {
        out.push_str(&format!(
            "  {:?} id={} parent={:?} workspace={}\n",
            scope.kind,
            scope.id.0,
            scope.parent.map(|id| id.0),
            scope.workspace_id.0
        ));
    }

    out.push_str("symbols\n");
    for symbol in &result.symbols {
        out.push_str(&format!(
            "  {} kind={} scope={} workspace={} binding={:?}\n",
            symbol.name,
            render_symbol_kind(symbol.kind),
            symbol.scope_id.0,
            symbol.workspace_id.0,
            symbol.binding_id.map(|id| id.0)
        ));
    }

    out.push_str("bindings\n");
    for binding in &result.bindings {
        out.push_str(&format!(
            "  {} id={} storage={} owner_symbol={} owner_scope={} owner_workspace={} shared={}\n",
            binding.name,
            binding.id.0,
            render_binding_storage(binding.storage),
            binding.owner_symbol.0,
            binding.owner_scope.0,
            binding.owner_workspace.0,
            binding.shared_with_closures
        ));
    }

    if !result.classes.is_empty() {
        out.push_str("classes\n");
        for class in &result.classes {
            out.push_str(&format!(
                "  {} package={:?} superclass={:?} handle={} constructor={:?} source_path={:?}\n",
                class.name,
                class.package,
                class.superclass_name,
                class.inherits_handle,
                class.constructor,
                class.source_path
            ));
            if !class.properties.is_empty() {
                out.push_str(&format!(
                    "    properties [{}]\n",
                    class.properties
                        .iter()
                        .map(|property| {
                            if property.default.is_some() {
                                format!("{}=default", property.name)
                            } else {
                                property.name.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !class.inline_methods.is_empty() {
                out.push_str(&format!(
                    "    inline_methods [{}]\n",
                    class.inline_methods.join(", ")
                ));
            }
            if !class.static_inline_methods.is_empty() {
                out.push_str(&format!(
                    "    static_inline_methods [{}]\n",
                    class.static_inline_methods.join(", ")
                ));
            }
            if !class.private_properties.is_empty() {
                out.push_str(&format!(
                    "    private_properties [{}]\n",
                    class.private_properties.join(", ")
                ));
            }
            if !class.private_inline_methods.is_empty() {
                out.push_str(&format!(
                    "    private_inline_methods [{}]\n",
                    class.private_inline_methods.join(", ")
                ));
            }
            if !class.private_static_inline_methods.is_empty() {
                out.push_str(&format!(
                    "    private_static_inline_methods [{}]\n",
                    class.private_static_inline_methods.join(", ")
                ));
            }
            if !class.external_methods.is_empty() {
                out.push_str(&format!(
                    "    external_methods [{}]\n",
                    class.external_methods
                        .iter()
                        .map(|method| format!("{}={}", method.name, method.path.display()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }

    out.push_str("references\n");
    for reference in &result.references {
        out.push_str(&format!(
            "  {} role={} resolution={} symbol={:?} kind={} scope={:?} workspace={:?} binding={:?} capture={}\n",
            reference.name,
            render_role(reference.role),
            render_resolution(&reference.resolution),
            reference.resolved_symbol.map(|id| id.0),
            reference
                .resolved_kind
                .map(render_symbol_kind)
                .unwrap_or("none"),
            reference.resolved_scope.map(|id| id.0),
            reference.resolved_workspace.map(|id| id.0),
            reference.binding_id.map(|id| id.0),
            reference
                .capture_access
                .map(render_capture_access)
                .unwrap_or("none")
        ));
    }

    out.push_str("captures\n");
    for capture in &result.captures {
        out.push_str(&format!(
            "  {} binding={} access={} from_symbol={} from_scope={} from_workspace={} into_scope={} into_workspace={}\n",
            capture.name,
            capture.binding_id.0,
            render_capture_access(capture.access),
            capture.from_symbol.0,
            capture.from_scope.0,
            capture.from_workspace.0,
            capture.into_scope.0,
            capture.into_workspace.0
        ));
    }

    out.push_str("diagnostics\n");
    for diagnostic in &result.diagnostics {
        out.push_str(&format!("  {} {}\n", diagnostic.code, diagnostic.message));
    }

    if !result.resolved_references.is_empty() {
        out.push_str("resolved_references\n");
        for reference in &result.resolved_references {
            out.push_str(&format!(
                "  {} role={} semantic={} final={}\n",
                reference.name,
                render_role(reference.role),
                render_resolution(&reference.semantic_resolution),
                render_final_resolution(&reference.final_resolution)
            ));
        }
    }

    out
}

fn render_role(role: ReferenceRole) -> &'static str {
    match role {
        ReferenceRole::Value => "value",
        ReferenceRole::CallTarget => "call_target",
        ReferenceRole::FunctionHandleTarget => "function_handle_target",
    }
}

fn render_resolution(resolution: &ReferenceResolution) -> &'static str {
    match resolution {
        ReferenceResolution::WorkspaceValue => "workspace_value",
        ReferenceResolution::BuiltinValue => "builtin_value",
        ReferenceResolution::NestedFunction => "nested_function",
        ReferenceResolution::FileFunction => "file_function",
        ReferenceResolution::BuiltinFunction => "builtin_function",
        ReferenceResolution::ExternalFunctionCandidate => "external_function_candidate",
        ReferenceResolution::UnresolvedValue => "unresolved_value",
    }
}

fn render_symbol_kind(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Variable => "variable",
        SymbolKind::Parameter => "parameter",
        SymbolKind::Output => "output",
        SymbolKind::Function => "function",
        SymbolKind::Class => "class",
        SymbolKind::Method => "method",
        SymbolKind::Property => "property",
        SymbolKind::Global => "global",
        SymbolKind::Persistent => "persistent",
    }
}

fn render_binding_storage(storage: BindingStorage) -> &'static str {
    match storage {
        BindingStorage::Local => "local",
        BindingStorage::Global => "global",
        BindingStorage::Persistent => "persistent",
    }
}

fn render_capture_access(access: CaptureAccess) -> &'static str {
    match access {
        CaptureAccess::Read => "read",
        CaptureAccess::Write => "write",
        CaptureAccess::ReadWrite => "read_write",
    }
}

fn render_final_resolution(resolution: &FinalReferenceResolution) -> String {
    match resolution {
        FinalReferenceResolution::WorkspaceValue => "workspace_value".to_string(),
        FinalReferenceResolution::BuiltinValue => "builtin_value".to_string(),
        FinalReferenceResolution::NestedFunction => "nested_function".to_string(),
        FinalReferenceResolution::FileFunction => "file_function".to_string(),
        FinalReferenceResolution::BuiltinFunction => "builtin_function".to_string(),
        FinalReferenceResolution::ResolvedPath {
            kind,
            path,
            package,
            shadowed_builtin,
        } => {
            let mut rendered = format!(
                "{} path={}",
                render_path_resolution_kind(*kind),
                path.display()
            );
            if let Some(package) = package {
                rendered.push_str(&format!(" package={package}"));
            }
            if *shadowed_builtin {
                rendered.push_str(" shadowed_builtin=true");
            }
            rendered
        }
        FinalReferenceResolution::UnresolvedExternal => "unresolved_external".to_string(),
        FinalReferenceResolution::UnresolvedValue => "unresolved_value".to_string(),
    }
}

fn render_path_resolution_kind(kind: PathResolutionKind) -> &'static str {
    match kind {
        PathResolutionKind::PrivateDirectory => "private_function",
        PathResolutionKind::CurrentDirectory => "current_directory_function",
        PathResolutionKind::SearchPath => "search_path_function",
        PathResolutionKind::PackageDirectory => "package_function",
        PathResolutionKind::ClassCurrentDirectory => "current_directory_class",
        PathResolutionKind::ClassSearchPath => "search_path_class",
        PathResolutionKind::ClassPackageDirectory => "package_class",
        PathResolutionKind::ClassFolderCurrentDirectory => "current_directory_folder_class",
        PathResolutionKind::ClassFolderSearchPath => "search_path_folder_class",
        PathResolutionKind::ClassFolderPackageDirectory => "package_folder_class",
    }
}
