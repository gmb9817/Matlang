use crate::hir::{
    HirAnonymousFunction, HirAssignmentTarget, HirBinding, HirCallTarget, HirCallableRef,
    HirCapture, HirConditionalBranch, HirExpression, HirFunction, HirIndexArgument, HirItem,
    HirModule, HirStatement, HirSwitchCase, HirValueRef,
};
use matlab_semantics::symbols::{
    BindingStorage, CaptureAccess, FinalReferenceResolution, PathResolutionKind,
    ReferenceResolution, SymbolKind,
};

pub fn render_hir(module: &HirModule) -> String {
    let mut out = String::new();
    push_line(
        &mut out,
        0,
        format!(
            "module {:?} scope={} workspace={}",
            module.kind, module.scope_id.0, module.workspace_id.0
        ),
    );
    for item in &module.items {
        render_item(item, 1, &mut out);
    }
    out
}

fn render_item(item: &HirItem, depth: usize, out: &mut String) {
    match item {
        HirItem::Statement(statement) => render_statement(statement, depth, out),
        HirItem::Function(function) => render_function(function, depth, out),
    }
}

fn render_function(function: &HirFunction, depth: usize, out: &mut String) {
    push_line(
        out,
        depth,
        format!(
            "function {} scope={} workspace={} captures=[{}]",
            function.name,
            function.scope_id.0,
            function.workspace_id.0,
            join_captures(&function.captures)
        ),
    );
    push_line(
        out,
        depth + 1,
        format!("inputs [{}]", join_bindings(&function.inputs)),
    );
    push_line(
        out,
        depth + 1,
        format!("outputs [{}]", join_bindings(&function.outputs)),
    );
    for statement in &function.body {
        render_statement(statement, depth + 1, out);
    }
    for local in &function.local_functions {
        render_function(local, depth + 1, out);
    }
}

fn render_statement(statement: &HirStatement, depth: usize, out: &mut String) {
    match statement {
        HirStatement::Assignment { targets, value, .. } => push_line(
            out,
            depth,
            format!(
                "assign [{}] = {}",
                join_assignment_targets(targets),
                render_expression(value)
            ),
        ),
        HirStatement::Expression { expression, .. } => {
            push_line(
                out,
                depth,
                format!("expr {}", render_expression(expression)),
            );
        }
        HirStatement::If {
            branches,
            else_body,
        } => {
            for HirConditionalBranch { condition, body } in branches {
                push_line(out, depth, format!("if {}", render_expression(condition)));
                for statement in body {
                    render_statement(statement, depth + 1, out);
                }
            }
            if !else_body.is_empty() {
                push_line(out, depth, "else".to_string());
                for statement in else_body {
                    render_statement(statement, depth + 1, out);
                }
            }
            push_line(out, depth, "end_if".to_string());
        }
        HirStatement::Switch {
            expression,
            cases,
            otherwise_body,
        } => {
            push_line(
                out,
                depth,
                format!("switch {}", render_expression(expression)),
            );
            for HirSwitchCase { matcher, body } in cases {
                push_line(
                    out,
                    depth + 1,
                    format!("case {}", render_expression(matcher)),
                );
                for statement in body {
                    render_statement(statement, depth + 2, out);
                }
            }
            if !otherwise_body.is_empty() {
                push_line(out, depth + 1, "otherwise".to_string());
                for statement in otherwise_body {
                    render_statement(statement, depth + 2, out);
                }
            }
            push_line(out, depth, "end_switch".to_string());
        }
        HirStatement::Try {
            body,
            catch_binding,
            catch_body,
        } => {
            push_line(out, depth, "try".to_string());
            for statement in body {
                render_statement(statement, depth + 1, out);
            }
            match catch_binding {
                Some(binding) => {
                    push_line(out, depth, format!("catch {}", render_binding(binding)))
                }
                None => push_line(out, depth, "catch".to_string()),
            }
            for statement in catch_body {
                render_statement(statement, depth + 1, out);
            }
            push_line(out, depth, "end_try".to_string());
        }
        HirStatement::For {
            variable,
            iterable,
            body,
        } => {
            push_line(
                out,
                depth,
                format!(
                    "for {} = {}",
                    render_binding(variable),
                    render_expression(iterable)
                ),
            );
            for statement in body {
                render_statement(statement, depth + 1, out);
            }
            push_line(out, depth, "end_for".to_string());
        }
        HirStatement::While { condition, body } => {
            push_line(
                out,
                depth,
                format!("while {}", render_expression(condition)),
            );
            for statement in body {
                render_statement(statement, depth + 1, out);
            }
            push_line(out, depth, "end_while".to_string());
        }
        HirStatement::Break => push_line(out, depth, "break".to_string()),
        HirStatement::Continue => push_line(out, depth, "continue".to_string()),
        HirStatement::Return => push_line(out, depth, "return".to_string()),
        HirStatement::Global(names) => {
            push_line(out, depth, format!("global [{}]", join_bindings(names)));
        }
        HirStatement::Persistent(names) => {
            push_line(out, depth, format!("persistent [{}]", join_bindings(names)));
        }
    }
}

fn render_expression(expression: &HirExpression) -> String {
    match expression {
        HirExpression::ValueRef(reference) => render_value_ref(reference),
        HirExpression::NumberLiteral(text) => format!("number({text})"),
        HirExpression::CharLiteral(text) => format!("char({text})"),
        HirExpression::StringLiteral(text) => format!("string({text})"),
        HirExpression::MatrixLiteral(rows) => format!(
            "matrix[{}]",
            rows.iter()
                .map(|row| row
                    .iter()
                    .map(render_expression)
                    .collect::<Vec<_>>()
                    .join(", "))
                .collect::<Vec<_>>()
                .join(" ; ")
        ),
        HirExpression::CellLiteral(rows) => format!(
            "cell{{{}}}",
            rows.iter()
                .map(|row| row
                    .iter()
                    .map(render_expression)
                    .collect::<Vec<_>>()
                    .join(", "))
                .collect::<Vec<_>>()
                .join(" ; ")
        ),
        HirExpression::FunctionHandle(reference) => {
            format!("handle({})", render_callable_ref(reference))
        }
        HirExpression::EndKeyword => "end".to_string(),
        HirExpression::Unary { op, rhs } => {
            format!("unary({op:?}, {})", render_expression(rhs))
        }
        HirExpression::Binary { op, lhs, rhs } => {
            format!(
                "binary({op:?}, {}, {})",
                render_expression(lhs),
                render_expression(rhs)
            )
        }
        HirExpression::Range { start, step, end } => match step {
            Some(step) => format!(
                "range({}, {}, {})",
                render_expression(start),
                render_expression(step),
                render_expression(end)
            ),
            None => format!(
                "range({}, {})",
                render_expression(start),
                render_expression(end)
            ),
        },
        HirExpression::Call { target, args } => match target {
            HirCallTarget::Callable(reference) => format!(
                "call({}, [{}])",
                render_callable_ref(reference),
                join_index_arguments(args)
            ),
            HirCallTarget::Expression(target) => format!(
                "call_expr({}, [{}])",
                render_expression(target),
                join_index_arguments(args)
            ),
        },
        HirExpression::CellIndex { target, indices } => format!(
            "cell_index({}, [{}])",
            render_expression(target),
            join_index_arguments(indices)
        ),
        HirExpression::FieldAccess { target, field } => {
            format!("field({}, {})", render_expression(target), field)
        }
        HirExpression::AnonymousFunction(anonymous) => render_anonymous_function(anonymous),
    }
}

fn render_anonymous_function(anonymous: &HirAnonymousFunction) -> String {
    format!(
        "anon(scope={} workspace={} captures=[{}] params=[{}] body={})",
        anonymous.scope_id.0,
        anonymous.workspace_id.0,
        join_captures(&anonymous.captures),
        join_bindings(&anonymous.params),
        render_expression(&anonymous.body)
    )
}

fn render_value_ref(reference: &HirValueRef) -> String {
    let mut parts = vec![
        format!("name={}", reference.name),
        format!("resolution={}", render_resolution(&reference.resolution)),
    ];
    if let Some(binding_id) = reference.binding_id {
        parts.push(format!("binding={}", binding_id.0));
    }
    if let Some(kind) = reference.symbol_kind {
        parts.push(format!("kind={}", render_symbol_kind(kind)));
    }
    if let Some(access) = reference.capture_access {
        parts.push(format!("capture={}", render_capture_access(access)));
    }
    format!("value({})", parts.join(" "))
}

fn render_callable_ref(reference: &HirCallableRef) -> String {
    let mut parts = vec![
        format!("name={}", reference.name),
        format!(
            "semantic={}",
            render_resolution(&reference.semantic_resolution)
        ),
    ];
    if let Some(final_resolution) = &reference.final_resolution {
        parts.push(format!(
            "final={}",
            render_final_resolution_brief(final_resolution)
        ));
    }
    if let Some(symbol_id) = reference.resolved_symbol {
        parts.push(format!("symbol={}", symbol_id.0));
    }
    if let Some(kind) = reference.resolved_kind {
        parts.push(format!("kind={}", render_symbol_kind(kind)));
    }
    if let Some(binding_id) = reference.binding_id {
        parts.push(format!("binding={}", binding_id.0));
    }
    if let Some(access) = reference.capture_access {
        parts.push(format!("capture={}", render_capture_access(access)));
    }
    format!("callable({})", parts.join(" "))
}

fn join_bindings(bindings: &[HirBinding]) -> String {
    bindings
        .iter()
        .map(render_binding)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_binding(binding: &HirBinding) -> String {
    let mut parts = vec![
        format!("name={}", binding.name),
        format!("kind={}", render_symbol_kind(binding.symbol_kind)),
    ];
    if let Some(binding_id) = binding.binding_id {
        parts.push(format!("binding={}", binding_id.0));
    }
    if let Some(storage) = binding.storage {
        parts.push(format!("storage={}", render_binding_storage(storage)));
    }
    format!("binding({})", parts.join(" "))
}

fn join_assignment_targets(targets: &[HirAssignmentTarget]) -> String {
    targets
        .iter()
        .map(render_assignment_target)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_assignment_target(target: &HirAssignmentTarget) -> String {
    match target {
        HirAssignmentTarget::Binding(binding) => render_binding(binding),
        HirAssignmentTarget::Index { target, indices } => format!(
            "index({}, [{}])",
            render_expression(target),
            join_index_arguments(indices)
        ),
        HirAssignmentTarget::CellIndex { target, indices } => format!(
            "cell_index({}, [{}])",
            render_expression(target),
            join_index_arguments(indices)
        ),
        HirAssignmentTarget::Field { target, field } => {
            format!("field({}, {})", render_expression(target), field)
        }
    }
}

fn join_index_arguments(indices: &[HirIndexArgument]) -> String {
    indices
        .iter()
        .map(render_index_argument)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_index_argument(index: &HirIndexArgument) -> String {
    match index {
        HirIndexArgument::Expression(expression) => render_expression(expression),
        HirIndexArgument::FullSlice => ":".to_string(),
        HirIndexArgument::End => "end".to_string(),
    }
}

fn join_captures(captures: &[HirCapture]) -> String {
    captures
        .iter()
        .map(render_capture)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_capture(capture: &HirCapture) -> String {
    format!(
        "capture(name={} binding={} access={} from_scope={} from_workspace={})",
        capture.name,
        capture.binding_id.0,
        render_capture_access(capture.access),
        capture.from_scope.0,
        capture.from_workspace.0
    )
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

fn render_final_resolution_brief(resolution: &FinalReferenceResolution) -> String {
    match resolution {
        FinalReferenceResolution::WorkspaceValue => "workspace_value".to_string(),
        FinalReferenceResolution::BuiltinValue => "builtin_value".to_string(),
        FinalReferenceResolution::NestedFunction => "nested_function".to_string(),
        FinalReferenceResolution::FileFunction => "file_function".to_string(),
        FinalReferenceResolution::BuiltinFunction => "builtin_function".to_string(),
        FinalReferenceResolution::ResolvedPath {
            kind,
            package,
            shadowed_builtin,
            ..
        } => {
            let mut parts = vec![render_path_kind(*kind).to_string()];
            if let Some(package) = package {
                parts.push(format!("package={package}"));
            }
            if *shadowed_builtin {
                parts.push("shadowed_builtin=true".to_string());
            }
            parts.join(" ")
        }
        FinalReferenceResolution::UnresolvedExternal => "unresolved_external".to_string(),
        FinalReferenceResolution::UnresolvedValue => "unresolved_value".to_string(),
    }
}

fn render_path_kind(kind: PathResolutionKind) -> &'static str {
    match kind {
        PathResolutionKind::PrivateDirectory => "private_function",
        PathResolutionKind::CurrentDirectory => "current_directory_function",
        PathResolutionKind::SearchPath => "search_path_function",
        PathResolutionKind::PackageDirectory => "package_function",
    }
}

fn push_line(out: &mut String, depth: usize, line: String) {
    out.push_str(&"  ".repeat(depth));
    out.push_str(&line);
    out.push('\n');
}
