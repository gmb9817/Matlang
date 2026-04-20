//! Code generation crate for backend emission and linking.

use std::collections::BTreeSet;

use matlab_ir::{
    HirAnonymousFunction, HirAssignmentTarget, HirBinding, HirCallTarget, HirCallableRef,
    HirConditionalBranch, HirExpression, HirFunction, HirFunctionHandleTarget, HirIndexArgument,
    HirItem, HirModule, HirStatement, HirSwitchCase, HirValueRef,
};

pub const CRATE_NAME: &str = "matlab-codegen";

fn qualified_class_name(name: &str, package: Option<&str>) -> String {
    match package {
        Some(package) if !package.is_empty() => format!("{package}.{name}"),
        _ => name.to_string(),
    }
}

pub type TempId = u32;
pub type LabelId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Bytecode,
    C,
    Llvm,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bytecode => "bytecode",
            Self::C => "c",
            Self::Llvm => "llvm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeModule {
    pub backend: BackendKind,
    pub unit_kind: String,
    pub entry: String,
    pub classes: Vec<BytecodeClass>,
    pub functions: Vec<BytecodeFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeClass {
    pub name: String,
    pub package: Option<String>,
    pub superclass_name: Option<String>,
    pub superclass_path: Option<String>,
    pub superclass_bundle_module_id: Option<String>,
    pub inherits_handle: bool,
    pub source_path: Option<String>,
    pub property_names: Vec<String>,
    pub private_property_names: Vec<String>,
    pub default_initializer: Option<String>,
    pub constructor: Option<String>,
    pub inline_methods: Vec<String>,
    pub static_inline_methods: Vec<String>,
    pub private_inline_methods: Vec<String>,
    pub private_static_inline_methods: Vec<String>,
    pub external_methods: Vec<BytecodeExternalMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeExternalMethod {
    pub name: String,
    pub path: Option<String>,
    pub bundle_module_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeFunction {
    pub name: String,
    pub role: String,
    pub owner_class_name: Option<String>,
    pub params: Vec<String>,
    pub outputs: Vec<String>,
    pub captures: Vec<String>,
    pub temp_count: TempId,
    pub label_count: LabelId,
    pub instructions: Vec<BytecodeInstruction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytecodeInstruction {
    Label(LabelId),
    LoadConst {
        dst: TempId,
        value: String,
    },
    LoadBinding {
        dst: TempId,
        binding: String,
    },
    LoadBindingLValue {
        dst: TempId,
        binding: String,
    },
    StoreBinding {
        binding: String,
        src: TempId,
    },
    StoreBindingIfPresent {
        binding: String,
        src: TempId,
    },
    Unary {
        dst: TempId,
        op: String,
        src: TempId,
    },
    Binary {
        dst: TempId,
        op: String,
        lhs: TempId,
        rhs: TempId,
    },
    BuildMatrix {
        dst: TempId,
        rows: usize,
        cols: usize,
        elements: Vec<TempId>,
    },
    BuildMatrixList {
        dst: TempId,
        row_item_counts: Vec<usize>,
        elements: Vec<String>,
    },
    BuildCell {
        dst: TempId,
        rows: usize,
        cols: usize,
        elements: Vec<TempId>,
    },
    BuildCellList {
        dst: TempId,
        row_item_counts: Vec<usize>,
        elements: Vec<String>,
    },
    PackSpreadMatrix {
        dst: TempId,
        src: TempId,
    },
    PackSpreadCell {
        dst: TempId,
        src: TempId,
    },
    MakeHandle {
        dst: TempId,
        target: String,
    },
    Range {
        dst: TempId,
        start: TempId,
        step: Option<TempId>,
        end: TempId,
    },
    Call {
        outputs: Vec<TempId>,
        target: String,
        args: Vec<String>,
    },
    LoadIndex {
        dst: TempId,
        target: TempId,
        kind: &'static str,
        args: Vec<String>,
    },
    LoadIndexList {
        dst: TempId,
        target: TempId,
        kind: &'static str,
        args: Vec<String>,
    },
    StoreIndex {
        target: TempId,
        kind: &'static str,
        args: Vec<String>,
        src: TempId,
    },
    LoadField {
        dst: TempId,
        target: TempId,
        field: String,
    },
    LoadFieldList {
        dst: TempId,
        target: TempId,
        field: String,
    },
    StoreField {
        target: TempId,
        field: String,
        src: TempId,
        list_assignment: bool,
    },
    SplitList {
        outputs: Vec<TempId>,
        src: TempId,
    },
    PushTry {
        catch: LabelId,
    },
    StoreLastError {
        binding: String,
    },
    JumpIfFalse {
        condition: TempId,
        target: LabelId,
    },
    Jump {
        target: LabelId,
    },
    IterStart {
        iter: TempId,
        source: TempId,
    },
    IterHasNext {
        dst: TempId,
        iter: TempId,
    },
    IterNext {
        dst: TempId,
        iter: TempId,
    },
    DeclareGlobal {
        bindings: Vec<String>,
    },
    DeclarePersistent {
        bindings: Vec<String>,
    },
    Return {
        values: Vec<TempId>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodegenSummary {
    pub functions: usize,
    pub instructions: usize,
    pub anonymous_functions: usize,
    pub nested_functions: usize,
    pub max_temps: TempId,
    pub max_labels: LabelId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationIssue {
    pub function: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerificationSummary {
    pub functions_checked: usize,
    pub issues: Vec<VerificationIssue>,
}

impl VerificationSummary {
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Default)]
struct ModuleEmitter {
    functions: Vec<BytecodeFunction>,
    classes: Vec<BytecodeClass>,
    next_anonymous_id: u32,
}

struct FunctionEmitter<'a> {
    module: &'a mut ModuleEmitter,
    instructions: Vec<BytecodeInstruction>,
    next_temp: TempId,
    next_label: LabelId,
    break_labels: Vec<LabelId>,
    continue_labels: Vec<LabelId>,
    outputs: Vec<String>,
    implicit_ans: Option<String>,
}

struct EmittedFunctionBody {
    instructions: Vec<BytecodeInstruction>,
    temp_count: TempId,
    label_count: LabelId,
}

pub fn emit_bytecode(module: &HirModule) -> BytecodeModule {
    let mut emitter = ModuleEmitter::default();
    let entry = emitter.emit_module(module);
    BytecodeModule {
        backend: BackendKind::Bytecode,
        unit_kind: format!("{:?}", module.kind),
        entry,
        classes: emitter.classes,
        functions: emitter.functions,
    }
}

pub fn summarize_bytecode(module: &BytecodeModule) -> CodegenSummary {
    CodegenSummary {
        functions: module.functions.len(),
        instructions: module
            .functions
            .iter()
            .map(|function| function.instructions.len())
            .sum(),
        anonymous_functions: module
            .functions
            .iter()
            .filter(|function| function.role == "anonymous_function")
            .count(),
        nested_functions: module
            .functions
            .iter()
            .filter(|function| function.role == "nested_function")
            .count(),
        max_temps: module
            .functions
            .iter()
            .map(|function| function.temp_count)
            .max()
            .unwrap_or(0),
        max_labels: module
            .functions
            .iter()
            .map(|function| function.label_count)
            .max()
            .unwrap_or(0),
    }
}

pub fn render_codegen_summary(summary: &CodegenSummary) -> String {
    format!(
        "codegen\n  backend = bytecode\n  functions = {}\n  instructions = {}\n  anonymous_functions = {}\n  nested_functions = {}\n  max_temps = {}\n  max_labels = {}\n",
        summary.functions,
        summary.instructions,
        summary.anonymous_functions,
        summary.nested_functions,
        summary.max_temps,
        summary.max_labels
    )
}

pub fn verify_bytecode(module: &BytecodeModule) -> VerificationSummary {
    let mut issues = Vec::new();
    let mut names = BTreeSet::new();

    for function in &module.functions {
        if !names.insert(function.name.clone()) {
            issues.push(VerificationIssue {
                function: function.name.clone(),
                message: "duplicate function name".to_string(),
            });
        }
    }

    if !module
        .functions
        .iter()
        .any(|function| function.name == module.entry)
    {
        issues.push(VerificationIssue {
            function: "<module>".to_string(),
            message: format!("entry function `{}` is missing", module.entry),
        });
    }

    for function in &module.functions {
        verify_function(function, &mut issues);
    }

    VerificationSummary {
        functions_checked: module.functions.len(),
        issues,
    }
}

pub fn render_verification_summary(summary: &VerificationSummary) -> String {
    let mut out = format!(
        "verification\n  ok = {}\n  functions_checked = {}\n  issues = {}\n",
        summary.ok(),
        summary.functions_checked,
        summary.issues.len()
    );
    for issue in &summary.issues {
        out.push_str(&format!(
            "  issue function={} message={}\n",
            issue.function, issue.message
        ));
    }
    out
}

pub fn render_bytecode(module: &BytecodeModule) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "bytecode module backend={} unit_kind={} entry={}\n",
        module.backend.as_str(),
        module.unit_kind,
        module.entry
    ));
    for class in &module.classes {
        out.push_str(&format!(
            "class {} package={:?} superclass={:?} superclass_path={:?} handle={} initializer={:?} constructor={:?} properties=[{}] private_properties=[{}] inline_methods=[{}] static_inline_methods=[{}] private_inline_methods=[{}] private_static_inline_methods=[{}] source_path={:?}\n",
            class.name,
            class.package,
            class.superclass_name,
            class.superclass_path,
            class.inherits_handle,
            class.default_initializer,
            class.constructor,
            class.property_names.join(", "),
            class.private_property_names.join(", "),
            class.inline_methods.join(", "),
            class.static_inline_methods.join(", "),
            class.private_inline_methods.join(", "),
            class.private_static_inline_methods.join(", "),
            class.source_path
        ));
    }
    for function in &module.functions {
        out.push_str(&format!(
            "function {} role={} owner_class={:?} params=[{}] outputs=[{}] captures=[{}] temps={} labels={}\n",
            function.name,
            function.role,
            function.owner_class_name,
            function.params.join(", "),
            function.outputs.join(", "),
            function.captures.join(", "),
            function.temp_count,
            function.label_count
        ));
        for instruction in &function.instructions {
            out.push_str("  ");
            out.push_str(&render_instruction(instruction));
            out.push('\n');
        }
    }
    out
}

impl ModuleEmitter {
    fn emit_module(&mut self, module: &HirModule) -> String {
        for class in &module.classes {
            let initializer = if class.properties.iter().any(|property| property.default.is_some()) {
                Some(self.emit_class_initializer(class))
            } else {
                None
            };
            self.classes.push(BytecodeClass {
                name: class.name.clone(),
                package: class.package.clone(),
                superclass_name: class.superclass_name.clone(),
                superclass_path: class
                    .superclass_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                superclass_bundle_module_id: None,
                inherits_handle: class.inherits_handle,
                source_path: class
                    .source_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                property_names: class
                    .properties
                    .iter()
                    .map(|property| property.name.clone())
                    .collect(),
                private_property_names: class.private_properties.clone(),
                default_initializer: initializer,
                constructor: class.constructor.clone(),
                inline_methods: class.inline_methods.clone(),
                static_inline_methods: class.static_inline_methods.clone(),
                private_inline_methods: class.private_inline_methods.clone(),
                private_static_inline_methods: class.private_static_inline_methods.clone(),
                external_methods: class
                    .external_methods
                    .iter()
                    .map(|method| BytecodeExternalMethod {
                        name: method.name.clone(),
                        path: Some(method.path.display().to_string()),
                        bundle_module_id: None,
                    })
                    .collect(),
            });
        }

        let statements = module
            .items
            .iter()
            .filter_map(|item| match item {
                HirItem::Statement(statement) => Some(statement),
                HirItem::Function(_) => None,
            })
            .collect::<Vec<_>>();

        let mut entry = String::new();
        if !statements.is_empty()
            || module
                .items
                .iter()
                .all(|item| matches!(item, HirItem::Statement(_)))
        {
            let function = self.emit_script_entry(&statements, module.implicit_ans.as_ref());
            entry = function.name.clone();
            self.functions.push(function);
        }

        for function in module.items.iter().filter_map(|item| match item {
            HirItem::Function(function) => Some(function),
            HirItem::Statement(_) => None,
        }) {
            let name = self.emit_named_function(function, "function");
            if entry.is_empty() {
                entry = name;
            }
        }

        if entry.is_empty() {
            let function = self.emit_script_entry(&[], module.implicit_ans.as_ref());
            entry = function.name.clone();
            self.functions.push(function);
        }

        entry
    }

    fn emit_class_initializer(&mut self, class: &matlab_ir::HirClass) -> String {
        let name = format!("__classinit_{}_{}", class.name, self.functions.len());
        let output_names = class
            .properties
            .iter()
            .map(|property| format!("__default_{}", property.name))
            .collect::<Vec<_>>();
        let mut emitter = FunctionEmitter::new(self, output_names.clone(), None);
        let mut values = Vec::new();
        for property in &class.properties {
            let temp = match &property.default {
                Some(default) => emitter.lower_expression(default),
                None => {
                    let temp = emitter.new_temp();
                    emitter.instructions.push(BytecodeInstruction::BuildMatrix {
                        dst: temp,
                        rows: 0,
                        cols: 0,
                        elements: Vec::new(),
                    });
                    temp
                }
            };
            values.push(temp);
        }
        emitter
            .instructions
            .push(BytecodeInstruction::Return { values });
        let body = emitter.finish();
        self.functions.push(BytecodeFunction {
            name: name.clone(),
            role: "class_initializer".to_string(),
            owner_class_name: Some(qualified_class_name(&class.name, class.package.as_deref())),
            params: Vec::new(),
            outputs: output_names,
            captures: Vec::new(),
            temp_count: body.temp_count,
            label_count: body.label_count,
            instructions: body.instructions,
        });
        name
    }

    fn emit_script_entry(
        &mut self,
        statements: &[&HirStatement],
        implicit_ans: Option<&HirBinding>,
    ) -> BytecodeFunction {
        let mut emitter = FunctionEmitter::new(self, Vec::new(), implicit_ans.map(binding_name));
        for statement in statements {
            emitter.lower_statement(statement);
        }
        emitter
            .instructions
            .push(BytecodeInstruction::Return { values: Vec::new() });
        let body = emitter.finish();
        BytecodeFunction {
            name: "<script>".to_string(),
            role: "script_entry".to_string(),
            owner_class_name: None,
            params: Vec::new(),
            outputs: Vec::new(),
            captures: Vec::new(),
            temp_count: body.temp_count,
            label_count: body.label_count,
            instructions: body.instructions,
        }
    }

    fn emit_named_function(&mut self, function: &HirFunction, role: &str) -> String {
        let name = format!(
            "{}#s{}w{}",
            function.name, function.scope_id.0, function.workspace_id.0
        );
        let outputs = function
            .outputs
            .iter()
            .map(binding_name)
            .collect::<Vec<_>>();
        let mut emitter = FunctionEmitter::new(
            self,
            outputs.clone(),
            function.implicit_ans.as_ref().map(binding_name),
        );
        for statement in &function.body {
            emitter.lower_statement(statement);
        }
        emitter.emit_return_from_outputs();
        let body = emitter.finish();
        self.functions.push(BytecodeFunction {
            name: name.clone(),
            role: role.to_string(),
            owner_class_name: function.owner_class_name.clone(),
            params: function.inputs.iter().map(binding_name).collect(),
            outputs,
            captures: function
                .captures
                .iter()
                .map(|capture| {
                    format!(
                        "{} binding={} access={:?}",
                        capture.name, capture.binding_id.0, capture.access
                    )
                })
                .collect(),
            temp_count: body.temp_count,
            label_count: body.label_count,
            instructions: body.instructions,
        });
        for local in &function.local_functions {
            self.emit_named_function(local, "nested_function");
        }
        name
    }

    fn emit_anonymous_function(&mut self, anonymous: &HirAnonymousFunction) -> String {
        let name = format!(
            "<anon:{}>#s{}w{}",
            self.next_anonymous_id, anonymous.scope_id.0, anonymous.workspace_id.0
        );
        self.next_anonymous_id += 1;
        let mut emitter = FunctionEmitter::new(self, vec!["<anon_result>".to_string()], None);
        let value = emitter.lower_expression(&anonymous.body);
        emitter.instructions.push(BytecodeInstruction::Return {
            values: vec![value],
        });
        let body = emitter.finish();
        self.functions.push(BytecodeFunction {
            name: name.clone(),
            role: "anonymous_function".to_string(),
            owner_class_name: None,
            params: anonymous.params.iter().map(binding_name).collect(),
            outputs: vec!["<anon_result>".to_string()],
            captures: anonymous
                .captures
                .iter()
                .map(|capture| {
                    format!(
                        "{} binding={} access={:?}",
                        capture.name, capture.binding_id.0, capture.access
                    )
                })
                .collect(),
            temp_count: body.temp_count,
            label_count: body.label_count,
            instructions: body.instructions,
        });
        name
    }
}

impl<'a> FunctionEmitter<'a> {
    fn new(
        module: &'a mut ModuleEmitter,
        outputs: Vec<String>,
        implicit_ans: Option<String>,
    ) -> Self {
        Self {
            module,
            instructions: Vec::new(),
            next_temp: 0,
            next_label: 0,
            break_labels: Vec::new(),
            continue_labels: Vec::new(),
            outputs,
            implicit_ans,
        }
    }

    fn finish(self) -> EmittedFunctionBody {
        EmittedFunctionBody {
            instructions: self.instructions,
            temp_count: self.next_temp,
            label_count: self.next_label,
        }
    }

    fn lower_statement(&mut self, statement: &HirStatement) {
        match statement {
            HirStatement::Assignment {
                targets,
                value,
                list_assignment,
                ..
            } => {
                self.lower_assignment(targets, value, *list_assignment)
            }
            HirStatement::Expression { expression, .. } => {
                if let HirExpression::Call { target, args } = expression {
                    if is_statement_builtin_call_with_args(target, args.len()) {
                        self.lower_call(target, args, 0);
                        return;
                    }
                }
                let value = self.lower_expression(expression);
                if let Some(binding) = &self.implicit_ans {
                    self.instructions
                        .push(BytecodeInstruction::StoreBindingIfPresent {
                            binding: binding.clone(),
                            src: value,
                        });
                }
            }
            HirStatement::If {
                branches,
                else_body,
            } => self.lower_if(branches, else_body),
            HirStatement::Switch {
                expression,
                cases,
                otherwise_body,
            } => self.lower_switch(expression, cases, otherwise_body),
            HirStatement::Try {
                body,
                catch_binding,
                catch_body,
            } => self.lower_try(body, catch_binding.as_ref(), catch_body),
            HirStatement::For {
                variable,
                iterable,
                body,
            } => {
                let iterable = self.lower_expression(iterable);
                let iter = self.new_temp();
                let head = self.new_label();
                let end = self.new_label();
                self.instructions.push(BytecodeInstruction::IterStart {
                    iter,
                    source: iterable,
                });
                self.instructions.push(BytecodeInstruction::Label(head));
                let has_next = self.new_temp();
                self.instructions.push(BytecodeInstruction::IterHasNext {
                    dst: has_next,
                    iter,
                });
                self.instructions.push(BytecodeInstruction::JumpIfFalse {
                    condition: has_next,
                    target: end,
                });
                let value = self.new_temp();
                self.instructions
                    .push(BytecodeInstruction::IterNext { dst: value, iter });
                self.instructions.push(BytecodeInstruction::StoreBinding {
                    binding: binding_name(variable),
                    src: value,
                });
                self.break_labels.push(end);
                self.continue_labels.push(head);
                for statement in body {
                    self.lower_statement(statement);
                }
                self.break_labels.pop();
                self.continue_labels.pop();
                self.instructions
                    .push(BytecodeInstruction::Jump { target: head });
                self.instructions.push(BytecodeInstruction::Label(end));
            }
            HirStatement::While { condition, body } => {
                let head = self.new_label();
                let end = self.new_label();
                self.instructions.push(BytecodeInstruction::Label(head));
                let condition = self.lower_expression(condition);
                self.instructions.push(BytecodeInstruction::JumpIfFalse {
                    condition,
                    target: end,
                });
                self.break_labels.push(end);
                self.continue_labels.push(head);
                for statement in body {
                    self.lower_statement(statement);
                }
                self.break_labels.pop();
                self.continue_labels.pop();
                self.instructions
                    .push(BytecodeInstruction::Jump { target: head });
                self.instructions.push(BytecodeInstruction::Label(end));
            }
            HirStatement::Break => {
                if let Some(label) = self.break_labels.last().copied() {
                    self.instructions
                        .push(BytecodeInstruction::Jump { target: label });
                }
            }
            HirStatement::Continue => {
                if let Some(label) = self.continue_labels.last().copied() {
                    self.instructions
                        .push(BytecodeInstruction::Jump { target: label });
                }
            }
            HirStatement::Return => self.emit_return_from_outputs(),
            HirStatement::Global(bindings) => {
                self.instructions.push(BytecodeInstruction::DeclareGlobal {
                    bindings: bindings.iter().map(binding_name).collect(),
                });
            }
            HirStatement::Persistent(bindings) => {
                self.instructions
                    .push(BytecodeInstruction::DeclarePersistent {
                        bindings: bindings.iter().map(binding_name).collect(),
                    });
            }
        }
    }

    fn lower_assignment(
        &mut self,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
        list_assignment: bool,
    ) {
        let temps = if targets.len() > 1 {
            match value {
                HirExpression::Call { target, args } => {
                    self.lower_call(target, args, targets.len())
                }
                HirExpression::CellIndex { .. } | HirExpression::FieldAccess { .. } => {
                    let src = self.lower_list_expression(value);
                    let outputs = (0..targets.len())
                        .map(|_| self.new_temp())
                        .collect::<Vec<_>>();
                    self.instructions.push(BytecodeInstruction::SplitList {
                        outputs: outputs.clone(),
                        src,
                    });
                    outputs
                }
                _ => {
                    let value = self.lower_expression(value);
                    vec![value; targets.len()]
                }
            }
        } else if matches!(targets.first(), Some(HirAssignmentTarget::CellIndex { .. })) {
            match value {
                HirExpression::Call {
                    target: HirCallTarget::Callable(reference),
                    args,
                } if reference.name == "deal" => {
                    let outputs = self.lower_call(
                        &HirCallTarget::Callable(reference.clone()),
                        args,
                        args.len(),
                    );
                    let cell = self.new_temp();
                    self.instructions.push(BytecodeInstruction::BuildCell {
                        dst: cell,
                        rows: 1,
                        cols: outputs.len(),
                        elements: outputs,
                    });
                    vec![cell]
                }
                value if expression_supports_list_expansion(value) => {
                    let src = self.lower_list_expression(value);
                    let cell = self.new_temp();
                    self.instructions
                        .push(BytecodeInstruction::PackSpreadCell { dst: cell, src });
                    vec![cell]
                }
                _ => vec![self.lower_expression(value)],
            }
        } else if matches!(targets.first(), Some(HirAssignmentTarget::Field { .. }))
            && (list_assignment
                || target_expression_requires_field_list_assignment(
                    match targets.first() {
                        Some(HirAssignmentTarget::Field { target, .. }) => target,
                        _ => unreachable!("guard restricted to field target"),
                    },
                ))
        {
            match value {
                HirExpression::Call {
                    target: HirCallTarget::Callable(reference),
                    args,
                } if reference.name == "deal" => {
                    let outputs = self.lower_call(
                        &HirCallTarget::Callable(reference.clone()),
                        args,
                        args.len(),
                    );
                    let matrix = self.new_temp();
                    self.instructions.push(BytecodeInstruction::BuildMatrix {
                        dst: matrix,
                        rows: 1,
                        cols: outputs.len(),
                        elements: outputs,
                    });
                    vec![matrix]
                }
                value if expression_supports_list_expansion(value) => {
                    let src = self.lower_list_expression(value);
                    let matrix = self.new_temp();
                    self.instructions
                        .push(BytecodeInstruction::PackSpreadMatrix { dst: matrix, src });
                    vec![matrix]
                }
                _ => vec![self.lower_expression(value)],
            }
        } else {
            vec![self.lower_expression(value)]
        };

        for (target, temp) in targets.iter().zip(temps.into_iter()) {
            self.store_target(target, temp, list_assignment);
        }
    }

    fn store_target(&mut self, target: &HirAssignmentTarget, src: TempId, list_assignment: bool) {
        match target {
            HirAssignmentTarget::Binding(binding) => {
                self.instructions.push(BytecodeInstruction::StoreBinding {
                    binding: binding_name(binding),
                    src,
                });
            }
            HirAssignmentTarget::Index { target, indices } => {
                let target = self.lower_lvalue_expression(target);
                let args = self.lower_index_args(indices);
                self.instructions.push(BytecodeInstruction::StoreIndex {
                    target,
                    kind: "paren",
                    args,
                    src,
                });
            }
            HirAssignmentTarget::CellIndex { target, indices } => {
                let target = self.lower_lvalue_expression(target);
                let args = self.lower_index_args(indices);
                self.instructions.push(BytecodeInstruction::StoreIndex {
                    target,
                    kind: "brace",
                    args,
                    src,
                });
            }
            HirAssignmentTarget::Field { target, field } => {
                let target = self.lower_lvalue_expression(target);
                self.instructions.push(BytecodeInstruction::StoreField {
                    target,
                    field: field.clone(),
                    src,
                    list_assignment,
                });
            }
        }
    }

    fn lower_if(&mut self, branches: &[HirConditionalBranch], else_body: &[HirStatement]) {
        let end = self.new_label();
        for branch in branches {
            let miss = self.new_label();
            let condition = self.lower_expression(&branch.condition);
            self.instructions.push(BytecodeInstruction::JumpIfFalse {
                condition,
                target: miss,
            });
            for statement in &branch.body {
                self.lower_statement(statement);
            }
            self.instructions
                .push(BytecodeInstruction::Jump { target: end });
            self.instructions.push(BytecodeInstruction::Label(miss));
        }
        for statement in else_body {
            self.lower_statement(statement);
        }
        self.instructions.push(BytecodeInstruction::Label(end));
    }

    fn lower_try(
        &mut self,
        body: &[HirStatement],
        catch_binding: Option<&HirBinding>,
        catch_body: &[HirStatement],
    ) {
        let catch_label = self.new_label();
        let end = self.new_label();
        self.instructions
            .push(BytecodeInstruction::PushTry { catch: catch_label });
        for statement in body {
            self.lower_statement(statement);
        }
        self.instructions
            .push(BytecodeInstruction::Jump { target: end });
        self.instructions
            .push(BytecodeInstruction::Label(catch_label));
        if let Some(binding) = catch_binding {
            self.instructions.push(BytecodeInstruction::StoreLastError {
                binding: binding_name(binding),
            });
        }
        for statement in catch_body {
            self.lower_statement(statement);
        }
        self.instructions.push(BytecodeInstruction::Label(end));
    }

    fn lower_switch(
        &mut self,
        expression: &HirExpression,
        cases: &[HirSwitchCase],
        otherwise_body: &[HirStatement],
    ) {
        let switch_value = self.lower_expression(expression);
        let end = self.new_label();
        for case in cases {
            let matcher = self.lower_expression(&case.matcher);
            let matched = self.new_temp();
            let miss = self.new_label();
            self.instructions.push(BytecodeInstruction::Binary {
                dst: matched,
                op: "Equal".to_string(),
                lhs: switch_value,
                rhs: matcher,
            });
            self.instructions.push(BytecodeInstruction::JumpIfFalse {
                condition: matched,
                target: miss,
            });
            for statement in &case.body {
                self.lower_statement(statement);
            }
            self.instructions
                .push(BytecodeInstruction::Jump { target: end });
            self.instructions.push(BytecodeInstruction::Label(miss));
        }
        for statement in otherwise_body {
            self.lower_statement(statement);
        }
        self.instructions.push(BytecodeInstruction::Label(end));
    }

    fn lower_expression(&mut self, expression: &HirExpression) -> TempId {
        match expression {
            HirExpression::ValueRef(reference) => {
                if is_builtin_logical_value(reference) {
                    self.load_const(format!("logical({})", reference.name))
                } else {
                    let temp = self.new_temp();
                    self.instructions.push(BytecodeInstruction::LoadBinding {
                        dst: temp,
                        binding: value_ref_name(reference),
                    });
                    temp
                }
            }
            HirExpression::NumberLiteral(text) => self.load_const(format!("number({text})")),
            HirExpression::CharLiteral(text) => self.load_const(format!("char({text})")),
            HirExpression::StringLiteral(text) => self.load_const(format!("string({text})")),
            HirExpression::MatrixLiteral(rows) => {
                let (row_item_counts, elements) = self.lower_literal_row_sources(rows);
                let temp = self.new_temp();
                self.instructions
                    .push(BytecodeInstruction::BuildMatrixList {
                        dst: temp,
                        row_item_counts,
                        elements,
                    });
                temp
            }
            HirExpression::CellLiteral(rows) => {
                if literal_rows_need_list_expansion(rows) {
                    let (row_item_counts, elements) = self.lower_literal_row_sources(rows);
                    let temp = self.new_temp();
                    self.instructions.push(BytecodeInstruction::BuildCellList {
                        dst: temp,
                        row_item_counts,
                        elements,
                    });
                    temp
                } else {
                    let mut elements = Vec::new();
                    let row_count = rows.len();
                    let col_count = rows.first().map(|row| row.len()).unwrap_or(0);
                    for row in rows {
                        for element in row {
                            elements.push(self.lower_expression(element));
                        }
                    }
                    let temp = self.new_temp();
                    self.instructions.push(BytecodeInstruction::BuildCell {
                        dst: temp,
                        rows: row_count,
                        cols: col_count,
                        elements,
                    });
                    temp
                }
            }
            HirExpression::FunctionHandle(target) => match target {
                HirFunctionHandleTarget::Callable(reference) => {
                    let temp = self.new_temp();
                    self.instructions.push(BytecodeInstruction::MakeHandle {
                        dst: temp,
                        target: callable_target_name(reference),
                    });
                    temp
                }
                HirFunctionHandleTarget::Expression(expression) => {
                    self.lower_expression(expression)
                }
            },
            HirExpression::EndKeyword => self.load_const("keyword(end)".to_string()),
            HirExpression::Unary { op, rhs } => {
                let src = self.lower_expression(rhs);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::Unary {
                    dst: temp,
                    op: format!("{op:?}"),
                    src,
                });
                temp
            }
            HirExpression::Binary { op, lhs, rhs } => {
                let lhs = self.lower_expression(lhs);
                let rhs = self.lower_expression(rhs);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::Binary {
                    dst: temp,
                    op: format!("{op:?}"),
                    lhs,
                    rhs,
                });
                temp
            }
            HirExpression::Range { start, step, end } => {
                let start = self.lower_expression(start);
                let step = step.as_ref().map(|step| self.lower_expression(step));
                let end = self.lower_expression(end);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::Range {
                    dst: temp,
                    start,
                    step,
                    end,
                });
                temp
            }
            HirExpression::Call { target, args } => self.lower_call(target, args, 1)[0],
            HirExpression::CellIndex { target, indices } => {
                let target = self.lower_expression(target);
                let args = self.lower_index_args(indices);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::LoadIndex {
                    dst: temp,
                    target,
                    kind: "brace",
                    args,
                });
                temp
            }
            HirExpression::FieldAccess { target, field } => {
                let target = self.lower_expression(target);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::LoadField {
                    dst: temp,
                    target,
                    field: field.clone(),
                });
                temp
            }
            HirExpression::AnonymousFunction(anonymous) => {
                let name = self.module.emit_anonymous_function(anonymous);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::MakeHandle {
                    dst: temp,
                    target: name,
                });
                temp
            }
        }
    }

    fn lower_lvalue_expression(&mut self, expression: &HirExpression) -> TempId {
        match expression {
            HirExpression::ValueRef(reference) => {
                let temp = self.new_temp();
                self.instructions
                    .push(BytecodeInstruction::LoadBindingLValue {
                        dst: temp,
                        binding: value_ref_name(reference),
                    });
                temp
            }
            HirExpression::Call { target, args } => self.lower_lvalue_call(target, args),
            HirExpression::CellIndex { target, indices } => {
                let target = self.lower_lvalue_expression(target);
                let args = self.lower_index_args(indices);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::LoadIndex {
                    dst: temp,
                    target,
                    kind: "brace",
                    args,
                });
                temp
            }
            HirExpression::FieldAccess { target, field } => {
                let target = self.lower_lvalue_expression(target);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::LoadField {
                    dst: temp,
                    target,
                    field: field.clone(),
                });
                temp
            }
            _ => self.lower_expression(expression),
        }
    }

    fn lower_call(
        &mut self,
        target: &HirCallTarget,
        args: &[HirIndexArgument],
        outputs: usize,
    ) -> Vec<TempId> {
        let output_temps = (0..outputs).map(|_| self.new_temp()).collect::<Vec<_>>();
        let args = self.lower_call_args(args);
        let target = match target {
            HirCallTarget::Callable(reference) if is_workspace_value_callable(reference) => {
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::LoadBinding {
                    dst: temp,
                    binding: callable_binding_name(reference),
                });
                format!("t{temp}")
            }
            HirCallTarget::Callable(reference) => callable_target_name(reference),
            HirCallTarget::Expression(expression) => {
                let temp = if expression_supports_list_expansion(expression) {
                    self.lower_list_expression(expression)
                } else {
                    self.lower_expression(expression)
                };
                format!("t{temp}")
            }
        };
        self.instructions.push(BytecodeInstruction::Call {
            outputs: output_temps.clone(),
            target,
            args,
        });
        output_temps
    }

    fn lower_lvalue_call(&mut self, target: &HirCallTarget, args: &[HirIndexArgument]) -> TempId {
        let output = self.new_temp();
        let target = match target {
            HirCallTarget::Callable(reference) if is_workspace_value_callable(reference) => {
                let temp = self.new_temp();
                self.instructions
                    .push(BytecodeInstruction::LoadBindingLValue {
                        dst: temp,
                        binding: callable_binding_name(reference),
                    });
                format!("t{temp}")
            }
            HirCallTarget::Expression(expression) => {
                let temp = self.lower_lvalue_expression(expression);
                format!("t{temp}")
            }
            _ => return self.lower_call(target, args, 1)[0],
        };
        let args = self.lower_call_args(args);
        self.instructions.push(BytecodeInstruction::Call {
            outputs: vec![output],
            target,
            args,
        });
        output
    }

    fn lower_list_expression(&mut self, expression: &HirExpression) -> TempId {
        match expression {
            HirExpression::Call { target, args }
                if matches!(
                    target,
                    HirCallTarget::Expression(target_expression)
                        if expression_supports_list_expansion(target_expression)
                ) =>
            {
                let HirCallTarget::Expression(target_expression) = target else {
                    unreachable!("guard ensured expression target");
                };
                self.lower_list_call_expression(target_expression, args)
            }
            HirExpression::CellIndex { target, indices } => {
                let target = self.lower_list_expression(target);
                let args = self.lower_index_args(indices);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::LoadIndexList {
                    dst: temp,
                    target,
                    kind: "brace",
                    args,
                });
                temp
            }
            HirExpression::FieldAccess { target, field } => {
                let target = self.lower_list_expression(target);
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::LoadFieldList {
                    dst: temp,
                    target,
                    field: field.clone(),
                });
                temp
            }
            _ => self.lower_expression(expression),
        }
    }

    fn lower_list_call_expression(
        &mut self,
        expression: &HirExpression,
        args: &[HirIndexArgument],
    ) -> TempId {
        let target = self.lower_list_expression(expression);
        let args = self.lower_index_args(args);
        let temp = self.new_temp();
        self.instructions.push(BytecodeInstruction::LoadIndexList {
            dst: temp,
            target,
            kind: "paren",
            args,
        });
        temp
    }

    fn lower_index_args(&mut self, args: &[HirIndexArgument]) -> Vec<String> {
        args.iter()
            .map(|arg| match arg {
                HirIndexArgument::Expression(expression) => {
                    self.lower_index_expression_arg(expression)
                }
                HirIndexArgument::FullSlice => ":".to_string(),
                HirIndexArgument::End => "end".to_string(),
            })
            .collect()
    }

    fn lower_call_args(&mut self, args: &[HirIndexArgument]) -> Vec<String> {
        let mut lowered = Vec::new();
        for arg in args {
            match arg {
                HirIndexArgument::Expression(
                    expression @ (HirExpression::CellIndex { .. }
                    | HirExpression::FieldAccess { .. }),
                ) if !expression_contains_end_keyword(expression) => {
                    lowered.push(format!("t{}", self.lower_list_expression(expression)))
                }
                HirIndexArgument::Expression(expression) => {
                    lowered.push(self.lower_index_expression_arg(expression));
                }
                HirIndexArgument::FullSlice => lowered.push(":".to_string()),
                HirIndexArgument::End => lowered.push("end".to_string()),
            }
        }
        lowered
    }

    fn lower_index_expression_arg(&mut self, expression: &HirExpression) -> String {
        if !expression_contains_end_keyword(expression) {
            return format!("t{}", self.lower_expression(expression));
        }
        format!("idx:{}", self.encode_index_expression(expression))
    }

    fn encode_index_expression(&mut self, expression: &HirExpression) -> String {
        match expression {
            HirExpression::EndKeyword => "end".to_string(),
            HirExpression::Unary { op, rhs } => {
                format!("unary({:?},{})", op, self.encode_index_expression_part(rhs))
            }
            HirExpression::Binary { op, lhs, rhs } => format!(
                "binary({:?},{},{})",
                op,
                self.encode_index_expression_part(lhs),
                self.encode_index_expression_part(rhs)
            ),
            HirExpression::Range { start, step, end } => match step {
                Some(step) => format!(
                    "range3({},{},{})",
                    self.encode_index_expression_part(start),
                    self.encode_index_expression_part(step),
                    self.encode_index_expression_part(end)
                ),
                None => format!(
                    "range({},{})",
                    self.encode_index_expression_part(start),
                    self.encode_index_expression_part(end)
                ),
            },
            HirExpression::MatrixLiteral(rows) => self.encode_index_literal("matrix", rows),
            HirExpression::CellLiteral(rows) => self.encode_index_literal("cell", rows),
            HirExpression::Call { target, args } => {
                let target = self.encode_index_call_target(target);
                let args = self.encode_index_call_args(args);
                if args.is_empty() {
                    format!("call({target})")
                } else {
                    format!("call({target},{})", args.join(","))
                }
            }
            HirExpression::CellIndex { target, indices } => {
                let target = self.encode_index_expression_part(target);
                let args = self.encode_index_call_args(indices);
                if args.is_empty() {
                    format!("cellindex({target})")
                } else {
                    format!("cellindex({target},{})", args.join(","))
                }
            }
            HirExpression::FieldAccess { target, field } => format!(
                "field({},{field})",
                self.encode_index_expression_part(target)
            ),
            other => format!("t{}", self.lower_expression(other)),
        }
    }

    fn encode_index_call_target(&mut self, target: &HirCallTarget) -> String {
        match target {
            HirCallTarget::Callable(reference) if is_workspace_value_callable(reference) => {
                let temp = self.new_temp();
                self.instructions.push(BytecodeInstruction::LoadBinding {
                    dst: temp,
                    binding: callable_binding_name(reference),
                });
                format!("t{temp}")
            }
            HirCallTarget::Callable(reference) => callable_target_name(reference),
            HirCallTarget::Expression(expression) => self.encode_index_expression_part(expression),
        }
    }

    fn encode_index_call_args(&mut self, args: &[HirIndexArgument]) -> Vec<String> {
        let mut lowered = Vec::new();
        for arg in args {
            match arg {
                HirIndexArgument::Expression(
                    expression @ (HirExpression::CellIndex { .. }
                    | HirExpression::FieldAccess { .. }),
                ) if !expression_contains_end_keyword(expression) => {
                    lowered.push(format!("t{}", self.lower_list_expression(expression)))
                }
                HirIndexArgument::Expression(expression) => {
                    lowered.push(self.encode_index_expression_part(expression));
                }
                HirIndexArgument::FullSlice => lowered.push(":".to_string()),
                HirIndexArgument::End => lowered.push("end".to_string()),
            }
        }
        lowered
    }

    fn encode_index_literal(&mut self, kind: &str, rows: &[Vec<HirExpression>]) -> String {
        let mut parts = Vec::new();
        parts.push(format!(
            "rows({})",
            rows.iter()
                .map(|row| row.len().to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
        for row in rows {
            for expression in row {
                parts.push(self.encode_index_expression_part(expression));
            }
        }
        format!("{kind}({})", parts.join(","))
    }

    fn encode_index_expression_part(&mut self, expression: &HirExpression) -> String {
        if expression_contains_end_keyword(expression) {
            self.encode_index_expression(expression)
        } else {
            format!("t{}", self.lower_expression(expression))
        }
    }

    fn lower_literal_row_sources(
        &mut self,
        rows: &[Vec<HirExpression>],
    ) -> (Vec<usize>, Vec<String>) {
        let mut row_item_counts = Vec::with_capacity(rows.len());
        let mut elements = Vec::new();
        for row in rows {
            row_item_counts.push(row.len());
            for expression in row {
                let temp = if expression_supports_list_literal_expansion(expression) {
                    self.lower_list_expression(expression)
                } else {
                    self.lower_expression(expression)
                };
                elements.push(format!("t{temp}"));
            }
        }
        (row_item_counts, elements)
    }

    fn emit_return_from_outputs(&mut self) {
        let outputs = self.outputs.clone();
        let mut values = Vec::new();
        for binding in outputs {
            let temp = self.new_temp();
            self.instructions
                .push(BytecodeInstruction::LoadBinding { dst: temp, binding });
            values.push(temp);
        }
        self.instructions
            .push(BytecodeInstruction::Return { values });
    }

    fn new_temp(&mut self) -> TempId {
        let temp = self.next_temp;
        self.next_temp += 1;
        temp
    }

    fn new_label(&mut self) -> LabelId {
        let label = self.next_label;
        self.next_label += 1;
        label
    }

    fn load_const(&mut self, value: String) -> TempId {
        let temp = self.new_temp();
        self.instructions
            .push(BytecodeInstruction::LoadConst { dst: temp, value });
        temp
    }
}

fn binding_name(binding: &HirBinding) -> String {
    match binding.binding_id {
        Some(binding_id) => format!("{}#{}", binding.name, binding_id.0),
        None => binding.name.clone(),
    }
}

fn value_ref_name(reference: &HirValueRef) -> String {
    match reference.binding_id {
        Some(binding_id) => format!("{}#{}", reference.name, binding_id.0),
        None => reference.name.clone(),
    }
}

fn is_builtin_logical_value(reference: &HirValueRef) -> bool {
    reference.binding_id.is_none() && matches!(reference.name.as_str(), "true" | "false")
}

fn callable_binding_name(reference: &HirCallableRef) -> String {
    match reference.binding_id {
        Some(binding_id) => format!("{}#{}", reference.name, binding_id.0),
        None => reference.name.clone(),
    }
}

fn callable_target_name(reference: &HirCallableRef) -> String {
    let binding = reference
        .binding_id
        .map(|binding_id| format!(" binding={}", binding_id.0))
        .unwrap_or_default();
    let super_constructor = if reference.super_constructor {
        " super_ctor=true"
    } else {
        ""
    };
    match &reference.final_resolution {
        Some(final_resolution) => format!(
            "{} [semantic={:?}{binding}{super_constructor} final={:?}]",
            reference.name, reference.semantic_resolution, final_resolution
        ),
        None => format!(
            "{} [semantic={:?}{binding}{super_constructor}]",
            reference.name, reference.semantic_resolution
        ),
    }
}

fn is_workspace_value_callable(reference: &HirCallableRef) -> bool {
    reference.binding_id.is_some()
}

fn is_statement_builtin_call_with_args(target: &HirCallTarget, arg_count: usize) -> bool {
    let HirCallTarget::Callable(reference) = target else {
        return false;
    };
    if reference.binding_id.is_some() {
        return false;
    }
    if reference
        .final_resolution
        .as_ref()
        .is_some_and(|resolution| format!("{resolution:?}").starts_with("ResolvedPath"))
    {
        return false;
    }
    statement_builtin_suppresses_ans(reference.name.as_str(), arg_count)
}

fn statement_builtin_suppresses_ans(name: &str, arg_count: usize) -> bool {
    matches!(
        name,
        "disp"
            | "display"
            | "fprintf"
            | "drawnow"
            | "clc"
            | "format"
            | "save"
            | "load"
            | "who"
            | "whos"
            | "what"
            | "help"
            | "lookfor"
            | "tic"
            | "toc"
            | "clear"
            | "clearvars"
            | "addpoints"
            | "clearpoints"
            | "getpoints"
            | "pause"
            | "figure"
            | "clf"
            | "cla"
            | "closereq"
            | "close"
            | "delete"
            | "copyobj"
            | "reset"
            | "subplot"
            | "tiledlayout"
            | "nexttile"
            | "hold"
            | "grid"
            | "box"
            | "line"
            | "xline"
            | "yline"
            | "plot"
            | "fplot"
            | "fsurf"
            | "fmesh"
            | "fimplicit"
            | "fcontour"
            | "fcontour3"
            | "fplot3"
            | "plot3"
            | "plotyy"
            | "errorbar"
            | "semilogx"
            | "semilogy"
            | "loglog"
            | "scatter"
            | "scatter3"
            | "quiver"
            | "quiver3"
            | "pie"
            | "pie3"
            | "histogram"
            | "histogram2"
            | "area"
            | "stairs"
            | "bar"
            | "barh"
            | "stem"
            | "stem3"
            | "contour"
            | "contour3"
            | "contourf"
            | "mesh"
            | "meshc"
            | "meshz"
            | "waterfall"
            | "ribbon"
            | "bar3"
            | "bar3h"
            | "surf"
            | "surfc"
            | "image"
            | "imagesc"
            | "imshow"
            | "text"
            | "rectangle"
            | "annotation"
            | "patch"
            | "fill"
            | "fill3"
            | "animatedline"
            | "axes"
            | "colorbar"
            | "legend"
            | "sgtitle"
            | "title"
            | "subtitle"
            | "xlabel"
            | "ylabel"
            | "yyaxis"
            | "zlabel"
            | "rotate3d"
            | "linkaxes"
            | "print"
            | "saveas"
            | "exportgraphics"
    ) || (arg_count > 0
        && matches!(
            name,
            "axis"
                | "view"
                | "rotate3d"
                | "xscale"
                | "yscale"
                | "shading"
                | "caxis"
                | "colormap"
                | "xticks"
                | "yticks"
                | "zticks"
                | "xticklabels"
                | "yticklabels"
                | "zticklabels"
                | "xtickangle"
                | "ytickangle"
                | "ztickangle"
                | "xlim"
                | "ylim"
                | "zlim"
        ))
}

fn expression_supports_list_expansion(expression: &HirExpression) -> bool {
    match expression {
        HirExpression::CellIndex { .. } => true,
        HirExpression::FieldAccess { target, .. } => expression_supports_list_expansion(target),
        HirExpression::Call { target, .. } => matches!(
            target,
            HirCallTarget::Expression(target_expression)
                if expression_supports_list_expansion(target_expression)
        ),
        _ => false,
    }
}

fn target_expression_requires_field_list_assignment(expression: &HirExpression) -> bool {
    match expression {
        HirExpression::FieldAccess { target, .. } => target_expression_requires_field_list_assignment(target),
        HirExpression::Call { .. } | HirExpression::CellIndex { .. } => true,
        _ => false,
    }
}

fn literal_rows_need_list_expansion(rows: &[Vec<HirExpression>]) -> bool {
    rows.iter()
        .flatten()
        .any(expression_supports_list_literal_expansion)
}

fn expression_supports_list_literal_expansion(expression: &HirExpression) -> bool {
    match expression {
        HirExpression::CellIndex { .. } | HirExpression::FieldAccess { .. } => true,
        HirExpression::Call { target, .. } => matches!(
            target,
            HirCallTarget::Expression(target_expression)
                if expression_supports_list_expansion(target_expression)
        ),
        _ => false,
    }
}

fn render_instruction(instruction: &BytecodeInstruction) -> String {
    match instruction {
        BytecodeInstruction::Label(label) => format!("L{label}:"),
        BytecodeInstruction::LoadConst { dst, value } => format!("t{dst} = const {value}"),
        BytecodeInstruction::LoadBinding { dst, binding } => format!("t{dst} = load {binding}"),
        BytecodeInstruction::LoadBindingLValue { dst, binding } => {
            format!("t{dst} = load-lvalue {binding}")
        }
        BytecodeInstruction::StoreBinding { binding, src } => {
            format!("store {binding} <- t{src}")
        }
        BytecodeInstruction::StoreBindingIfPresent { binding, src } => {
            format!("store_if_present {binding} <- t{src}")
        }
        BytecodeInstruction::Unary { dst, op, src } => format!("t{dst} = unary {op} t{src}"),
        BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
            format!("t{dst} = binary {op} t{lhs}, t{rhs}")
        }
        BytecodeInstruction::BuildMatrix {
            dst,
            rows,
            cols,
            elements,
        } => format!("t{dst} = matrix {rows}x{cols} [{}]", join_temps(elements)),
        BytecodeInstruction::BuildMatrixList {
            dst,
            row_item_counts,
            elements,
        } => format!(
            "t{dst} = matrix_list rows=[{}] [{}]",
            row_item_counts
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            elements.join(", ")
        ),
        BytecodeInstruction::BuildCell {
            dst,
            rows,
            cols,
            elements,
        } => format!("t{dst} = cell {rows}x{cols} [{}]", join_temps(elements)),
        BytecodeInstruction::BuildCellList {
            dst,
            row_item_counts,
            elements,
        } => format!(
            "t{dst} = cell_list rows=[{}] [{}]",
            row_item_counts
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            elements.join(", ")
        ),
        BytecodeInstruction::PackSpreadMatrix { dst, src } => {
            format!("t{dst} = pack_spread_matrix t{src}")
        }
        BytecodeInstruction::PackSpreadCell { dst, src } => {
            format!("t{dst} = pack_spread_cell t{src}")
        }
        BytecodeInstruction::MakeHandle { dst, target } => format!("t{dst} = handle {target}"),
        BytecodeInstruction::Range {
            dst,
            start,
            step,
            end,
        } => match step {
            Some(step) => format!("t{dst} = range t{start}, t{step}, t{end}"),
            None => format!("t{dst} = range t{start}, t{end}"),
        },
        BytecodeInstruction::Call {
            outputs,
            target,
            args,
        } => format!(
            "[{}] = call {target} ({})",
            join_temps(outputs),
            args.join(", ")
        ),
        BytecodeInstruction::LoadIndex {
            dst,
            target,
            kind,
            args,
        } => format!("t{dst} = index {kind} t{target} [{}]", args.join(", ")),
        BytecodeInstruction::LoadIndexList {
            dst,
            target,
            kind,
            args,
        } => format!("t{dst} = index_list {kind} t{target} [{}]", args.join(", ")),
        BytecodeInstruction::StoreIndex {
            target,
            kind,
            args,
            src,
        } => format!(
            "store_index {kind} t{target} [{}] <- t{src}",
            args.join(", ")
        ),
        BytecodeInstruction::LoadField { dst, target, field } => {
            format!("t{dst} = field t{target}.{field}")
        }
        BytecodeInstruction::LoadFieldList { dst, target, field } => {
            format!("t{dst} = field_list t{target}.{field}")
        }
        BytecodeInstruction::StoreField {
            target,
            field,
            src,
            list_assignment,
        } => {
            if *list_assignment {
                format!("store_field[list] t{target}.{field} <- t{src}")
            } else {
                format!("store_field t{target}.{field} <- t{src}")
            }
        }
        BytecodeInstruction::SplitList { outputs, src } => {
            format!("[{}] = split_list t{src}", join_temps(outputs))
        }
        BytecodeInstruction::PushTry { catch } => format!("push_try -> L{catch}"),
        BytecodeInstruction::StoreLastError { binding } => {
            format!("store_last_error {binding}")
        }
        BytecodeInstruction::JumpIfFalse { condition, target } => {
            format!("jump_if_false t{condition} -> L{target}")
        }
        BytecodeInstruction::Jump { target } => format!("jump -> L{target}"),
        BytecodeInstruction::IterStart { iter, source } => {
            format!("t{iter} = iter_start t{source}")
        }
        BytecodeInstruction::IterHasNext { dst, iter } => {
            format!("t{dst} = iter_has_next t{iter}")
        }
        BytecodeInstruction::IterNext { dst, iter } => format!("t{dst} = iter_next t{iter}"),
        BytecodeInstruction::DeclareGlobal { bindings } => {
            format!("declare_global [{}]", bindings.join(", "))
        }
        BytecodeInstruction::DeclarePersistent { bindings } => {
            format!("declare_persistent [{}]", bindings.join(", "))
        }
        BytecodeInstruction::Return { values } => format!("return [{}]", join_temps(values)),
    }
}

fn join_temps(temps: &[TempId]) -> String {
    temps
        .iter()
        .map(|temp| format!("t{temp}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn verify_function(function: &BytecodeFunction, issues: &mut Vec<VerificationIssue>) {
    let mut labels = BTreeSet::new();
    let mut saw_return = false;

    for instruction in &function.instructions {
        if let BytecodeInstruction::Label(label) = instruction {
            if !labels.insert(*label) {
                issues.push(VerificationIssue {
                    function: function.name.clone(),
                    message: format!("duplicate label L{label}"),
                });
            }
            if *label >= function.label_count {
                issues.push(VerificationIssue {
                    function: function.name.clone(),
                    message: format!(
                        "label L{label} exceeds declared label_count {}",
                        function.label_count
                    ),
                });
            }
        }
    }

    for instruction in &function.instructions {
        match instruction {
            BytecodeInstruction::Label(_) => {}
            BytecodeInstruction::LoadConst { dst, .. }
            | BytecodeInstruction::LoadBinding { dst, .. }
            | BytecodeInstruction::LoadBindingLValue { dst, .. }
            | BytecodeInstruction::MakeHandle { dst, .. } => {
                verify_temp(function, *dst, "destination temp", issues);
            }
            BytecodeInstruction::StoreBinding { src, .. } => {
                verify_temp(function, *src, "store source temp", issues);
            }
            BytecodeInstruction::StoreBindingIfPresent { src, .. } => {
                verify_temp(function, *src, "conditional store source temp", issues);
            }
            BytecodeInstruction::Unary { dst, src, .. } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_temp(function, *src, "unary source temp", issues);
            }
            BytecodeInstruction::Binary { dst, lhs, rhs, .. } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_temp(function, *lhs, "binary lhs temp", issues);
                verify_temp(function, *rhs, "binary rhs temp", issues);
            }
            BytecodeInstruction::BuildMatrix { dst, elements, .. }
            | BytecodeInstruction::BuildCell { dst, elements, .. } => {
                verify_temp(function, *dst, "destination temp", issues);
                for element in elements {
                    verify_temp(function, *element, "aggregate element temp", issues);
                }
            }
            BytecodeInstruction::BuildMatrixList {
                dst,
                row_item_counts,
                elements,
            }
            | BytecodeInstruction::BuildCellList {
                dst,
                row_item_counts,
                elements,
            } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_string_temp_refs(function, elements, "aggregate list element", issues);
                if row_item_counts.iter().sum::<usize>() != elements.len() {
                    issues.push(VerificationIssue {
                        function: function.name.clone(),
                        message: format!(
                            "aggregate list row item counts sum to {}, but {} source temp(s) were provided",
                            row_item_counts.iter().sum::<usize>(),
                            elements.len()
                        ),
                    });
                }
            }
            BytecodeInstruction::Range {
                dst,
                start,
                step,
                end,
            } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_temp(function, *start, "range start temp", issues);
                if let Some(step) = step {
                    verify_temp(function, *step, "range step temp", issues);
                }
                verify_temp(function, *end, "range end temp", issues);
            }
            BytecodeInstruction::Call {
                outputs,
                args,
                target,
                ..
            } => {
                for output in outputs {
                    verify_temp(function, *output, "call output temp", issues);
                }
                verify_string_temp_refs(function, args, "call argument", issues);
                verify_string_temp_ref(function, target, "call target", issues);
            }
            BytecodeInstruction::LoadIndex {
                dst, target, args, ..
            } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_temp(function, *target, "index target temp", issues);
                verify_string_temp_refs(function, args, "index argument", issues);
            }
            BytecodeInstruction::LoadIndexList {
                dst, target, args, ..
            } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_temp(function, *target, "list index target temp", issues);
                verify_string_temp_refs(function, args, "list index argument", issues);
            }
            BytecodeInstruction::StoreIndex {
                target, args, src, ..
            } => {
                verify_temp(function, *target, "store index target temp", issues);
                verify_temp(function, *src, "store index source temp", issues);
                verify_string_temp_refs(function, args, "index argument", issues);
            }
            BytecodeInstruction::LoadField { dst, target, .. } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_temp(function, *target, "field target temp", issues);
            }
            BytecodeInstruction::LoadFieldList { dst, target, .. } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_temp(function, *target, "list field target temp", issues);
            }
            BytecodeInstruction::StoreField { target, src, .. } => {
                verify_temp(function, *target, "store field target temp", issues);
                verify_temp(function, *src, "store field source temp", issues);
            }
            BytecodeInstruction::PackSpreadMatrix { dst, src }
            | BytecodeInstruction::PackSpreadCell { dst, src } => {
                verify_temp(function, *dst, "destination temp", issues);
                verify_temp(function, *src, "spread source temp", issues);
            }
            BytecodeInstruction::SplitList { outputs, src } => {
                for output in outputs {
                    verify_temp(function, *output, "split-list output temp", issues);
                }
                verify_temp(function, *src, "split-list source temp", issues);
            }
            BytecodeInstruction::PushTry { catch } => {
                verify_label(function, *catch, &labels, issues);
            }
            BytecodeInstruction::StoreLastError { .. } => {}
            BytecodeInstruction::JumpIfFalse { condition, target } => {
                verify_temp(function, *condition, "jump condition temp", issues);
                verify_label(function, *target, &labels, issues);
            }
            BytecodeInstruction::Jump { target } => {
                verify_label(function, *target, &labels, issues);
            }
            BytecodeInstruction::IterStart { iter, source } => {
                verify_temp(function, *iter, "iterator temp", issues);
                verify_temp(function, *source, "iterator source temp", issues);
            }
            BytecodeInstruction::IterHasNext { dst, iter }
            | BytecodeInstruction::IterNext { dst, iter } => {
                verify_temp(function, *dst, "iterator result temp", issues);
                verify_temp(function, *iter, "iterator temp", issues);
            }
            BytecodeInstruction::DeclareGlobal { .. }
            | BytecodeInstruction::DeclarePersistent { .. } => {}
            BytecodeInstruction::Return { values } => {
                saw_return = true;
                for value in values {
                    verify_temp(function, *value, "return temp", issues);
                }
            }
        }
    }

    if !saw_return {
        issues.push(VerificationIssue {
            function: function.name.clone(),
            message: "missing return instruction".to_string(),
        });
    }
}

fn verify_temp(
    function: &BytecodeFunction,
    temp: TempId,
    context: &str,
    issues: &mut Vec<VerificationIssue>,
) {
    if temp >= function.temp_count {
        issues.push(VerificationIssue {
            function: function.name.clone(),
            message: format!(
                "{context} t{temp} exceeds declared temp_count {}",
                function.temp_count
            ),
        });
    }
}

fn verify_label(
    function: &BytecodeFunction,
    label: LabelId,
    labels: &BTreeSet<LabelId>,
    issues: &mut Vec<VerificationIssue>,
) {
    if !labels.contains(&label) {
        issues.push(VerificationIssue {
            function: function.name.clone(),
            message: format!("jump target L{label} is missing"),
        });
    }
}

fn verify_string_temp_refs(
    function: &BytecodeFunction,
    values: &[String],
    context: &str,
    issues: &mut Vec<VerificationIssue>,
) {
    for value in values {
        verify_string_temp_ref(function, value, context, issues);
    }
}

fn verify_string_temp_ref(
    function: &BytecodeFunction,
    value: &str,
    context: &str,
    issues: &mut Vec<VerificationIssue>,
) {
    if let Some(temp) = parse_temp_ref(value) {
        verify_temp(function, temp, context, issues);
        return;
    }

    for temp in scan_temp_refs(value) {
        verify_temp(function, temp, context, issues);
    }
}

fn parse_temp_ref(value: &str) -> Option<TempId> {
    value.strip_prefix('t')?.parse::<TempId>().ok()
}

fn scan_temp_refs(value: &str) -> Vec<TempId> {
    let bytes = value.as_bytes();
    let mut refs = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b't'
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric())
            && index + 1 < bytes.len()
            && bytes[index + 1].is_ascii_digit()
        {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if let Ok(temp) = value[start..end].parse::<TempId>() {
                refs.push(temp);
            }
            index = end;
            continue;
        }
        index += 1;
    }
    refs
}

fn expression_contains_end_keyword(expression: &HirExpression) -> bool {
    match expression {
        HirExpression::EndKeyword => true,
        HirExpression::Unary { rhs, .. } => expression_contains_end_keyword(rhs),
        HirExpression::Binary { lhs, rhs, .. } => {
            expression_contains_end_keyword(lhs) || expression_contains_end_keyword(rhs)
        }
        HirExpression::Range { start, step, end } => {
            expression_contains_end_keyword(start)
                || step
                    .as_ref()
                    .is_some_and(|step| expression_contains_end_keyword(step))
                || expression_contains_end_keyword(end)
        }
        HirExpression::CellIndex { target, indices } => {
            expression_contains_end_keyword(target)
                || indices.iter().any(index_argument_contains_end_keyword)
        }
        HirExpression::Call { target, args } => {
            call_target_contains_end_keyword(target)
                || args.iter().any(index_argument_contains_end_keyword)
        }
        HirExpression::FieldAccess { target, .. } => expression_contains_end_keyword(target),
        HirExpression::MatrixLiteral(rows) | HirExpression::CellLiteral(rows) => rows
            .iter()
            .any(|row| row.iter().any(expression_contains_end_keyword)),
        _ => false,
    }
}

fn call_target_contains_end_keyword(target: &HirCallTarget) -> bool {
    match target {
        HirCallTarget::Callable(_) => false,
        HirCallTarget::Expression(expression) => expression_contains_end_keyword(expression),
    }
}

fn index_argument_contains_end_keyword(argument: &HirIndexArgument) -> bool {
    match argument {
        HirIndexArgument::Expression(expression) => expression_contains_end_keyword(expression),
        HirIndexArgument::FullSlice => false,
        HirIndexArgument::End => true,
    }
}

pub fn summary() -> &'static str {
    "Owns bytecode-style backend emission, target-oriented lowering glue, and future packaging support."
}
