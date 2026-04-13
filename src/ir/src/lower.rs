use std::collections::{HashMap, VecDeque};

use matlab_frontend::ast::{
    AssignmentTarget, CompilationUnit, Expression, ExpressionKind, FunctionDef, Identifier,
    IndexArgument, Item, QualifiedName, Statement, StatementKind,
};
use matlab_frontend::source::SourceSpan;
use matlab_semantics::{
    symbols::{
        Binding, BindingId, ReferenceResolution, ReferenceRole, ResolvedReference, Symbol,
        SymbolId, SymbolKind, SymbolReference,
    },
    workspace::{ScopeId, WorkspaceId, WorkspaceKind},
    AnalysisResult,
};

use crate::hir::{
    HirAnonymousFunction, HirAssignmentTarget, HirBinding, HirCallTarget, HirCallableRef,
    HirCapture, HirConditionalBranch, HirExpression, HirFunction, HirIndexArgument, HirItem,
    HirModule, HirStatement, HirSwitchCase, HirValueRef,
};

pub fn lower_to_hir(unit: &CompilationUnit, analysis: &AnalysisResult) -> HirModule {
    LoweringContext::new(analysis).lower_module(unit)
}

struct LoweringContext<'a> {
    symbols_by_id: HashMap<SymbolId, &'a Symbol>,
    bindings_by_id: HashMap<BindingId, &'a Binding>,
    scope_parent: HashMap<ScopeId, Option<ScopeId>>,
    scope_values: HashMap<ScopeId, HashMap<String, SymbolId>>,
    references: HashMap<ReferenceKey, &'a SymbolReference>,
    resolved_references: HashMap<ReferenceKey, &'a ResolvedReference>,
    function_frames: VecDeque<FunctionFrame>,
    anonymous_frames: VecDeque<AnonymousFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionFrame {
    name: String,
    scope_id: ScopeId,
    workspace_id: WorkspaceId,
    captures: Vec<HirCapture>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AnonymousFrame {
    scope_id: ScopeId,
    workspace_id: WorkspaceId,
    captures: Vec<HirCapture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReferenceKey {
    name: String,
    role: u8,
    start: u32,
    end: u32,
}

impl<'a> LoweringContext<'a> {
    fn new(analysis: &'a AnalysisResult) -> Self {
        let symbols_by_id = analysis
            .symbols
            .iter()
            .map(|symbol| (symbol.id, symbol))
            .collect::<HashMap<_, _>>();
        let bindings_by_id = analysis
            .bindings
            .iter()
            .map(|binding| (binding.id, binding))
            .collect::<HashMap<_, _>>();
        let scope_parent = analysis
            .scopes
            .iter()
            .map(|scope| (scope.id, scope.parent))
            .collect::<HashMap<_, _>>();

        let mut scope_values = HashMap::<ScopeId, HashMap<String, SymbolId>>::new();
        for symbol in &analysis.symbols {
            if symbol.kind.is_function() {
                continue;
            }
            scope_values
                .entry(symbol.scope_id)
                .or_default()
                .insert(symbol.name.clone(), symbol.id);
        }

        let references = analysis
            .references
            .iter()
            .map(|reference| {
                (
                    reference_key(&reference.name, reference.span, reference.role),
                    reference,
                )
            })
            .collect::<HashMap<_, _>>();
        let resolved_references = analysis
            .resolved_references
            .iter()
            .map(|reference| {
                (
                    reference_key(&reference.name, reference.span, reference.role),
                    reference,
                )
            })
            .collect::<HashMap<_, _>>();

        let captures_by_scope = build_captures_by_scope(analysis);
        let function_frames = analysis
            .workspaces
            .iter()
            .filter(|workspace| {
                workspace.kind == WorkspaceKind::Function && workspace.name.is_some()
            })
            .map(|workspace| FunctionFrame {
                name: workspace.name.clone().expect("function workspace name"),
                scope_id: workspace.scope_id,
                workspace_id: workspace.id,
                captures: captures_by_scope
                    .get(&workspace.scope_id)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect::<VecDeque<_>>();
        let anonymous_frames = analysis
            .workspaces
            .iter()
            .filter(|workspace| workspace.kind == WorkspaceKind::AnonymousFunction)
            .map(|workspace| AnonymousFrame {
                scope_id: workspace.scope_id,
                workspace_id: workspace.id,
                captures: captures_by_scope
                    .get(&workspace.scope_id)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect::<VecDeque<_>>();

        Self {
            symbols_by_id,
            bindings_by_id,
            scope_parent,
            scope_values,
            references,
            resolved_references,
            function_frames,
            anonymous_frames,
        }
    }

    fn lower_module(mut self, unit: &CompilationUnit) -> HirModule {
        HirModule {
            kind: unit.kind,
            scope_id: ScopeId(0),
            workspace_id: WorkspaceId(0),
            implicit_ans: Some(self.lookup_current_binding(
                ScopeId(0),
                "ans",
                SymbolKind::Variable,
            )),
            items: self.lower_items(&unit.items, ScopeId(0)),
        }
    }

    fn lower_items(&mut self, items: &[Item], scope_id: ScopeId) -> Vec<HirItem> {
        items
            .iter()
            .map(|item| match item {
                Item::Statement(statement) => {
                    HirItem::Statement(self.lower_statement(statement, scope_id))
                }
                Item::Function(function) => HirItem::Function(self.lower_function(function)),
            })
            .collect()
    }

    fn lower_function(&mut self, function: &FunctionDef) -> HirFunction {
        let frame = self
            .function_frames
            .pop_front()
            .expect("function frame should exist during lowering");
        debug_assert_eq!(frame.name, function.name.name);

        HirFunction {
            name: function.name.name.clone(),
            scope_id: frame.scope_id,
            workspace_id: frame.workspace_id,
            implicit_ans: Some(self.lookup_current_binding(
                frame.scope_id,
                "ans",
                SymbolKind::Variable,
            )),
            inputs: function
                .inputs
                .iter()
                .map(|input| {
                    self.lookup_current_binding(frame.scope_id, &input.name, SymbolKind::Parameter)
                })
                .collect(),
            outputs: function
                .outputs
                .iter()
                .map(|output| {
                    self.lookup_current_binding(frame.scope_id, &output.name, SymbolKind::Output)
                })
                .collect(),
            captures: frame.captures,
            body: function
                .body
                .iter()
                .map(|statement| self.lower_statement(statement, frame.scope_id))
                .collect(),
            local_functions: function
                .local_functions
                .iter()
                .map(|local| self.lower_function(local))
                .collect(),
        }
    }

    fn lower_statement(&mut self, statement: &Statement, scope_id: ScopeId) -> HirStatement {
        match &statement.kind {
            StatementKind::Assignment { targets, value } => HirStatement::Assignment {
                targets: targets
                    .iter()
                    .map(|target| self.lower_assignment_target(target, scope_id))
                    .collect(),
                value: self.lower_expression(value, scope_id),
                display_suppressed: statement.display_suppressed,
            },
            StatementKind::Expression(expression) => HirStatement::Expression {
                expression: self.lower_statement_expression(expression, scope_id),
                display_suppressed: statement.display_suppressed,
            },
            StatementKind::If {
                branches,
                else_body,
            } => HirStatement::If {
                branches: branches
                    .iter()
                    .map(|branch| HirConditionalBranch {
                        condition: self.lower_expression(&branch.condition, scope_id),
                        body: branch
                            .body
                            .iter()
                            .map(|statement| self.lower_statement(statement, scope_id))
                            .collect(),
                    })
                    .collect(),
                else_body: else_body
                    .iter()
                    .map(|statement| self.lower_statement(statement, scope_id))
                    .collect(),
            },
            StatementKind::Switch {
                expression,
                cases,
                otherwise_body,
            } => HirStatement::Switch {
                expression: self.lower_expression(expression, scope_id),
                cases: cases
                    .iter()
                    .map(|case| HirSwitchCase {
                        matcher: self.lower_expression(&case.matcher, scope_id),
                        body: case
                            .body
                            .iter()
                            .map(|statement| self.lower_statement(statement, scope_id))
                            .collect(),
                    })
                    .collect(),
                otherwise_body: otherwise_body
                    .iter()
                    .map(|statement| self.lower_statement(statement, scope_id))
                    .collect(),
            },
            StatementKind::Try {
                body,
                catch_binding,
                catch_body,
            } => HirStatement::Try {
                body: body
                    .iter()
                    .map(|statement| self.lower_statement(statement, scope_id))
                    .collect(),
                catch_binding: catch_binding.as_ref().map(|binding| {
                    self.lookup_assignment_binding(scope_id, &binding.name, SymbolKind::Variable)
                }),
                catch_body: catch_body
                    .iter()
                    .map(|statement| self.lower_statement(statement, scope_id))
                    .collect(),
            },
            StatementKind::For {
                variable,
                iterable,
                body,
            } => HirStatement::For {
                variable: self.lookup_current_binding(
                    scope_id,
                    &variable.name,
                    SymbolKind::Variable,
                ),
                iterable: self.lower_expression(iterable, scope_id),
                body: body
                    .iter()
                    .map(|statement| self.lower_statement(statement, scope_id))
                    .collect(),
            },
            StatementKind::While { condition, body } => HirStatement::While {
                condition: self.lower_expression(condition, scope_id),
                body: body
                    .iter()
                    .map(|statement| self.lower_statement(statement, scope_id))
                    .collect(),
            },
            StatementKind::Break => HirStatement::Break,
            StatementKind::Continue => HirStatement::Continue,
            StatementKind::Return => HirStatement::Return,
            StatementKind::Global(names) => HirStatement::Global(
                names
                    .iter()
                    .map(|name| {
                        self.lookup_current_binding(scope_id, &name.name, SymbolKind::Global)
                    })
                    .collect(),
            ),
            StatementKind::Persistent(names) => HirStatement::Persistent(
                names
                    .iter()
                    .map(|name| {
                        self.lookup_current_binding(scope_id, &name.name, SymbolKind::Persistent)
                    })
                    .collect(),
            ),
        }
    }

    fn lower_assignment_target(
        &mut self,
        target: &AssignmentTarget,
        scope_id: ScopeId,
    ) -> HirAssignmentTarget {
        match target {
            AssignmentTarget::Identifier(identifier) => HirAssignmentTarget::Binding(
                self.lookup_assignment_binding(scope_id, &identifier.name, SymbolKind::Variable),
            ),
            AssignmentTarget::Index { target, indices } => HirAssignmentTarget::Index {
                target: Box::new(self.lower_expression(target, scope_id)),
                indices: self.lower_index_arguments(indices, scope_id),
            },
            AssignmentTarget::CellIndex { target, indices } => HirAssignmentTarget::CellIndex {
                target: Box::new(self.lower_expression(target, scope_id)),
                indices: self.lower_index_arguments(indices, scope_id),
            },
            AssignmentTarget::Field { target, field } => HirAssignmentTarget::Field {
                target: Box::new(self.lower_expression(target, scope_id)),
                field: field.name.clone(),
            },
        }
    }

    fn lower_expression(&mut self, expression: &Expression, scope_id: ScopeId) -> HirExpression {
        match &expression.kind {
            ExpressionKind::Identifier(identifier) => {
                HirExpression::ValueRef(self.lower_value_reference(identifier))
            }
            ExpressionKind::NumberLiteral(text) => HirExpression::NumberLiteral(text.clone()),
            ExpressionKind::CharLiteral(text) => HirExpression::CharLiteral(text.clone()),
            ExpressionKind::StringLiteral(text) => HirExpression::StringLiteral(text.clone()),
            ExpressionKind::MatrixLiteral(rows) => HirExpression::MatrixLiteral(
                rows.iter()
                    .map(|row| {
                        row.iter()
                            .map(|expression| self.lower_expression(expression, scope_id))
                            .collect()
                    })
                    .collect(),
            ),
            ExpressionKind::CellLiteral(rows) => HirExpression::CellLiteral(
                rows.iter()
                    .map(|row| {
                        row.iter()
                            .map(|expression| self.lower_expression(expression, scope_id))
                            .collect()
                    })
                    .collect(),
            ),
            ExpressionKind::FunctionHandle(name) => {
                HirExpression::FunctionHandle(self.lower_callable_reference(
                    &qualified_name_string(name),
                    name.span,
                    ReferenceRole::FunctionHandleTarget,
                ))
            }
            ExpressionKind::EndKeyword => HirExpression::EndKeyword,
            ExpressionKind::Unary { op, rhs } => HirExpression::Unary {
                op: *op,
                rhs: Box::new(self.lower_expression(rhs, scope_id)),
            },
            ExpressionKind::Binary { op, lhs, rhs } => HirExpression::Binary {
                op: *op,
                lhs: Box::new(self.lower_expression(lhs, scope_id)),
                rhs: Box::new(self.lower_expression(rhs, scope_id)),
            },
            ExpressionKind::Range { start, step, end } => HirExpression::Range {
                start: Box::new(self.lower_expression(start, scope_id)),
                step: step
                    .as_ref()
                    .map(|step| Box::new(self.lower_expression(step, scope_id))),
                end: Box::new(self.lower_expression(end, scope_id)),
            },
            ExpressionKind::ParenApply { target, indices } => HirExpression::Call {
                target: self.lower_call_target(target, scope_id),
                args: self.lower_index_arguments(indices, scope_id),
            },
            ExpressionKind::CellIndex { target, indices } => HirExpression::CellIndex {
                target: Box::new(self.lower_expression(target, scope_id)),
                indices: self.lower_index_arguments(indices, scope_id),
            },
            ExpressionKind::FieldAccess { target, field } => HirExpression::FieldAccess {
                target: Box::new(self.lower_expression(target, scope_id)),
                field: field.name.clone(),
            },
            ExpressionKind::AnonymousFunction { params, body } => {
                let frame = self
                    .anonymous_frames
                    .pop_front()
                    .expect("anonymous function frame should exist during lowering");
                HirExpression::AnonymousFunction(HirAnonymousFunction {
                    scope_id: frame.scope_id,
                    workspace_id: frame.workspace_id,
                    params: params
                        .iter()
                        .map(|param| {
                            self.lookup_current_binding(
                                frame.scope_id,
                                &param.name,
                                SymbolKind::Parameter,
                            )
                        })
                        .collect(),
                    captures: frame.captures,
                    body: Box::new(self.lower_expression(body, frame.scope_id)),
                })
            }
        }
    }

    fn lower_statement_expression(
        &mut self,
        expression: &Expression,
        scope_id: ScopeId,
    ) -> HirExpression {
        if let Some((name, span)) = call_target_name_span(expression) {
            if let Some(reference) = self.lookup_reference(&name, span, ReferenceRole::CallTarget) {
                if reference.resolution != ReferenceResolution::WorkspaceValue {
                    return HirExpression::Call {
                        target: self.lower_call_target(expression, scope_id),
                        args: Vec::new(),
                    };
                }
            }
        }

        self.lower_expression(expression, scope_id)
    }

    fn lower_call_target(&mut self, target: &Expression, scope_id: ScopeId) -> HirCallTarget {
        if let Some((name, span)) = call_target_name_span(target) {
            if self
                .lookup_reference(&name, span, ReferenceRole::CallTarget)
                .is_some()
            {
                return HirCallTarget::Callable(self.lower_callable_reference(
                    &name,
                    span,
                    ReferenceRole::CallTarget,
                ));
            }
        }

        HirCallTarget::Expression(Box::new(self.lower_expression(target, scope_id)))
    }

    fn lower_index_arguments(
        &mut self,
        indices: &[IndexArgument],
        scope_id: ScopeId,
    ) -> Vec<HirIndexArgument> {
        indices
            .iter()
            .map(|index| match index {
                IndexArgument::Expression(expression) => {
                    HirIndexArgument::Expression(self.lower_expression(expression, scope_id))
                }
                IndexArgument::FullSlice => HirIndexArgument::FullSlice,
                IndexArgument::End => HirIndexArgument::End,
            })
            .collect()
    }

    fn lower_value_reference(&self, identifier: &Identifier) -> HirValueRef {
        if let Some(reference) =
            self.lookup_reference(&identifier.name, identifier.span, ReferenceRole::Value)
        {
            HirValueRef {
                name: reference.name.clone(),
                resolution: reference.resolution.clone(),
                binding_id: reference.binding_id,
                symbol_kind: reference.resolved_kind,
                capture_access: reference.capture_access,
            }
        } else {
            HirValueRef {
                name: identifier.name.clone(),
                resolution: ReferenceResolution::UnresolvedValue,
                binding_id: None,
                symbol_kind: None,
                capture_access: None,
            }
        }
    }

    fn lower_callable_reference(
        &self,
        name: &str,
        span: SourceSpan,
        role: ReferenceRole,
    ) -> HirCallableRef {
        if let Some(reference) = self.lookup_reference(name, span, role) {
            HirCallableRef {
                name: reference.name.clone(),
                semantic_resolution: reference.resolution.clone(),
                final_resolution: self
                    .lookup_resolved_reference(name, span, role)
                    .map(|resolved| resolved.final_resolution.clone()),
                resolved_symbol: reference.resolved_symbol,
                resolved_kind: reference.resolved_kind,
                binding_id: reference.binding_id,
                capture_access: reference.capture_access,
            }
        } else {
            HirCallableRef {
                name: name.to_string(),
                semantic_resolution: ReferenceResolution::ExternalFunctionCandidate,
                final_resolution: None,
                resolved_symbol: None,
                resolved_kind: None,
                binding_id: None,
                capture_access: None,
            }
        }
    }

    fn lookup_reference(
        &self,
        name: &str,
        span: SourceSpan,
        role: ReferenceRole,
    ) -> Option<&'a SymbolReference> {
        self.references
            .get(&reference_key(name, span, role))
            .copied()
    }

    fn lookup_resolved_reference(
        &self,
        name: &str,
        span: SourceSpan,
        role: ReferenceRole,
    ) -> Option<&'a ResolvedReference> {
        self.resolved_references
            .get(&reference_key(name, span, role))
            .copied()
    }

    fn lookup_current_binding(
        &self,
        scope_id: ScopeId,
        name: &str,
        fallback_kind: SymbolKind,
    ) -> HirBinding {
        self.scope_values
            .get(&scope_id)
            .and_then(|values| values.get(name))
            .and_then(|symbol_id| self.symbols_by_id.get(symbol_id))
            .map(|symbol| self.binding_from_symbol(symbol))
            .unwrap_or_else(|| HirBinding {
                name: name.to_string(),
                symbol_kind: fallback_kind,
                binding_id: None,
                storage: fallback_kind.binding_storage(),
            })
    }

    fn lookup_assignment_binding(
        &self,
        scope_id: ScopeId,
        name: &str,
        fallback_kind: SymbolKind,
    ) -> HirBinding {
        if let Some(symbol) = self.lookup_assignment_symbol(scope_id, name) {
            self.binding_from_symbol(symbol)
        } else {
            HirBinding {
                name: name.to_string(),
                symbol_kind: fallback_kind,
                binding_id: None,
                storage: fallback_kind.binding_storage(),
            }
        }
    }

    fn lookup_assignment_symbol(&self, scope_id: ScopeId, name: &str) -> Option<&'a Symbol> {
        if let Some(symbol) = self
            .scope_values
            .get(&scope_id)
            .and_then(|values| values.get(name))
            .and_then(|symbol_id| self.symbols_by_id.get(symbol_id))
        {
            return Some(symbol);
        }

        let mut current = self.scope_parent.get(&scope_id).copied().flatten();
        while let Some(scope_id) = current {
            if let Some(symbol) = self
                .scope_values
                .get(&scope_id)
                .and_then(|values| values.get(name))
                .and_then(|symbol_id| self.symbols_by_id.get(symbol_id))
            {
                if symbol.kind.is_capture_eligible() {
                    return Some(symbol);
                }
            }
            current = self.scope_parent.get(&scope_id).copied().flatten();
        }

        None
    }

    fn binding_from_symbol(&self, symbol: &Symbol) -> HirBinding {
        HirBinding {
            name: symbol.name.clone(),
            symbol_kind: symbol.kind,
            binding_id: symbol.binding_id,
            storage: symbol.binding_id.and_then(|binding_id| {
                self.bindings_by_id
                    .get(&binding_id)
                    .map(|binding| binding.storage)
            }),
        }
    }
}

fn build_captures_by_scope(analysis: &AnalysisResult) -> HashMap<ScopeId, Vec<HirCapture>> {
    let mut captures_by_scope = HashMap::new();
    for capture in &analysis.captures {
        captures_by_scope
            .entry(capture.into_scope)
            .or_insert_with(Vec::new)
            .push(HirCapture {
                name: capture.name.clone(),
                binding_id: capture.binding_id,
                access: capture.access,
                from_scope: capture.from_scope,
                from_workspace: capture.from_workspace,
            });
    }
    captures_by_scope
}

fn call_target_name_span(target: &Expression) -> Option<(String, SourceSpan)> {
    match &target.kind {
        ExpressionKind::Identifier(identifier) => Some((identifier.name.clone(), identifier.span)),
        _ => expression_as_qualified_name(target)
            .map(|name| (qualified_name_string(&name), name.span)),
    }
}

fn expression_as_qualified_name(expression: &Expression) -> Option<QualifiedName> {
    match &expression.kind {
        ExpressionKind::Identifier(identifier) => Some(QualifiedName {
            segments: vec![identifier.clone()],
            span: identifier.span,
        }),
        ExpressionKind::FieldAccess { target, field } => {
            let mut qualified = expression_as_qualified_name(target)?;
            qualified.segments.push(field.clone());
            qualified.span = expression.span;
            Some(qualified)
        }
        _ => None,
    }
}

fn qualified_name_string(name: &QualifiedName) -> String {
    name.segments
        .iter()
        .map(|segment| segment.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn reference_key(name: &str, span: SourceSpan, role: ReferenceRole) -> ReferenceKey {
    ReferenceKey {
        name: name.to_string(),
        role: reference_role_key(role),
        start: span.start.offset,
        end: span.end.offset,
    }
}

fn reference_role_key(role: ReferenceRole) -> u8 {
    match role {
        ReferenceRole::Value => 0,
        ReferenceRole::CallTarget => 1,
        ReferenceRole::FunctionHandleTarget => 2,
    }
}
