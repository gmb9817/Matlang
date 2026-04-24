//! Optimizer crate for IR transformations and canonicalization passes.

use matlab_frontend::ast::{BinaryOp, UnaryOp};
use matlab_ir::{
    HirAnonymousFunction, HirAssignmentTarget, HirCallTarget, HirConditionalBranch, HirExpression,
    HirFunction, HirFunctionHandleTarget, HirIndexArgument, HirItem, HirModule, HirStatement,
    HirSwitchCase,
};

pub const CRATE_NAME: &str = "matlab-optimizer";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptimizationSummary {
    pub constant_folds: usize,
    pub branch_eliminations: usize,
    pub loop_eliminations: usize,
    pub statement_eliminations: usize,
}

impl OptimizationSummary {
    pub fn changed(&self) -> bool {
        self.constant_folds > 0
            || self.branch_eliminations > 0
            || self.loop_eliminations > 0
            || self.statement_eliminations > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptimizationResult {
    pub module: HirModule,
    pub summary: OptimizationSummary,
}

#[derive(Default)]
struct Optimizer {
    summary: OptimizationSummary,
}

pub fn optimize_module(module: &HirModule) -> OptimizationResult {
    let mut optimizer = Optimizer::default();
    let module = optimizer.optimize_module(module);
    OptimizationResult {
        module,
        summary: optimizer.summary,
    }
}

pub fn render_optimization_summary(summary: &OptimizationSummary) -> String {
    format!(
        "optimization\n  changed = {}\n  constant_folds = {}\n  branch_eliminations = {}\n  loop_eliminations = {}\n  statement_eliminations = {}\n",
        summary.changed(),
        summary.constant_folds,
        summary.branch_eliminations,
        summary.loop_eliminations,
        summary.statement_eliminations
    )
}

impl Optimizer {
    fn optimize_module(&mut self, module: &HirModule) -> HirModule {
        HirModule {
            kind: module.kind,
            scope_id: module.scope_id,
            workspace_id: module.workspace_id,
            implicit_ans: module.implicit_ans.clone(),
            classes: module.classes.clone(),
            items: self.optimize_items(&module.items),
        }
    }

    fn optimize_items(&mut self, items: &[HirItem]) -> Vec<HirItem> {
        let mut optimized = Vec::new();
        for item in items {
            match item {
                HirItem::Statement(statement) => {
                    optimized.extend(
                        self.optimize_statement(statement)
                            .into_iter()
                            .map(HirItem::Statement),
                    );
                }
                HirItem::Function(function) => {
                    optimized.push(HirItem::Function(self.optimize_function(function)));
                }
            }
        }
        optimized
    }

    fn optimize_function(&mut self, function: &HirFunction) -> HirFunction {
        HirFunction {
            name: function.name.clone(),
            owner_class_name: function.owner_class_name.clone(),
            scope_id: function.scope_id,
            workspace_id: function.workspace_id,
            implicit_ans: function.implicit_ans.clone(),
            inputs: function.inputs.clone(),
            outputs: function.outputs.clone(),
            captures: function.captures.clone(),
            body: self.optimize_block(&function.body),
            local_functions: function
                .local_functions
                .iter()
                .map(|local| self.optimize_function(local))
                .collect(),
        }
    }

    fn optimize_block(&mut self, statements: &[HirStatement]) -> Vec<HirStatement> {
        let mut optimized = Vec::new();
        for statement in statements {
            optimized.extend(self.optimize_statement(statement));
        }
        optimized
    }

    fn optimize_statement(&mut self, statement: &HirStatement) -> Vec<HirStatement> {
        match statement {
            HirStatement::Assignment {
                targets,
                value,
                list_assignment,
                display_suppressed,
            } => vec![HirStatement::Assignment {
                targets: targets
                    .iter()
                    .map(|target| self.optimize_assignment_target(target))
                    .collect(),
                value: self.optimize_expression(value),
                list_assignment: *list_assignment,
                display_suppressed: *display_suppressed,
            }],
            HirStatement::Expression {
                expression,
                display_suppressed,
            } => vec![HirStatement::Expression {
                expression: self.optimize_expression(expression),
                display_suppressed: *display_suppressed,
            }],
            HirStatement::If {
                branches,
                else_body,
            } => self.optimize_if(branches, else_body),
            HirStatement::Switch {
                expression,
                cases,
                otherwise_body,
            } => self.optimize_switch(expression, cases, otherwise_body),
            HirStatement::Try {
                body,
                catch_binding,
                catch_body,
            } => vec![HirStatement::Try {
                body: self.optimize_block(body),
                catch_binding: catch_binding.clone(),
                catch_body: self.optimize_block(catch_body),
            }],
            HirStatement::For {
                variable,
                iterable,
                body,
            } => {
                let iterable = self.optimize_expression(iterable);
                if is_empty_matrix_literal(&iterable) {
                    self.summary.loop_eliminations += 1;
                    self.summary.statement_eliminations += 1;
                    Vec::new()
                } else {
                    vec![HirStatement::For {
                        variable: variable.clone(),
                        iterable,
                        body: self.optimize_block(body),
                    }]
                }
            }
            HirStatement::While { condition, body } => {
                let condition = self.optimize_expression(condition);
                if matches!(constant_truthy(&condition), Some(false)) {
                    self.summary.loop_eliminations += 1;
                    self.summary.statement_eliminations += 1;
                    Vec::new()
                } else {
                    vec![HirStatement::While {
                        condition,
                        body: self.optimize_block(body),
                    }]
                }
            }
            HirStatement::Break => vec![HirStatement::Break],
            HirStatement::Continue => vec![HirStatement::Continue],
            HirStatement::Return => vec![HirStatement::Return],
            HirStatement::Global(bindings) => vec![HirStatement::Global(bindings.clone())],
            HirStatement::Persistent(bindings) => {
                vec![HirStatement::Persistent(bindings.clone())]
            }
        }
    }

    fn optimize_if(
        &mut self,
        branches: &[HirConditionalBranch],
        else_body: &[HirStatement],
    ) -> Vec<HirStatement> {
        let mut optimized_branches = Vec::new();
        let optimized_else_body = self.optimize_block(else_body);

        for branch in branches {
            let condition = self.optimize_expression(&branch.condition);
            let body = self.optimize_block(&branch.body);
            match constant_truthy(&condition) {
                Some(false) => {
                    self.summary.branch_eliminations += 1;
                }
                Some(true) => {
                    self.summary.branch_eliminations += 1;
                    return body;
                }
                None => optimized_branches.push(HirConditionalBranch { condition, body }),
            }
        }

        if optimized_branches.is_empty() {
            if !branches.is_empty() {
                self.summary.statement_eliminations += 1;
            }
            return optimized_else_body;
        }

        vec![HirStatement::If {
            branches: optimized_branches,
            else_body: optimized_else_body,
        }]
    }

    fn optimize_switch(
        &mut self,
        expression: &HirExpression,
        cases: &[HirSwitchCase],
        otherwise_body: &[HirStatement],
    ) -> Vec<HirStatement> {
        let expression = self.optimize_expression(expression);
        let optimized_cases = cases
            .iter()
            .map(|case| HirSwitchCase {
                matcher: self.optimize_expression(&case.matcher),
                body: self.optimize_block(&case.body),
            })
            .collect::<Vec<_>>();
        let optimized_otherwise = self.optimize_block(otherwise_body);

        if let Some(value) = scalar_literal_value(&expression) {
            let mut all_literal = true;
            for case in &optimized_cases {
                match scalar_literal_value(&case.matcher) {
                    Some(matcher) if matcher == value => {
                        self.summary.branch_eliminations += 1;
                        self.summary.statement_eliminations += 1;
                        return case.body.clone();
                    }
                    Some(_) => {}
                    None => {
                        all_literal = false;
                        break;
                    }
                }
            }
            if all_literal {
                self.summary.branch_eliminations += 1;
                self.summary.statement_eliminations += 1;
                return optimized_otherwise;
            }
        }

        vec![HirStatement::Switch {
            expression,
            cases: optimized_cases,
            otherwise_body: optimized_otherwise,
        }]
    }

    fn optimize_assignment_target(&mut self, target: &HirAssignmentTarget) -> HirAssignmentTarget {
        match target {
            HirAssignmentTarget::Binding(binding) => HirAssignmentTarget::Binding(binding.clone()),
            HirAssignmentTarget::Index { target, indices } => HirAssignmentTarget::Index {
                target: Box::new(self.optimize_expression(target)),
                indices: indices
                    .iter()
                    .map(|index| self.optimize_index_argument(index))
                    .collect(),
            },
            HirAssignmentTarget::CellIndex { target, indices } => HirAssignmentTarget::CellIndex {
                target: Box::new(self.optimize_expression(target)),
                indices: indices
                    .iter()
                    .map(|index| self.optimize_index_argument(index))
                    .collect(),
            },
            HirAssignmentTarget::Field { target, field } => HirAssignmentTarget::Field {
                target: Box::new(self.optimize_expression(target)),
                field: field.clone(),
            },
        }
    }

    fn optimize_call_target(&mut self, target: &HirCallTarget) -> HirCallTarget {
        match target {
            HirCallTarget::Callable(reference) => HirCallTarget::Callable(reference.clone()),
            HirCallTarget::Expression(expression) => {
                HirCallTarget::Expression(Box::new(self.optimize_expression(expression)))
            }
        }
    }

    fn optimize_index_argument(&mut self, argument: &HirIndexArgument) -> HirIndexArgument {
        match argument {
            HirIndexArgument::Expression(expression) => {
                HirIndexArgument::Expression(self.optimize_expression(expression))
            }
            HirIndexArgument::FullSlice => HirIndexArgument::FullSlice,
            HirIndexArgument::End => HirIndexArgument::End,
        }
    }

    fn optimize_expression(&mut self, expression: &HirExpression) -> HirExpression {
        match expression {
            HirExpression::ValueRef(reference) => HirExpression::ValueRef(reference.clone()),
            HirExpression::NumberLiteral(text) => HirExpression::NumberLiteral(text.clone()),
            HirExpression::CharLiteral(text) => HirExpression::CharLiteral(text.clone()),
            HirExpression::StringLiteral(text) => HirExpression::StringLiteral(text.clone()),
            HirExpression::MatrixLiteral(rows) => HirExpression::MatrixLiteral(
                rows.iter()
                    .map(|row| {
                        row.iter()
                            .map(|expr| self.optimize_expression(expr))
                            .collect()
                    })
                    .collect(),
            ),
            HirExpression::CellLiteral(rows) => HirExpression::CellLiteral(
                rows.iter()
                    .map(|row| {
                        row.iter()
                            .map(|expr| self.optimize_expression(expr))
                            .collect()
                    })
                    .collect(),
            ),
            HirExpression::FunctionHandle(target) => HirExpression::FunctionHandle(match target {
                HirFunctionHandleTarget::Callable(reference) => {
                    HirFunctionHandleTarget::Callable(reference.clone())
                }
                HirFunctionHandleTarget::Expression(expression) => {
                    HirFunctionHandleTarget::Expression(Box::new(
                        self.optimize_expression(expression),
                    ))
                }
            }),
            HirExpression::EndKeyword => HirExpression::EndKeyword,
            HirExpression::Unary { op, rhs } => {
                let rhs = self.optimize_expression(rhs);
                if let Some(folded) = try_fold_unary(*op, &rhs) {
                    self.summary.constant_folds += 1;
                    folded
                } else {
                    HirExpression::Unary {
                        op: *op,
                        rhs: Box::new(rhs),
                    }
                }
            }
            HirExpression::Binary { op, lhs, rhs } => {
                let lhs = self.optimize_expression(lhs);
                let rhs = self.optimize_expression(rhs);
                if let Some(folded) = try_fold_binary(*op, &lhs, &rhs) {
                    self.summary.constant_folds += 1;
                    folded
                } else if let Some(simplified) = simplify_binary_identity(*op, &lhs, &rhs) {
                    self.summary.constant_folds += 1;
                    simplified
                } else {
                    HirExpression::Binary {
                        op: *op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    }
                }
            }
            HirExpression::Range { start, step, end } => {
                let start = self.optimize_expression(start);
                let step = step
                    .as_ref()
                    .map(|value| Box::new(self.optimize_expression(value)));
                let end = self.optimize_expression(end);
                if let Some(folded) = try_fold_range(&start, step.as_deref(), &end) {
                    self.summary.constant_folds += 1;
                    folded
                } else {
                    HirExpression::Range {
                        start: Box::new(start),
                        step,
                        end: Box::new(end),
                    }
                }
            }
            HirExpression::Call { target, args } => HirExpression::Call {
                target: self.optimize_call_target(target),
                args: args
                    .iter()
                    .map(|index| self.optimize_index_argument(index))
                    .collect(),
            },
            HirExpression::CellIndex { target, indices } => HirExpression::CellIndex {
                target: Box::new(self.optimize_expression(target)),
                indices: indices
                    .iter()
                    .map(|index| self.optimize_index_argument(index))
                    .collect(),
            },
            HirExpression::FieldAccess { target, field } => HirExpression::FieldAccess {
                target: Box::new(self.optimize_expression(target)),
                field: field.clone(),
            },
            HirExpression::AnonymousFunction(anonymous) => {
                HirExpression::AnonymousFunction(self.optimize_anonymous(anonymous))
            }
        }
    }

    fn optimize_anonymous(&mut self, anonymous: &HirAnonymousFunction) -> HirAnonymousFunction {
        HirAnonymousFunction {
            scope_id: anonymous.scope_id,
            workspace_id: anonymous.workspace_id,
            params: anonymous.params.clone(),
            captures: anonymous.captures.clone(),
            body: Box::new(self.optimize_expression(&anonymous.body)),
        }
    }
}

fn try_fold_unary(op: UnaryOp, rhs: &HirExpression) -> Option<HirExpression> {
    let rhs = scalar_literal_value(rhs)?;
    Some(match op {
        UnaryOp::Plus => number_literal(rhs),
        UnaryOp::Minus => number_literal(-rhs),
        UnaryOp::LogicalNot => number_literal(truth_number(rhs == 0.0)),
        _ => return None,
    })
}

fn try_fold_binary(
    op: BinaryOp,
    lhs: &HirExpression,
    rhs: &HirExpression,
) -> Option<HirExpression> {
    let lhs = scalar_literal_value(lhs)?;
    let rhs = scalar_literal_value(rhs)?;
    let value = match op {
        BinaryOp::Add => lhs + rhs,
        BinaryOp::Subtract => lhs - rhs,
        BinaryOp::Multiply | BinaryOp::ElementwiseMultiply => lhs * rhs,
        BinaryOp::MatrixRightDivide
        | BinaryOp::MatrixLeftDivide
        | BinaryOp::ElementwiseRightDivide
        | BinaryOp::ElementwiseLeftDivide => lhs / rhs,
        BinaryOp::Power | BinaryOp::ElementwisePower => lhs.powf(rhs),
        BinaryOp::LessThan => truth_number(lhs < rhs),
        BinaryOp::LessThanOrEqual => truth_number(lhs <= rhs),
        BinaryOp::GreaterThan => truth_number(lhs > rhs),
        BinaryOp::GreaterThanOrEqual => truth_number(lhs >= rhs),
        BinaryOp::Equal => truth_number(lhs == rhs),
        BinaryOp::NotEqual => truth_number(lhs != rhs),
        BinaryOp::LogicalAnd | BinaryOp::ShortCircuitAnd => truth_number(lhs != 0.0 && rhs != 0.0),
        BinaryOp::LogicalOr | BinaryOp::ShortCircuitOr => truth_number(lhs != 0.0 || rhs != 0.0),
        _ => return None,
    };
    Some(number_literal(value))
}

fn simplify_binary_identity(
    op: BinaryOp,
    lhs: &HirExpression,
    rhs: &HirExpression,
) -> Option<HirExpression> {
    match op {
        BinaryOp::Add => {
            if is_zero_literal(lhs) {
                Some(rhs.clone())
            } else if is_zero_literal(rhs) {
                Some(lhs.clone())
            } else {
                None
            }
        }
        BinaryOp::Subtract => {
            if is_zero_literal(rhs) {
                Some(lhs.clone())
            } else {
                None
            }
        }
        BinaryOp::Multiply | BinaryOp::ElementwiseMultiply => {
            if is_zero_literal(lhs) || is_zero_literal(rhs) {
                Some(number_literal(0.0))
            } else if is_one_literal(lhs) {
                Some(rhs.clone())
            } else if is_one_literal(rhs) {
                Some(lhs.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn try_fold_range(
    start: &HirExpression,
    step: Option<&HirExpression>,
    end: &HirExpression,
) -> Option<HirExpression> {
    let start = scalar_literal_value(start)?;
    let step = step.and_then(scalar_literal_value).unwrap_or(1.0);
    let end = scalar_literal_value(end)?;
    if step == 0.0 {
        return None;
    }

    let mut values = Vec::new();
    let mut current = start;
    if step > 0.0 {
        while current <= end {
            values.push(number_literal(current));
            current += step;
        }
    } else {
        while current >= end {
            values.push(number_literal(current));
            current += step;
        }
    }

    if values.is_empty() {
        Some(HirExpression::MatrixLiteral(Vec::new()))
    } else {
        Some(HirExpression::MatrixLiteral(vec![values]))
    }
}

fn scalar_literal_value(expression: &HirExpression) -> Option<f64> {
    match expression {
        HirExpression::NumberLiteral(text) => text.parse::<f64>().ok(),
        HirExpression::MatrixLiteral(rows) if rows.len() == 1 && rows[0].len() == 1 => {
            scalar_literal_value(&rows[0][0])
        }
        _ => None,
    }
}

fn constant_truthy(expression: &HirExpression) -> Option<bool> {
    Some(scalar_literal_value(expression)? != 0.0)
}

fn is_empty_matrix_literal(expression: &HirExpression) -> bool {
    matches!(expression, HirExpression::MatrixLiteral(rows) if rows.is_empty())
}

fn is_zero_literal(expression: &HirExpression) -> bool {
    matches!(scalar_literal_value(expression), Some(value) if value == 0.0)
}

fn is_one_literal(expression: &HirExpression) -> bool {
    matches!(scalar_literal_value(expression), Some(value) if value == 1.0)
}

fn number_literal(value: f64) -> HirExpression {
    HirExpression::NumberLiteral(format_number(value))
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn truth_number(value: bool) -> f64 {
    if value {
        1.0
    } else {
        0.0
    }
}

pub fn summary() -> &'static str {
    "Owns HIR canonicalization, constant folding, branch cleanup, and future optimization passes."
}

#[cfg(test)]
mod tests {
    use super::{optimize_module, OptimizationSummary};
    use matlab_frontend::{
        parser::{parse_source, ParseMode},
        source::SourceFileId,
    };
    use matlab_ir::lower_to_hir;
    use matlab_resolver::ResolverContext;
    use matlab_semantics::analyze_compilation_unit_with_context;

    fn optimize_source(source: &str) -> OptimizationSummary {
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(
            !parsed.has_errors(),
            "frontend diagnostics: {:?}",
            parsed.diagnostics
        );
        let unit = parsed.unit.expect("compilation unit");
        let analysis =
            analyze_compilation_unit_with_context(&unit, &ResolverContext::new(None, Vec::new()));
        assert!(
            !analysis.has_errors(),
            "semantic diagnostics: {:?}",
            analysis.diagnostics
        );
        let hir = lower_to_hir(&unit, &analysis);
        optimize_module(&hir).summary
    }

    #[test]
    fn reports_constant_folding_for_literal_math() {
        let summary = optimize_source("x = 1 + 2 * 3;");
        assert!(summary.constant_folds >= 2);
    }

    #[test]
    fn reports_branch_elimination_for_constant_if() {
        let summary = optimize_source("if 1; x = 1; else; x = 2; end");
        assert!(summary.branch_eliminations >= 1);
    }

    #[test]
    fn reports_loop_elimination_for_empty_iterable() {
        let summary = optimize_source("for i = []; x = i; end");
        assert!(summary.loop_eliminations >= 1);
    }
}
