//! Test-oriented rendering helpers for golden fixtures.

use crate::ast::{
    AssignmentTarget, ClassDef, ClassMethodBlock, ClassPropertyBlock, ClassPropertyDef,
    CompilationUnit, Expression, ExpressionKind, FunctionDef, FunctionHandleTarget, Identifier,
    IndexArgument, Item, QualifiedName, Statement, StatementKind,
};

pub fn render_compilation_unit(unit: &CompilationUnit) -> String {
    let mut out = String::new();
    out.push_str(&format!("unit {:?}\n", unit.kind));
    for item in &unit.items {
        render_item(item, 0, &mut out);
    }
    out
}

fn render_item(item: &Item, depth: usize, out: &mut String) {
    match item {
        Item::Statement(statement) => render_statement(statement, depth, out),
        Item::Function(function) => render_function(function, depth, out),
        Item::Class(class_def) => render_class(class_def, depth, out),
    }
}

fn render_function(function: &FunctionDef, depth: usize, out: &mut String) {
    push_line(
        out,
        depth,
        format!(
            "function {}({}) -> [{}]",
            function.name.name,
            join_identifiers(&function.inputs),
            join_identifiers(&function.outputs)
        ),
    );
    for statement in &function.body {
        render_statement(statement, depth + 1, out);
    }
    for nested in &function.local_functions {
        render_function(nested, depth + 1, out);
    }
}

fn render_class(class_def: &ClassDef, depth: usize, out: &mut String) {
    let superclass = class_def
        .superclass
        .as_ref()
        .map(|name| format!(" < {}", join_qualified_name(name)))
        .unwrap_or_default();
    push_line(
        out,
        depth,
        format!("class {}{}", class_def.name.name, superclass),
    );
    for block in &class_def.property_blocks {
        render_property_block(block, depth + 1, out);
    }
    for block in &class_def.method_blocks {
        render_method_block(block, depth + 1, out);
    }
}

fn render_property_block(block: &ClassPropertyBlock, depth: usize, out: &mut String) {
    let header = match block.access {
        crate::ast::ClassMemberAccess::Public => "properties".to_string(),
        crate::ast::ClassMemberAccess::Private => "properties (Access=private)".to_string(),
    };
    push_line(out, depth, header);
    for property in &block.properties {
        render_property(property, depth + 1, out);
    }
    push_line(out, depth, "end_properties".to_string());
}

fn render_property(property: &ClassPropertyDef, depth: usize, out: &mut String) {
    let line = match &property.default {
        Some(default) => format!(
            "property {} = {}",
            property.name.name,
            render_expression(default)
        ),
        None => format!("property {}", property.name.name),
    };
    push_line(out, depth, line);
}

fn render_method_block(block: &ClassMethodBlock, depth: usize, out: &mut String) {
    let header = match (block.is_static, block.access) {
        (true, crate::ast::ClassMemberAccess::Private) => {
            "methods (Static, Access=private)".to_string()
        }
        (true, crate::ast::ClassMemberAccess::Public) => "methods (Static)".to_string(),
        (false, crate::ast::ClassMemberAccess::Private) => "methods (Access=private)".to_string(),
        (false, crate::ast::ClassMemberAccess::Public) => "methods".to_string(),
    };
    push_line(out, depth, header);
    for method in &block.methods {
        render_function(method, depth + 1, out);
    }
    push_line(out, depth, "end_methods".to_string());
}

fn render_statement(statement: &Statement, depth: usize, out: &mut String) {
    match &statement.kind {
        StatementKind::Assignment { targets, value, .. } => {
            push_line(
                out,
                depth,
                format!(
                    "assign [{}] = {}",
                    join_assignment_targets(targets),
                    render_expression(value)
                ),
            );
        }
        StatementKind::Expression(expression) => {
            push_line(
                out,
                depth,
                format!("expr {}", render_expression(expression)),
            );
        }
        StatementKind::If {
            branches,
            else_body,
        } => {
            for (index, branch) in branches.iter().enumerate() {
                let label = if index == 0 { "if" } else { "elseif" };
                push_line(
                    out,
                    depth,
                    format!("{label} {}", render_expression(&branch.condition)),
                );
                for statement in &branch.body {
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
        StatementKind::Switch {
            expression,
            cases,
            otherwise_body,
        } => {
            push_line(
                out,
                depth,
                format!("switch {}", render_expression(expression)),
            );
            for case in cases {
                push_line(
                    out,
                    depth + 1,
                    format!("case {}", render_expression(&case.matcher)),
                );
                for statement in &case.body {
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
        StatementKind::Try {
            body,
            catch_binding,
            catch_body,
        } => {
            push_line(out, depth, "try".to_string());
            for statement in body {
                render_statement(statement, depth + 1, out);
            }
            match catch_binding {
                Some(binding) => push_line(out, depth, format!("catch {}", binding.name)),
                None => push_line(out, depth, "catch".to_string()),
            }
            for statement in catch_body {
                render_statement(statement, depth + 1, out);
            }
            push_line(out, depth, "end_try".to_string());
        }
        StatementKind::For {
            variable,
            iterable,
            body,
        } => {
            push_line(
                out,
                depth,
                format!("for {} = {}", variable.name, render_expression(iterable)),
            );
            for statement in body {
                render_statement(statement, depth + 1, out);
            }
            push_line(out, depth, "end_for".to_string());
        }
        StatementKind::While { condition, body } => {
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
        StatementKind::Break => push_line(out, depth, "break".to_string()),
        StatementKind::Continue => push_line(out, depth, "continue".to_string()),
        StatementKind::Return => push_line(out, depth, "return".to_string()),
        StatementKind::Global(names) => {
            push_line(out, depth, format!("global [{}]", join_identifiers(names)));
        }
        StatementKind::Persistent(names) => {
            push_line(
                out,
                depth,
                format!("persistent [{}]", join_identifiers(names)),
            );
        }
    }
}

fn render_expression(expression: &Expression) -> String {
    match &expression.kind {
        ExpressionKind::Identifier(identifier) => identifier.name.clone(),
        ExpressionKind::NumberLiteral(text) => format!("number({text})"),
        ExpressionKind::CharLiteral(text) => format!("char({text})"),
        ExpressionKind::StringLiteral(text) => format!("string({text})"),
        ExpressionKind::MatrixLiteral(rows) => {
            let rows = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(render_expression)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .collect::<Vec<_>>()
                .join(" ; ");
            format!("matrix[{rows}]")
        }
        ExpressionKind::CellLiteral(rows) => {
            let rows = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(render_expression)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .collect::<Vec<_>>()
                .join(" ; ");
            format!("cell{{{rows}}}")
        }
        ExpressionKind::FunctionHandle(target) => {
            format!("handle(@{})", render_function_handle_target(target))
        }
        ExpressionKind::EndKeyword => "end".to_string(),
        ExpressionKind::Unary { op, rhs } => format!("unary({op:?}, {})", render_expression(rhs)),
        ExpressionKind::Binary { op, lhs, rhs } => {
            format!(
                "binary({op:?}, {}, {})",
                render_expression(lhs),
                render_expression(rhs)
            )
        }
        ExpressionKind::Range { start, step, end } => match step {
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
        ExpressionKind::ParenApply { target, indices } => format!(
            "apply({}, [{}])",
            render_expression(target),
            join_index_arguments(indices)
        ),
        ExpressionKind::CellIndex { target, indices } => format!(
            "cell_index({}, [{}])",
            render_expression(target),
            join_index_arguments(indices)
        ),
        ExpressionKind::FieldAccess { target, field } => {
            format!("field({}, {})", render_expression(target), field.name)
        }
        ExpressionKind::AnonymousFunction { params, body } => format!(
            "anon(@({}) {})",
            join_identifiers(params),
            render_expression(body)
        ),
    }
}

fn render_function_handle_target(target: &FunctionHandleTarget) -> String {
    match target {
        FunctionHandleTarget::Name(name) => join_qualified_name(name),
        FunctionHandleTarget::Expression(expression) => render_expression(expression),
    }
}

fn join_assignment_targets(targets: &[AssignmentTarget]) -> String {
    targets
        .iter()
        .map(render_assignment_target)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_assignment_target(target: &AssignmentTarget) -> String {
    match target {
        AssignmentTarget::Identifier(identifier) => identifier.name.clone(),
        AssignmentTarget::Index { target, indices } => {
            format!(
                "index({}, [{}])",
                render_expression(target),
                join_index_arguments(indices)
            )
        }
        AssignmentTarget::CellIndex { target, indices } => {
            format!(
                "cell_index({}, [{}])",
                render_expression(target),
                join_index_arguments(indices)
            )
        }
        AssignmentTarget::Field { target, field } => {
            format!("field({}, {})", render_expression(target), field.name)
        }
    }
}

fn join_index_arguments(args: &[IndexArgument]) -> String {
    args.iter()
        .map(render_index_argument)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_index_argument(arg: &IndexArgument) -> String {
    match arg {
        IndexArgument::Expression(expression) => render_expression(expression),
        IndexArgument::FullSlice => ":".to_string(),
        IndexArgument::End => "end".to_string(),
    }
}

fn join_identifiers(names: &[Identifier]) -> String {
    names
        .iter()
        .map(|name| name.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn join_qualified_name(name: &QualifiedName) -> String {
    name.segments
        .iter()
        .map(|segment| segment.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn push_line(out: &mut String, depth: usize, line: String) {
    out.push_str(&"  ".repeat(depth));
    out.push_str(&line);
    out.push('\n');
}
