use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use super::*;
use matlab_codegen::{
    emit_bytecode, verify_bytecode, BytecodeFunction, BytecodeInstruction, BytecodeModule,
};
use matlab_platform::BytecodeBundle;
use matlab_runtime::RuntimeStackFrame;

#[derive(Clone)]
struct VmTemp {
    value: Value,
    lvalue: Option<TempLValue>,
    spread: Option<Vec<Value>>,
    bound_method: Option<BoundMethod>,
    list_origin: bool,
    multi_struct_field_origin: Option<String>,
}

impl VmTemp {
    fn value(value: Value) -> Self {
        Self {
            value,
            lvalue: None,
            spread: None,
            bound_method: None,
            list_origin: false,
            multi_struct_field_origin: None,
        }
    }

    fn with_lvalue(value: Value, lvalue: Option<TempLValue>) -> Self {
        Self {
            value,
            lvalue,
            spread: None,
            bound_method: None,
            list_origin: false,
            multi_struct_field_origin: None,
        }
    }

    fn spread(values: Vec<Value>) -> Self {
        Self {
            value: first_output_or_unit(values.clone()),
            lvalue: None,
            spread: Some(values),
            bound_method: None,
            list_origin: true,
            multi_struct_field_origin: None,
        }
    }

    fn bound_method(value: Value, method: BoundMethod) -> Self {
        Self {
            value,
            lvalue: None,
            spread: None,
            bound_method: Some(method),
            list_origin: false,
            multi_struct_field_origin: None,
        }
    }

    fn list_origin_value(value: Value) -> Self {
        Self {
            value,
            lvalue: None,
            spread: None,
            bound_method: None,
            list_origin: true,
            multi_struct_field_origin: None,
        }
    }
}

#[derive(Clone)]
struct BoundMethod {
    builtin_name: String,
    receiver: Value,
}

#[derive(Clone)]
enum TempLValue {
    Path {
        root: BindingSpec,
        projections: Vec<TempLValueProjection>,
    },
}

#[derive(Clone)]
enum TempLValueProjection {
    Paren(Vec<String>),
    Brace(Vec<String>),
    Field(String),
}

#[derive(Clone)]
enum TempLValueLeaf {
    Index {
        kind: IndexAssignmentKind,
        args: Vec<String>,
        value: Value,
    },
    Field {
        field: String,
        value: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BindingSpec {
    name: String,
    binding_id: Option<BindingId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureSpec {
    name: String,
    binding_id: BindingId,
}

#[derive(Clone)]
struct VmFrame {
    cells: HashMap<BindingId, Cell>,
    names: BTreeMap<String, BindingId>,
    global_names: BTreeSet<String>,
    persistent_names: BTreeSet<String>,
    visible_functions: BTreeMap<String, String>,
}

#[derive(Clone)]
struct BytecodeHandleClosure {
    target: ClosureTarget,
    captured_cells: HashMap<BindingId, Cell>,
}

#[derive(Clone)]
enum ClosureTarget {
    Function(String),
}

#[derive(Clone)]
struct VmIterator {
    values: Vec<Value>,
    cursor: usize,
}

#[derive(Debug, Clone)]
struct ParsedTarget {
    display_name: String,
    semantic_resolution: Option<String>,
    resolved_path: Option<PathBuf>,
    resolved_class: bool,
    bundle_module_id: Option<String>,
}

pub fn execute_script_bytecode(module: &HirModule) -> Result<ExecutionResult, RuntimeError> {
    if module.kind != CompilationUnitKind::Script {
        return Err(RuntimeError::Unsupported(
            "execute_script_bytecode expects a script compilation unit".to_string(),
        ));
    }

    let mut vm = BytecodeVm::from_hir(
        module,
        "<root>".to_string(),
        Rc::new(RefCell::new(SharedRuntimeState::default())),
    )?;
    vm.execute_script()
}

pub fn execute_function_file_bytecode(
    module: &HirModule,
    args: &[Value],
) -> Result<ExecutionResult, RuntimeError> {
    if module.kind != CompilationUnitKind::FunctionFile {
        return Err(RuntimeError::Unsupported(
            "execute_function_file_bytecode expects a function-file compilation unit".to_string(),
        ));
    }

    let mut vm = BytecodeVm::from_hir(
        module,
        "<root>".to_string(),
        Rc::new(RefCell::new(SharedRuntimeState::default())),
    )?;
    vm.execute_function_file(args)
}

pub fn execute_script_bytecode_module(
    bytecode: &BytecodeModule,
    module_identity: String,
) -> Result<ExecutionResult, RuntimeError> {
    if bytecode.unit_kind != "Script" {
        return Err(RuntimeError::Unsupported(format!(
            "execute_script_bytecode_module expects a Script bytecode module, found `{}`",
            bytecode.unit_kind
        )));
    }

    let mut vm = BytecodeVm::from_bytecode(
        bytecode.clone(),
        module_identity,
        Rc::new(RefCell::new(SharedRuntimeState::default())),
        Vec::new(),
    )?;
    vm.execute_script()
}

pub fn execute_script_bytecode_bundle(
    bundle: &BytecodeBundle,
) -> Result<ExecutionResult, RuntimeError> {
    if bundle.root_module.unit_kind != "Script" {
        return Err(RuntimeError::Unsupported(format!(
            "execute_script_bytecode_bundle expects a Script bytecode module, found `{}`",
            bundle.root_module.unit_kind
        )));
    }

    let mut vm = BytecodeVm::from_bytecode_with_bundle(
        bundle.root_module.clone(),
        bundle.root_source_path.clone(),
        Rc::new(bundle_registry(bundle)),
        Rc::new(bundle_registry_by_id(bundle)),
        Rc::new(RefCell::new(SharedRuntimeState::default())),
        Vec::new(),
    )?;
    vm.execute_script()
}

pub fn execute_function_file_bytecode_module(
    bytecode: &BytecodeModule,
    args: &[Value],
    module_identity: String,
) -> Result<ExecutionResult, RuntimeError> {
    if bytecode.unit_kind != "FunctionFile" {
        return Err(RuntimeError::Unsupported(format!(
            "execute_function_file_bytecode_module expects a FunctionFile bytecode module, found `{}`",
            bytecode.unit_kind
        )));
    }

    let mut vm = BytecodeVm::from_bytecode(
        bytecode.clone(),
        module_identity,
        Rc::new(RefCell::new(SharedRuntimeState::default())),
        Vec::new(),
    )?;
    vm.execute_function_file(args)
}

pub fn execute_function_file_bytecode_bundle(
    bundle: &BytecodeBundle,
    args: &[Value],
) -> Result<ExecutionResult, RuntimeError> {
    if bundle.root_module.unit_kind != "FunctionFile" {
        return Err(RuntimeError::Unsupported(format!(
            "execute_function_file_bytecode_bundle expects a FunctionFile bytecode module, found `{}`",
            bundle.root_module.unit_kind
        )));
    }

    let mut vm = BytecodeVm::from_bytecode_with_bundle(
        bundle.root_module.clone(),
        bundle.root_source_path.clone(),
        Rc::new(bundle_registry(bundle)),
        Rc::new(bundle_registry_by_id(bundle)),
        Rc::new(RefCell::new(SharedRuntimeState::default())),
        Vec::new(),
    )?;
    vm.execute_function_file(args)
}

struct BytecodeVm {
    module_identity: String,
    bytecode: BytecodeModule,
    functions: HashMap<String, BytecodeFunction>,
    visible_functions: BTreeMap<String, String>,
    bundled_modules: Rc<HashMap<PathBuf, BytecodeModule>>,
    bundled_modules_by_id: Rc<HashMap<String, BytecodeModule>>,
    shared_state: Rc<RefCell<SharedRuntimeState>>,
    call_stack: Vec<RuntimeStackFrame>,
    handle_closures: HashMap<String, BytecodeHandleClosure>,
    next_handle_id: u32,
}

impl BytecodeVm {
    fn from_hir(
        module: &HirModule,
        module_identity: String,
        shared_state: Rc<RefCell<SharedRuntimeState>>,
    ) -> Result<Self, RuntimeError> {
        let bytecode = emit_bytecode(module);
        Self::from_bytecode(bytecode, module_identity, shared_state, Vec::new())
    }

    fn from_bytecode(
        bytecode: BytecodeModule,
        module_identity: String,
        shared_state: Rc<RefCell<SharedRuntimeState>>,
        call_stack: Vec<RuntimeStackFrame>,
    ) -> Result<Self, RuntimeError> {
        Self::from_bytecode_with_bundle(
            bytecode,
            module_identity,
            Rc::new(HashMap::new()),
            Rc::new(HashMap::new()),
            shared_state,
            call_stack,
        )
    }

    fn from_bytecode_with_bundle(
        bytecode: BytecodeModule,
        module_identity: String,
        bundled_modules: Rc<HashMap<PathBuf, BytecodeModule>>,
        bundled_modules_by_id: Rc<HashMap<String, BytecodeModule>>,
        shared_state: Rc<RefCell<SharedRuntimeState>>,
        call_stack: Vec<RuntimeStackFrame>,
    ) -> Result<Self, RuntimeError> {
        let verification = verify_bytecode(&bytecode);
        if !verification.ok() {
            let message = verification
                .issues
                .into_iter()
                .map(|issue| format!("{}: {}", issue.function, issue.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(RuntimeError::Unsupported(format!(
                "bytecode verification failed: {message}"
            )));
        }
        let functions = bytecode
            .functions
            .iter()
            .cloned()
            .map(|function| (function.name.clone(), function))
            .collect::<HashMap<_, _>>();
        let visible_functions = bytecode
            .functions
            .iter()
            .filter(|function| function.role == "function" || function.role == "nested_function")
            .map(|function| (base_function_name(&function.name), function.name.clone()))
            .collect::<BTreeMap<_, _>>();

        Ok(Self {
            module_identity,
            bytecode,
            functions,
            visible_functions,
            bundled_modules,
            bundled_modules_by_id,
            shared_state,
            call_stack,
            handle_closures: HashMap::new(),
            next_handle_id: 0,
        })
    }

    fn make_stack_frame(&self, name: impl Into<String>) -> RuntimeStackFrame {
        RuntimeStackFrame {
            file: self.module_identity.clone(),
            name: name.into(),
            line: 0,
        }
    }

    fn with_stack_frame<T>(
        &mut self,
        frame: RuntimeStackFrame,
        run: impl FnOnce(&mut Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        self.call_stack.push(frame);
        let result = run(self).map_err(|error| error.capture_stack(self.call_stack.clone()));
        self.call_stack.pop();
        result
    }

    fn execute_script(&mut self) -> Result<ExecutionResult, RuntimeError> {
        let entry = self.bytecode.entry.clone();
        let (_, frame) = self.invoke_function_with_frame(&entry, &[], None, None)?;
        flush_figure_backend(&self.shared_state);
        Ok(ExecutionResult {
            workspace: frame.export_workspace()?,
            displayed_outputs: take_displayed_outputs(&self.shared_state),
            figures: rendered_figures(&self.shared_state.borrow().graphics),
            display_format: current_display_format(&self.shared_state),
        })
    }

    fn execute_function_file(&mut self, args: &[Value]) -> Result<ExecutionResult, RuntimeError> {
        let outputs = self.invoke_primary_function(args)?;
        flush_figure_backend(&self.shared_state);
        Ok(ExecutionResult {
            workspace: outputs.into_iter().collect(),
            displayed_outputs: take_displayed_outputs(&self.shared_state),
            figures: rendered_figures(&self.shared_state.borrow().graphics),
            display_format: current_display_format(&self.shared_state),
        })
    }

    fn invoke_primary_function(
        &mut self,
        args: &[Value],
    ) -> Result<Vec<(String, Value)>, RuntimeError> {
        let function = self
            .functions
            .get(&self.bytecode.entry)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "bytecode entry `{}` is not available",
                    self.bytecode.entry
                ))
            })?;
        let output_specs = parse_binding_specs(&function.outputs)?;
        let (values, _) = self.invoke_function_with_frame(&function.name, args, None, None)?;
        Ok(output_specs
            .into_iter()
            .zip(values)
            .map(|(binding, value)| (binding.name, value))
            .collect())
    }

    fn invoke_function_with_frame(
        &mut self,
        function_name: &str,
        args: &[Value],
        caller: Option<&VmFrame>,
        captured_cells: Option<&HashMap<BindingId, Cell>>,
    ) -> Result<(Vec<Value>, VmFrame), RuntimeError> {
        let function = self.functions.get(function_name).cloned().ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "bytecode function `{function_name}` is not available"
            ))
        })?;

        let run = |this: &mut Self| {
            let params = parse_binding_specs(&function.params)?;
            let outputs = parse_binding_specs(&function.outputs)?;
            let captures = parse_capture_specs(&function.captures)?;
            if args.len() != params.len() {
                return Err(RuntimeError::Unsupported(format!(
                    "function `{}` expects {} input(s), got {}",
                    function.name,
                    params.len(),
                    args.len()
                )));
            }

            let mut visible_functions = this.visible_functions.clone();
            if let Some(caller) = caller {
                visible_functions.extend(caller.visible_functions.clone());
            }
            let mut frame = VmFrame::new(visible_functions);
            if let Some(ans_binding) = binding_spec_for_name(&function, "ans")? {
                frame.declare_binding_spec(&ans_binding)?;
            }
            if let Some(caller) = caller {
                frame.inherit_hidden_cells_from(caller);
            }

            for capture in captures {
                let cell = if let Some(captured_cells) = captured_cells {
                    captured_cells.get(&capture.binding_id).cloned()
                } else {
                    caller.and_then(|caller| caller.cell(capture.binding_id))
                }
                .ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "captured binding `{}` is not available when calling `{}`",
                        capture.name, function.name
                    ))
                })?;
                frame.bind_existing(capture.binding_id, &capture.name, cell);
            }

            for (binding, value) in params.iter().zip(args.iter()) {
                frame.assign_binding_spec(binding, value.clone())?;
            }
            for output in &outputs {
                if output.binding_id.is_some() {
                    frame.declare_binding_spec(output)?;
                }
            }

            let values = this.execute_function_body(&function, &mut frame)?;
            Ok((values, frame))
        };
        if function.role == "script"
            && self
                .call_stack
                .last()
                .is_some_and(|frame| frame.name == "<script>")
        {
            run(self)
        } else {
            let frame_name = if function.role == "script" {
                "<script>".to_string()
            } else {
                base_function_name(&function.name)
            };
            let stack_frame = self.make_stack_frame(frame_name);
            self.with_stack_frame(stack_frame, run)
        }
    }

    fn execute_function_body(
        &mut self,
        function: &BytecodeFunction,
        frame: &mut VmFrame,
    ) -> Result<Vec<Value>, RuntimeError> {
        let label_map = function
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| match instruction {
                BytecodeInstruction::Label(label) => Some((*label, index)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let mut temps = vec![None; function.temp_count as usize];
        let mut iterators = HashMap::<u32, VmIterator>::new();
        let mut try_stack = Vec::<usize>::new();
        let mut last_error = None::<RuntimeError>;
        let mut pc = 0usize;
        let lookup_label = |target: &u32| {
            label_map.get(target).copied().ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "missing jump label L{target} in `{}`",
                    function.name
                ))
            })
        };

        loop {
            while try_stack.last().is_some_and(|catch_pc| pc >= *catch_pc) {
                try_stack.pop();
            }

            let Some(instruction) = function.instructions.get(pc) else {
                return Ok(Vec::new());
            };

            let outcome = (|| -> Result<Option<Vec<Value>>, RuntimeError> {
                match instruction {
                    BytecodeInstruction::Label(_) => pc += 1,
                    BytecodeInstruction::LoadConst { dst, value } => {
                        set_temp(&mut temps, *dst, VmTemp::value(parse_const_value(value)?))?;
                        pc += 1;
                    }
                    BytecodeInstruction::LoadBinding { dst, binding } => {
                        let spec = parse_binding_spec(binding)?;
                        let value = frame.read_reference(spec.binding_id, &spec.name)?;
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::LoadBindingLValue { dst, binding } => {
                        let spec = parse_binding_spec(binding)?;
                        frame.declare_binding_spec(&spec)?;
                        let value = match frame.read_reference(spec.binding_id, &spec.name) {
                            Ok(value) => value,
                            Err(RuntimeError::MissingVariable(_)) => {
                                Value::Struct(StructValue::default())
                            }
                            Err(error) => return Err(error),
                        };
                        set_temp(
                            &mut temps,
                            *dst,
                            VmTemp::with_lvalue(
                                value,
                                Some(TempLValue::Path {
                                    root: spec,
                                    projections: Vec::new(),
                                }),
                            ),
                        )?;
                        pc += 1;
                    }
                    BytecodeInstruction::StoreBinding { binding, src } => {
                        let spec = parse_binding_spec(binding)?;
                        let value = temp_value(&temps, *src)?;
                        frame.assign_binding_spec(&spec, value)?;
                        pc += 1;
                    }
                    BytecodeInstruction::StoreBindingIfPresent { binding, src } => {
                        let spec = parse_binding_spec(binding)?;
                        if let Some(value) = temps
                            .get(*src as usize)
                            .and_then(|slot| slot.as_ref())
                            .map(|temp| temp.value.clone())
                        {
                            frame.assign_binding_spec(&spec, value)?;
                        }
                        pc += 1;
                    }
                    BytecodeInstruction::Unary { dst, op, src } => {
                        let value = apply_unary(op, &temp_value(&temps, *src)?)?;
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
                        let value = apply_binary(
                            op,
                            &temp_value(&temps, *lhs)?,
                            &temp_value(&temps, *rhs)?,
                        )?;
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::BuildMatrix {
                        dst,
                        rows,
                        cols,
                        elements,
                    } => {
                        let list_origin = *rows == 1
                            && *cols == elements.len()
                            && elements
                                .iter()
                                .all(|temp_id| temp(&temps, *temp_id).map(|temp| temp.list_origin).unwrap_or(false));
                        let values = elements
                            .iter()
                            .map(|temp| temp_value(&temps, *temp))
                            .collect::<Result<Vec<_>, _>>()?;
                        let value = Value::Matrix(MatrixValue::new(*rows, *cols, values)?);
                        set_temp(
                            &mut temps,
                            *dst,
                            if list_origin {
                                VmTemp::list_origin_value(value)
                            } else {
                                VmTemp::value(value)
                            },
                        )?;
                        pc += 1;
                    }
                    BytecodeInstruction::BuildMatrixList {
                        dst,
                        row_item_counts,
                        elements,
                    } => {
                        let rows =
                            literal_rows_from_temp_sources(&temps, row_item_counts, elements)?;
                        let value = Value::Matrix(MatrixValue::from_rows(rows)?);
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::BuildCell {
                        dst,
                        rows,
                        cols,
                        elements,
                    } => {
                        let values = elements
                            .iter()
                            .map(|temp| temp_value(&temps, *temp))
                            .collect::<Result<Vec<_>, _>>()?;
                        let value = Value::Cell(CellValue::new(*rows, *cols, values)?);
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::BuildCellList {
                        dst,
                        row_item_counts,
                        elements,
                    } => {
                        let rows =
                            literal_rows_from_temp_sources(&temps, row_item_counts, elements)?;
                        let value = Value::Cell(CellValue::from_rows(rows)?);
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::PackSpreadMatrix { dst, src } => {
                        let source = temp(&temps, *src)?;
                        let values = source
                            .spread
                            .clone()
                            .unwrap_or_else(|| vec![source.value.clone()]);
                        let value = Value::Matrix(MatrixValue::new(1, values.len(), values)?);
                        set_temp(&mut temps, *dst, VmTemp::list_origin_value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::PackSpreadCell { dst, src } => {
                        let source = temp(&temps, *src)?;
                        let values = source
                            .spread
                            .clone()
                            .unwrap_or_else(|| vec![source.value.clone()]);
                        let value = Value::Cell(CellValue::new(1, values.len(), values)?);
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::MakeHandle { dst, target } => {
                        let value = self.make_function_handle(frame, target)?;
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::Range {
                        dst,
                        start,
                        step,
                        end,
                    } => {
                        let start = temp_value(&temps, *start)?.as_scalar()?;
                        let step = match step {
                            Some(step) => temp_value(&temps, *step)?.as_scalar()?,
                            None => 1.0,
                        };
                        let end = temp_value(&temps, *end)?.as_scalar()?;
                        if step == 0.0 {
                            return Err(RuntimeError::InvalidIndex(
                                "range step cannot be zero".to_string(),
                            ));
                        }
                        let mut values = Vec::new();
                        let mut current = start;
                        if step > 0.0 {
                            while current <= end {
                                values.push(Value::Scalar(current));
                                current += step;
                            }
                        } else {
                            while current >= end {
                                values.push(Value::Scalar(current));
                                current += step;
                            }
                        }
                        let value = Value::Matrix(MatrixValue::new(1, values.len(), values)?);
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::Call {
                        outputs,
                        target,
                        args,
                    } => {
                        let values =
                            self.execute_call(frame, &temps, target, args, outputs.len())?;
                        let list_origin = parse_target(target).display_name == "deal";
                        for (dst, temp_value) in outputs.iter().zip(values.into_iter()) {
                            set_temp(
                                &mut temps,
                                *dst,
                                if list_origin {
                                    VmTemp {
                                        list_origin: true,
                                        ..temp_value
                                    }
                                } else {
                                    temp_value
                                },
                            )?;
                        }
                        pc += 1;
                    }
                    BytecodeInstruction::LoadIndex {
                        dst,
                        target,
                        kind,
                        args,
                    } => {
                        let target_temp = temp(&temps, *target)?.clone();
                        let next_needs_struct_cell_selection =
                            matches!(
                                function.instructions.get(pc + 1),
                                Some(BytecodeInstruction::LoadField { target, .. }) if *target == *dst
                            ) || matches!(
                                function.instructions.get(pc + 1),
                                Some(BytecodeInstruction::StoreField { target, .. }) if *target == *dst
                            );
                        let value = match *kind {
                            "brace" => {
                                let primary = (|| {
                                    let indices = evaluate_index_arguments_from_strings(
                                        &temps,
                                        &target_temp.value,
                                        args,
                                    )?;
                                    if target_temp.lvalue.is_some() {
                                        materialize_cell_content_index(&target_temp.value, &indices)
                                    } else {
                                        evaluate_cell_content_index(&target_temp.value, &indices)
                                    }
                                })();
                                match primary {
                                    Ok(value) => value,
                                    Err(error @ RuntimeError::TypeError(_))
                                    | Err(error @ RuntimeError::InvalidIndex(_))
                                        if target_temp.lvalue.is_some() =>
                                    {
                                        let Some(fallback_target) =
                                            default_cell_contents_value_like(&target_temp.value)
                                        else {
                                            return Err(error);
                                        };
                                        let indices = evaluate_index_arguments_from_strings(
                                            &temps,
                                            &fallback_target,
                                            args,
                                        )?;
                                        if next_needs_struct_cell_selection {
                                            if let Some(value) = materialize_default_struct_cell_selection(
                                                &fallback_target,
                                                &indices,
                                            )? {
                                                value
                                            } else {
                                                evaluate_cell_content_index(&fallback_target, &indices)?
                                            }
                                        } else {
                                            evaluate_cell_content_index(&fallback_target, &indices)?
                                        }
                                    }
                                    Err(error) => return Err(error),
                                }
                            }
                            "paren" => {
                                if let Some(field) = target_temp.multi_struct_field_origin.as_ref() {
                                    return Err(unsupported_multi_struct_field_subindexing_error(field));
                                }
                                let indices = evaluate_index_arguments_from_strings(
                                    &temps,
                                    &target_temp.value,
                                    args,
                                )?;
                                evaluate_expression_call(&target_temp.value, &indices)?
                            }
                            other => {
                                return Err(RuntimeError::Unsupported(format!(
                                    "index load kind `{other}` is not implemented"
                                )))
                            }
                        };
                        set_temp(
                            &mut temps,
                            *dst,
                            VmTemp::with_lvalue(
                                value,
                                append_temp_lvalue_projection(
                                    target_temp.lvalue,
                                    match *kind {
                                        "brace" => TempLValueProjection::Brace(args.clone()),
                                        "paren" => TempLValueProjection::Paren(args.clone()),
                                        _ => unreachable!("kind already matched"),
                                    },
                                ),
                            ),
                        )?;
                        pc += 1;
                    }
                    BytecodeInstruction::LoadIndexList {
                        dst,
                        target,
                        kind,
                        args,
                    } => {
                        let target_temp = temp(&temps, *target)?.clone();
                        let values = match *kind {
                            "brace" => {
                                if let Some(spread) = target_temp.spread.as_ref() {
                                    let mut values = Vec::new();
                                    for target_value in spread {
                                        let indices = evaluate_index_arguments_from_strings(
                                            &temps,
                                            target_value,
                                            args,
                                        )?;
                                        values.extend(evaluate_cell_content_outputs(
                                            target_value,
                                            &indices,
                                        )?);
                                    }
                                    values
                                } else {
                                    let indices = evaluate_index_arguments_from_strings(
                                        &temps,
                                        &target_temp.value,
                                        args,
                                    )?;
                                    evaluate_cell_content_outputs(&target_temp.value, &indices)?
                                }
                            }
                            "paren" => {
                                if let Some(field) = target_temp.multi_struct_field_origin.as_ref() {
                                    return Err(unsupported_multi_struct_field_subindexing_error(field));
                                }
                                if let Some(spread) = target_temp.spread.as_ref() {
                                    let mut values = Vec::new();
                                    for target_value in spread {
                                        let indices = evaluate_index_arguments_from_strings(
                                            &temps,
                                            target_value,
                                            args,
                                        )?;
                                        values.push(evaluate_expression_call(
                                            target_value,
                                            &indices,
                                        )?);
                                    }
                                    values
                                } else {
                                    let indices = evaluate_index_arguments_from_strings(
                                        &temps,
                                        &target_temp.value,
                                        args,
                                    )?;
                                    vec![evaluate_expression_call(&target_temp.value, &indices)?]
                                }
                            }
                            other => {
                                return Err(RuntimeError::Unsupported(format!(
                                    "list index load kind `{other}` is not implemented"
                                )))
                            }
                        };
                        set_temp(&mut temps, *dst, VmTemp::spread(values))?;
                        pc += 1;
                    }
                    BytecodeInstruction::StoreIndex {
                        target,
                        kind,
                        args,
                        src,
                    } => {
                        let target_temp = temp(&temps, *target)?.clone();
                        let value = temp_value(&temps, *src)?;
                        self.store_index(frame, &target_temp, kind, args, &temps, value)?;
                        pc += 1;
                    }
                    BytecodeInstruction::LoadField { dst, target, field } => {
                        let target_temp = temp(&temps, *target)?.clone();
                        if let Some(method_name) =
                            mexception_method_builtin_name(&target_temp.value, field)
                        {
                            let handle = Value::FunctionHandle(FunctionHandleValue {
                                display_name: format!("@{}.{}", "MException", field),
                                target: FunctionHandleTarget::Named(method_name.to_string()),
                            });
                            let bound = BoundMethod {
                                builtin_name: method_name.to_string(),
                                receiver: target_temp.value.clone(),
                            };
                            set_temp(&mut temps, *dst, VmTemp::bound_method(handle, bound))?;
                            pc += 1;
                        } else {
                            let target_had_lvalue = target_temp.lvalue.is_some();
                            let value = if target_temp.lvalue.is_some() {
                                read_field_lvalue_value(&target_temp.value, field)?
                            } else {
                                read_field_value(&target_temp.value, field)?
                            };
                            let lvalue = append_temp_lvalue_projection(
                                target_temp.lvalue,
                                TempLValueProjection::Field(field.clone()),
                            );
                            let mut temp_value = VmTemp::with_lvalue(value, lvalue);
                            if target_had_lvalue
                                && matches!(
                                    &target_temp.value,
                                    Value::Matrix(matrix)
                                        if matrix_is_struct_array(matrix)
                                            && matrix.element_count() > 1
                                )
                            {
                                temp_value.multi_struct_field_origin = Some(field.clone());
                            }
                            set_temp(&mut temps, *dst, temp_value)?;
                            pc += 1;
                        }
                    }
                    BytecodeInstruction::LoadFieldList { dst, target, field } => {
                        let target_temp = temp(&temps, *target)?.clone();
                        let values = if let Some(spread) = &target_temp.spread {
                            read_field_outputs_from_values(spread, field)?
                        } else {
                            read_field_outputs(&target_temp.value, field)?
                        };
                        set_temp(&mut temps, *dst, VmTemp::spread(values))?;
                        pc += 1;
                    }
                    BytecodeInstruction::StoreField {
                        target,
                        field,
                        src,
                        list_assignment,
                    } => {
                        let target_temp = temp(&temps, *target)?.clone();
                        let source_temp = temp(&temps, *src)?.clone();
                        self.store_field(
                            frame,
                            &target_temp,
                            field,
                            &temps,
                            &source_temp,
                            *list_assignment,
                        )?;
                        pc += 1;
                    }
                    BytecodeInstruction::SplitList { outputs, src } => {
                        let source = temp(&temps, *src)?;
                        let values = source
                            .spread
                            .clone()
                            .unwrap_or_else(|| vec![source.value.clone()]);
                        if values.len() < outputs.len() {
                            return Err(RuntimeError::Unsupported(format!(
                                "split-list requests {} output(s), but source only produced {}",
                                outputs.len(),
                                values.len()
                            )));
                        }
                        for (dst, value) in outputs.iter().zip(values.into_iter()) {
                            set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        }
                        pc += 1;
                    }
                    BytecodeInstruction::PushTry { catch } => {
                        try_stack.push(lookup_label(catch)?);
                        pc += 1;
                    }
                    BytecodeInstruction::StoreLastError { binding } => {
                        let spec = parse_binding_spec(binding)?;
                        let value = runtime_error_value(
                            last_error.as_ref().ok_or_else(|| {
                                RuntimeError::Unsupported(
                                    "no active runtime error is available for catch binding"
                                        .to_string(),
                                )
                            })?,
                            &self.call_stack,
                        );
                        frame.assign_binding_spec(&spec, value)?;
                        pc += 1;
                    }
                    BytecodeInstruction::JumpIfFalse { condition, target } => {
                        if !temp_value(&temps, *condition)?.truthy()? {
                            pc = lookup_label(target)?;
                        } else {
                            pc += 1;
                        }
                    }
                    BytecodeInstruction::Jump { target } => {
                        pc = lookup_label(target)?;
                    }
                    BytecodeInstruction::IterStart { iter, source } => {
                        let values = iteration_values(&temp_value(&temps, *source)?)?;
                        iterators.insert(*iter, VmIterator { values, cursor: 0 });
                        pc += 1;
                    }
                    BytecodeInstruction::IterHasNext { dst, iter } => {
                        let iterator = iterators.get(iter).ok_or_else(|| {
                            RuntimeError::Unsupported(format!(
                                "iterator temp t{iter} is not initialized"
                            ))
                        })?;
                        set_temp(
                            &mut temps,
                            *dst,
                            VmTemp::value(logical_value(iterator.cursor < iterator.values.len())),
                        )?;
                        pc += 1;
                    }
                    BytecodeInstruction::IterNext { dst, iter } => {
                        let iterator = iterators.get_mut(iter).ok_or_else(|| {
                            RuntimeError::Unsupported(format!(
                                "iterator temp t{iter} is not initialized"
                            ))
                        })?;
                        let value =
                            iterator
                                .values
                                .get(iterator.cursor)
                                .cloned()
                                .ok_or_else(|| {
                                    RuntimeError::Unsupported(format!(
                                        "iterator temp t{iter} has no next value"
                                    ))
                                })?;
                        iterator.cursor += 1;
                        set_temp(&mut temps, *dst, VmTemp::value(value))?;
                        pc += 1;
                    }
                    BytecodeInstruction::DeclareGlobal { bindings } => {
                        for binding in parse_binding_specs(bindings)? {
                            self.bind_global(frame, &binding)?;
                        }
                        pc += 1;
                    }
                    BytecodeInstruction::DeclarePersistent { bindings } => {
                        for binding in parse_binding_specs(bindings)? {
                            self.bind_persistent(frame, &binding)?;
                        }
                        pc += 1;
                    }
                    BytecodeInstruction::Return { values } => {
                        return values
                            .iter()
                            .map(|temp| temp_value(&temps, *temp))
                            .collect::<Result<Vec<_>, _>>()
                            .map(Some);
                    }
                }
                Ok(None)
            })();

            match outcome {
                Ok(Some(values)) => {
                    self.drain_pending_host_figure_events(frame)?;
                    return Ok(values);
                }
                Ok(None) => {
                    self.drain_pending_host_figure_events(frame)?;
                }
                Err(error) => {
                    if let Some(catch_pc) = try_stack.pop() {
                        last_error = Some(error);
                        pc = catch_pc;
                    } else {
                        return Err(error);
                    }
                }
            }
        }
    }

    fn execute_call(
        &mut self,
        frame: &mut VmFrame,
        temps: &[Option<VmTemp>],
        target: &str,
        args: &[String],
        requested_outputs: usize,
    ) -> Result<Vec<VmTemp>, RuntimeError> {
        if let Some(temp_id) = parse_temp_ref(target) {
            let target = temp(temps, temp_id)?.clone();
            return self.call_runtime_value_outputs(frame, &target, temps, args, requested_outputs);
        }

        let parsed = parse_target(target);
        let args = evaluate_function_arguments_from_strings(temps, args)?;
        if parsed.bundle_module_id.is_some() || parsed.resolved_path.is_some() {
            return self
                .load_and_invoke_external_target(&parsed, &args)
                .map(wrap_vm_values);
        }
        if let Some(Value::Object(object)) = args.first() {
            if object_has_method(object, &parsed.display_name) {
                return self
                    .invoke_object_method_outputs(object, &parsed.display_name, &args[1..])
                    .map(wrap_vm_values);
            }
        }
        if matches!(
            parsed.semantic_resolution.as_deref(),
            Some("BuiltinFunction")
        ) {
            if parsed.display_name == "fplot" {
                return self
                    .invoke_fplot_builtin_outputs(frame, &args, requested_outputs)
                    .map(wrap_vm_values);
            }
            if parsed.display_name == "fplot3" {
                return self
                    .invoke_fplot3_builtin_outputs(frame, &args, requested_outputs)
                    .map(wrap_vm_values);
            }
            if parsed.display_name == "fsurf" {
                return self
                    .invoke_fsurf_builtin_outputs(frame, &args, requested_outputs)
                    .map(wrap_vm_values);
            }
            if parsed.display_name == "fmesh" {
                return self
                    .invoke_fmesh_builtin_outputs(frame, &args, requested_outputs)
                    .map(wrap_vm_values);
            }
            if parsed.display_name == "fimplicit" {
                return self
                    .invoke_fimplicit_builtin_outputs(frame, &args, requested_outputs)
                    .map(wrap_vm_values);
            }
            if parsed.display_name == "fcontour" {
                return self
                    .invoke_fcontour_builtin_outputs(frame, &args, requested_outputs)
                    .map(wrap_vm_values);
            }
            if parsed.display_name == "fcontour3" {
                return self
                    .invoke_fcontour3_builtin_outputs(frame, &args, requested_outputs)
                    .map(wrap_vm_values);
            }
            if matches!(
                parsed.display_name.as_str(),
                "clear" | "clearvars" | "save" | "load" | "who" | "whos" | "tic" | "toc" | "format"
            ) {
                return self
                    .invoke_workspace_builtin_outputs(
                        frame,
                        &parsed.display_name,
                        &args,
                        requested_outputs,
                    )
                    .map(wrap_vm_values);
            }
            if parsed.display_name == "close" {
                return self
                    .invoke_close_builtin_outputs(frame, &args, requested_outputs)
                    .map(wrap_vm_values);
            }
            if matches!(parsed.display_name.as_str(), "figure" | "set") {
                return self
                    .invoke_graphics_builtin_with_resize_callbacks(
                        frame,
                        &parsed.display_name,
                        &args,
                        requested_outputs,
                    )
                    .map(wrap_vm_values);
            }
            if parsed.display_name == "arrayfun" {
                return super::execute_arrayfun_builtin_outputs(
                    &args,
                    requested_outputs,
                    |callback, call_args, requested| {
                        let Value::FunctionHandle(handle) = callback else {
                            unreachable!("arrayfun callback parser produces function handles");
                        };
                        self.call_function_value_outputs(frame, handle, call_args, requested)
                    },
                )
                .map(wrap_vm_values);
            }
            if parsed.display_name == "cellfun" {
                return super::execute_cellfun_builtin_outputs(
                    &args,
                    requested_outputs,
                    |callback, call_args, requested| {
                        let Value::FunctionHandle(handle) = callback else {
                            unreachable!("cellfun callback parser produces function handles");
                        };
                        self.call_function_value_outputs(frame, handle, call_args, requested)
                    },
                )
                .map(wrap_vm_values);
            }
            if parsed.display_name == "structfun" {
                return super::execute_structfun_builtin_outputs(
                    &args,
                    requested_outputs,
                    |callback, call_args, requested| {
                        let Value::FunctionHandle(handle) = callback else {
                            unreachable!("structfun callback parser produces function handles");
                        };
                        self.call_function_value_outputs(frame, handle, call_args, requested)
                    },
                )
                .map(wrap_vm_values);
            }
            return invoke_runtime_builtin_outputs(
                &self.shared_state,
                None,
                &parsed.display_name,
                &args,
                requested_outputs,
            )
            .map(wrap_vm_values);
        }
        if let Some(function_name) = frame
            .visible_functions
            .get(&parsed.display_name)
            .or_else(|| self.visible_functions.get(&parsed.display_name))
            .cloned()
        {
            return self
                .invoke_function_with_frame(&function_name, &args, Some(frame), None)
                .map(|(values, _)| wrap_vm_values(values));
        }

        if parsed.display_name == "fplot" {
            return self
                .invoke_fplot_builtin_outputs(frame, &args, requested_outputs)
                .map(wrap_vm_values);
        }
        if parsed.display_name == "fplot3" {
            return self
                .invoke_fplot3_builtin_outputs(frame, &args, requested_outputs)
                .map(wrap_vm_values);
        }
        if parsed.display_name == "fsurf" {
            return self
                .invoke_fsurf_builtin_outputs(frame, &args, requested_outputs)
                .map(wrap_vm_values);
        }
        if parsed.display_name == "fmesh" {
            return self
                .invoke_fmesh_builtin_outputs(frame, &args, requested_outputs)
                .map(wrap_vm_values);
        }
        if parsed.display_name == "fimplicit" {
            return self
                .invoke_fimplicit_builtin_outputs(frame, &args, requested_outputs)
                .map(wrap_vm_values);
        }
        if parsed.display_name == "fcontour" {
            return self
                .invoke_fcontour_builtin_outputs(frame, &args, requested_outputs)
                .map(wrap_vm_values);
        }
        if parsed.display_name == "fcontour3" {
            return self
                .invoke_fcontour3_builtin_outputs(frame, &args, requested_outputs)
                .map(wrap_vm_values);
        }
        if parsed.display_name == "arrayfun" {
            return super::execute_arrayfun_builtin_outputs(
                &args,
                requested_outputs,
                |callback, call_args, requested| {
                    let Value::FunctionHandle(handle) = callback else {
                        unreachable!("arrayfun callback parser produces function handles");
                    };
                    self.call_function_value_outputs(frame, handle, call_args, requested)
                },
            )
            .map(wrap_vm_values);
        }
        if parsed.display_name == "cellfun" {
            return super::execute_cellfun_builtin_outputs(
                &args,
                requested_outputs,
                |callback, call_args, requested| {
                    let Value::FunctionHandle(handle) = callback else {
                        unreachable!("cellfun callback parser produces function handles");
                    };
                    self.call_function_value_outputs(frame, handle, call_args, requested)
                },
            )
            .map(wrap_vm_values);
        }
        if parsed.display_name == "structfun" {
            return super::execute_structfun_builtin_outputs(
                &args,
                requested_outputs,
                |callback, call_args, requested| {
                    let Value::FunctionHandle(handle) = callback else {
                        unreachable!("structfun callback parser produces function handles");
                    };
                    self.call_function_value_outputs(frame, handle, call_args, requested)
                },
            )
            .map(wrap_vm_values);
        }
        if matches!(
            parsed.display_name.as_str(),
            "clear" | "clearvars" | "save" | "load" | "who" | "whos" | "tic" | "toc" | "format"
        ) {
            return self
                .invoke_workspace_builtin_outputs(
                    frame,
                    &parsed.display_name,
                    &args,
                    requested_outputs,
                )
                .map(wrap_vm_values);
        }
        if parsed.display_name == "close" {
            return self
                .invoke_close_builtin_outputs(frame, &args, requested_outputs)
                .map(wrap_vm_values);
        }
        if matches!(parsed.display_name.as_str(), "figure" | "set") {
            return self
                .invoke_graphics_builtin_with_resize_callbacks(
                    frame,
                    &parsed.display_name,
                    &args,
                    requested_outputs,
                )
                .map(wrap_vm_values);
        }

        invoke_runtime_builtin_outputs(
            &self.shared_state,
            None,
            &parsed.display_name,
            &args,
            requested_outputs,
        )
        .map(wrap_vm_values)
        .map_err(|_| {
            RuntimeError::Unsupported(format!(
                "call target `{}` is not executable in the current bytecode VM",
                parsed.display_name
            ))
        })
    }

    fn call_runtime_value_outputs(
        &mut self,
        frame: &mut VmFrame,
        target: &VmTemp,
        temps: &[Option<VmTemp>],
        args: &[String],
        requested_outputs: usize,
    ) -> Result<Vec<VmTemp>, RuntimeError> {
        if let Some(method) = &target.bound_method {
            let mut evaluated_args = Vec::with_capacity(args.len() + 1);
            evaluated_args.push(method.receiver.clone());
            evaluated_args.extend(evaluate_function_arguments_from_strings(temps, args)?);
            if matches!(
                method.builtin_name.as_str(),
                "clear" | "clearvars" | "save" | "load" | "who" | "whos" | "tic" | "toc" | "format"
            ) {
                return self
                    .invoke_workspace_builtin_outputs(
                        frame,
                        &method.builtin_name,
                        &evaluated_args,
                        requested_outputs,
                    )
                    .map(wrap_vm_values);
            }
            return invoke_runtime_builtin_outputs(
                &self.shared_state,
                None,
                &method.builtin_name,
                &evaluated_args,
                requested_outputs,
            )
            .map(wrap_vm_values);
        }

        if let Some(values) = &target.spread {
            let mut outputs = Vec::new();
            for value in values {
                match value {
                    Value::FunctionHandle(handle) => {
                        let evaluated = evaluate_function_arguments_from_strings(temps, args)?;
                        outputs.extend(
                            self.call_function_value_outputs(
                                frame,
                                handle,
                                &evaluated,
                                requested_outputs,
                            )?
                            .into_iter()
                            .map(VmTemp::value),
                        );
                    }
                    _ => {
                        let evaluated = evaluate_index_arguments_from_strings(temps, value, args)?;
                        let next = evaluate_expression_call(value, &evaluated)?;
                        outputs.push(VmTemp::value(next));
                    }
                }
            }
            return Ok(outputs);
        }

        match &target.value {
            Value::FunctionHandle(handle) => {
                let args = evaluate_function_arguments_from_strings(temps, args)?;
                self.call_function_value_outputs(frame, handle, &args, requested_outputs)
                    .map(wrap_vm_values)
            }
            _ => {
                let evaluated = evaluate_index_arguments_from_strings(temps, &target.value, args)?;
                let value = match evaluate_expression_call(&target.value, &evaluated) {
                    Ok(value) => value,
                    Err(RuntimeError::InvalidIndex(_)) if target.lvalue.is_some() => {
                        default_struct_value_like(&target.value)
                    }
                    Err(error) => return Err(error),
                };
                Ok(vec![VmTemp::with_lvalue(
                    value,
                    append_temp_lvalue_projection(
                        target.lvalue.clone(),
                        TempLValueProjection::Paren(args.to_vec()),
                    ),
                )])
            }
        }
    }

    fn call_function_value_outputs(
        &mut self,
        frame: &mut VmFrame,
        handle: &FunctionHandleValue,
        args: &[Value],
        requested_outputs: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        match &handle.target {
            FunctionHandleTarget::ResolvedPath(path) => {
                self.load_and_invoke_external_function(path, args)
            }
            FunctionHandleTarget::BundleModule(module_id) => {
                self.load_and_invoke_bundled_module(module_id, args)
            }
            FunctionHandleTarget::Named(name) => {
                if let Some(closure) = self.handle_closures.get(name).cloned() {
                    return match closure.target {
                        ClosureTarget::Function(function_name) => self
                            .invoke_function_with_frame(
                                &function_name,
                                args,
                                None,
                                Some(&closure.captured_cells),
                            )
                            .map(|(values, _)| values),
                    };
                }
                if let Some(function_name) = frame
                    .visible_functions
                    .get(name)
                    .or_else(|| self.visible_functions.get(name))
                    .cloned()
                {
                    return self
                        .invoke_function_with_frame(&function_name, args, Some(frame), None)
                        .map(|(values, _)| values);
                }
                if name == "close" {
                    return self.invoke_close_builtin_outputs(frame, args, requested_outputs);
                }
                if name == "arrayfun" {
                    return super::execute_arrayfun_builtin_outputs(
                        args,
                        requested_outputs,
                        |callback, call_args, requested| {
                            let Value::FunctionHandle(handle) = callback else {
                                unreachable!("arrayfun callback parser produces function handles");
                            };
                            self.call_function_value_outputs(frame, handle, call_args, requested)
                        },
                    );
                }
                if name == "cellfun" {
                    return super::execute_cellfun_builtin_outputs(
                        args,
                        requested_outputs,
                        |callback, call_args, requested| {
                            let Value::FunctionHandle(handle) = callback else {
                                unreachable!("cellfun callback parser produces function handles");
                            };
                            self.call_function_value_outputs(frame, handle, call_args, requested)
                        },
                    );
                }
                if name == "structfun" {
                    return super::execute_structfun_builtin_outputs(
                        args,
                        requested_outputs,
                        |callback, call_args, requested| {
                            let Value::FunctionHandle(handle) = callback else {
                                unreachable!("structfun callback parser produces function handles");
                            };
                            self.call_function_value_outputs(frame, handle, call_args, requested)
                        },
                    );
                }
                if matches!(name.as_str(), "figure" | "set") {
                    return self.invoke_graphics_builtin_with_resize_callbacks(
                        frame,
                        name,
                        args,
                        requested_outputs,
                    );
                }
                if matches!(
                    name.as_str(),
                    "clear"
                        | "clearvars"
                        | "save"
                        | "load"
                        | "who"
                        | "whos"
                        | "tic"
                        | "toc"
                        | "format"
                ) {
                    return self.invoke_workspace_builtin_outputs(
                        frame,
                        name,
                        args,
                        requested_outputs,
                    );
                }
                if name == "fplot" {
                    return self.invoke_fplot_builtin_outputs(frame, args, requested_outputs);
                }
                if name == "fplot3" {
                    return self.invoke_fplot3_builtin_outputs(frame, args, requested_outputs);
                }
                if name == "fsurf" {
                    return self.invoke_fsurf_builtin_outputs(frame, args, requested_outputs);
                }
                if name == "fmesh" {
                    return self.invoke_fmesh_builtin_outputs(frame, args, requested_outputs);
                }
                if name == "fimplicit" {
                    return self.invoke_fimplicit_builtin_outputs(frame, args, requested_outputs);
                }
                if name == "fcontour" {
                    return self.invoke_fcontour_builtin_outputs(frame, args, requested_outputs);
                }
                if name == "fcontour3" {
                    return self.invoke_fcontour3_builtin_outputs(frame, args, requested_outputs);
                }
                invoke_runtime_builtin_outputs(
                    &self.shared_state,
                    None,
                    name,
                    args,
                    requested_outputs,
                )
                .map_err(|_| {
                    RuntimeError::Unsupported(format!(
                        "function handle `{}` is not executable in the current bytecode VM",
                        handle.display_name
                    ))
                })
            }
            FunctionHandleTarget::BoundMethod { receiver, method_name, .. } => {
                let Value::Object(object) = receiver.as_ref() else {
                    return Err(RuntimeError::Unsupported(format!(
                        "bound method handle `{}` does not carry an object receiver",
                        handle.display_name
                    )));
                };
                self.invoke_object_method_outputs(object, method_name, args)
            }
        }
    }

    fn find_module_function<'b>(&self, module: &'b HirModule, name: &str) -> Option<&'b HirFunction> {
        module.items.iter().find_map(|item| match item {
            HirItem::Function(function) if function.name == name => Some(function),
            HirItem::Statement(_) | HirItem::Function(_) => None,
        })
    }

    fn build_default_object_from_class(
        &mut self,
        class_module: &HirModule,
        class: &matlab_ir::HirClass,
        module_identity: String,
    ) -> Result<Value, RuntimeError> {
        let mut evaluator = Interpreter::with_shared_state(
            class_module,
            module_identity,
            Rc::clone(&self.shared_state),
            self.call_stack.clone(),
        );
        let mut frame = Frame::new(evaluator.module_functions.clone());
        let mut fields = BTreeMap::new();
        let mut field_order = Vec::new();
        for property in &class.properties {
            let value = if let Some(default) = &property.default {
                evaluator.evaluate_expression(&mut frame, default)?
            } else {
                empty_matrix_value()
            };
            field_order.push(property.name.clone());
            fields.insert(property.name.clone(), value);
        }
        Ok(Value::Object(ObjectValue::new(
            ObjectClassMetadata {
                class_name: class.name.clone(),
                package: class.package.clone(),
                storage_kind: if class.inherits_handle {
                    ObjectStorageKind::Handle
                } else {
                    ObjectStorageKind::Value
                },
                source_path: class.source_path.clone(),
                property_order: field_order.clone(),
                inline_methods: class.inline_methods.iter().cloned().collect(),
                external_methods: class
                    .external_methods
                    .iter()
                    .map(|method| (method.name.clone(), method.path.clone()))
                    .collect(),
                constructor: class.constructor.clone(),
            },
            StructValue::with_field_order(fields, field_order),
        )))
    }

    fn construct_class_from_path(
        &mut self,
        path: &Path,
        args: &[Value],
    ) -> Result<Vec<Value>, RuntimeError> {
        let loaded = super::load_class_module_from_path(path)?;
        let default_object = self.build_default_object_from_class(
            &loaded.module,
            &loaded.class,
            loaded.source_path.display().to_string(),
        )?;
        if let Some(constructor_name) = &loaded.class.constructor {
            let constructor = self.find_module_function(&loaded.module, constructor_name).ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "constructor `{constructor_name}` is not available in class `{}`",
                    loaded.class.name
                ))
            })?;
            let prebound_outputs = constructor
                .outputs
                .first()
                .map(|binding| vec![(binding.name.clone(), default_object.clone())])
                .unwrap_or_default();
            let mut interpreter = Interpreter::with_shared_state(
                &loaded.module,
                loaded.source_path.display().to_string(),
                Rc::clone(&self.shared_state),
                self.call_stack.clone(),
            );
            let mut values = interpreter.invoke_function_with_prebound_outputs(
                constructor,
                args,
                None,
                &prebound_outputs,
            )?;
            if values.is_empty() {
                values.push(default_object);
            }
            return Ok(values);
        }
        Ok(vec![default_object])
    }

    fn invoke_object_method_outputs(
        &mut self,
        object: &ObjectValue,
        method_name: &str,
        args: &[Value],
    ) -> Result<Vec<Value>, RuntimeError> {
        if object.class.inline_methods.contains(method_name) {
            let source_path = object.class.source_path.as_ref().ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "class `{}` does not record its source path for inline method dispatch",
                    object.class.class_name
                ))
            })?;
            let loaded = super::load_class_module_from_path(source_path)?;
            let method = self.find_module_function(&loaded.module, method_name).ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "inline method `{method_name}` is not available in class `{}`",
                    object.class.class_name
                ))
            })?;
            let mut method_args = Vec::with_capacity(args.len() + 1);
            method_args.push(Value::Object(object.clone()));
            method_args.extend(args.iter().cloned());
            let mut interpreter = Interpreter::with_shared_state(
                &loaded.module,
                loaded.source_path.display().to_string(),
                Rc::clone(&self.shared_state),
                self.call_stack.clone(),
            );
            return interpreter.invoke_function(method, &method_args, None);
        }

        let path = object
            .class
            .external_methods
            .get(method_name)
            .ok_or_else(|| {
                RuntimeError::MissingVariable(format!(
                    "object method `{method_name}` is not defined for class `{}`",
                    object.class.class_name
                ))
            })?
            .clone();
        let source = fs::read_to_string(&path).map_err(|error| {
            RuntimeError::Unsupported(format!(
                "failed to read external method `{}`: {error}",
                path.display()
            ))
        })?;
        let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
        if parsed.has_errors() {
            return Err(RuntimeError::Unsupported(format!(
                "failed to parse external method `{}`: {}",
                path.display(),
                format_frontend_diagnostics(&parsed.diagnostics)
            )));
        }
        let unit = parsed.unit.ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "parser produced no compilation unit for external method `{}`",
                path.display()
            ))
        })?;
        let context =
            ResolverContext::from_source_file(path.clone()).with_env_search_roots("MATC_PATH");
        let analysis = analyze_compilation_unit_with_context(&unit, &context);
        if analysis.has_errors() {
            return Err(RuntimeError::Unsupported(format!(
                "failed to analyze external method `{}`: {}",
                path.display(),
                format_semantic_diagnostics(&analysis.diagnostics)
            )));
        }
        let hir = lower_to_hir(&unit, &analysis);
        let mut method_args = Vec::with_capacity(args.len() + 1);
        method_args.push(Value::Object(object.clone()));
        method_args.extend(args.iter().cloned());
        let mut interpreter = Interpreter::with_shared_state(
            &hir,
            path.display().to_string(),
            Rc::clone(&self.shared_state),
            self.call_stack.clone(),
        );
        interpreter.invoke_primary_function(&method_args).map(|values| {
            values.into_iter().map(|(_, value)| value).collect()
        })
    }

    fn invoke_workspace_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        name: &str,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        match name {
            "clear" => {
                invoke_clear_builtin_outputs_vm(frame, &self.shared_state, args, output_arity)
            }
            "clearvars" => invoke_clearvars_builtin_outputs_vm(frame, args, output_arity),
            "save" => invoke_save_builtin_outputs_vm(frame, &self.shared_state, args, output_arity),
            "load" => invoke_load_builtin_outputs_vm(frame, &self.shared_state, args, output_arity),
            "who" => invoke_who_builtin_outputs_vm(frame, &self.shared_state, args, output_arity),
            "whos" => invoke_whos_builtin_outputs_vm(frame, &self.shared_state, args, output_arity),
            "tic" => invoke_tic_builtin_outputs(&self.shared_state, args, output_arity),
            "toc" => invoke_toc_builtin_outputs(&self.shared_state, args, output_arity),
            "format" => invoke_format_builtin_outputs(&self.shared_state, args, output_arity),
            _ => Err(RuntimeError::Unsupported(format!(
                "workspace builtin `{name}` is not implemented in the current bytecode VM"
            ))),
        }
    }

    fn invoke_close_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let request = {
            let state = self.shared_state.borrow();
            close_request_handles(&state.graphics, args, "close")?
        };

        for handle in &request.handles {
            let callback = {
                let state = self.shared_state.borrow();
                figure_close_request_callback(&state.graphics, *handle)?
            };
            if let Some(callback) = callback {
                {
                    let mut state = self.shared_state.borrow_mut();
                    select_current_figure_handle(&mut state.graphics, *handle)?;
                }
                self.invoke_figure_close_callback(frame, &callback)?;
            } else {
                let mut state = self.shared_state.borrow_mut();
                close_figures_now(&mut state.graphics, &[*handle])?;
            }
        }

        flush_figure_backend(&self.shared_state);
        close_status_outputs(
            request.status_if_empty || !request.handles.is_empty(),
            output_arity,
            "close",
        )
    }

    fn invoke_figure_close_callback(
        &mut self,
        frame: &mut VmFrame,
        callback: &Value,
    ) -> Result<(), RuntimeError> {
        if matches!(callback, Value::FunctionHandle(handle) if handle.display_name.eq_ignore_ascii_case("close"))
        {
            return Err(RuntimeError::Unsupported(
                "figure `CloseRequestFcn` callbacks should use `closereq`, not `close`, to avoid recursive close handling"
                    .to_string(),
            ));
        }
        let callback_value = match callback {
            Value::FunctionHandle(handle) => handle.clone(),
            Value::CharArray(text) | Value::String(text) => FunctionHandleValue {
                display_name: {
                    if text.eq_ignore_ascii_case("close") {
                        return Err(RuntimeError::Unsupported(
                            "figure `CloseRequestFcn` callbacks should use `closereq`, not `close`, to avoid recursive close handling"
                                .to_string(),
                        ));
                    }
                    text.clone()
                },
                target: FunctionHandleTarget::Named(text.clone()),
            },
            _ => return Err(RuntimeError::Unsupported(
                "figure close callbacks currently support function handles or text function names"
                    .to_string(),
            )),
        };
        self.call_function_value_outputs(frame, &callback_value, &[], 0)?;
        Ok(())
    }

    fn invoke_graphics_builtin_with_resize_callbacks(
        &mut self,
        frame: &mut VmFrame,
        name: &str,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let before = {
            let state = self.shared_state.borrow();
            figure_resize_callback_snapshot(&state.graphics)
        };
        let result = {
            let mut state = self.shared_state.borrow_mut();
            invoke_graphics_builtin_outputs(&mut state.graphics, name, args, output_arity)
                .ok_or_else(|| {
                    RuntimeError::Unsupported(format!("{name} builtin is unavailable"))
                })?
        }?;
        let callbacks = {
            let state = self.shared_state.borrow();
            let after = figure_resize_callback_snapshot(&state.graphics);
            changed_resize_callbacks(&before, &after, &state.active_resize_callbacks)
        };
        for (handle, callback) in callbacks {
            {
                let mut state = self.shared_state.borrow_mut();
                select_current_figure_handle(&mut state.graphics, handle)?;
                state.active_resize_callbacks.insert(handle);
            }
            let callback_result = self.invoke_figure_resize_callback(frame, &callback);
            {
                let mut state = self.shared_state.borrow_mut();
                state.active_resize_callbacks.remove(&handle);
            }
            callback_result?;
        }
        flush_figure_backend(&self.shared_state);
        Ok(result)
    }

    fn drain_pending_host_figure_events(
        &mut self,
        frame: &mut VmFrame,
    ) -> Result<(), RuntimeError> {
        loop {
            let (close_handles, resize_handles) = {
                let mut state = self.shared_state.borrow_mut();
                let close_handles = state
                    .pending_host_close_events
                    .iter()
                    .copied()
                    .collect::<Vec<_>>();
                let resize_handles = state
                    .pending_host_resize_events
                    .iter()
                    .copied()
                    .collect::<Vec<_>>();
                state.pending_host_close_events.clear();
                state.pending_host_resize_events.clear();
                (close_handles, resize_handles)
            };

            if close_handles.is_empty() && resize_handles.is_empty() {
                break;
            }

            let mut did_work = false;

            for handle in close_handles {
                let (active, callback) = {
                    let state = self.shared_state.borrow();
                    (
                        state.active_close_callbacks.contains(&handle),
                        figure_close_request_callback(&state.graphics, handle)?,
                    )
                };
                if let Some(callback) = callback {
                    if active {
                        let mut state = self.shared_state.borrow_mut();
                        close_figures_now(&mut state.graphics, &[handle])?;
                    } else {
                        {
                            let mut state = self.shared_state.borrow_mut();
                            select_current_figure_handle(&mut state.graphics, handle)?;
                            state.active_close_callbacks.insert(handle);
                        }
                        let callback_result = self.invoke_figure_close_callback(frame, &callback);
                        {
                            let mut state = self.shared_state.borrow_mut();
                            state.active_close_callbacks.remove(&handle);
                        }
                        callback_result?;
                    }
                } else {
                    let mut state = self.shared_state.borrow_mut();
                    close_figures_now(&mut state.graphics, &[handle])?;
                }
                did_work = true;
            }

            for handle in resize_handles {
                let (active, callback) = {
                    let state = self.shared_state.borrow();
                    (
                        state.active_resize_callbacks.contains(&handle),
                        resize_callback_for_handle(&state, handle),
                    )
                };
                if let Some(callback) = callback {
                    if active {
                        continue;
                    }
                    {
                        let mut state = self.shared_state.borrow_mut();
                        select_current_figure_handle(&mut state.graphics, handle)?;
                        state.active_resize_callbacks.insert(handle);
                    }
                    let callback_result = self.invoke_figure_resize_callback(frame, &callback);
                    {
                        let mut state = self.shared_state.borrow_mut();
                        state.active_resize_callbacks.remove(&handle);
                    }
                    callback_result?;
                    did_work = true;
                }
            }

            if did_work {
                flush_figure_backend(&self.shared_state);
            }
        }
        Ok(())
    }

    fn invoke_figure_resize_callback(
        &mut self,
        frame: &mut VmFrame,
        callback: &Value,
    ) -> Result<(), RuntimeError> {
        let callback_value = match callback {
            Value::FunctionHandle(handle) => handle.clone(),
            Value::CharArray(text) | Value::String(text) => FunctionHandleValue {
                display_name: text.clone(),
                target: FunctionHandleTarget::Named(text.clone()),
            },
            _ => return Err(RuntimeError::Unsupported(
                "figure resize callbacks currently support function handles or text function names"
                    .to_string(),
            )),
        };
        self.call_function_value_outputs(frame, &callback_value, &[], 0)?;
        Ok(())
    }

    fn invoke_fplot_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let spec = parse_fplot_spec(args)?;
        let x_values = sampled_fplot_x_values(spec.interval, spec.sample_count);
        let x_arg = fplot_vector_value(&x_values)?;
        let function_value = normalize_fplot_function_arg(&spec.function, "fplot")?;
        let mut outputs = self.call_function_value_outputs(
            frame,
            match &function_value {
                Value::FunctionHandle(handle) => handle,
                _ => unreachable!("normalize_fplot_function_arg always returns a function handle"),
            },
            &[x_arg.clone()],
            1,
        )?;
        let y_value = outputs.pop().ok_or_else(|| {
            RuntimeError::Unsupported("fplot function did not produce an output".to_string())
        })?;
        let y_values = fplot_numeric_output_values(&y_value, x_values.len(), "fplot")?;
        let mut plot_args = vec![x_arg, fplot_vector_value(&y_values)?];
        if let Some(style) = spec.style {
            plot_args.push(style);
        }
        plot_args.extend(spec.property_pairs);
        let mut state = self.shared_state.borrow_mut();
        let result =
            invoke_graphics_builtin_outputs(&mut state.graphics, "plot", &plot_args, output_arity)
                .ok_or_else(|| {
                    RuntimeError::Unsupported("plot builtin is unavailable".to_string())
                })??;
        drop(state);
        flush_figure_backend(&self.shared_state);
        Ok(result)
    }

    fn invoke_fplot3_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let spec = parse_fplot3_spec(args)?;
        let x_values = sampled_fplot_x_values(spec.interval, spec.sample_count);
        let x_arg = fplot_vector_value(&x_values)?;
        let x_function = normalize_fplot_function_arg(&spec.x_function, "fplot3")?;
        let y_function = normalize_fplot_function_arg(&spec.y_function, "fplot3")?;
        let z_function = normalize_fplot_function_arg(&spec.z_function, "fplot3")?;

        let x_values = {
            let mut outputs = self.call_function_value_outputs(
                frame,
                match &x_function {
                    Value::FunctionHandle(handle) => handle,
                    _ => unreachable!(
                        "normalize_fplot_function_arg always returns a function handle"
                    ),
                },
                &[x_arg.clone()],
                1,
            )?;
            let x_value = outputs.pop().ok_or_else(|| {
                RuntimeError::Unsupported("fplot3 X function did not produce an output".to_string())
            })?;
            fplot_numeric_output_values(&x_value, x_values.len(), "fplot3")?
        };
        let y_values = {
            let mut outputs = self.call_function_value_outputs(
                frame,
                match &y_function {
                    Value::FunctionHandle(handle) => handle,
                    _ => unreachable!(
                        "normalize_fplot_function_arg always returns a function handle"
                    ),
                },
                &[x_arg.clone()],
                1,
            )?;
            let y_value = outputs.pop().ok_or_else(|| {
                RuntimeError::Unsupported("fplot3 Y function did not produce an output".to_string())
            })?;
            fplot_numeric_output_values(&y_value, x_values.len(), "fplot3")?
        };
        let z_values = {
            let mut outputs = self.call_function_value_outputs(
                frame,
                match &z_function {
                    Value::FunctionHandle(handle) => handle,
                    _ => unreachable!(
                        "normalize_fplot_function_arg always returns a function handle"
                    ),
                },
                &[x_arg.clone()],
                1,
            )?;
            let z_value = outputs.pop().ok_or_else(|| {
                RuntimeError::Unsupported("fplot3 Z function did not produce an output".to_string())
            })?;
            fplot_numeric_output_values(&z_value, x_values.len(), "fplot3")?
        };

        let mut plot_args = vec![
            fplot_vector_value(&x_values)?,
            fplot_vector_value(&y_values)?,
            fplot_vector_value(&z_values)?,
        ];
        if let Some(style) = spec.style {
            plot_args.push(style);
        }
        plot_args.extend(spec.property_pairs);
        let mut state = self.shared_state.borrow_mut();
        let result =
            invoke_graphics_builtin_outputs(&mut state.graphics, "plot3", &plot_args, output_arity)
                .ok_or_else(|| {
                    RuntimeError::Unsupported("plot3 builtin is unavailable".to_string())
                })??;
        drop(state);
        flush_figure_backend(&self.shared_state);
        Ok(result)
    }

    fn invoke_fsurf_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.invoke_function_surface_builtin_outputs(frame, args, output_arity, "fsurf", "surf")
    }

    fn invoke_fmesh_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.invoke_function_surface_builtin_outputs(frame, args, output_arity, "fmesh", "mesh")
    }

    fn invoke_fimplicit_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let spec = parse_fimplicit_spec(args)?;
        let (rows, cols, x_values, y_values, x_grid, y_grid) =
            sampled_surface_grid(spec.domain, spec.sample_count);
        let x_arg = surface_matrix_value(rows, cols, &x_grid)?;
        let y_arg = surface_matrix_value(rows, cols, &y_grid)?;
        let function_value = normalize_fplot_function_arg(&spec.function, "fimplicit")?;
        let handle = match &function_value {
            Value::FunctionHandle(handle) => handle,
            _ => unreachable!("normalize_fplot_function_arg always returns a function handle"),
        };
        let mut outputs = self.call_function_value_outputs(frame, handle, &[x_arg, y_arg], 1)?;
        let z_value = outputs.pop().ok_or_else(|| {
            RuntimeError::Unsupported("fimplicit function did not produce an output".to_string())
        })?;
        let z_values = fsurf_numeric_output_values(&z_value, rows, cols, "fimplicit")?;
        let plot_args = vec![
            fplot_vector_value(&x_values)?,
            fplot_vector_value(&y_values)?,
            surface_matrix_value(rows, cols, &z_values)?,
            Value::Scalar(0.0),
        ];
        let mut state = self.shared_state.borrow_mut();
        let result = invoke_graphics_builtin_outputs(
            &mut state.graphics,
            "contour",
            &plot_args,
            output_arity,
        )
        .ok_or_else(|| RuntimeError::Unsupported("contour builtin is unavailable".to_string()))??;
        drop(state);
        flush_figure_backend(&self.shared_state);
        Ok(result)
    }

    fn invoke_fcontour_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.invoke_function_contour_builtin_outputs(
            frame,
            args,
            output_arity,
            "fcontour",
            "contour",
        )
    }

    fn invoke_fcontour3_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.invoke_function_contour_builtin_outputs(
            frame,
            args,
            output_arity,
            "fcontour3",
            "contour3",
        )
    }

    fn invoke_function_surface_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
        builtin_name: &str,
        graphics_builtin: &str,
    ) -> Result<Vec<Value>, RuntimeError> {
        let spec = parse_fsurf_spec(args, builtin_name)?;
        let (rows, cols, x_values, y_values, x_grid, y_grid) =
            sampled_surface_grid(spec.domain, spec.sample_count);
        let x_arg = surface_matrix_value(rows, cols, &x_grid)?;
        let y_arg = surface_matrix_value(rows, cols, &y_grid)?;
        let function_value = normalize_fplot_function_arg(&spec.function, builtin_name)?;
        let handle = match &function_value {
            Value::FunctionHandle(handle) => handle,
            _ => unreachable!("normalize_fplot_function_arg always returns a function handle"),
        };
        let mut outputs = self.call_function_value_outputs(frame, handle, &[x_arg, y_arg], 1)?;
        let z_value = outputs.pop().ok_or_else(|| {
            RuntimeError::Unsupported(format!("{builtin_name} function did not produce an output"))
        })?;
        let z_values = fsurf_numeric_output_values(&z_value, rows, cols, builtin_name)?;
        let mut plot_args = vec![
            fplot_vector_value(&x_values)?,
            fplot_vector_value(&y_values)?,
            surface_matrix_value(rows, cols, &z_values)?,
        ];
        plot_args.extend(spec.property_pairs);
        let mut state = self.shared_state.borrow_mut();
        let result = invoke_graphics_builtin_outputs(
            &mut state.graphics,
            graphics_builtin,
            &plot_args,
            output_arity,
        )
        .ok_or_else(|| {
            RuntimeError::Unsupported(format!("{graphics_builtin} builtin is unavailable"))
        })??;
        drop(state);
        flush_figure_backend(&self.shared_state);
        Ok(result)
    }

    fn invoke_function_contour_builtin_outputs(
        &mut self,
        frame: &mut VmFrame,
        args: &[Value],
        output_arity: usize,
        builtin_name: &str,
        graphics_builtin: &str,
    ) -> Result<Vec<Value>, RuntimeError> {
        let spec = parse_fcontour_spec(args, builtin_name)?;
        let (rows, cols, x_values, y_values, x_grid, y_grid) =
            sampled_surface_grid(spec.domain, spec.sample_count);
        let x_arg = surface_matrix_value(rows, cols, &x_grid)?;
        let y_arg = surface_matrix_value(rows, cols, &y_grid)?;
        let function_value = normalize_fplot_function_arg(&spec.function, builtin_name)?;
        let handle = match &function_value {
            Value::FunctionHandle(handle) => handle,
            _ => unreachable!("normalize_fplot_function_arg always returns a function handle"),
        };
        let mut outputs = self.call_function_value_outputs(frame, handle, &[x_arg, y_arg], 1)?;
        let z_value = outputs.pop().ok_or_else(|| {
            RuntimeError::Unsupported(format!("{builtin_name} function did not produce an output"))
        })?;
        let z_values = fsurf_numeric_output_values(&z_value, rows, cols, builtin_name)?;
        let mut plot_args = vec![
            fplot_vector_value(&x_values)?,
            fplot_vector_value(&y_values)?,
            surface_matrix_value(rows, cols, &z_values)?,
        ];
        if let Some(levels) = spec.levels {
            plot_args.push(levels);
        }
        let mut state = self.shared_state.borrow_mut();
        let result = invoke_graphics_builtin_outputs(
            &mut state.graphics,
            graphics_builtin,
            &plot_args,
            output_arity,
        )
        .ok_or_else(|| {
            RuntimeError::Unsupported(format!("{graphics_builtin} builtin is unavailable"))
        })??;
        drop(state);
        flush_figure_backend(&self.shared_state);
        Ok(result)
    }

    fn make_function_handle(
        &mut self,
        frame: &VmFrame,
        target: &str,
    ) -> Result<Value, RuntimeError> {
        let parsed = parse_target(target);
        if let Some(module_id) = parsed.bundle_module_id {
            return Ok(Value::FunctionHandle(FunctionHandleValue {
                display_name: parsed.display_name,
                target: FunctionHandleTarget::BundleModule(module_id),
            }));
        }
        if let Some(path) = parsed.resolved_path {
            return Ok(Value::FunctionHandle(FunctionHandleValue {
                display_name: parsed.display_name,
                target: FunctionHandleTarget::ResolvedPath(path),
            }));
        }

        if let Some(function_name) = frame
            .visible_functions
            .get(&parsed.display_name)
            .or_else(|| self.visible_functions.get(&parsed.display_name))
            .cloned()
            .or_else(|| {
                self.functions
                    .contains_key(&parsed.display_name)
                    .then_some(parsed.display_name.clone())
            })
        {
            let captures = parse_capture_specs(&self.function(&function_name)?.captures)?;
            let mut captured_cells = HashMap::new();
            for capture in captures {
                let cell = frame.cell(capture.binding_id).ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "captured binding `{}` is not available for function handle `{}`",
                        capture.name, parsed.display_name
                    ))
                })?;
                captured_cells.insert(capture.binding_id, cell);
            }
            let handle_name = format!("<bytecode-handle:{}>", self.next_handle_id);
            self.next_handle_id += 1;
            self.handle_closures.insert(
                handle_name.clone(),
                BytecodeHandleClosure {
                    target: ClosureTarget::Function(function_name),
                    captured_cells,
                },
            );
            return Ok(Value::FunctionHandle(FunctionHandleValue {
                display_name: parsed.display_name,
                target: FunctionHandleTarget::Named(handle_name),
            }));
        }

        if matches!(
            parsed.semantic_resolution.as_deref(),
            Some("BuiltinFunction")
        ) {
            return Ok(Value::FunctionHandle(FunctionHandleValue {
                display_name: parsed.display_name.clone(),
                target: FunctionHandleTarget::Named(parsed.display_name),
            }));
        }

        Err(RuntimeError::Unsupported(format!(
            "function handle target `{target}` is not executable in the current bytecode VM"
        )))
    }

    fn load_and_invoke_external_target(
        &mut self,
        parsed: &ParsedTarget,
        args: &[Value],
    ) -> Result<Vec<Value>, RuntimeError> {
        if let Some(module_id) = &parsed.bundle_module_id {
            return self.load_and_invoke_bundled_module(module_id, args);
        }
        if let Some(path) = &parsed.resolved_path {
            if parsed.resolved_class {
                return self.construct_class_from_path(path, args);
            }
            return self.load_and_invoke_external_function(path, args);
        }
        Err(RuntimeError::Unsupported(format!(
            "external call target `{}` has no resolvable module identity",
            parsed.display_name
        )))
    }

    fn load_and_invoke_bundled_module(
        &mut self,
        module_id: &str,
        args: &[Value],
    ) -> Result<Vec<Value>, RuntimeError> {
        let module = self
            .bundled_modules_by_id
            .get(module_id)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "bundle module `{module_id}` is not available in the current bytecode bundle"
                ))
            })?;
        let mut vm = BytecodeVm::from_bytecode_with_bundle(
            module,
            format!("<bundle:{module_id}>"),
            Rc::clone(&self.bundled_modules),
            Rc::clone(&self.bundled_modules_by_id),
            Rc::clone(&self.shared_state),
            self.call_stack.clone(),
        )?;
        Ok(vm
            .invoke_primary_function(args)?
            .into_iter()
            .map(|(_, value)| value)
            .collect())
    }

    fn load_and_invoke_external_function(
        &mut self,
        path: &Path,
        args: &[Value],
    ) -> Result<Vec<Value>, RuntimeError> {
        if let Some(module) = self.bundled_modules.get(path).cloned() {
            let mut vm = BytecodeVm::from_bytecode_with_bundle(
                module,
                path.display().to_string(),
                Rc::clone(&self.bundled_modules),
                Rc::clone(&self.bundled_modules_by_id),
                Rc::clone(&self.shared_state),
                self.call_stack.clone(),
            )?;
            return Ok(vm
                .invoke_primary_function(args)?
                .into_iter()
                .map(|(_, value)| value)
                .collect());
        }

        let source = fs::read_to_string(path).map_err(|error| {
            RuntimeError::Unsupported(format!(
                "failed to read external bytecode function `{}`: {error}",
                path.display()
            ))
        })?;
        let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
        if parsed.has_errors() {
            return Err(RuntimeError::Unsupported(format!(
                "failed to parse external function `{}`: {}",
                path.display(),
                format_frontend_diagnostics(&parsed.diagnostics)
            )));
        }

        let unit = parsed.unit.ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "parser produced no compilation unit for external function `{}`",
                path.display()
            ))
        })?;
        let context = ResolverContext::from_source_file(path.to_path_buf())
            .with_env_search_roots("MATC_PATH");
        let analysis = analyze_compilation_unit_with_context(&unit, &context);
        if analysis.has_errors() {
            return Err(RuntimeError::Unsupported(format!(
                "failed to analyze external function `{}`: {}",
                path.display(),
                format_semantic_diagnostics(&analysis.diagnostics)
            )));
        }

        let hir = lower_to_hir(&unit, &analysis);
        let mut vm = BytecodeVm::from_hir(
            &hir,
            path.display().to_string(),
            Rc::clone(&self.shared_state),
        )?;
        vm.call_stack = self.call_stack.clone();
        Ok(vm
            .invoke_primary_function(args)?
            .into_iter()
            .map(|(_, value)| value)
            .collect())
    }

    fn function(&self, name: &str) -> Result<&BytecodeFunction, RuntimeError> {
        self.functions.get(name).ok_or_else(|| {
            RuntimeError::Unsupported(format!("bytecode function `{name}` is not available"))
        })
    }

    fn bind_global(
        &mut self,
        frame: &mut VmFrame,
        binding: &BindingSpec,
    ) -> Result<(), RuntimeError> {
        let binding_id = binding.binding_id.ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "global binding `{}` does not have runtime identity",
                binding.name
            ))
        })?;
        let cell = {
            let mut state = self.shared_state.borrow_mut();
            state
                .globals
                .entry(binding.name.clone())
                .or_insert_with(|| Rc::new(RefCell::new(None)))
                .clone()
        };
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(empty_matrix_value());
        }
        frame.bind_existing(binding_id, &binding.name, cell);
        frame.global_names.insert(binding.name.clone());
        Ok(())
    }

    fn bind_persistent(
        &mut self,
        frame: &mut VmFrame,
        binding: &BindingSpec,
    ) -> Result<(), RuntimeError> {
        let binding_id = binding.binding_id.ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "persistent binding `{}` does not have runtime identity",
                binding.name
            ))
        })?;
        let key = PersistentKey {
            module_identity: self.module_identity.clone(),
            binding_id,
            name: binding.name.clone(),
        };
        let cell = {
            let mut state = self.shared_state.borrow_mut();
            state
                .persistents
                .entry(key)
                .or_insert_with(|| Rc::new(RefCell::new(None)))
                .clone()
        };
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(empty_matrix_value());
        }
        frame.bind_existing(binding_id, &binding.name, cell);
        frame.persistent_names.insert(binding.name.clone());
        Ok(())
    }

    fn store_field(
        &mut self,
        frame: &VmFrame,
        target: &VmTemp,
        field: &str,
        temps: &[Option<VmTemp>],
        source: &VmTemp,
        list_assignment: bool,
    ) -> Result<(), RuntimeError> {
        let (root, projections) = lvalue_root_and_projections(&target.lvalue)?;
        if !list_assignment && temp_field_target_requires_list_assignment(&projections) {
            if let Some(count) =
                simple_temp_struct_field_assignment_target_count(frame, target, temps)?
            {
                return Err(unsupported_simple_csl_assignment_error(count));
            }
        }
        let cell = frame
            .cell_for_reference(root.binding_id, &root.name)
            .ok_or_else(|| {
                RuntimeError::MissingVariable(format!("variable `{}` is not defined", root.name))
            })?;
        if cell.borrow().is_none()
            && source.list_origin
            && (list_assignment || temp_field_target_requires_list_assignment(&projections))
        {
            if let Some(updated) =
                list_assignment
                    .then(|| default_temp_struct_csl_root_value(&projections, field, source))
                    .transpose()?
                    .flatten()
            {
                *cell.borrow_mut() = Some(updated);
                return Ok(());
            }
            if let Some(updated) =
                default_temp_indexed_struct_csl_root_value(temps, &projections, field, source)?
            {
                *cell.borrow_mut() = Some(updated);
                return Ok(());
            }
            if let Some(updated) =
                default_temp_cell_struct_csl_root_value(temps, &projections, field, source)?
            {
                *cell.borrow_mut() = Some(updated);
                return Ok(());
            }
            if !projections
                .iter()
                .any(|projection| matches!(projection, TempLValueProjection::Paren(_)))
            {
                return Err(RuntimeError::Unsupported(
                    "comma-separated list assignment to a nonexistent struct array requires an explicit indexed receiver"
                        .to_string(),
                ));
            }
        }
        if cell.borrow().is_none() {
            if let Some(updated) =
                list_assignment
                    .then(|| default_temp_struct_direct_root_value(&projections, field, source))
                    .transpose()?
                    .flatten()
            {
                *cell.borrow_mut() = Some(updated);
                return Ok(());
            }
            if let Some(updated) =
                default_temp_indexed_struct_direct_root_value(temps, &projections, field, source)?
            {
                *cell.borrow_mut() = Some(updated);
                return Ok(());
            }
            if let Some(updated) =
                default_temp_cell_struct_direct_root_value(temps, &projections, field, source)?
            {
                *cell.borrow_mut() = Some(updated);
                return Ok(());
            }
        }
        let current = match cell.borrow().clone() {
            Some(current) => current,
            None => default_temp_lvalue_root_value(
                &projections,
                &TempLValueLeaf::Field {
                    field: field.to_string(),
                    value: source.value.clone(),
                },
            )
            .ok_or_else(|| {
                RuntimeError::MissingVariable(format!(
                    "variable `{}` is declared but has no runtime value",
                    root.name
                ))
            })?,
        };
        let updated = self.assign_lvalue_path(
            current,
            &projections,
            false,
            TempLValueLeaf::Field {
                field: field.to_string(),
                value: source.value.clone(),
            },
            temps,
        )?;
        *cell.borrow_mut() = Some(updated);
        Ok(())
    }

    fn store_index(
        &mut self,
        frame: &VmFrame,
        target: &VmTemp,
        kind: &str,
        args: &[String],
        temps: &[Option<VmTemp>],
        value: Value,
    ) -> Result<(), RuntimeError> {
        let (root, projections) = lvalue_root_and_projections(&target.lvalue)?;
        let cell = frame
            .cell_for_reference(root.binding_id, &root.name)
            .ok_or_else(|| {
                RuntimeError::MissingVariable(format!("variable `{}` is not defined", root.name))
            })?;
        let leaf = TempLValueLeaf::Index {
            kind: match kind {
                "paren" => IndexAssignmentKind::Paren,
                "brace" => IndexAssignmentKind::Brace,
                other => {
                    return Err(RuntimeError::Unsupported(format!(
                        "indexed assignment kind `{other}` is not implemented"
                    )))
                }
            },
            args: args.to_vec(),
            value: value.clone(),
        };
        let current_value = cell.borrow().clone();
        let current = match current_value {
            Some(current) => current,
            None
                if matches!(
                    leaf,
                    TempLValueLeaf::Index {
                        kind: IndexAssignmentKind::Brace,
                        ..
                    }
                )
                    && projections.iter().any(|projection| {
                        matches!(projection, TempLValueProjection::Brace(_))
                    })
                    && projections.iter().all(|projection| {
                        matches!(
                            projection,
                            TempLValueProjection::Field(_) | TempLValueProjection::Brace(_)
                        )
                    }) =>
            {
                if let Some(updated) =
                    try_materialize_undefined_cell_receiver_struct_field_brace_csl_assignment(
                        &projections,
                        args,
                        temps,
                        &value,
                    )?
                {
                    *cell.borrow_mut() = Some(updated);
                    return Ok(());
                }
                default_temp_lvalue_root_value(&projections, &leaf).ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "variable `{}` is declared but has no runtime value",
                        root.name
                    ))
                })?
            }
            None
                if matches!(
                    leaf,
                    TempLValueLeaf::Index {
                        kind: IndexAssignmentKind::Brace,
                        ..
                    }
                )
                    && projections
                        .iter()
                        .any(|projection| matches!(projection, TempLValueProjection::Paren(_)))
                    && projections.iter().all(|projection| {
                        matches!(projection, TempLValueProjection::Field(_) | TempLValueProjection::Paren(_))
                    }) =>
            {
                if let Some(updated) =
                    try_materialize_undefined_indexed_struct_field_brace_csl_assignment(
                        &projections,
                        args,
                        temps,
                        &value,
                    )?
                {
                    *cell.borrow_mut() = Some(updated);
                    return Ok(());
                }
                default_temp_lvalue_root_value(&projections, &leaf).ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "variable `{}` is declared but has no runtime value",
                        root.name
                    ))
                })?
            }
            None
                if matches!(
                    leaf,
                    TempLValueLeaf::Index {
                        kind: IndexAssignmentKind::Brace,
                        ..
                    }
                )
                    && projections
                        .iter()
                        .all(|projection| matches!(projection, TempLValueProjection::Field(_))) =>
            {
                if let Some(updated) = try_materialize_undefined_struct_field_brace_csl_assignment(
                    &projections,
                    args,
                    temps,
                    &value,
                )? {
                    *cell.borrow_mut() = Some(updated);
                    return Ok(());
                }
                default_temp_lvalue_root_value(&projections, &leaf).ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "variable `{}` is declared but has no runtime value",
                        root.name
                    ))
                })?
            }
            None
                if matches!(
                    leaf,
                    TempLValueLeaf::Index {
                        kind: IndexAssignmentKind::Brace,
                        ..
                    }
                )
                    && projections.is_empty() =>
            {
                if let Some(updated) =
                    try_materialize_undefined_root_brace_csl_assignment(temps, args, &value)?
                {
                    *cell.borrow_mut() = Some(updated);
                    return Ok(());
                }
                if undefined_temp_brace_assignment_with_colon(&projections, &leaf) {
                    return Err(RuntimeError::Unsupported(
                        "comma-separated list assignment to a nonexistent variable is not supported when any index is a colon"
                            .to_string(),
                    ));
                }
                default_temp_lvalue_root_value(&projections, &leaf).ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "variable `{}` is declared but has no runtime value",
                        root.name
                    ))
                })?
            }
            None if undefined_temp_brace_assignment_with_colon(&projections, &leaf) => {
                return Err(RuntimeError::Unsupported(
                    "comma-separated list assignment to a nonexistent variable is not supported when any index is a colon"
                        .to_string(),
                ));
            }
            None => default_temp_lvalue_root_value(&projections, &leaf).ok_or_else(|| {
                RuntimeError::MissingVariable(format!(
                    "variable `{}` is declared but has no runtime value",
                    root.name
                ))
            })?,
        };
        let updated = self.assign_lvalue_path(current, &projections, false, leaf, temps)?;
        *cell.borrow_mut() = Some(updated);
        Ok(())
    }

    fn assign_lvalue_path(
        &mut self,
        current: Value,
        projections: &[TempLValueProjection],
        nested_context: bool,
        leaf: TempLValueLeaf,
        temps: &[Option<VmTemp>],
    ) -> Result<Value, RuntimeError> {
        let Some((projection, rest)) = projections.split_first() else {
            return self.apply_lvalue_leaf(current, nested_context, leaf, temps);
        };

        match projection {
            TempLValueProjection::Field(field) => {
                if matches!(
                    &current,
                    Value::Matrix(matrix)
                        if matrix_is_struct_array(matrix)
                            && matrix.element_count() > 1
                            && (temp_field_subindexing_requires_single_struct(rest)
                                || matches!(leaf, TempLValueLeaf::Index { kind: IndexAssignmentKind::Paren, .. }))
                ) {
                    return Err(unsupported_multi_struct_field_subindexing_error(field));
                }
                if let Some(updated) = self.try_assign_distributed_struct_field_projection(
                    &current, field, rest, &leaf, temps,
                )? {
                    return Ok(updated);
                }
                let next =
                    read_field_lvalue_value_for_temp_assignment(&current, field, rest, &leaf)?;
                let updated_next = self.assign_lvalue_path(next, rest, true, leaf, temps)?;
                assign_struct_path(current, std::slice::from_ref(field), updated_next)
            }
            TempLValueProjection::Paren(args) => {
                let evaluated_indices =
                    evaluate_index_arguments_from_strings(temps, &current, args)?;
                let next = match evaluate_expression_call(&current, &evaluated_indices) {
                    Ok(next) => next,
                    Err(RuntimeError::InvalidIndex(_)) => match &leaf {
                        TempLValueLeaf::Field { .. } => {
                            default_struct_selection_value_for_index_update(
                                &current,
                                &evaluated_indices,
                            )?
                            .or_else(|| default_nested_temp_lvalue_value(rest, &leaf))
                            .ok_or_else(|| {
                                RuntimeError::InvalidIndex(
                                    "indexed bytecode assignment path could not synthesize a missing intermediate value"
                                        .to_string(),
                                )
                            })?
                        }
                        _ => default_nested_temp_lvalue_value(rest, &leaf).ok_or_else(|| {
                            RuntimeError::InvalidIndex(
                                "indexed bytecode assignment path could not synthesize a missing intermediate value"
                                    .to_string(),
                            )
                        })?,
                    },
                    Err(error) => return Err(error),
                };
                let updated_next = self.assign_lvalue_path(next, rest, true, leaf, temps)?;
                apply_index_update(
                    current,
                    &evaluated_indices,
                    updated_next,
                    IndexAssignmentKind::Paren,
                )
            }
            TempLValueProjection::Brace(args) => {
                let evaluated_indices =
                    evaluate_index_arguments_from_strings(temps, &current, args)?;
                let next = match materialize_cell_content_index(&current, &evaluated_indices) {
                    Ok(next) => next,
                    Err(RuntimeError::InvalidIndex(_)) => {
                        default_nested_temp_lvalue_value(rest, &leaf).ok_or_else(|| {
                            RuntimeError::InvalidIndex(
                                "cell bytecode assignment path could not synthesize a missing intermediate value"
                                    .to_string(),
                            )
                        })?
                    }
                    Err(error) => return Err(error),
                };
                if rest.is_empty() {
                    if let TempLValueLeaf::Index {
                        kind: IndexAssignmentKind::Brace,
                        args: leaf_args,
                        value,
                    } = &leaf
                    {
                        if let Some(updated_next) = self
                            .try_assign_nested_list_expanded_cell_contents(
                                &next, leaf_args, value, temps,
                            )?
                        {
                            return apply_index_update(
                                current,
                                &evaluated_indices,
                                updated_next,
                                IndexAssignmentKind::Brace,
                            );
                        }
                    }
                }
                let updated_next = self.assign_lvalue_path(next, rest, true, leaf, temps)?;
                apply_index_update(
                    current,
                    &evaluated_indices,
                    updated_next,
                    IndexAssignmentKind::Brace,
                )
            }
        }
    }

    fn apply_lvalue_leaf(
        &mut self,
        current: Value,
        nested_context: bool,
        leaf: TempLValueLeaf,
        temps: &[Option<VmTemp>],
    ) -> Result<Value, RuntimeError> {
        match leaf {
            TempLValueLeaf::Field { field, value } => {
                if let Some(updated) =
                    try_assign_field_to_nested_cell_containers(&current, &field, &value)?
                {
                    return Ok(updated);
                }
                assign_struct_path(current, std::slice::from_ref(&field), value)
            }
            TempLValueLeaf::Index { kind, args, value } => {
                let evaluated_indices =
                    evaluate_index_arguments_from_strings(temps, &current, &args)?;
                if nested_context && kind == IndexAssignmentKind::Brace {
                    if let Some(updated) = try_assign_selected_nested_cell_contents(
                        &current,
                        &evaluated_indices,
                        &value,
                    )? {
                        return Ok(updated);
                    }
                }
                apply_index_update(current, &evaluated_indices, value, kind)
            }
        }
    }

    fn try_assign_nested_list_expanded_cell_contents(
        &mut self,
        current: &Value,
        args: &[String],
        value: &Value,
        temps: &[Option<VmTemp>],
    ) -> Result<Option<Value>, RuntimeError> {
        let Value::Cell(outer_cell) = current else {
            return Ok(None);
        };
        if !outer_cell
            .elements
            .iter()
            .all(|element| matches!(element, Value::Cell(_)))
        {
            return Ok(None);
        }

        let mut plans = Vec::with_capacity(outer_cell.elements.len());
        let mut total_count = 0;
        for element in &outer_cell.elements {
            let evaluated_indices = evaluate_index_arguments_from_strings(temps, element, args)?;
            let Value::Cell(cell) = element else {
                unreachable!("guard ensured nested cell assignment targets");
            };
            let selection = cell_selection(cell, &evaluated_indices)?;
            total_count += selection.positions.len();
            plans.push((element.clone(), evaluated_indices, selection));
        }

        let mut rhs_values = distributed_cell_assignment_values(value, total_count)?.into_iter();
        let mut updated_elements = Vec::with_capacity(plans.len());
        for (element, evaluated_indices, selection) in plans {
            let count = selection.positions.len();
            let chunk = rhs_values.by_ref().take(count).collect::<Vec<_>>();
            let rhs_value = pack_cell_assignment_rhs(&selection, chunk)?;
            updated_elements.push(apply_index_update(
                element,
                &evaluated_indices,
                rhs_value,
                IndexAssignmentKind::Brace,
            )?);
        }

        Ok(Some(Value::Cell(CellValue::with_dimensions(
            outer_cell.rows,
            outer_cell.cols,
            outer_cell.dims.clone(),
            updated_elements,
        )?)))
    }

    fn try_assign_distributed_struct_field_projection(
        &mut self,
        current: &Value,
        field: &String,
        rest: &[TempLValueProjection],
        leaf: &TempLValueLeaf,
        temps: &[Option<VmTemp>],
    ) -> Result<Option<Value>, RuntimeError> {
        let Some(TempLValueProjection::Brace(_)) = rest.first() else {
            return Ok(None);
        };

        match current {
            Value::Matrix(matrix) if matrix_is_struct_array(matrix) => {
                let mut elements = Vec::with_capacity(matrix.elements.len());
                for element in &matrix.elements {
                    let next = match read_field_value(element, field) {
                        Ok(value) => value,
                        Err(RuntimeError::MissingVariable(_)) => {
                            default_nested_temp_lvalue_value(rest, leaf).ok_or_else(|| {
                                RuntimeError::MissingVariable(format!(
                                    "struct field `{field}` is not defined"
                                ))
                            })?
                        }
                        Err(error) => return Err(error),
                    };
                    let updated_next =
                        self.assign_lvalue_path(next, rest, true, leaf.clone(), temps)?;
                    elements.push(assign_struct_path(
                        element.clone(),
                        std::slice::from_ref(field),
                        updated_next,
                    )?);
                }
                Ok(Some(Value::Matrix(MatrixValue::with_dimensions(
                    matrix.rows,
                    matrix.cols,
                    matrix.dims.clone(),
                    elements,
                )?)))
            }
            Value::Cell(cell) if cell.elements.iter().all(value_is_struct_assignment_target) => {
                let mut elements = Vec::with_capacity(cell.elements.len());
                for element in &cell.elements {
                    let next = match read_field_value(element, field) {
                        Ok(value) => value,
                        Err(RuntimeError::MissingVariable(_)) => {
                            default_nested_temp_lvalue_value(rest, leaf).ok_or_else(|| {
                                RuntimeError::MissingVariable(format!(
                                    "struct field `{field}` is not defined"
                                ))
                            })?
                        }
                        Err(error) => return Err(error),
                    };
                    let updated_next =
                        self.assign_lvalue_path(next, rest, true, leaf.clone(), temps)?;
                    elements.push(assign_struct_path(
                        element.clone(),
                        std::slice::from_ref(field),
                        updated_next,
                    )?);
                }
                Ok(Some(Value::Cell(CellValue::with_dimensions(
                    cell.rows,
                    cell.cols,
                    cell.dims.clone(),
                    elements,
                )?)))
            }
            _ => Ok(None),
        }
    }
}

fn bundle_registry(bundle: &BytecodeBundle) -> HashMap<PathBuf, BytecodeModule> {
    bundle
        .dependency_modules
        .iter()
        .map(|module| (PathBuf::from(&module.source_path), module.module.clone()))
        .collect()
}

fn bundle_registry_by_id(bundle: &BytecodeBundle) -> HashMap<String, BytecodeModule> {
    bundle
        .dependency_modules
        .iter()
        .map(|module| (module.module_id.clone(), module.module.clone()))
        .collect()
}

impl VmFrame {
    fn new(visible_functions: BTreeMap<String, String>) -> Self {
        Self {
            cells: HashMap::new(),
            names: BTreeMap::new(),
            global_names: BTreeSet::new(),
            persistent_names: BTreeSet::new(),
            visible_functions,
        }
    }

    fn declare_binding_spec(&mut self, binding: &BindingSpec) -> Result<(), RuntimeError> {
        let binding_id = binding.binding_id.ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "binding `{}` does not have runtime identity",
                binding.name
            ))
        })?;
        self.names.insert(binding.name.clone(), binding_id);
        self.cells
            .entry(binding_id)
            .or_insert_with(|| Rc::new(RefCell::new(None)));
        Ok(())
    }

    fn bind_existing(&mut self, binding_id: BindingId, name: &str, cell: Cell) {
        self.cells.insert(binding_id, cell);
        if !name.starts_with('<') {
            self.names.insert(name.to_string(), binding_id);
        }
    }

    fn inherit_hidden_cells_from(&mut self, caller: &VmFrame) {
        for (binding_id, cell) in &caller.cells {
            self.cells
                .entry(*binding_id)
                .or_insert_with(|| cell.clone());
        }
    }

    fn assign_binding_spec(
        &mut self,
        binding: &BindingSpec,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.declare_binding_spec(binding)?;
        let binding_id = binding.binding_id.expect("declare checked binding id");
        if let Some(cell) = self.cells.get(&binding_id) {
            *cell.borrow_mut() = Some(value);
        }
        Ok(())
    }

    fn read_reference(
        &self,
        binding_id: Option<BindingId>,
        name: &str,
    ) -> Result<Value, RuntimeError> {
        if let Some(binding_id) = binding_id {
            if let Some(cell) = self.cells.get(&binding_id) {
                return cell.borrow().clone().ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "variable `{name}` is declared but has no runtime value"
                    ))
                });
            }
        }
        if let Some(binding_id) = self.names.get(name).copied() {
            if let Some(cell) = self.cells.get(&binding_id) {
                return cell.borrow().clone().ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "variable `{name}` is declared but has no runtime value"
                    ))
                });
            }
        }
        if let Ok(value) = builtin_value_from_name(name) {
            return Ok(value);
        }
        Err(RuntimeError::MissingVariable(format!(
            "variable `{name}` is not defined"
        )))
    }

    fn cell(&self, binding_id: BindingId) -> Option<Cell> {
        self.cells.get(&binding_id).cloned()
    }

    fn cell_for_reference(&self, binding_id: Option<BindingId>, name: &str) -> Option<Cell> {
        if let Some(binding_id) = binding_id {
            if let Some(cell) = self.cells.get(&binding_id) {
                return Some(cell.clone());
            }
        }
        self.names
            .get(name)
            .and_then(|binding_id| self.cells.get(binding_id))
            .cloned()
    }

    fn export_workspace(&self) -> Result<Workspace, RuntimeError> {
        let mut workspace = Workspace::new();
        for (name, binding_id) in &self.names {
            if let Some(cell) = self.cells.get(binding_id) {
                if let Some(value) = cell.borrow().clone() {
                    workspace.insert(name.clone(), value);
                }
            }
        }
        Ok(workspace)
    }

    fn visible_workspace_names(&self) -> Vec<String> {
        self.names
            .keys()
            .filter(|name| !name.starts_with('<'))
            .cloned()
            .collect()
    }

    fn clear_workspace_names(&mut self, names: &BTreeSet<String>) {
        let mut binding_ids = Vec::new();
        for name in names {
            if let Some(binding_id) = self.names.remove(name) {
                binding_ids.push(binding_id);
            }
            self.global_names.remove(name);
            self.persistent_names.remove(name);
        }
        for binding_id in binding_ids {
            self.cells.remove(&binding_id);
        }
    }

    fn clear_global_names(&mut self) -> BTreeSet<String> {
        let names = self.global_names.clone();
        self.clear_workspace_names(&names);
        names
    }

    fn insert_workspace_value(
        &mut self,
        shared_state: &Rc<RefCell<SharedRuntimeState>>,
        name: String,
        value: Value,
    ) {
        if let Some(binding_id) = self.names.get(&name).copied() {
            if let Some(cell) = self.cells.get(&binding_id) {
                *cell.borrow_mut() = Some(value);
                return;
            }
        }
        let binding_id = allocate_dynamic_binding_id(shared_state);
        let cell = Rc::new(RefCell::new(Some(value)));
        self.bind_existing(binding_id, &name, cell);
    }
}

fn invoke_clear_builtin_outputs_vm(
    frame: &mut VmFrame,
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(
            "clear currently does not return outputs".to_string(),
        ));
    }
    let visible_names = frame.visible_workspace_names();
    let spec = parse_clear_spec(args, "clear")?;
    let clears_current_workspace = spec.all
        || !spec.names.is_empty()
        || !spec.regexes.is_empty()
        || (!spec.clear_globals && !spec.clear_functions);
    let names_to_clear = if clears_current_workspace {
        select_workspace_names(visible_names, &spec)?
    } else {
        BTreeSet::new()
    };
    frame.clear_workspace_names(&names_to_clear);
    if spec.clear_globals {
        clear_global_state_vm(
            frame,
            shared_state,
            if spec.all || (spec.names.is_empty() && spec.regexes.is_empty()) {
                None
            } else {
                Some(&names_to_clear)
            },
        );
    }
    if spec.clear_functions {
        clear_persistent_state_vm(frame, shared_state);
    }
    Ok(Vec::new())
}

fn invoke_clearvars_builtin_outputs_vm(
    frame: &mut VmFrame,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(
            "clearvars currently does not return outputs".to_string(),
        ));
    }
    let spec = parse_clearvars_spec(args)?;
    let visible_names = frame.visible_workspace_names();
    let mut names_to_clear = select_workspace_names(visible_names.clone(), &spec.targets)?;
    apply_clearvars_keep_filters(
        &mut names_to_clear,
        &visible_names,
        &spec.keep,
        &spec.keep_regex,
    )?;
    frame.clear_workspace_names(&names_to_clear);
    Ok(Vec::new())
}

fn invoke_save_builtin_outputs_vm(
    frame: &mut VmFrame,
    _shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(
            "save currently does not return outputs".to_string(),
        ));
    }
    let spec = parse_save_spec(args)?;
    let workspace = frame.export_workspace()?;
    let selected = if let Some(struct_name) = spec.struct_name {
        let value = workspace.get(&struct_name).cloned().ok_or_else(|| {
            RuntimeError::MissingVariable(format!(
                "variable `{struct_name}` is not defined for save"
            ))
        })?;
        let Value::Struct(struct_value) = value else {
            return Err(RuntimeError::TypeError(format!(
                "save -struct currently expects `{struct_name}` to be a scalar struct"
            )));
        };
        let mut filtered = Workspace::new();
        let field_names = if let Some(names) = spec.names {
            names.into_iter().collect()
        } else if spec.regexes.is_empty() {
            struct_value.field_names().to_vec()
        } else {
            let mut names = BTreeSet::new();
            for pattern in &spec.regexes {
                for field_name in struct_value.field_names() {
                    if matlab_regexp_is_match(pattern, field_name)? {
                        names.insert(field_name.clone());
                    }
                }
            }
            names.into_iter().collect()
        };
        for field_name in field_names {
            let value = struct_value
                .fields
                .get(&field_name)
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "field `{field_name}` is not defined on struct `{struct_name}`"
                    ))
                })?;
            filtered.insert(field_name, value);
        }
        filtered
    } else {
        let visible_names = workspace.keys().cloned().collect::<Vec<_>>();
        let selection = select_workspace_names(
            visible_names,
            &ClearSelectionSpec {
                all: spec.names.is_none() && spec.regexes.is_empty(),
                clear_globals: false,
                clear_functions: false,
                names: spec.names.unwrap_or_default(),
                regexes: spec.regexes.clone(),
            },
        )?;
        let mut filtered = Workspace::new();
        for name in selection {
            let value = workspace.get(&name).cloned().ok_or_else(|| {
                RuntimeError::MissingVariable(format!("variable `{name}` is not defined for save"))
            })?;
            filtered.insert(name, value);
        }
        filtered
    };
    let merged = if spec.append && spec.path.exists() {
        let mut existing = if workspace_snapshot_extension(&spec.path) {
            read_workspace_snapshot(&spec.path)
                .map_err(|error| RuntimeError::Unsupported(error.to_string()))?
        } else {
            read_mat_file(&spec.path)
                .map_err(|error| RuntimeError::Unsupported(error.to_string()))?
        };
        for (name, value) in selected {
            existing.insert(name, value);
        }
        existing
    } else {
        selected
    };
    if workspace_snapshot_extension(&spec.path) {
        write_workspace_snapshot(&spec.path, &merged)
            .map_err(|error| RuntimeError::Unsupported(error.to_string()))?;
    } else {
        write_mat_file(&spec.path, &merged)
            .map_err(|error| RuntimeError::Unsupported(error.to_string()))?;
    }
    Ok(Vec::new())
}

fn invoke_load_builtin_outputs_vm(
    frame: &mut VmFrame,
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_load_arguments(args)?;
    let mut workspace = if workspace_snapshot_extension(&spec.path) {
        read_workspace_snapshot(&spec.path)
            .map_err(|error| RuntimeError::Unsupported(error.to_string()))?
    } else {
        read_mat_file(&spec.path).map_err(|error| RuntimeError::Unsupported(error.to_string()))?
    };
    let selected_names = select_name_filters(
        workspace.keys().cloned().collect(),
        &spec.names,
        &spec.regexes,
    )?;
    if !selected_names.is_empty() {
        workspace.retain(|name, _| selected_names.contains(name));
    }
    match output_arity {
        0 => {
            for (name, value) in workspace {
                frame.insert_workspace_value(shared_state, name, value);
            }
            Ok(Vec::new())
        }
        1 => Ok(vec![workspace_struct_value(workspace)]),
        _ => Err(RuntimeError::Unsupported(
            "load currently supports at most one output".to_string(),
        )),
    }
}

fn invoke_who_builtin_outputs_vm(
    frame: &VmFrame,
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let entries = workspace_entries_vm(frame, args, "who")?;
    match output_arity {
        0 => {
            let mut out = String::from("Your variables are:\n\n");
            if entries.is_empty() {
                out.push_str("  (none)");
            } else {
                out.push_str("  ");
                out.push_str(
                    &entries
                        .iter()
                        .map(|(name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        .join("  "),
                );
            }
            push_text_displayed_output(shared_state, out, true);
            Ok(Vec::new())
        }
        1 => Ok(vec![Value::Cell(CellValue::new(
            1,
            entries.len(),
            entries
                .into_iter()
                .map(|(name, _)| Value::CharArray(name))
                .collect(),
        )?)]),
        _ => Err(RuntimeError::Unsupported(
            "who currently supports at most one output".to_string(),
        )),
    }
}

fn invoke_whos_builtin_outputs_vm(
    frame: &VmFrame,
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let entries = workspace_entries_vm(frame, args, "whos")?;
    match output_arity {
        0 => {
            let mut out = String::from("  Name      Size            Bytes  Class\n");
            for (name, value) in &entries {
                let size = matlab_size_text(value);
                let bytes = approximate_value_bytes(value);
                let class_name = matlab_class_name(value);
                out.push_str(&format!(
                    "  {name:<8}  {size:<14}  {bytes:>5}  {class_name}\n"
                ));
            }
            if entries.is_empty() {
                out.push_str("  (none)\n");
            }
            push_text_displayed_output(shared_state, out, false);
            Ok(Vec::new())
        }
        1 => Ok(vec![Value::Matrix(MatrixValue::new(
            1,
            entries.len(),
            entries
                .into_iter()
                .map(|(name, value)| whos_struct_value(&name, &value))
                .collect(),
        )?)]),
        _ => Err(RuntimeError::Unsupported(
            "whos currently supports at most one output".to_string(),
        )),
    }
}

fn workspace_entries_vm(
    frame: &VmFrame,
    args: &[Value],
    builtin_name: &str,
) -> Result<Vec<(String, Value)>, RuntimeError> {
    match parse_workspace_query_spec(args, builtin_name)? {
        WorkspaceQuerySpec::Current { names, regexes } => {
            let visible_names = frame
                .names
                .keys()
                .filter(|name| !name.starts_with('<'))
                .cloned()
                .collect::<Vec<_>>();
            let filter_names = select_name_filters(visible_names, &names, &regexes)?;
            let mut entries = Vec::new();
            for (name, binding_id) in &frame.names {
                if name.starts_with('<') {
                    continue;
                }
                if !filter_names.is_empty() && !filter_names.contains(name) {
                    continue;
                }
                let Some(cell) = frame.cells.get(binding_id) else {
                    continue;
                };
                let Some(value) = cell.borrow().clone() else {
                    continue;
                };
                entries.push((name.clone(), value));
            }
            Ok(entries)
        }
        WorkspaceQuerySpec::File {
            path,
            names,
            regexes,
        } => {
            let workspace = if workspace_snapshot_extension(&path) {
                read_workspace_snapshot(&path)
                    .map_err(|error| RuntimeError::Unsupported(error.to_string()))?
            } else {
                read_mat_file(&path)
                    .map_err(|error| RuntimeError::Unsupported(error.to_string()))?
            };
            let filter_names =
                select_name_filters(workspace.keys().cloned().collect(), &names, &regexes)?;
            Ok(workspace
                .into_iter()
                .filter(|(name, _)| filter_names.is_empty() || filter_names.contains(name))
                .collect())
        }
    }
}

fn clear_global_state_vm(
    frame: &mut VmFrame,
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    names: Option<&BTreeSet<String>>,
) {
    match names {
        Some(names) => {
            let filtered = names
                .iter()
                .filter(|name| frame.global_names.contains(*name))
                .cloned()
                .collect::<BTreeSet<_>>();
            frame.clear_workspace_names(&filtered);
            let mut state = shared_state.borrow_mut();
            for name in filtered {
                state.globals.remove(&name);
            }
        }
        None => {
            frame.clear_global_names();
            shared_state.borrow_mut().globals.clear();
        }
    }
}

fn clear_persistent_state_vm(frame: &mut VmFrame, shared_state: &Rc<RefCell<SharedRuntimeState>>) {
    let names = frame.persistent_names.clone();
    frame.clear_workspace_names(&names);
    shared_state.borrow_mut().persistents.clear();
}

fn parse_binding_specs(values: &[String]) -> Result<Vec<BindingSpec>, RuntimeError> {
    values
        .iter()
        .map(|value| parse_binding_spec(value))
        .collect()
}

fn binding_spec_for_name(
    function: &BytecodeFunction,
    name: &str,
) -> Result<Option<BindingSpec>, RuntimeError> {
    let mut candidates = function
        .params
        .iter()
        .chain(function.outputs.iter())
        .chain(function.captures.iter())
        .filter_map(|value| parse_binding_spec(value).ok())
        .filter(|binding| binding.name == name)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(None);
    }
    candidates.sort_by_key(|binding| binding.binding_id.map(|id| id.0).unwrap_or(0));
    Ok(candidates.into_iter().next())
}

fn parse_binding_spec(value: &str) -> Result<BindingSpec, RuntimeError> {
    if let Some((name, id)) = value.rsplit_once('#') {
        if let Ok(id) = id.parse::<u32>() {
            return Ok(BindingSpec {
                name: name.to_string(),
                binding_id: Some(BindingId(id)),
            });
        }
    }
    Ok(BindingSpec {
        name: value.to_string(),
        binding_id: None,
    })
}

fn parse_capture_specs(values: &[String]) -> Result<Vec<CaptureSpec>, RuntimeError> {
    values
        .iter()
        .map(|value| {
            let (name, remainder) = value.split_once(" binding=").ok_or_else(|| {
                RuntimeError::Unsupported(format!("invalid capture descriptor `{value}`"))
            })?;
            let (binding_id, _) = remainder.split_once(' ').unwrap_or((remainder, ""));
            let binding_id = binding_id.parse::<u32>().map_err(|error| {
                RuntimeError::Unsupported(format!(
                    "invalid capture binding id in `{value}`: {error}"
                ))
            })?;
            Ok(CaptureSpec {
                name: name.to_string(),
                binding_id: BindingId(binding_id),
            })
        })
        .collect()
}

fn parse_const_value(value: &str) -> Result<Value, RuntimeError> {
    if let Some(number) = value
        .strip_prefix("number(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return super::parse_numeric_literal(number);
    }

    if let Some(text) = value
        .strip_prefix("char(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return super::decode_text_literal(text, '\'').map(Value::CharArray);
    }

    if let Some(text) = value
        .strip_prefix("string(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return super::decode_text_literal(text, '"').map(Value::String);
    }

    if let Some(flag) = value
        .strip_prefix("logical(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return match flag {
            "true" => Ok(Value::Logical(true)),
            "false" => Ok(Value::Logical(false)),
            other => Err(RuntimeError::TypeError(format!(
                "failed to parse logical bytecode constant `{other}`"
            ))),
        };
    }

    Err(RuntimeError::Unsupported(format!(
        "bytecode constant `{value}` is not executable yet"
    )))
}

fn apply_unary(op: &str, rhs: &Value) -> Result<Value, RuntimeError> {
    match op {
        "Plus" => map_numeric_unary(rhs, |value| value),
        "Minus" => map_numeric_unary(rhs, |value| NumericComplexParts {
            real: -value.real,
            imag: -value.imag,
        }),
        "LogicalNot" => map_numeric_unary_logical(rhs, |value| value == 0.0),
        "DotTranspose" => transpose_value(rhs, false),
        "Transpose" => transpose_value(rhs, true),
        _ => Err(RuntimeError::Unsupported(format!(
            "bytecode unary operator `{op}` is not implemented"
        ))),
    }
}

fn apply_binary(op: &str, lhs: &Value, rhs: &Value) -> Result<Value, RuntimeError> {
    match op {
        "Add" => map_numeric_binary(lhs, rhs, |lhs, rhs| lhs.plus(rhs)),
        "Subtract" => map_numeric_binary(lhs, rhs, |lhs, rhs| lhs.minus(rhs)),
        "Multiply" => matrix_multiply(lhs, rhs),
        "ElementwiseMultiply" => map_numeric_binary(lhs, rhs, |lhs, rhs| lhs.times(rhs)),
        "MatrixRightDivide" => matrix_right_divide(lhs, rhs),
        "ElementwiseRightDivide" => map_numeric_binary(lhs, rhs, |lhs, rhs| lhs.rdivide(rhs)),
        "MatrixLeftDivide" => matrix_left_divide(lhs, rhs),
        "ElementwiseLeftDivide" => map_numeric_binary(lhs, rhs, |lhs, rhs| rhs.rdivide(lhs)),
        "Power" => matrix_power(lhs, rhs),
        "ElementwisePower" => map_numeric_binary(lhs, rhs, |lhs, rhs| {
            normalize_numeric_complex_parts(lhs.pow(rhs))
        }),
        "GreaterThan" => map_numeric_binary_logical(lhs, rhs, |lhs, rhs| lhs > rhs),
        "GreaterThanOrEqual" => map_numeric_binary_logical(lhs, rhs, |lhs, rhs| lhs >= rhs),
        "LessThan" => map_numeric_binary_logical(lhs, rhs, |lhs, rhs| lhs < rhs),
        "LessThanOrEqual" => map_numeric_binary_logical(lhs, rhs, |lhs, rhs| lhs <= rhs),
        "Equal" => map_numeric_binary_equality(lhs, rhs, |lhs, rhs| lhs.exact_eq(rhs)),
        "NotEqual" => map_numeric_binary_equality(lhs, rhs, |lhs, rhs| lhs.exact_ne(rhs)),
        "LogicalAnd" | "ShortCircuitAnd" => {
            map_numeric_binary_logical(lhs, rhs, |lhs, rhs| lhs != 0.0 && rhs != 0.0)
        }
        "LogicalOr" | "ShortCircuitOr" => {
            map_numeric_binary_logical(lhs, rhs, |lhs, rhs| lhs != 0.0 || rhs != 0.0)
        }
        _ => Err(RuntimeError::Unsupported(format!(
            "bytecode binary operator `{op}` is not implemented"
        ))),
    }
}

fn evaluate_function_arguments_from_strings(
    temps: &[Option<VmTemp>],
    args: &[String],
) -> Result<Vec<Value>, RuntimeError> {
    let mut values = Vec::new();
    for arg in args {
        if is_encoded_index_expression(arg) || matches!(arg.as_str(), ":" | "end") {
            return Err(RuntimeError::Unsupported(
                "`end` and slice selectors are not implemented for function-call arguments in the current bytecode VM"
                    .to_string(),
            ));
        }
        let temp_id = parse_temp_ref(arg).ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "function-call bytecode argument `{arg}` is not supported"
            ))
        })?;
        let temp = temp(temps, temp_id)?;
        if let Some(spread) = &temp.spread {
            values.extend(spread.clone());
        } else {
            values.push(temp.value.clone());
        }
    }
    Ok(values)
}

fn evaluate_index_function_arguments_from_strings(
    temps: &[Option<VmTemp>],
    target: &Value,
    args: &[String],
    position: usize,
    total_arguments: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let mut values = Vec::new();
    for arg in args {
        if arg == ":" {
            return Err(RuntimeError::Unsupported(
                "slice-style indexing arguments are not implemented in the current bytecode VM"
                    .to_string(),
            ));
        }
        if arg == "end" {
            values.push(Value::Scalar(
                end_index_extent(target, position, total_arguments)? as f64,
            ));
            continue;
        }
        if let Some(temp_id) = parse_temp_ref(arg) {
            let temp = temp(temps, temp_id)?;
            if let Some(spread) = &temp.spread {
                values.extend(spread.clone());
            } else {
                values.push(temp.value.clone());
            }
            continue;
        }
        values.push(evaluate_index_expression_text(
            temps,
            target,
            arg,
            position,
            total_arguments,
        )?);
    }
    Ok(values)
}

fn literal_rows_from_temp_sources(
    temps: &[Option<VmTemp>],
    row_item_counts: &[usize],
    elements: &[String],
) -> Result<Vec<Vec<Value>>, RuntimeError> {
    if row_item_counts.iter().sum::<usize>() != elements.len() {
        return Err(RuntimeError::Unsupported(format!(
            "literal row item counts sum to {}, but {} source temp(s) were provided",
            row_item_counts.iter().sum::<usize>(),
            elements.len()
        )));
    }

    let mut cursor = 0usize;
    let mut rows = Vec::with_capacity(row_item_counts.len());
    for row_item_count in row_item_counts {
        let mut row = Vec::new();
        for element in &elements[cursor..cursor + row_item_count] {
            let temp_id = parse_temp_ref(element).ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "literal bytecode source `{element}` is not supported"
                ))
            })?;
            let temp = temp(temps, temp_id)?;
            if let Some(spread) = &temp.spread {
                row.extend(spread.clone());
            } else {
                row.push(temp.value.clone());
            }
        }
        rows.push(row);
        cursor += row_item_count;
    }
    Ok(rows)
}

fn evaluate_index_arguments_from_strings(
    temps: &[Option<VmTemp>],
    target: &Value,
    args: &[String],
) -> Result<Vec<EvaluatedIndexArgument>, RuntimeError> {
    args.iter()
        .enumerate()
        .map(|(position, arg)| {
            if arg == ":" {
                return Ok(EvaluatedIndexArgument::FullSlice);
            }
            if arg == "end" {
                return Ok(EvaluatedIndexArgument::Numeric {
                    values: vec![end_index_extent(target, position, args.len())? as f64],
                    rows: 1,
                    cols: 1,
                    dims: vec![1, 1],
                    logical: false,
                });
            }
            if is_encoded_index_expression(arg) {
                return evaluated_index_argument(evaluate_encoded_index_expression(
                    temps,
                    target,
                    arg,
                    position,
                    args.len(),
                )?);
            }
            let temp = parse_temp_ref(arg).ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "index bytecode argument `{arg}` is not supported"
                ))
            })?;
            evaluated_index_argument(temp_value(temps, temp)?)
        })
        .collect()
}

fn is_encoded_index_expression(value: &str) -> bool {
    value.starts_with("idx:")
}

fn evaluate_encoded_index_expression(
    temps: &[Option<VmTemp>],
    target: &Value,
    text: &str,
    position: usize,
    total_arguments: usize,
) -> Result<Value, RuntimeError> {
    let expression = text.strip_prefix("idx:").ok_or_else(|| {
        RuntimeError::Unsupported(format!("index bytecode argument `{text}` is not supported"))
    })?;
    evaluate_index_expression_text(temps, target, expression, position, total_arguments)
}

fn evaluate_index_expression_text(
    temps: &[Option<VmTemp>],
    target: &Value,
    expression: &str,
    position: usize,
    total_arguments: usize,
) -> Result<Value, RuntimeError> {
    if expression == "end" {
        return Ok(Value::Scalar(
            end_index_extent(target, position, total_arguments)? as f64,
        ));
    }
    if let Some(temp) = parse_temp_ref(expression) {
        return temp_value(temps, temp);
    }
    if let Some(inner) = expression
        .strip_prefix("unary(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_index_expression_args(inner)?;
        if parts.len() != 2 {
            return Err(RuntimeError::Unsupported(format!(
                "encoded bytecode index expression `{expression}` is malformed"
            )));
        }
        let rhs =
            evaluate_index_expression_text(temps, target, &parts[1], position, total_arguments)?;
        return apply_unary(&parts[0], &rhs);
    }
    if let Some(inner) = expression
        .strip_prefix("binary(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_index_expression_args(inner)?;
        if parts.len() != 3 {
            return Err(RuntimeError::Unsupported(format!(
                "encoded bytecode index expression `{expression}` is malformed"
            )));
        }
        let lhs =
            evaluate_index_expression_text(temps, target, &parts[1], position, total_arguments)?;
        let rhs =
            evaluate_index_expression_text(temps, target, &parts[2], position, total_arguments)?;
        return apply_binary(&parts[0], &lhs, &rhs);
    }
    if let Some(inner) = expression
        .strip_prefix("range(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_index_expression_args(inner)?;
        if parts.len() != 2 {
            return Err(RuntimeError::Unsupported(format!(
                "encoded bytecode index expression `{expression}` is malformed"
            )));
        }
        let start =
            evaluate_index_expression_text(temps, target, &parts[0], position, total_arguments)?
                .as_scalar()?;
        let end =
            evaluate_index_expression_text(temps, target, &parts[1], position, total_arguments)?
                .as_scalar()?;
        return range_value(start, None, end);
    }
    if let Some(inner) = expression
        .strip_prefix("range3(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_index_expression_args(inner)?;
        if parts.len() != 3 {
            return Err(RuntimeError::Unsupported(format!(
                "encoded bytecode index expression `{expression}` is malformed"
            )));
        }
        let start =
            evaluate_index_expression_text(temps, target, &parts[0], position, total_arguments)?
                .as_scalar()?;
        let step =
            evaluate_index_expression_text(temps, target, &parts[1], position, total_arguments)?
                .as_scalar()?;
        let end =
            evaluate_index_expression_text(temps, target, &parts[2], position, total_arguments)?
                .as_scalar()?;
        return range_value(start, Some(step), end);
    }
    if let Some(inner) = expression
        .strip_prefix("matrix(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let rows = index_literal_rows(temps, target, inner, position, total_arguments, "matrix")?;
        return Ok(Value::Matrix(MatrixValue::from_rows(rows)?));
    }
    if let Some(inner) = expression
        .strip_prefix("cell(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let rows = index_literal_rows(temps, target, inner, position, total_arguments, "cell")?;
        return Ok(Value::Cell(CellValue::from_rows(rows)?));
    }
    if let Some(inner) = expression
        .strip_prefix("call(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_index_expression_args(inner)?;
        let Some((target_spec, arg_specs)) = parts.split_first() else {
            return Err(RuntimeError::Unsupported(format!(
                "encoded bytecode index call `{expression}` is malformed"
            )));
        };
        if let Some(temp_id) = parse_temp_ref(target_spec) {
            let target_temp = temp(temps, temp_id)?;
            if target_temp.spread.is_some() || target_temp.bound_method.is_some() {
                return Err(RuntimeError::Unsupported(
                    "encoded bytecode index calls currently expect a single concrete call target"
                        .to_string(),
                ));
            }
            if let Value::FunctionHandle(handle) = &target_temp.value {
                let args = evaluate_index_function_arguments_from_strings(
                    temps,
                    target,
                    arg_specs,
                    position,
                    total_arguments,
                )?;
                return match &handle.target {
                    FunctionHandleTarget::Named(name) => Ok(first_output_or_unit(
                        invoke_stdlib_builtin_outputs(name, &args, 1)?,
                    )),
                    _ => Err(RuntimeError::Unsupported(
                        "encoded bytecode index calls currently support only named builtin function-handle targets"
                            .to_string(),
                    )),
                };
            }
            let evaluated_args =
                evaluate_index_arguments_from_strings(temps, &target_temp.value, arg_specs)?;
            return evaluate_expression_call(&target_temp.value, &evaluated_args);
        }
        let args = evaluate_index_function_arguments_from_strings(
            temps,
            target,
            arg_specs,
            position,
            total_arguments,
        )?;
        let parsed = parse_target(target_spec);
        return Ok(first_output_or_unit(invoke_stdlib_builtin_outputs(
            &parsed.display_name,
            &args,
            1,
        )?));
    }
    if let Some(inner) = expression
        .strip_prefix("cellindex(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_index_expression_args(inner)?;
        let Some((target_spec, arg_specs)) = parts.split_first() else {
            return Err(RuntimeError::Unsupported(format!(
                "encoded bytecode cell-index expression `{expression}` is malformed"
            )));
        };
        let target_value = evaluate_index_expression_target_text(
            temps,
            target,
            target_spec,
            position,
            total_arguments,
        )?;
        let args = evaluate_index_arguments_from_strings(temps, &target_value, arg_specs)?;
        return evaluate_cell_content_index(&target_value, &args);
    }
    if let Some(inner) = expression
        .strip_prefix("field(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_index_expression_args(inner)?;
        if parts.len() != 2 {
            return Err(RuntimeError::Unsupported(format!(
                "encoded bytecode field expression `{expression}` is malformed"
            )));
        }
        let target_value = evaluate_index_expression_target_text(
            temps,
            target,
            &parts[0],
            position,
            total_arguments,
        )?;
        return read_field_value(&target_value, &parts[1]);
    }
    Err(RuntimeError::Unsupported(format!(
        "encoded bytecode index expression `{expression}` is not supported"
    )))
}

fn evaluate_index_expression_target_text(
    temps: &[Option<VmTemp>],
    target: &Value,
    expression: &str,
    position: usize,
    total_arguments: usize,
) -> Result<Value, RuntimeError> {
    if let Some(temp_id) = parse_temp_ref(expression) {
        return temp_value(temps, temp_id);
    }
    evaluate_index_expression_text(temps, target, expression, position, total_arguments)
}

fn index_literal_rows(
    temps: &[Option<VmTemp>],
    target: &Value,
    text: &str,
    position: usize,
    total_arguments: usize,
    kind: &str,
) -> Result<Vec<Vec<Value>>, RuntimeError> {
    let parts = split_index_expression_args(text)?;
    let Some((row_spec, values)) = parts.split_first() else {
        return Err(RuntimeError::Unsupported(format!(
            "encoded bytecode index {kind} literal `{text}` is malformed"
        )));
    };
    let row_counts = parse_index_literal_row_counts(row_spec)?;
    let expected = row_counts.iter().sum::<usize>();
    if expected != values.len() {
        return Err(RuntimeError::Unsupported(format!(
            "encoded bytecode index {kind} literal `{text}` has mismatched row metadata"
        )));
    }

    let mut rows = Vec::with_capacity(row_counts.len());
    let mut cursor = 0usize;
    for count in row_counts {
        let mut row = Vec::with_capacity(count);
        for _ in 0..count {
            row.push(evaluate_index_expression_text(
                temps,
                target,
                &values[cursor],
                position,
                total_arguments,
            )?);
            cursor += 1;
        }
        rows.push(row);
    }
    Ok(rows)
}

fn parse_index_literal_row_counts(text: &str) -> Result<Vec<usize>, RuntimeError> {
    let inner = text
        .strip_prefix("rows(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "encoded bytecode index literal row spec `{text}` is malformed"
            ))
        })?;
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                RuntimeError::Unsupported(format!(
                    "encoded bytecode index literal row count `{value}` is malformed"
                ))
            })
        })
        .collect()
}

fn split_index_expression_args(text: &str) -> Result<Vec<String>, RuntimeError> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (index, ch) in text.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(text[start..index].to_string());
                start = index + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return Err(RuntimeError::Unsupported(format!(
                "encoded bytecode index expression `{text}` is malformed"
            )));
        }
    }
    parts.push(text[start..].to_string());
    Ok(parts)
}

fn range_value(start: f64, step: Option<f64>, end: f64) -> Result<Value, RuntimeError> {
    let step = step.unwrap_or(1.0);
    if step == 0.0 {
        return Err(RuntimeError::InvalidIndex(
            "range step cannot be zero".to_string(),
        ));
    }
    let mut values = Vec::new();
    let mut current = start;
    if step > 0.0 {
        while current <= end {
            values.push(Value::Scalar(current));
            current += step;
        }
    } else {
        while current >= end {
            values.push(Value::Scalar(current));
            current += step;
        }
    }
    Ok(Value::Matrix(MatrixValue::new(1, values.len(), values)?))
}

fn lvalue_root_and_projections(
    lvalue: &Option<TempLValue>,
) -> Result<(BindingSpec, Vec<TempLValueProjection>), RuntimeError> {
    match lvalue {
        Some(TempLValue::Path { root, projections }) => Ok((root.clone(), projections.clone())),
        None => Err(RuntimeError::Unsupported(
            "bytecode store target does not preserve assignable lvalue identity".to_string(),
        )),
    }
}

fn append_temp_lvalue_projection(
    lvalue: Option<TempLValue>,
    projection: TempLValueProjection,
) -> Option<TempLValue> {
    match lvalue {
        Some(TempLValue::Path {
            root,
            mut projections,
        }) => {
            projections.push(projection);
            Some(TempLValue::Path { root, projections })
        }
        None => None,
    }
}

fn default_temp_lvalue_root_value(
    projections: &[TempLValueProjection],
    leaf: &TempLValueLeaf,
) -> Option<Value> {
    match projections.first() {
        Some(TempLValueProjection::Field(_)) => Some(Value::Struct(StructValue::default())),
        Some(TempLValueProjection::Paren(_)) => Some(empty_matrix_value()),
        Some(TempLValueProjection::Brace(_)) => Some(empty_cell_value()),
        None => match leaf {
            TempLValueLeaf::Field { .. } => Some(Value::Struct(StructValue::default())),
            TempLValueLeaf::Index {
                kind: IndexAssignmentKind::Paren,
                ..
            } => Some(empty_matrix_value()),
            TempLValueLeaf::Index {
                kind: IndexAssignmentKind::Brace,
                ..
            } => Some(empty_cell_value()),
        },
    }
}

fn default_temp_struct_csl_root_value(
    projections: &[TempLValueProjection],
    field: &str,
    source: &VmTemp,
) -> Result<Option<Value>, RuntimeError> {
    if projections
        .iter()
        .any(|projection| !matches!(projection, TempLValueProjection::Field(_)))
    {
        return Ok(None);
    }

    let Value::Matrix(matrix) = &source.value else {
        return Ok(None);
    };
    if matrix.rows != 1 {
        return Ok(None);
    }

    let nested = if matrix.cols <= 1 {
        assign_struct_path(
            Value::Struct(StructValue::default()),
            &[field.to_string()],
            matrix
                .elements()
                .first()
                .cloned()
                .unwrap_or_else(empty_matrix_value),
        )?
    } else {
        assign_struct_path(
            Value::Matrix(MatrixValue::with_dimensions(
                1,
                matrix.cols,
                vec![1, matrix.cols],
                vec![Value::Struct(StructValue::default()); matrix.cols],
            )?),
            &[field.to_string()],
            source.value.clone(),
        )?
    };
    if projections.is_empty() {
        Ok(Some(nested))
    } else {
        Ok(Some(assign_struct_path(
            Value::Struct(StructValue::default()),
            &projections
                .iter()
                .map(|projection| match projection {
                    TempLValueProjection::Field(field) => field.clone(),
                    _ => unreachable!("guard restricted to field projections"),
                })
                .collect::<Vec<_>>(),
            nested,
        )?))
    }
}

fn default_temp_struct_direct_root_value(
    projections: &[TempLValueProjection],
    field: &str,
    source: &VmTemp,
) -> Result<Option<Value>, RuntimeError> {
    let count = match &source.value {
        Value::Matrix(matrix) if matrix.element_count() > 1 => matrix.element_count(),
        Value::Cell(cell) if cell.element_count() > 1 => cell.element_count(),
        _ => return Ok(None),
    };
    if projections
        .iter()
        .any(|projection| !matches!(projection, TempLValueProjection::Field(_)))
    {
        return Ok(None);
    }

    let nested = assign_struct_path(
        empty_struct_assignment_target_row(count)?,
        &[field.to_string()],
        source.value.clone(),
    )?;
    if projections.is_empty() {
        Ok(Some(nested))
    } else {
        Ok(Some(assign_struct_path(
            Value::Struct(StructValue::default()),
            &projections
                .iter()
                .map(|projection| match projection {
                    TempLValueProjection::Field(field) => field.clone(),
                    _ => unreachable!("guard restricted to field projections"),
                })
                .collect::<Vec<_>>(),
            nested,
        )?))
    }
}

fn default_temp_indexed_struct_csl_root_value(
    temps: &[Option<VmTemp>],
    projections: &[TempLValueProjection],
    field: &str,
    source: &VmTemp,
) -> Result<Option<Value>, RuntimeError> {
    let output_count = match &source.value {
        Value::Matrix(matrix) if matrix.element_count() > 0 => matrix.element_count(),
        Value::Cell(cell) if cell.element_count() > 0 => cell.element_count(),
        _ => return Ok(None),
    };
    let mut prefix_fields = Vec::new();
    let mut paren_index = None;
    for (index, projection) in projections.iter().enumerate() {
        match projection {
            TempLValueProjection::Field(field) if paren_index.is_none() => {
                prefix_fields.push(field.clone());
            }
            TempLValueProjection::Paren(_) if paren_index.is_none() => paren_index = Some(index),
            _ => {}
        }
    }
    let Some(paren_index) = paren_index else {
        return Ok(None);
    };
    let TempLValueProjection::Paren(receiver_args) = &projections[paren_index] else {
        unreachable!("paren index identified a paren projection");
    };
    let field_projections = &projections[paren_index + 1..];
    if field_projections
        .iter()
        .any(|projection| !matches!(projection, TempLValueProjection::Field(_)))
        || projections.iter().any(|projection| {
            !matches!(projection, TempLValueProjection::Field(_) | TempLValueProjection::Paren(_))
        })
    {
        return Ok(None);
    }

    let Some(receivers) = materialize_missing_root_indexed_struct_receivers(
        temps,
        receiver_args,
        output_count,
    )? else {
        return Ok(None);
    };
    let receiver_count = match &receivers {
        Value::Struct(_) => 1,
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => matrix.elements.len(),
        _ => return Ok(None),
    };
    if receiver_count != output_count {
        return Ok(None);
    }

    let mut field_path = field_projections
        .iter()
        .map(|projection| match projection {
            TempLValueProjection::Field(field) => field.clone(),
            _ => unreachable!("guard restricted to field projections"),
        })
        .collect::<Vec<_>>();
    field_path.push(field.to_string());
    let mut updated = assign_struct_path(receivers, &field_path, source.value.clone())?;
    if !prefix_fields.is_empty() {
        updated =
            assign_struct_path(Value::Struct(StructValue::default()), &prefix_fields, updated)?;
    }
    Ok(Some(updated))
}

fn default_temp_indexed_struct_direct_root_value(
    temps: &[Option<VmTemp>],
    projections: &[TempLValueProjection],
    field: &str,
    source: &VmTemp,
) -> Result<Option<Value>, RuntimeError> {
    let count = match &source.value {
        Value::Matrix(matrix) if matrix.element_count() > 1 => matrix.element_count(),
        Value::Cell(cell) if cell.element_count() > 1 => cell.element_count(),
        _ => return Ok(None),
    };

    let mut prefix_fields = Vec::new();
    let mut paren_index = None;
    for (index, projection) in projections.iter().enumerate() {
        match projection {
            TempLValueProjection::Field(field) if paren_index.is_none() => {
                prefix_fields.push(field.clone());
            }
            TempLValueProjection::Paren(_) if paren_index.is_none() => paren_index = Some(index),
            _ => {}
        }
    }
    let Some(paren_index) = paren_index else {
        return Ok(None);
    };
    let TempLValueProjection::Paren(receiver_args) = &projections[paren_index] else {
        unreachable!("paren index identified a paren projection");
    };
    let field_projections = &projections[paren_index + 1..];
    if field_projections
        .iter()
        .any(|projection| !matches!(projection, TempLValueProjection::Field(_)))
        || projections.iter().any(|projection| {
            !matches!(projection, TempLValueProjection::Field(_) | TempLValueProjection::Paren(_))
        })
    {
        return Ok(None);
    }

    let Some(receivers) = materialize_missing_root_indexed_struct_receivers(
        temps,
        receiver_args,
        count,
    )? else {
        return Ok(None);
    };
    let receiver_count = match &receivers {
        Value::Struct(_) => 1,
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => matrix.elements.len(),
        _ => return Ok(None),
    };
    if receiver_count != count {
        return Ok(None);
    }

    let mut field_path = field_projections
        .iter()
        .map(|projection| match projection {
            TempLValueProjection::Field(field) => field.clone(),
            _ => unreachable!("guard restricted to field projections"),
        })
        .collect::<Vec<_>>();
    field_path.push(field.to_string());
    let mut updated = assign_struct_path(receivers, &field_path, source.value.clone())?;
    if !prefix_fields.is_empty() {
        updated =
            assign_struct_path(Value::Struct(StructValue::default()), &prefix_fields, updated)?;
    }
    Ok(Some(updated))
}

fn materialize_missing_root_indexed_struct_receivers(
    temps: &[Option<VmTemp>],
    receiver_args: &[String],
    output_count: usize,
) -> Result<Option<Value>, RuntimeError> {
    if output_count == 0 {
        return Ok(None);
    }

    let receiver_target = empty_matrix_value();
    let Value::Matrix(empty_receiver) = &receiver_target else {
        unreachable!("empty matrix helper should return a matrix value");
    };
    let receiver_indices =
        evaluate_index_arguments_from_strings(temps, &receiver_target, receiver_args)?;
    if let Some(receivers) =
        default_struct_selection_value_for_index_update(&receiver_target, &receiver_indices)?
    {
        let receiver_count = match &receivers {
            Value::Struct(_) => 1,
            Value::Matrix(matrix) if matrix_is_struct_array(matrix) => matrix.elements.len(),
            _ => 0,
        };
        if receiver_count > 0 {
            return Ok(Some(receivers));
        }
    }

    if receiver_args.iter().any(|argument| argument.contains("end")) {
        return Ok(None);
    }

    let full_slice_axes = receiver_args
        .iter()
        .enumerate()
        .filter_map(|(axis, argument)| (argument == ":").then_some(axis))
        .collect::<Vec<_>>();
    if full_slice_axes.len() != 1 {
        return Ok(None);
    }

    let effective_dims = indexing_dimensions_from_dims(empty_receiver.dims(), receiver_args.len());
    let mut target_dims = effective_dims.clone();
    let mut known_selection_product = 1usize;
    for (axis, argument) in receiver_indices.iter().enumerate() {
        if matches!(argument, EvaluatedIndexArgument::FullSlice) {
            continue;
        }
        let label = format!("dimension {}", axis + 1);
        let (selected, target_extent) =
            assignment_dimension_indices(argument, effective_dims[axis], &label, "struct array")?;
        if selected.is_empty() {
            return Ok(None);
        }
        known_selection_product *= selected.len();
        target_dims[axis] = target_extent;
    }

    if output_count % known_selection_product != 0 {
        return Ok(None);
    }
    let unknown_selection_count = output_count / known_selection_product;
    if receiver_args.len() == 1 {
        target_dims = vec![1, unknown_selection_count];
    } else {
        target_dims[full_slice_axes[0]] = unknown_selection_count;
    }

    let receiver_count = target_dims.iter().product::<usize>();
    if receiver_count == 0 {
        return Ok(None);
    }
    if receiver_count == 1 {
        return Ok(Some(Value::Struct(StructValue::default())));
    }

    let (rows, cols) = storage_shape_from_dimensions(&target_dims);
    Ok(Some(Value::Matrix(MatrixValue::with_dimensions(
        rows,
        cols,
        target_dims,
        vec![Value::Struct(StructValue::default()); receiver_count],
    )?)))
}

fn default_nested_temp_lvalue_value(
    projections: &[TempLValueProjection],
    leaf: &TempLValueLeaf,
) -> Option<Value> {
    match projections.first() {
        Some(TempLValueProjection::Field(_)) => Some(Value::Struct(StructValue::default())),
        Some(TempLValueProjection::Paren(_)) => Some(empty_matrix_value()),
        Some(TempLValueProjection::Brace(_)) => Some(empty_cell_value()),
        None => match leaf {
            TempLValueLeaf::Field { .. } => Some(Value::Struct(StructValue::default())),
            TempLValueLeaf::Index {
                kind: IndexAssignmentKind::Paren,
                ..
            } => Some(empty_matrix_value()),
            TempLValueLeaf::Index {
                kind: IndexAssignmentKind::Brace,
                ..
            } => Some(empty_cell_value()),
        },
    }
}

fn temp_field_target_requires_list_assignment(projections: &[TempLValueProjection]) -> bool {
    projections.iter().any(|projection| {
        matches!(
            projection,
            TempLValueProjection::Paren(_) | TempLValueProjection::Brace(_)
        )
    })
}

fn temp_field_subindexing_requires_single_struct(rest: &[TempLValueProjection]) -> bool {
    rest.iter()
        .any(|projection| matches!(projection, TempLValueProjection::Paren(_)))
}

fn simple_temp_struct_field_assignment_target_count(
    frame: &VmFrame,
    target: &VmTemp,
    temps: &[Option<VmTemp>],
) -> Result<Option<usize>, RuntimeError> {
    if let Some(count) = nested_struct_assignment_target_count(&target.value).filter(|&count| count > 1)
    {
        return Ok(Some(count));
    }

    let Ok((root, projections)) = lvalue_root_and_projections(&target.lvalue) else {
        return Ok(None);
    };
    let Some(cell) = frame.cell_for_reference(root.binding_id, &root.name) else {
        return Ok(None);
    };
    if cell.borrow().is_some() || projections.is_empty() {
        return Ok(None);
    }

    let mut current = empty_matrix_value();
    for projection in &projections {
        match projection {
            TempLValueProjection::Paren(args) => {
                let evaluated = evaluate_index_arguments_from_strings(temps, &current, args)?;
                let Some(selected) =
                    default_struct_selection_value_for_index_update(&current, &evaluated)?
                else {
                    return Ok(None);
                };
                current = selected;
            }
            TempLValueProjection::Field(field) => {
                let leaf = TempLValueLeaf::Field {
                    field: field.clone(),
                    value: empty_matrix_value(),
                };
                let Some(selected) = default_field_projection_value_for_temp(&current, &[], &leaf)
                else {
                    return Ok(None);
                };
                current = selected;
            }
            TempLValueProjection::Brace(_) => return Ok(None),
        }
    }

    Ok(nested_struct_assignment_target_count(&current).filter(|&count| count > 1))
}

fn temp_brace_args_contain_full_slice(args: &[String]) -> bool {
    args.iter().any(|argument| argument == ":")
}

fn undefined_temp_brace_assignment_with_colon(
    projections: &[TempLValueProjection],
    leaf: &TempLValueLeaf,
) -> bool {
    projections.iter().any(|projection| match projection {
        TempLValueProjection::Brace(args) => temp_brace_args_contain_full_slice(args),
        _ => false,
    }) || matches!(
        leaf,
        TempLValueLeaf::Index {
            kind: IndexAssignmentKind::Brace,
            args,
            ..
        } if temp_brace_args_contain_full_slice(args)
    )
}

fn infer_missing_root_receiver_count_from_rhs(args: &[String], value: &Value) -> Option<usize> {
    let full_slice_axes = args
        .iter()
        .enumerate()
        .filter_map(|(axis, argument)| (argument == ":").then_some(axis))
        .collect::<Vec<_>>();
    if full_slice_axes.len() != 1 {
        return None;
    }

    let axis = full_slice_axes[0];
    match value {
        Value::Matrix(matrix) => match axis {
            0 => Some(matrix.rows),
            1 => Some(matrix.cols),
            _ => None,
        },
        Value::Cell(cell) => match axis {
            0 => Some(cell.rows),
            1 => Some(cell.cols),
            _ => None,
        },
        _ => None,
    }
}

fn try_materialize_undefined_root_brace_csl_assignment(
    temps: &[Option<VmTemp>],
    args: &[String],
    value: &Value,
) -> Result<Option<Value>, RuntimeError> {
    let Some(rhs_values) = bytecode_csl_values(value)? else {
        return Ok(None);
    };
    let output_count = rhs_values.len();
    if output_count == 0 {
        return Ok(None);
    }
    if args.iter().any(|argument| argument.contains("end")) {
        return Ok(None);
    }

    let full_slice_axes = args
        .iter()
        .enumerate()
        .filter_map(|(axis, argument)| (argument == ":").then_some(axis))
        .collect::<Vec<_>>();
    if full_slice_axes.len() > 1 {
        return Ok(None);
    }

    let empty = empty_cell_value();
    let Value::Cell(empty_cell) = &empty else {
        unreachable!("empty cell helper should return a cell value");
    };
    let evaluated = evaluate_index_arguments_from_strings(temps, &empty, args)?;
    let target_dims = if full_slice_axes.is_empty() {
        let plan = cell_assignment_plan(empty_cell, &evaluated)?;
        if plan.selection.positions.len() != output_count {
            return Ok(None);
        }
        plan.target_dims
    } else {
        let effective_dims = indexing_dimensions_from_dims(empty_cell.dims(), args.len());
        let mut target_dims = effective_dims.clone();
        let mut known_selection_product = 1usize;
        for (axis, argument) in evaluated.iter().enumerate() {
            if matches!(argument, EvaluatedIndexArgument::FullSlice) {
                continue;
            }
            let label = format!("dimension {}", axis + 1);
            let (selected, target_extent) =
                assignment_dimension_indices(argument, effective_dims[axis], &label, "cell array")?;
            if selected.is_empty() {
                return Ok(None);
            }
            known_selection_product *= selected.len();
            target_dims[axis] = target_extent;
        }

        if output_count % known_selection_product != 0 {
            return Ok(None);
        }
        let unknown_selection_count = output_count / known_selection_product;
        if args.len() == 1 {
            vec![1, unknown_selection_count]
        } else {
            target_dims[full_slice_axes[0]] = unknown_selection_count;
            target_dims
        }
    };

    let element_count = target_dims.iter().product::<usize>();
    let (rows, cols) = storage_shape_from_dimensions(&target_dims);
    let current = CellValue::with_dimensions(
        rows,
        cols,
        target_dims,
        vec![empty_matrix_value(); element_count],
    )?;
    let current_value = Value::Cell(current.clone());
    let evaluated_indices = evaluate_index_arguments_from_strings(temps, &current_value, args)?;
    let selection = cell_selection(&current, &evaluated_indices)?;
    if selection.positions.len() != output_count {
        return Ok(None);
    }
    let rhs = Value::Cell(CellValue::with_dimensions(
        selection.rows,
        selection.cols,
        selection.dims.clone(),
        rhs_values,
    )?);
    Ok(Some(Value::Cell(assign_cell_content_index(
        current,
        &evaluated_indices,
        rhs,
    )?)))
}

fn try_materialize_undefined_struct_field_brace_csl_assignment(
    projections: &[TempLValueProjection],
    args: &[String],
    temps: &[Option<VmTemp>],
    value: &Value,
) -> Result<Option<Value>, RuntimeError> {
    let Some(Value::Cell(materialized)) =
        try_materialize_undefined_root_brace_csl_assignment(temps, args, value)?
    else {
        return Ok(None);
    };
    Ok(Some(assign_struct_path(
        Value::Struct(StructValue::default()),
        &projections
            .iter()
            .map(|projection| match projection {
                TempLValueProjection::Field(field) => field.clone(),
                _ => unreachable!("guard restricted to field projections"),
            })
            .collect::<Vec<_>>(),
        Value::Cell(materialized),
    )?))
}

fn default_temp_cell_struct_csl_root_value(
    temps: &[Option<VmTemp>],
    projections: &[TempLValueProjection],
    field: &str,
    source: &VmTemp,
) -> Result<Option<Value>, RuntimeError> {
    let mut prefix_fields = Vec::new();
    let mut brace_index = None;
    for (index, projection) in projections.iter().enumerate() {
        match projection {
            TempLValueProjection::Field(field) if brace_index.is_none() => {
                prefix_fields.push(field.clone());
            }
            TempLValueProjection::Brace(_) if brace_index.is_none() => brace_index = Some(index),
            _ => {}
        }
    }
    let Some(brace_index) = brace_index else {
        return Ok(None);
    };
    let TempLValueProjection::Brace(args) = &projections[brace_index] else {
        unreachable!("brace index identified a brace projection");
    };
    let field_projections = &projections[brace_index + 1..];
    if field_projections
        .iter()
        .any(|projection| !matches!(projection, TempLValueProjection::Field(_)))
        || projections.iter().any(|projection| {
            !matches!(projection, TempLValueProjection::Field(_) | TempLValueProjection::Brace(_))
        })
    {
        return Ok(None);
    }

    let Value::Matrix(matrix) = &source.value else {
        return Ok(None);
    };
    if matrix.rows != 1 {
        return Ok(None);
    }

    let receiver_defaults = Value::Cell(CellValue::new(
        1,
        matrix.cols,
        vec![Value::Struct(StructValue::default()); matrix.cols],
    )?);
    let Some(Value::Cell(receivers)) =
        try_materialize_undefined_root_brace_csl_assignment(temps, args, &receiver_defaults)?
    else {
        return Ok(None);
    };

    let mut field_path = field_projections
        .iter()
        .map(|projection| match projection {
            TempLValueProjection::Field(field) => field.clone(),
            _ => unreachable!("guard restricted to field projections"),
        })
        .collect::<Vec<_>>();
    field_path.push(field.to_string());
    let mut updated = assign_struct_path(
        Value::Cell(receivers),
        &field_path,
        source.value.clone(),
    )?;
    if !prefix_fields.is_empty() {
        updated = assign_struct_path(Value::Struct(StructValue::default()), &prefix_fields, updated)?;
    }
    Ok(Some(updated))
}

fn default_temp_cell_struct_direct_root_value(
    temps: &[Option<VmTemp>],
    projections: &[TempLValueProjection],
    field: &str,
    source: &VmTemp,
) -> Result<Option<Value>, RuntimeError> {
    let count = match &source.value {
        Value::Matrix(matrix) if matrix.element_count() > 1 => matrix.element_count(),
        Value::Cell(cell) if cell.element_count() > 1 => cell.element_count(),
        _ => return Ok(None),
    };

    let mut prefix_fields = Vec::new();
    let mut brace_index = None;
    for (index, projection) in projections.iter().enumerate() {
        match projection {
            TempLValueProjection::Field(field) if brace_index.is_none() => {
                prefix_fields.push(field.clone());
            }
            TempLValueProjection::Brace(_) if brace_index.is_none() => brace_index = Some(index),
            _ => {}
        }
    }
    let Some(brace_index) = brace_index else {
        return Ok(None);
    };
    let TempLValueProjection::Brace(args) = &projections[brace_index] else {
        unreachable!("brace index identified a brace projection");
    };
    let field_projections = &projections[brace_index + 1..];
    if field_projections
        .iter()
        .any(|projection| !matches!(projection, TempLValueProjection::Field(_)))
        || projections.iter().any(|projection| {
            !matches!(projection, TempLValueProjection::Field(_) | TempLValueProjection::Brace(_))
        })
    {
        return Ok(None);
    }

    let receiver_defaults = Value::Cell(CellValue::new(
        1,
        count,
        vec![Value::Struct(StructValue::default()); count],
    )?);
    let Some(Value::Cell(receivers)) =
        try_materialize_undefined_root_brace_csl_assignment(temps, args, &receiver_defaults)?
    else {
        return Ok(None);
    };

    let mut field_path = field_projections
        .iter()
        .map(|projection| match projection {
            TempLValueProjection::Field(field) => field.clone(),
            _ => unreachable!("guard restricted to field projections"),
        })
        .collect::<Vec<_>>();
    field_path.push(field.to_string());
    let mut updated = assign_struct_path(
        Value::Cell(receivers),
        &field_path,
        source.value.clone(),
    )?;
    if !prefix_fields.is_empty() {
        updated = assign_struct_path(Value::Struct(StructValue::default()), &prefix_fields, updated)?;
    }
    Ok(Some(updated))
}

fn try_materialize_undefined_indexed_struct_field_brace_csl_assignment(
    projections: &[TempLValueProjection],
    args: &[String],
    temps: &[Option<VmTemp>],
    value: &Value,
) -> Result<Option<Value>, RuntimeError> {
    let mut prefix_fields = Vec::new();
    let mut paren_index = None;
    for (index, projection) in projections.iter().enumerate() {
        match projection {
            TempLValueProjection::Field(field) if paren_index.is_none() => {
                prefix_fields.push(field.clone());
            }
            TempLValueProjection::Paren(_) if paren_index.is_none() => paren_index = Some(index),
            _ => {}
        }
    }
    let Some(paren_index) = paren_index else {
        return Ok(None);
    };
    let TempLValueProjection::Paren(receiver_args) = &projections[paren_index] else {
        unreachable!("paren index identified a paren projection");
    };
    let field_projections = &projections[paren_index + 1..];
    if field_projections.is_empty()
        || projections.iter().any(|projection| {
            !matches!(projection, TempLValueProjection::Field(_) | TempLValueProjection::Paren(_))
        })
        || field_projections
            .iter()
            .any(|projection| !matches!(projection, TempLValueProjection::Field(_)))
    {
        return Ok(None);
    }

    let Some(rhs_values) = bytecode_csl_values(value)? else {
        return Ok(None);
    };
    let Some(receivers) = materialize_missing_root_indexed_struct_brace_receivers(
        temps,
        receiver_args,
        args,
        rhs_values.len(),
    )? else {
        return Ok(None);
    };
    let receiver_count = match &receivers {
        Value::Struct(_) => 1,
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => matrix.elements.len(),
        _ => return Ok(None),
    };
    if receiver_count == 0 || rhs_values.len() % receiver_count != 0 {
        return Ok(None);
    }

    let chunk_len = rhs_values.len() / receiver_count;
    let field_path = field_projections
        .iter()
        .map(|projection| match projection {
            TempLValueProjection::Field(field) => field.clone(),
            _ => unreachable!("guard restricted to field projections"),
        })
        .collect::<Vec<_>>();

    let mut chunks = rhs_values.chunks(chunk_len);
    let updated = match receivers {
        Value::Struct(receiver) => {
            let chunk = chunks.next().expect("single receiver chunk").to_vec();
            let rhs = Value::Cell(CellValue::new(1, chunk.len(), chunk)?);
            let Some(Value::Cell(materialized)) =
                try_materialize_undefined_root_brace_csl_assignment(temps, args, &rhs)?
            else {
                return Ok(None);
            };
            Ok(Some(assign_struct_path(
                Value::Struct(receiver),
                &field_path,
                Value::Cell(materialized),
            )?))
        }
        Value::Matrix(matrix) if matrix_is_struct_array(&matrix) => {
            let mut elements = Vec::with_capacity(matrix.elements.len());
            for element in matrix.elements {
                let chunk = chunks.next().expect("chunk per indexed receiver").to_vec();
                let rhs = Value::Cell(CellValue::new(1, chunk.len(), chunk)?);
                let Some(Value::Cell(materialized)) =
                    try_materialize_undefined_root_brace_csl_assignment(temps, args, &rhs)?
                else {
                    return Ok(None);
                };
                elements.push(assign_struct_path(
                    element,
                    &field_path,
                    Value::Cell(materialized),
                )?);
            }
            Ok(Some(Value::Matrix(MatrixValue::with_dimensions(
                matrix.rows,
                matrix.cols,
                matrix.dims,
                elements,
            )?)))
        }
        _ => Ok(None),
    }?;

    let Some(updated) = updated else {
        return Ok(None);
    };
    if prefix_fields.is_empty() {
        Ok(Some(updated))
    } else {
        Ok(Some(assign_struct_path(
            Value::Struct(StructValue::default()),
            &prefix_fields,
            updated,
        )?))
    }
}

fn materialize_missing_root_indexed_struct_brace_receivers(
    temps: &[Option<VmTemp>],
    receiver_args: &[String],
    args: &[String],
    output_count: usize,
) -> Result<Option<Value>, RuntimeError> {
    let receiver_target = empty_matrix_value();
    let receiver_indices =
        evaluate_index_arguments_from_strings(temps, &receiver_target, receiver_args)?;
    if let Some(receivers) =
        default_struct_selection_value_for_index_update(&receiver_target, &receiver_indices)?
    {
        let receiver_count = match &receivers {
            Value::Struct(_) => 1,
            Value::Matrix(matrix) if matrix_is_struct_array(matrix) => matrix.elements.len(),
            _ => 0,
        };
        if receiver_count > 0 {
            return Ok(Some(receivers));
        }
    }

    let Some(receiver_count) =
        infer_missing_root_indexed_struct_brace_receiver_count_from_rhs(temps, args, output_count)?
    else {
        return Ok(None);
    };
    materialize_missing_root_indexed_struct_receivers(temps, receiver_args, receiver_count)
}

fn infer_missing_root_indexed_struct_brace_receiver_count_from_rhs(
    temps: &[Option<VmTemp>],
    args: &[String],
    output_count: usize,
) -> Result<Option<usize>, RuntimeError> {
    if output_count == 0 || args.iter().any(|argument| argument.contains("end")) {
        return Ok(None);
    }

    let empty = empty_cell_value();
    let Value::Cell(empty_cell) = &empty else {
        unreachable!("empty cell helper should return a cell value");
    };
    let evaluated = evaluate_index_arguments_from_strings(temps, &empty, args)?;
    if evaluated
        .iter()
        .any(|argument| matches!(argument, EvaluatedIndexArgument::FullSlice))
    {
        return Ok(None);
    }

    let plan = cell_assignment_plan(empty_cell, &evaluated)?;
    let per_receiver_count = plan.selection.positions.len();
    if per_receiver_count == 0 || output_count % per_receiver_count != 0 {
        return Ok(None);
    }
    Ok(Some(output_count / per_receiver_count))
}

fn try_materialize_undefined_cell_receiver_struct_field_brace_csl_assignment(
    projections: &[TempLValueProjection],
    args: &[String],
    temps: &[Option<VmTemp>],
    value: &Value,
) -> Result<Option<Value>, RuntimeError> {
    let mut prefix_fields = Vec::new();
    let mut brace_index = None;
    for (index, projection) in projections.iter().enumerate() {
        match projection {
            TempLValueProjection::Field(field) if brace_index.is_none() => {
                prefix_fields.push(field.clone());
            }
            TempLValueProjection::Brace(_) if brace_index.is_none() => brace_index = Some(index),
            _ => {}
        }
    }
    let Some(brace_index) = brace_index else {
        return Ok(None);
    };
    let TempLValueProjection::Brace(receiver_args) = &projections[brace_index] else {
        unreachable!("brace index identified a brace projection");
    };
    let field_projections = &projections[brace_index + 1..];
    if field_projections.is_empty()
        || projections.iter().any(|projection| {
            !matches!(projection, TempLValueProjection::Field(_) | TempLValueProjection::Brace(_))
        })
        || field_projections
            .iter()
            .any(|projection| !matches!(projection, TempLValueProjection::Field(_)))
        || receiver_args.iter().any(|argument| argument.contains("end"))
    {
        return Ok(None);
    }

    let empty = empty_cell_value();
    let Value::Cell(empty_cell) = &empty else {
        unreachable!("empty cell helper should return a cell value");
    };
    let evaluated_receiver_indices =
        evaluate_index_arguments_from_strings(temps, &empty, receiver_args)?;
    let receiver_plan = cell_assignment_plan(empty_cell, &evaluated_receiver_indices)?;
    let receiver_count = if receiver_plan.selection.positions.is_empty() {
        infer_missing_root_receiver_count_from_rhs(receiver_args, value).unwrap_or(0)
    } else {
        receiver_plan.selection.positions.len()
    };

    let Some(rhs_values) = bytecode_csl_values(value)? else {
        return Ok(None);
    };
    if receiver_count == 0 || rhs_values.len() % receiver_count != 0 {
        return Ok(None);
    }

    let receiver_defaults = Value::Cell(CellValue::new(
        1,
        receiver_count,
        vec![Value::Struct(StructValue::default()); receiver_count],
    )?);
    let Some(Value::Cell(receivers)) =
        try_materialize_undefined_root_brace_csl_assignment(temps, receiver_args, &receiver_defaults)?
    else {
        return Ok(None);
    };

    let field_path = field_projections
        .iter()
        .map(|projection| match projection {
            TempLValueProjection::Field(field) => field.clone(),
            _ => unreachable!("guard restricted to field projections"),
        })
        .collect::<Vec<_>>();
    let chunk_len = rhs_values.len() / receiver_count;
    let mut chunks = rhs_values.chunks(chunk_len);
    let mut elements = Vec::with_capacity(receivers.elements.len());
    for element in receivers.elements {
        let chunk = chunks.next().expect("chunk per cell receiver").to_vec();
        let rhs = Value::Cell(CellValue::new(1, chunk.len(), chunk)?);
        let Some(Value::Cell(materialized)) =
            try_materialize_undefined_root_brace_csl_assignment(temps, args, &rhs)?
        else {
            return Ok(None);
        };
        elements.push(assign_struct_path(
            element,
            &field_path,
            Value::Cell(materialized),
        )?);
    }

    let mut updated = Value::Cell(CellValue::with_dimensions(
        receivers.rows,
        receivers.cols,
        receivers.dims,
        elements,
    )?);
    if !prefix_fields.is_empty() {
        updated = assign_struct_path(Value::Struct(StructValue::default()), &prefix_fields, updated)?;
    }
    Ok(Some(updated))
}

fn bytecode_csl_values(value: &Value) -> Result<Option<Vec<Value>>, RuntimeError> {
    match value {
        Value::Cell(cell) if cell.element_count() > 1 => Ok(Some(linearized_cell_elements(cell)?)),
        Value::Matrix(matrix) if matrix.element_count() > 1 => {
            Ok(Some(linearized_matrix_elements(matrix)?))
        }
        _ => Ok(None),
    }
}

fn read_field_lvalue_value_for_temp_assignment(
    target: &Value,
    field: &str,
    rest: &[TempLValueProjection],
    leaf: &TempLValueLeaf,
) -> Result<Value, RuntimeError> {
    let fallback = || {
        default_field_projection_value_for_temp(target, rest, leaf).ok_or_else(|| {
            RuntimeError::MissingVariable(format!("struct field `{field}` is not defined"))
        })
    };

    match target {
        Value::Struct(_) => match read_field_value(target, field) {
            Ok(value) => Ok(value),
            Err(RuntimeError::MissingVariable(_)) => fallback(),
            Err(error) => Err(error),
        },
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => {
            if matrix.element_count() > 1 && temp_field_subindexing_requires_single_struct(rest) {
                return Err(unsupported_multi_struct_field_subindexing_error(field));
            }
            let mut elements = Vec::with_capacity(matrix.elements.len());
            let mut all_cells = true;
            for element in &matrix.elements {
                let value = match read_field_value(element, field) {
                    Ok(value) => value,
                    Err(RuntimeError::MissingVariable(_)) => {
                        default_nested_temp_lvalue_value(rest, leaf).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "struct field `{field}` is not defined"
                            ))
                        })?
                    }
                    Err(error) => return Err(error),
                };
                all_cells &= matches!(value, Value::Cell(_));
                elements.push(value);
            }
            if all_cells {
                Ok(Value::Cell(CellValue::with_dimensions(
                    matrix.rows,
                    matrix.cols,
                    matrix.dims.clone(),
                    elements,
                )?))
            } else {
                Ok(Value::Matrix(MatrixValue::with_dimensions(
                    matrix.rows,
                    matrix.cols,
                    matrix.dims.clone(),
                    elements,
                )?))
            }
        }
        Value::Cell(cell) if cell.elements.iter().all(value_is_struct_assignment_target) => {
            let mut elements = Vec::with_capacity(cell.elements.len());
            for element in &cell.elements {
                let value = match read_field_value(element, field) {
                    Ok(value) => value,
                    Err(RuntimeError::MissingVariable(_)) => {
                        default_nested_temp_lvalue_value(rest, leaf).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "struct field `{field}` is not defined"
                            ))
                        })?
                    }
                    Err(error) => return Err(error),
                };
                elements.push(value);
            }
            Ok(Value::Cell(CellValue::with_dimensions(
                cell.rows,
                cell.cols,
                cell.dims.clone(),
                elements,
            )?))
        }
        _ => match read_field_value(target, field) {
            Ok(value) => Ok(value),
            Err(RuntimeError::MissingVariable(_)) => fallback(),
            Err(error) => Err(error),
        },
    }
}

fn default_field_projection_value_for_temp(
    target: &Value,
    rest: &[TempLValueProjection],
    leaf: &TempLValueLeaf,
) -> Option<Value> {
    let element_default = default_nested_temp_lvalue_value(rest, leaf)?;
    match target {
        Value::Struct(_) => Some(element_default),
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => {
            if matches!(element_default, Value::Cell(_)) {
                Some(Value::Cell(
                    CellValue::with_dimensions(
                        matrix.rows,
                        matrix.cols,
                        matrix.dims.clone(),
                        vec![element_default; matrix.elements.len()],
                    )
                    .expect("field default should preserve struct-array dimensions"),
                ))
            } else {
                Some(Value::Matrix(
                    MatrixValue::with_dimensions(
                        matrix.rows,
                        matrix.cols,
                        matrix.dims.clone(),
                        vec![element_default; matrix.elements.len()],
                    )
                    .expect("field default should preserve struct-array dimensions"),
                ))
            }
        }
        Value::Cell(cell) if cell.elements.iter().all(value_is_struct_assignment_target) => {
            Some(Value::Cell(
                CellValue::with_dimensions(
                    cell.rows,
                    cell.cols,
                    cell.dims.clone(),
                    vec![element_default; cell.elements.len()],
                )
                .expect("field default should preserve container dimensions"),
            ))
        }
        _ => Some(element_default),
    }
}

fn wrap_vm_values(values: Vec<Value>) -> Vec<VmTemp> {
    values.into_iter().map(VmTemp::value).collect()
}

fn base_function_name(name: &str) -> String {
    name.split("#s").next().unwrap_or(name).to_string()
}

fn parse_temp_ref(value: &str) -> Option<u32> {
    value.strip_prefix('t')?.parse::<u32>().ok()
}

fn parse_target(value: &str) -> ParsedTarget {
    let display_name = value
        .split(" [semantic=")
        .next()
        .unwrap_or(value)
        .to_string();
    let semantic_resolution = value
        .split("semantic=")
        .nth(1)
        .map(|rest| rest.split([' ', ']']).next().unwrap_or(rest).to_string());
    let resolved_path = parse_resolved_path(value);
    let resolved_class = value.contains("ClassCurrentDirectory")
        || value.contains("ClassSearchPath")
        || value.contains("ClassPackageDirectory")
        || value.contains("ClassFolderCurrentDirectory")
        || value.contains("ClassFolderSearchPath")
        || value.contains("ClassFolderPackageDirectory");
    let bundle_module_id = parse_bundle_module_id(value);
    ParsedTarget {
        display_name,
        semantic_resolution,
        resolved_path,
        resolved_class,
        bundle_module_id,
    }
}

fn parse_bundle_module_id(value: &str) -> Option<String> {
    value
        .split("bundle_id=")
        .nth(1)
        .map(|rest| rest.split([' ', ']']).next().unwrap_or(rest).to_string())
}

fn parse_resolved_path(value: &str) -> Option<PathBuf> {
    let marker = "path: \"";
    let start = value.find(marker)? + marker.len();
    let rest = &value[start..];
    let mut escaped = false;
    let mut out = String::new();
    for ch in rest.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => break,
            _ => out.push(ch),
        }
    }
    Some(PathBuf::from(out))
}

fn temp<'a>(temps: &'a [Option<VmTemp>], temp_id: u32) -> Result<&'a VmTemp, RuntimeError> {
    temps
        .get(temp_id as usize)
        .and_then(|value| value.as_ref())
        .ok_or_else(|| RuntimeError::Unsupported(format!("temp t{temp_id} is not initialized")))
}

fn temp_value(temps: &[Option<VmTemp>], temp_id: u32) -> Result<Value, RuntimeError> {
    let temp = temp(temps, temp_id)?;
    if temp.spread.is_some() {
        return Err(RuntimeError::Unsupported(format!(
            "temp t{temp_id} carries a comma-separated list and cannot be used in scalar context"
        )));
    }
    Ok(temp.value.clone())
}

fn materialize_default_struct_cell_selection(
    target: &Value,
    indices: &[EvaluatedIndexArgument],
) -> Result<Option<Value>, RuntimeError> {
    let Value::Cell(cell) = target else {
        return Ok(None);
    };
    if cell.element_count() != 0 {
        return Ok(None);
    }
    let plan = cell_assignment_plan(cell, indices)?;
    let count = plan.selection.positions.len();
    Ok(Some(Value::Cell(CellValue::with_dimensions(
        plan.selection.rows,
        plan.selection.cols,
        plan.selection.dims,
        vec![Value::Struct(StructValue::default()); count],
    )?)))
}

fn set_temp(temps: &mut [Option<VmTemp>], temp: u32, value: VmTemp) -> Result<(), RuntimeError> {
    let slot = temps
        .get_mut(temp as usize)
        .ok_or_else(|| RuntimeError::Unsupported(format!("temp t{temp} is out of range")))?;
    *slot = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use matlab_codegen::BackendKind;

    #[test]
    fn default_temp_cell_struct_direct_root_value_materializes_missing_root() {
        let temps = vec![
            Some(VmTemp::value(Value::Scalar(1.0))),
            Some(VmTemp::value(Value::Scalar(2.0))),
        ];
        let projections =
            vec![TempLValueProjection::Brace(vec!["idx:range(t0,t1)".to_string()])];
        let source = VmTemp::value(
            Value::Matrix(
                MatrixValue::new(1, 2, vec![Value::Scalar(171.0), Value::Scalar(173.0)])
                    .expect("matrix rhs"),
            ),
        );

        let updated = default_temp_cell_struct_direct_root_value(
            &temps,
            &projections,
            "score",
            &source,
        )
        .expect("direct root synthesis should not error")
        .expect("direct root synthesis should materialize a value");

        assert_eq!(
            render_value(&updated),
            "{struct{score=171}, struct{score=173}}"
        );
    }

    #[test]
    fn default_temp_struct_direct_root_value_materializes_missing_root() {
        let projections = vec![TempLValueProjection::Field("inner".to_string())];
        let source = VmTemp::value(
            Value::Matrix(
                MatrixValue::new(1, 2, vec![Value::Scalar(41.0), Value::Scalar(42.0)])
                    .expect("matrix rhs"),
            ),
        );

        let updated = default_temp_struct_direct_root_value(&projections, "score", &source)
            .expect("direct struct root synthesis should not error")
            .expect("direct struct root synthesis should materialize a value");

        assert_eq!(
            render_value(&updated),
            "struct{inner=[struct{score=41}, struct{score=42}]}"
        );
    }

    #[test]
    fn store_field_direct_root_cell_struct_assignment_materializes_missing_root() {
        let bytecode = BytecodeModule {
            backend: BackendKind::Bytecode,
            unit_kind: "script".to_string(),
            entry: "<script>".to_string(),
            functions: Vec::new(),
        };
        let mut vm = BytecodeVm {
            module_identity: "<root>".to_string(),
            bytecode,
            functions: HashMap::new(),
            visible_functions: BTreeMap::new(),
            bundled_modules: Rc::new(HashMap::new()),
            bundled_modules_by_id: Rc::new(HashMap::new()),
            shared_state: Rc::new(RefCell::new(SharedRuntimeState::default())),
            call_stack: Vec::new(),
            handle_closures: HashMap::new(),
            next_handle_id: 0,
        };
        let mut frame = VmFrame::new(BTreeMap::new());
        let spec = BindingSpec {
            name: "direct_root_cells".to_string(),
            binding_id: Some(BindingId(1)),
        };
        frame
            .declare_binding_spec(&spec)
            .expect("binding declaration should succeed");
        let target = VmTemp::with_lvalue(
            Value::Cell(
                CellValue::new(
                    1,
                    2,
                    vec![
                        Value::Struct(StructValue::default()),
                        Value::Struct(StructValue::default()),
                    ],
                )
                .expect("receiver cell"),
            ),
            Some(TempLValue::Path {
                root: spec.clone(),
                projections: vec![TempLValueProjection::Brace(vec!["idx:range(t0,t1)".to_string()])],
            }),
        );
        let source = VmTemp::value(Value::Matrix(
            MatrixValue::new(1, 2, vec![Value::Scalar(171.0), Value::Scalar(173.0)])
                .expect("matrix rhs"),
        ));
        let temps = vec![
            Some(VmTemp::value(Value::Scalar(1.0))),
            Some(VmTemp::value(Value::Scalar(2.0))),
        ];

        vm.store_field(&frame, &target, "score", &temps, &source, true)
            .expect("store_field should synthesize the missing root");
        let stored = frame
            .read_reference(spec.binding_id, &spec.name)
            .expect("stored value should be visible");
        assert_eq!(
            render_value(&stored),
            "{struct{score=171}, struct{score=173}}"
        );
    }

    #[test]
    fn default_temp_indexed_struct_direct_root_value_materializes_missing_root() {
        let temps = vec![
            Some(VmTemp::value(Value::Scalar(1.0))),
            Some(VmTemp::value(Value::Scalar(2.0))),
        ];
        let projections =
            vec![TempLValueProjection::Paren(vec!["idx:range(t0,t1)".to_string()])];
        let source = VmTemp::value(
            Value::Matrix(
                MatrixValue::new(1, 2, vec![Value::Scalar(141.0), Value::Scalar(143.0)])
                    .expect("matrix rhs"),
            ),
        );

        let updated = default_temp_indexed_struct_direct_root_value(
            &temps,
            &projections,
            "score",
            &source,
        )
        .expect("direct indexed root synthesis should not error")
        .expect("direct indexed root synthesis should materialize a value");

        assert_eq!(render_value(&updated), "[struct{score=141}, struct{score=143}]");
    }

    #[test]
    fn default_temp_indexed_struct_csl_root_value_materializes_missing_colon_root() {
        let projections = vec![TempLValueProjection::Paren(vec![":".to_string()])];
        let source = VmTemp::list_origin_value(
            Value::Matrix(
                MatrixValue::new(1, 2, vec![Value::Scalar(51.0), Value::Scalar(53.0)])
                    .expect("matrix rhs"),
            ),
        );

        let updated =
            default_temp_indexed_struct_csl_root_value(&[], &projections, "score", &source)
                .expect("colon indexed root synthesis should not error")
                .expect("colon indexed root synthesis should materialize a value");

        assert_eq!(render_value(&updated), "[struct{score=51}, struct{score=53}]");
    }
}
