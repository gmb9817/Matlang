//! Execution crate for interpreter, bytecode VM, and runtime orchestration.

mod bytecode;
mod graphics;

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    env, fs,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

pub use bytecode::{
    execute_function_file_bytecode, execute_function_file_bytecode_bundle,
    execute_function_file_bytecode_module, execute_script_bytecode, execute_script_bytecode_bundle,
    execute_script_bytecode_module,
};
use graphics::{
    apply_backend_figure_position, close_figures_now, close_request_handles,
    figure_close_request_callback, figure_resize_callback_snapshot,
    invoke_graphics_builtin_outputs, rendered_figures, select_current_figure_handle, GraphicsState,
};
use matlab_frontend::ast::CompilationUnitKind;
use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_interop::{
    read_mat_file, read_workspace_snapshot, write_mat_file, write_workspace_snapshot,
};
use matlab_ir::{
    lower_to_hir, HirAnonymousFunction, HirAssignmentTarget, HirBinding, HirCallTarget,
    HirCallableRef, HirConditionalBranch, HirExpression, HirFunction, HirIndexArgument, HirItem,
    HirModule, HirStatement, HirSwitchCase,
};
use matlab_resolver::ResolverContext;
use matlab_runtime::{
    render_named_value, render_value, render_workspace, ArrayStorageClass, CellValue,
    ComplexValue, FunctionHandleTarget, FunctionHandleValue, MatrixValue, ObjectClassMetadata,
    ObjectStorageKind, ObjectValue, RuntimeError, RuntimeStackFrame, StructValue, Value,
    Workspace,
};
use matlab_semantics::{
    analyze_compilation_unit_with_context,
    symbols::{BindingId, BindingStorage, FinalReferenceResolution, ReferenceResolution},
};
use matlab_stdlib::{
    format_text_builtin as format_stdlib_text,
    invoke_builtin_outputs as invoke_stdlib_builtin_outputs,
};

type Cell = Rc<RefCell<Option<Value>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Interpreter,
    Bytecode,
    Aot,
    Jit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionResult {
    pub workspace: Workspace,
    pub displayed_outputs: Vec<DisplayedOutput>,
    pub figures: Vec<RenderedFigure>,
    display_format: DisplayFormatState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayedOutput {
    NamedValue {
        name: String,
        value: Value,
    },
    Text {
        text: String,
        ensure_trailing_newline: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumericDisplayFormat {
    Legacy,
    Short,
    Long,
    ShortE,
    LongE,
    ShortG,
    LongG,
    Bank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplaySpacingMode {
    Compact,
    Loose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisplayFormatState {
    numeric: NumericDisplayFormat,
    spacing: Option<DisplaySpacingMode>,
}

impl Default for DisplayFormatState {
    fn default() -> Self {
        Self {
            numeric: NumericDisplayFormat::Legacy,
            spacing: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedFigure {
    pub handle: u32,
    pub title: String,
    pub visible: bool,
    pub position: [f64; 4],
    pub window_style: String,
    pub svg: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlFlow {
    Continue,
    Break,
    Return,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexAssignmentKind {
    Paren,
    Brace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LValueProjection {
    Paren(Vec<HirIndexArgument>),
    Brace(Vec<HirIndexArgument>),
    Field(String),
}

#[derive(Debug, Clone, PartialEq)]
enum LValueLeaf {
    Index {
        kind: IndexAssignmentKind,
        indices: Vec<HirIndexArgument>,
        value: Value,
    },
    Field {
        field: String,
        value: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum EvaluatedIndexArgument {
    Numeric {
        values: Vec<f64>,
        rows: usize,
        cols: usize,
        dims: Vec<usize>,
        logical: bool,
    },
    FullSlice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexSelection {
    positions: Vec<usize>,
    rows: usize,
    cols: usize,
    dims: Vec<usize>,
    linear: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssignmentPlan {
    selection: IndexSelection,
    target_rows: usize,
    target_cols: usize,
    target_dims: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorSource {
    Numeric,
    LogicalMask,
    ScalarLogical,
    FullSlice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorPlanMode {
    Selection,
    Assignment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectorPlan {
    source: SelectorSource,
    indices: Vec<usize>,
    output_rows: usize,
    output_cols: usize,
    output_dims: Vec<usize>,
    target_extent: usize,
}

#[derive(Clone)]
struct Frame<'a> {
    cells: HashMap<BindingId, Cell>,
    names: BTreeMap<String, BindingId>,
    global_names: BTreeSet<String>,
    persistent_names: BTreeSet<String>,
    visible_functions: BTreeMap<String, &'a HirFunction>,
}

#[derive(Debug)]
struct SharedRuntimeState {
    globals: HashMap<String, Cell>,
    persistents: HashMap<PersistentKey, Cell>,
    last_warning: Option<WarningState>,
    last_tic_seconds: Option<f64>,
    next_dynamic_binding_id: u32,
    pause_enabled: bool,
    warnings_enabled: bool,
    warning_overrides: HashMap<String, bool>,
    display_format: DisplayFormatState,
    graphics: GraphicsState,
    pending_host_close_events: BTreeSet<u32>,
    pending_host_resize_events: BTreeSet<u32>,
    active_close_callbacks: BTreeSet<u32>,
    active_resize_callbacks: BTreeSet<u32>,
    displayed_outputs: Vec<DisplayedOutput>,
    figure_backend: Option<FigureBackendState>,
}

impl Default for SharedRuntimeState {
    fn default() -> Self {
        Self {
            globals: HashMap::new(),
            persistents: HashMap::new(),
            last_warning: None,
            last_tic_seconds: None,
            next_dynamic_binding_id: 1_000_000,
            pause_enabled: true,
            warnings_enabled: true,
            warning_overrides: HashMap::new(),
            display_format: DisplayFormatState::default(),
            graphics: GraphicsState::default(),
            pending_host_close_events: BTreeSet::new(),
            pending_host_resize_events: BTreeSet::new(),
            active_close_callbacks: BTreeSet::new(),
            active_resize_callbacks: BTreeSet::new(),
            displayed_outputs: Vec::new(),
            figure_backend: figure_backend_from_env(),
        }
    }
}

#[derive(Debug, Clone)]
struct FigureBackendState {
    session_dir: PathBuf,
    title: String,
    known_handles: BTreeSet<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PersistentKey {
    module_identity: String,
    binding_id: BindingId,
    name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WarningState {
    message: String,
    identifier: String,
}

#[derive(Clone)]
struct AnonymousClosure<'a> {
    function: HirAnonymousFunction,
    captured_cells: HashMap<BindingId, Cell>,
    visible_functions: BTreeMap<String, &'a HirFunction>,
}

#[derive(Debug, Clone)]
struct LoadedClassModule {
    module: HirModule,
    class: matlab_ir::HirClass,
    source_path: PathBuf,
}

fn load_class_module_from_path(path: &Path) -> Result<LoadedClassModule, RuntimeError> {
    let source = fs::read_to_string(path).map_err(|error| {
        RuntimeError::Unsupported(format!(
            "failed to read class definition `{}`: {error}",
            path.display()
        ))
    })?;
    let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
    if parsed.has_errors() {
        return Err(RuntimeError::Unsupported(format!(
            "failed to parse class definition `{}`: {}",
            path.display(),
            format_frontend_diagnostics(&parsed.diagnostics)
        )));
    }
    let unit = parsed.unit.ok_or_else(|| {
        RuntimeError::Unsupported(format!(
            "parser produced no compilation unit for class definition `{}`",
            path.display()
        ))
    })?;
    if unit.kind != CompilationUnitKind::ClassFile {
        return Err(RuntimeError::Unsupported(format!(
            "`{}` is not a class definition file",
            path.display()
        )));
    }
    let context =
        ResolverContext::from_source_file(path.to_path_buf()).with_env_search_roots("MATC_PATH");
    let analysis = analyze_compilation_unit_with_context(&unit, &context);
    if analysis.has_errors() {
        return Err(RuntimeError::Unsupported(format!(
            "failed to analyze class definition `{}`: {}",
            path.display(),
            format_semantic_diagnostics(&analysis.diagnostics)
        )));
    }
    let hir = lower_to_hir(&unit, &analysis);
    let class = hir.classes.first().cloned().ok_or_else(|| {
        RuntimeError::Unsupported(format!(
            "class definition `{}` did not lower class metadata",
            path.display()
        ))
    })?;
    Ok(LoadedClassModule {
        module: hir,
        class,
        source_path: path.to_path_buf(),
    })
}

fn path_resolution_is_class(kind: matlab_semantics::symbols::PathResolutionKind) -> bool {
    matches!(
        kind,
        matlab_semantics::symbols::PathResolutionKind::ClassCurrentDirectory
            | matlab_semantics::symbols::PathResolutionKind::ClassSearchPath
            | matlab_semantics::symbols::PathResolutionKind::ClassPackageDirectory
            | matlab_semantics::symbols::PathResolutionKind::ClassFolderCurrentDirectory
            | matlab_semantics::symbols::PathResolutionKind::ClassFolderSearchPath
            | matlab_semantics::symbols::PathResolutionKind::ClassFolderPackageDirectory
    )
}

fn object_has_method(object: &ObjectValue, method_name: &str) -> bool {
    object.class.inline_methods.contains(method_name)
        || object.class.external_methods.contains_key(method_name)
}

fn bound_method_value(object: &ObjectValue, method_name: &str) -> Value {
    Value::FunctionHandle(FunctionHandleValue {
        display_name: format!("@{}.{}", object.class.class_name, method_name),
        target: FunctionHandleTarget::BoundMethod {
            class_name: object.class.class_name.clone(),
            package: object.class.package.clone(),
            method_name: method_name.to_string(),
            receiver: Box::new(Value::Object(object.clone())),
        },
    })
}

pub fn execute_script(module: &HirModule) -> Result<ExecutionResult, RuntimeError> {
    if module.kind != CompilationUnitKind::Script {
        return Err(RuntimeError::Unsupported(
            "execute_script expects a script compilation unit".to_string(),
        ));
    }

    let mut interpreter = Interpreter::new(module);
    interpreter.execute_script()
}

pub fn execute_function_file(
    module: &HirModule,
    args: &[Value],
) -> Result<ExecutionResult, RuntimeError> {
    if module.kind != CompilationUnitKind::FunctionFile {
        return Err(RuntimeError::Unsupported(
            "execute_function_file expects a function-file compilation unit".to_string(),
        ));
    }

    let mut interpreter = Interpreter::new(module);
    interpreter.execute_function_file(args)
}

pub fn render_execution_result(result: &ExecutionResult) -> String {
    let mut filtered = result.workspace.clone();
    filtered.remove("ans");
    render_workspace_with_format(&filtered, result.display_format)
}

pub fn render_matlab_execution_result(result: &ExecutionResult) -> String {
    let mut out = String::new();
    for (index, displayed) in result.displayed_outputs.iter().enumerate() {
        match displayed {
            DisplayedOutput::NamedValue { name, value } => {
                if index > 0 && matlab_named_output_uses_blank_separator(result.display_format) {
                    out.push('\n');
                }
                render_named_value_with_format(&mut out, "", name, value, result.display_format);
            }
            DisplayedOutput::Text {
                text,
                ensure_trailing_newline,
            } => {
                out.push_str(text);
                if *ensure_trailing_newline && !text.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn first_statement_value<'a>(
    interpreter: &mut Interpreter<'a>,
    frame: &mut Frame<'a>,
    expression: &HirExpression,
) -> Result<Option<Value>, RuntimeError> {
    match expression {
        HirExpression::Call { target, args }
            if call_target_suppresses_ans_in_statement(target, args.len()) =>
        {
            interpreter.execute_statement_builtin_call(frame, target, args)?;
            Ok(None)
        }
        HirExpression::Call { target, args } => Ok(interpreter
            .evaluate_call_outputs(frame, target, args, Some(1))?
            .into_iter()
            .next()),
        _ => Ok(Some(interpreter.evaluate_expression(frame, expression)?)),
    }
}

fn call_target_suppresses_ans_in_statement(target: &HirCallTarget, arg_count: usize) -> bool {
    let HirCallTarget::Callable(reference) = target else {
        return false;
    };
    if reference.binding_id.is_some() {
        return false;
    }
    if matches!(
        reference.final_resolution,
        Some(FinalReferenceResolution::ResolvedPath { .. })
    ) {
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

pub(crate) fn runtime_error_value(
    error: &RuntimeError,
    current_stack: &[RuntimeStackFrame],
) -> Value {
    runtime_error_value_with_stack_fallback(error, current_stack, true)
}

fn invoke_runtime_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    frame: Option<&Frame<'_>>,
    name: &str,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let graphics_result = {
        let mut state = shared_state.borrow_mut();
        invoke_graphics_builtin_outputs(&mut state.graphics, name, args, output_arity)
    };
    if let Some(result) = graphics_result {
        return result.map(|values| {
            flush_figure_backend(shared_state);
            values
        });
    }

    match name {
        "disp" | "display" => {
            invoke_display_builtin_outputs(shared_state, name, args, output_arity)
        }
        "fprintf" => invoke_fprintf_builtin_outputs(shared_state, args, output_arity),
        "drawnow" => invoke_drawnow_builtin_outputs(shared_state, args, output_arity),
        "clc" => invoke_clc_builtin_outputs(shared_state, args, output_arity),
        "format" => invoke_format_builtin_outputs(shared_state, args, output_arity),
        "who" => invoke_who_builtin_outputs(frame, shared_state, args, output_arity),
        "whos" => invoke_whos_builtin_outputs(frame, shared_state, args, output_arity),
        "tic" => invoke_tic_builtin_outputs(shared_state, args, output_arity),
        "toc" => invoke_toc_builtin_outputs(shared_state, args, output_arity),
        "pause" => invoke_pause_builtin_outputs(shared_state, args, output_arity),
        "warning" => invoke_warning_builtin_outputs(shared_state, args, output_arity),
        "lastwarn" => invoke_lastwarn_builtin_outputs(shared_state, args, output_arity),
        _ => invoke_stdlib_builtin_outputs(name, args, output_arity),
    }
}

#[derive(Debug, Clone)]
struct ArrayfunOperand {
    dims: Vec<usize>,
    elements: Vec<Value>,
}

#[derive(Debug, Clone)]
struct ElementwiseCallbackOptions {
    uniform_output: bool,
    error_handler: Option<Value>,
}

fn arrayfun_callback_value(value: &Value) -> Result<Value, RuntimeError> {
    match value {
        Value::FunctionHandle(_) => Ok(value.clone()),
        Value::CharArray(text) | Value::String(text) => Ok(Value::FunctionHandle(
            FunctionHandleValue {
                display_name: text.clone(),
                target: FunctionHandleTarget::Named(text.clone()),
            },
        )),
        other => Err(RuntimeError::TypeError(format!(
            "arrayfun currently expects a function handle or text function name, found {}",
            other.kind_name()
        ))),
    }
}

fn arrayfun_operand_from_value(value: &Value) -> Result<ArrayfunOperand, RuntimeError> {
    match value {
        Value::Matrix(matrix) => Ok(ArrayfunOperand {
            dims: matrix.dims().to_vec(),
            elements: matrix.elements().to_vec(),
        }),
        Value::Cell(_) => Err(RuntimeError::TypeError(
            "arrayfun currently does not accept cell array inputs; use cellfun instead"
                .to_string(),
        )),
        other => Ok(ArrayfunOperand {
            dims: vec![1, 1],
            elements: vec![other.clone()],
        }),
    }
}

fn split_elementwise_callback_options<'a>(
    rest: &'a [Value],
    builtin_name: &str,
) -> Result<(&'a [Value], ElementwiseCallbackOptions), RuntimeError> {
    let mut options = ElementwiseCallbackOptions {
        uniform_output: true,
        error_handler: None,
    };
    let mut input_end = rest.len();

    while input_end >= 2 {
        let option_name = match &rest[input_end - 2] {
            Value::CharArray(text) | Value::String(text) => text.as_str(),
            _ => break,
        };

        if option_name.eq_ignore_ascii_case("UniformOutput") {
            options.uniform_output = match &rest[input_end - 1] {
                Value::Logical(flag) => *flag,
                Value::Scalar(number) if *number == 0.0 => false,
                Value::Scalar(number) if *number == 1.0 => true,
                other => {
                    return Err(RuntimeError::TypeError(format!(
                        "{builtin_name} currently expects UniformOutput to be true or false, found {}",
                        other.kind_name()
                    )))
                }
            };
            input_end -= 2;
            continue;
        }

        if option_name.eq_ignore_ascii_case("ErrorHandler") {
            options.error_handler = Some(arrayfun_callback_value(&rest[input_end - 1])?);
            input_end -= 2;
            continue;
        }

        break;
    }

    Ok((&rest[..input_end], options))
}

fn callback_error_struct_value(error: &RuntimeError, index: usize) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(
        "identifier".to_string(),
        Value::String(error.identifier().to_string()),
    );
    fields.insert(
        "message".to_string(),
        Value::String(error.message().to_string()),
    );
    fields.insert("index".to_string(), Value::Scalar(index as f64));
    Value::Struct(StructValue::from_fields(fields))
}

fn error_handler_arguments(
    error: &RuntimeError,
    index: usize,
    element_args: &[Value],
) -> Vec<Value> {
    let mut args = Vec::with_capacity(element_args.len() + 1);
    args.push(callback_error_struct_value(error, index));
    args.extend(element_args.iter().cloned());
    args
}

fn parse_arrayfun_arguments(
    args: &[Value],
) -> Result<(Value, Vec<ArrayfunOperand>, ElementwiseCallbackOptions), RuntimeError> {
    let Some((callback, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(
            "arrayfun currently expects a function handle plus at least one input array"
                .to_string(),
        ));
    };
    let (inputs, options) = split_elementwise_callback_options(rest, "arrayfun")?;

    if inputs.is_empty() {
        return Err(RuntimeError::Unsupported(
            "arrayfun currently expects at least one input array".to_string(),
        ));
    }

    let callback = arrayfun_callback_value(callback)?;
    let operands = inputs
        .iter()
        .map(arrayfun_operand_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    let first = operands.first().expect("inputs are nonempty");
    for operand in operands.iter().skip(1) {
        if operand.elements.len() != first.elements.len()
            || !equivalent_dimensions(&operand.dims, &first.dims)
        {
            return Err(RuntimeError::ShapeError(
                "arrayfun currently expects all input arrays to have the same size".to_string(),
            ));
        }
    }

    Ok((callback, operands, options))
}

fn cellfun_operand_from_value(value: &Value) -> Result<ArrayfunOperand, RuntimeError> {
    match value {
        Value::Cell(cell) => Ok(ArrayfunOperand {
            dims: cell.dims().to_vec(),
            elements: cell.elements().to_vec(),
        }),
        other => Err(RuntimeError::TypeError(format!(
            "cellfun currently expects cell array inputs, found {}",
            other.kind_name()
        ))),
    }
}

fn parse_cellfun_arguments(
    args: &[Value],
) -> Result<(Value, Vec<ArrayfunOperand>, ElementwiseCallbackOptions), RuntimeError> {
    let Some((callback, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(
            "cellfun currently expects a function handle plus at least one cell array".to_string(),
        ));
    };
    let (inputs, options) = split_elementwise_callback_options(rest, "cellfun")?;
    if inputs.is_empty() {
        return Err(RuntimeError::Unsupported(
            "cellfun currently expects at least one cell array".to_string(),
        ));
    }

    let callback = arrayfun_callback_value(callback)?;
    let operands = inputs
        .iter()
        .map(cellfun_operand_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    let first = operands.first().expect("inputs are nonempty");
    for operand in operands.iter().skip(1) {
        if operand.elements.len() != first.elements.len()
            || !equivalent_dimensions(&operand.dims, &first.dims)
        {
            return Err(RuntimeError::ShapeError(
                "cellfun currently expects all input cell arrays to have the same size"
                    .to_string(),
            ));
        }
    }
    Ok((callback, operands, options))
}

fn parse_structfun_arguments(
    args: &[Value],
) -> Result<(Value, Vec<ArrayfunOperand>, ElementwiseCallbackOptions), RuntimeError> {
    let Some((callback, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(
            "structfun currently expects a function handle plus a scalar struct input".to_string(),
        ));
    };
    let (inputs, options) = split_elementwise_callback_options(rest, "structfun")?;
    let [value] = inputs else {
        return Err(RuntimeError::Unsupported(
            "structfun currently expects exactly one struct input".to_string(),
        ));
    };
    let Value::Struct(struct_value) = value else {
        return Err(RuntimeError::TypeError(format!(
            "structfun currently expects a scalar struct input, found {}",
            value.kind_name()
        )));
    };
    let callback = arrayfun_callback_value(callback)?;
    Ok((
        callback,
        vec![ArrayfunOperand {
            dims: vec![struct_value.fields.len(), 1],
            elements: struct_value.ordered_values().cloned().collect(),
        }],
        options,
    ))
}

fn normalize_arrayfun_uniform_output(value: Value) -> Result<Value, RuntimeError> {
    match value {
        Value::Matrix(matrix) if matrix.element_count() == 1 => Ok(matrix.elements()[0].clone()),
        Value::Cell(_) => Err(RuntimeError::Unsupported(
            "arrayfun with UniformOutput=true currently requires scalar outputs".to_string(),
        )),
        Value::Matrix(_) => Err(RuntimeError::Unsupported(
            "arrayfun with UniformOutput=true currently requires scalar outputs".to_string(),
        )),
        other => Ok(other),
    }
}

fn build_arrayfun_output(dims: &[usize], values: Vec<Value>, uniform_output: bool) -> Result<Value, RuntimeError> {
    let (rows, cols) = storage_shape_from_dimensions(dims);
    if uniform_output {
        if rows == 1 && cols == 1 && values.len() == 1 {
            return Ok(values.into_iter().next().expect("scalar arrayfun output"));
        }
        Ok(Value::Matrix(MatrixValue::with_dimensions(
            rows,
            cols,
            dims.to_vec(),
            values,
        )?))
    } else {
        Ok(Value::Cell(CellValue::with_dimensions(
            rows,
            cols,
            dims.to_vec(),
            values,
        )?))
    }
}

pub(crate) fn execute_arrayfun_builtin_outputs<F>(
    args: &[Value],
    output_arity: usize,
    invoke_callback: F,
) -> Result<Vec<Value>, RuntimeError>
where
    F: FnMut(&Value, &[Value], usize) -> Result<Vec<Value>, RuntimeError>,
{
    let output_arity = output_arity.max(1);
    let (callback, operands, options) = parse_arrayfun_arguments(args)?;
    execute_elementwise_callback_outputs(callback, operands, options, output_arity, "arrayfun", invoke_callback)
}

pub(crate) fn execute_cellfun_builtin_outputs<F>(
    args: &[Value],
    output_arity: usize,
    invoke_callback: F,
) -> Result<Vec<Value>, RuntimeError>
where
    F: FnMut(&Value, &[Value], usize) -> Result<Vec<Value>, RuntimeError>,
{
    let output_arity = output_arity.max(1);
    let (callback, operands, options) = parse_cellfun_arguments(args)?;
    execute_elementwise_callback_outputs(callback, operands, options, output_arity, "cellfun", invoke_callback)
}

pub(crate) fn execute_structfun_builtin_outputs<F>(
    args: &[Value],
    output_arity: usize,
    invoke_callback: F,
) -> Result<Vec<Value>, RuntimeError>
where
    F: FnMut(&Value, &[Value], usize) -> Result<Vec<Value>, RuntimeError>,
{
    let output_arity = output_arity.max(1);
    let (callback, operands, options) = parse_structfun_arguments(args)?;
    execute_elementwise_callback_outputs(callback, operands, options, output_arity, "structfun", invoke_callback)
}

fn execute_elementwise_callback_outputs<F>(
    callback: Value,
    operands: Vec<ArrayfunOperand>,
    options: ElementwiseCallbackOptions,
    output_arity: usize,
    builtin_name: &str,
    mut invoke_callback: F,
) -> Result<Vec<Value>, RuntimeError>
where
    F: FnMut(&Value, &[Value], usize) -> Result<Vec<Value>, RuntimeError>,
{
    let dims = operands
        .first()
        .map(|operand| operand.dims.clone())
        .unwrap_or_else(|| vec![1, 1]);
    let element_count = operands
        .first()
        .map(|operand| operand.elements.len())
        .unwrap_or(1);

    let mut per_output = (0..output_arity)
        .map(|_| Vec::with_capacity(element_count))
        .collect::<Vec<_>>();

    for index in 0..element_count {
        let element_args = operands
            .iter()
            .map(|operand| operand.elements[index].clone())
            .collect::<Vec<_>>();
        let outputs = match invoke_callback(&callback, &element_args, output_arity) {
            Ok(outputs) => outputs,
            Err(error) => {
                let Some(handler) = options.error_handler.as_ref() else {
                    return Err(error);
                };
                let handler_args = error_handler_arguments(&error, index + 1, &element_args);
                invoke_callback(handler, &handler_args, output_arity)?
            }
        };
        if outputs.len() < output_arity {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} requested {output_arity} output(s), but the callback produced {}",
                outputs.len()
            )));
        }
        for output_index in 0..output_arity {
            let value = if options.uniform_output {
                normalize_arrayfun_uniform_output(outputs[output_index].clone())?
            } else {
                outputs[output_index].clone()
            };
            per_output[output_index].push(value);
        }
    }

    per_output
        .into_iter()
        .map(|values| build_arrayfun_output(&dims, values, options.uniform_output))
        .collect()
}

fn invoke_display_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    name: &str,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::Unsupported(format!(
            "{name} currently supports exactly one input argument"
        )));
    }
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{name} currently does not return outputs"
        )));
    }
    let format = current_display_format(shared_state);
    let mut rendered = format_display_builtin_value(&args[0], format);
    if !shared_state.borrow().displayed_outputs.is_empty()
        && matches!(format.spacing, Some(DisplaySpacingMode::Loose))
    {
        rendered.insert(0, '\n');
    }
    push_text_displayed_output(shared_state, rendered, true);
    Ok(Vec::new())
}

fn invoke_fprintf_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (format, values) = match args {
        [] => {
            return Err(RuntimeError::Unsupported(
                "fprintf currently expects at least a format string".to_string(),
            ))
        }
        [format, values @ ..] => (text_value(format)?, values),
    };

    let (format, values) = if let Some(first) = args.first() {
        match first.as_scalar() {
            Ok(file_id)
                if (file_id - 1.0).abs() <= f64::EPSILON
                    || (file_id - 2.0).abs() <= f64::EPSILON =>
            {
                let format = text_value(args.get(1).ok_or_else(|| {
                    RuntimeError::Unsupported(
                        "fprintf currently expects a format string after the file identifier"
                            .to_string(),
                    )
                })?)?;
                (format, &args[2..])
            }
            _ => (format, values),
        }
    } else {
        (format, values)
    };

    let rendered = format_stdlib_text(format, values, "fprintf")?;
    push_text_displayed_output(shared_state, rendered.clone(), false);
    match output_arity {
        0 => Ok(Vec::new()),
        1 => Ok(vec![Value::Scalar(rendered.len() as f64)]),
        _ => Err(RuntimeError::Unsupported(
            "fprintf currently supports at most one output".to_string(),
        )),
    }
}

fn invoke_drawnow_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if !args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "drawnow currently supports no input arguments".to_string(),
        ));
    }
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(
            "drawnow currently does not return outputs".to_string(),
        ));
    }
    flush_figure_backend(shared_state);
    Ok(Vec::new())
}

fn invoke_clc_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if !args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "clc currently supports no input arguments".to_string(),
        ));
    }
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(
            "clc currently does not return outputs".to_string(),
        ));
    }
    shared_state.borrow_mut().displayed_outputs.clear();
    Ok(Vec::new())
}

fn invoke_format_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(
            "format currently does not return outputs".to_string(),
        ));
    }
    let update = parse_format_arguments(args)?;
    let mut state = shared_state.borrow_mut();
    if update.reset_default {
        state.display_format = DisplayFormatState::default();
    }
    if let Some(numeric) = update.numeric {
        state.display_format.numeric = numeric;
    }
    if let Some(spacing) = update.spacing {
        state.display_format.spacing = Some(spacing);
    }
    Ok(Vec::new())
}

fn invoke_who_builtin_outputs(
    frame: Option<&Frame<'_>>,
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let entries = workspace_entries(frame, args, "who")?;
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

fn invoke_whos_builtin_outputs(
    frame: Option<&Frame<'_>>,
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let entries = workspace_entries(frame, args, "whos")?;
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

fn invoke_clear_builtin_outputs(
    frame: &mut Frame<'_>,
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
        clear_global_state(
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
        clear_persistent_state(frame, shared_state);
    }
    Ok(Vec::new())
}

fn invoke_clearvars_builtin_outputs(
    frame: &mut Frame<'_>,
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

fn invoke_save_builtin_outputs(
    frame: &mut Frame<'_>,
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

fn invoke_load_builtin_outputs(
    frame: &mut Frame<'_>,
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

fn invoke_tic_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if !args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "tic currently supports no input arguments".to_string(),
        ));
    }
    let now = current_wall_clock_seconds();
    shared_state.borrow_mut().last_tic_seconds = Some(now);
    match output_arity {
        0 => Ok(Vec::new()),
        1 => Ok(vec![Value::Scalar(now)]),
        _ => Err(RuntimeError::Unsupported(
            "tic currently supports at most one output".to_string(),
        )),
    }
}

fn invoke_toc_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let start = match args {
        [] => shared_state.borrow().last_tic_seconds.ok_or_else(|| {
            RuntimeError::MissingVariable(
                "toc requires a prior tic in the current runtime".to_string(),
            )
        })?,
        [token] => finite_scalar_value(token, "toc")?,
        _ => {
            return Err(RuntimeError::Unsupported(
                "toc currently supports zero arguments or one timer token".to_string(),
            ))
        }
    };
    let elapsed = (current_wall_clock_seconds() - start).max(0.0);
    match output_arity {
        0 => {
            push_text_displayed_output(
                shared_state,
                format!(
                    "Elapsed time is {} seconds.",
                    format_elapsed_seconds(elapsed)
                ),
                true,
            );
            Ok(Vec::new())
        }
        1 => Ok(vec![Value::Scalar(elapsed)]),
        _ => Err(RuntimeError::Unsupported(
            "toc currently supports at most one output".to_string(),
        )),
    }
}

fn invoke_pause_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    match args {
        [] => {
            return Err(RuntimeError::Unsupported(
                "pause currently supports only `pause(seconds)` in this runtime".to_string(),
            ))
        }
        [argument] => {
            if let Ok(state_text) = text_value(argument) {
                let state_text = state_text.to_ascii_lowercase();
                let old_state = pause_state_value(shared_state.borrow().pause_enabled);
                match state_text.as_str() {
                    "on" => shared_state.borrow_mut().pause_enabled = true,
                    "off" => shared_state.borrow_mut().pause_enabled = false,
                    "query" => {}
                    _ => {
                        return Err(RuntimeError::Unsupported(
                            "pause currently supports numeric durations or the states 'on', 'off', and 'query'".to_string(),
                        ))
                    }
                }
                return match output_arity {
                    0 => Ok(Vec::new()),
                    1 => Ok(vec![old_state]),
                    _ => Err(RuntimeError::Unsupported(
                        "pause currently supports at most one output".to_string(),
                    )),
                };
            }

            if output_arity > 0 {
                return Err(RuntimeError::Unsupported(
                    "pause(seconds) currently does not return outputs".to_string(),
                ));
            }
            let duration = finite_scalar_value(argument, "pause")?;
            if duration < 0.0 {
                return Err(RuntimeError::TypeError(
                    "pause currently expects a nonnegative duration".to_string(),
                ));
            }
            if !shared_state.borrow().pause_enabled {
                return Ok(Vec::new());
            }
            flush_figure_backend(shared_state);
            thread::sleep(std::time::Duration::from_secs_f64(duration));
            flush_figure_backend(shared_state);
            Ok(Vec::new())
        }
        _ => Err(RuntimeError::Unsupported(
            "pause currently supports one numeric duration or one state argument".to_string(),
        )),
    }
}

fn pause_state_value(enabled: bool) -> Value {
    Value::CharArray(if enabled { "on" } else { "off" }.to_string())
}

fn invoke_warning_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if let Some(control) = parse_warning_control(args)? {
        return apply_warning_control(shared_state, control, output_arity);
    }

    let warning = parse_warning_arguments(args)?;
    if warning_is_enabled(shared_state, &warning.identifier) {
        store_last_warning(shared_state, warning.clone());
    }
    render_warning_outputs(&warning, output_arity, "warning")
}

fn invoke_lastwarn_builtin_outputs(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    match args {
        [] => {}
        [message] => store_last_warning(
            shared_state,
            WarningState {
                message: text_value(message)?.to_string(),
                identifier: String::new(),
            },
        ),
        [message, identifier] => store_last_warning(
            shared_state,
            WarningState {
                message: text_value(message)?.to_string(),
                identifier: text_value(identifier)?.to_string(),
            },
        ),
        _ => {
            return Err(RuntimeError::Unsupported(
                "lastwarn currently supports zero arguments for query or one/two text arguments for explicit state updates".to_string(),
            ))
        }
    }

    let warning = current_warning(shared_state);
    render_warning_outputs(&warning, output_arity, "lastwarn")
}

fn current_warning(shared_state: &Rc<RefCell<SharedRuntimeState>>) -> WarningState {
    shared_state
        .borrow()
        .last_warning
        .clone()
        .unwrap_or_default()
}

fn current_display_format(shared_state: &Rc<RefCell<SharedRuntimeState>>) -> DisplayFormatState {
    shared_state.borrow().display_format
}

fn push_named_displayed_output(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    name: String,
    value: Value,
) {
    let format = current_display_format(shared_state);
    let mut text = String::new();
    if !shared_state.borrow().displayed_outputs.is_empty()
        && matlab_named_output_uses_blank_separator(format)
    {
        text.push('\n');
    }
    render_named_value_with_format(&mut text, "", &name, &value, format);
    shared_state
        .borrow_mut()
        .displayed_outputs
        .push(DisplayedOutput::Text {
            text,
            ensure_trailing_newline: false,
        });
}

fn push_text_displayed_output(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    text: String,
    ensure_trailing_newline: bool,
) {
    shared_state
        .borrow_mut()
        .displayed_outputs
        .push(DisplayedOutput::Text {
            text,
            ensure_trailing_newline,
        });
}

fn take_displayed_outputs(shared_state: &Rc<RefCell<SharedRuntimeState>>) -> Vec<DisplayedOutput> {
    std::mem::take(&mut shared_state.borrow_mut().displayed_outputs)
}

fn workspace_entries(
    frame: Option<&Frame<'_>>,
    args: &[Value],
    builtin_name: &str,
) -> Result<Vec<(String, Value)>, RuntimeError> {
    match parse_workspace_query_spec(args, builtin_name)? {
        WorkspaceQuerySpec::Current { names, regexes } => {
            let Some(frame) = frame else {
                return Err(RuntimeError::Unsupported(format!(
                    "{builtin_name} requires an active workspace in the current runtime"
                )));
            };
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

enum WorkspaceQuerySpec {
    Current {
        names: BTreeSet<String>,
        regexes: Vec<String>,
    },
    File {
        path: PathBuf,
        names: BTreeSet<String>,
        regexes: Vec<String>,
    },
}

fn parse_workspace_query_spec(
    args: &[Value],
    builtin_name: &str,
) -> Result<WorkspaceQuerySpec, RuntimeError> {
    if let Some(first) = args.first().map(text_value).transpose()? {
        if first.eq_ignore_ascii_case("-file") {
            let Some(path_value) = args.get(1) else {
                return Err(RuntimeError::Unsupported(format!(
                    "{builtin_name} -file currently expects a filename"
                )));
            };
            let path = normalize_workspace_snapshot_path(text_value(path_value)?);
            let (names, regexes) = parse_query_filters(&args[2..], builtin_name)?;
            return Ok(WorkspaceQuerySpec::File {
                path,
                names,
                regexes,
            });
        }
    }

    let (names, regexes) = parse_query_filters(args, builtin_name)?;
    Ok(WorkspaceQuerySpec::Current { names, regexes })
}

fn parse_query_filters(
    args: &[Value],
    builtin_name: &str,
) -> Result<(BTreeSet<String>, Vec<String>), RuntimeError> {
    let mut names = BTreeSet::new();
    let mut regexes = Vec::new();
    let mut regex_mode = false;
    for argument in args {
        let text = text_value(argument)?;
        if text.eq_ignore_ascii_case("-regexp") {
            regex_mode = true;
            continue;
        }
        if text.starts_with('-') {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently supports variable names and `-regexp` only"
            )));
        }
        if regex_mode {
            regexes.push(text.to_string());
        } else {
            names.insert(text.to_string());
        }
    }
    Ok((names, regexes))
}

fn matlab_size_text(value: &Value) -> String {
    let dims = matlab_value_dimensions(value);
    dims.iter()
        .map(|dimension| dimension.to_string())
        .collect::<Vec<_>>()
        .join("x")
}

fn matlab_value_dimensions(value: &Value) -> Vec<usize> {
    match value {
        Value::Scalar(_) | Value::Complex(_) | Value::Logical(_) | Value::String(_) => vec![1, 1],
        Value::CharArray(text) => vec![1, text.chars().count().max(1)],
        Value::Matrix(matrix) => matrix.dims().to_vec(),
        Value::Cell(cell) => cell.dims().to_vec(),
        Value::Struct(_) | Value::Object(_) | Value::FunctionHandle(_) => vec![1, 1],
    }
}

fn matlab_class_name(value: &Value) -> String {
    match value {
        Value::Scalar(_) | Value::Complex(_) => "double".to_string(),
        Value::Logical(_) => "logical".to_string(),
        Value::CharArray(_) => "char".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Matrix(matrix) => matrix_class_name(matrix).to_string(),
        Value::Cell(_) => "cell".to_string(),
        Value::Struct(_) => "struct".to_string(),
        Value::Object(object) => object.class.class_name.clone(),
        Value::FunctionHandle(_) => "function_handle".to_string(),
    }
}

fn matrix_class_name(matrix: &MatrixValue) -> &'static str {
    match matrix.storage_class() {
        ArrayStorageClass::Logical => "logical",
        ArrayStorageClass::String => "string",
        _ => {
            let Some(first) = matrix.elements().first() else {
                return "double";
            };
            match first {
                Value::Struct(_) if matrix.iter().all(|value| matches!(value, Value::Struct(_))) => {
                    "struct"
                }
                _ => "double",
            }
        }
    }
}

fn approximate_value_bytes(value: &Value) -> usize {
    match value {
        Value::Scalar(_) => 8,
        Value::Complex(_) => 16,
        Value::Logical(_) => 1,
        Value::CharArray(text) | Value::String(text) => text.encode_utf16().count() * 2,
        Value::Matrix(matrix) => matrix.elements.iter().map(approximate_value_bytes).sum(),
        Value::Cell(cell) => cell.elements.iter().map(approximate_value_bytes).sum(),
        Value::Struct(struct_value) => struct_value
            .fields
            .iter()
            .map(|(name, value)| name.encode_utf16().count() * 2 + approximate_value_bytes(value))
            .sum(),
        Value::Object(object) => object
            .properties()
            .fields
            .iter()
            .map(|(name, value)| name.encode_utf16().count() * 2 + approximate_value_bytes(value))
            .sum(),
        Value::FunctionHandle(handle) => handle.display_name.encode_utf16().count() * 2,
    }
}

fn value_is_complex(value: &Value) -> bool {
    match value {
        Value::Complex(_) => true,
        Value::Matrix(matrix) => matrix.elements.iter().any(value_is_complex),
        Value::Cell(cell) => cell.elements.iter().any(value_is_complex),
        Value::Struct(struct_value) => struct_value.fields.values().any(value_is_complex),
        Value::Object(object) => object.properties().fields.values().any(value_is_complex),
        _ => false,
    }
}

fn whos_struct_value(name: &str, value: &Value) -> Value {
    let mut fields = BTreeMap::new();
    let dims = matlab_value_dimensions(value)
        .into_iter()
        .map(|dimension| Value::Scalar(dimension as f64))
        .collect::<Vec<_>>();
    fields.insert("name".to_string(), Value::CharArray(name.to_string()));
    fields.insert(
        "size".to_string(),
        Value::Matrix(MatrixValue::new(1, dims.len(), dims).expect("summary dims")),
    );
    fields.insert(
        "bytes".to_string(),
        Value::Scalar(approximate_value_bytes(value) as f64),
    );
    fields.insert(
        "class".to_string(),
        Value::CharArray(matlab_class_name(value)),
    );
    fields.insert("global".to_string(), Value::Logical(false));
    fields.insert("sparse".to_string(), Value::Logical(false));
    fields.insert(
        "complex".to_string(),
        Value::Logical(value_is_complex(value)),
    );
    fields.insert("nesting".to_string(), Value::Scalar(0.0));
    fields.insert("persistent".to_string(), Value::Logical(false));
    Value::Struct(StructValue::from_fields(fields))
}

fn current_wall_clock_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn allocate_dynamic_binding_id(shared_state: &Rc<RefCell<SharedRuntimeState>>) -> BindingId {
    let mut state = shared_state.borrow_mut();
    let next = state.next_dynamic_binding_id;
    state.next_dynamic_binding_id = state.next_dynamic_binding_id.saturating_add(1);
    BindingId(next)
}

fn format_elapsed_seconds(value: f64) -> String {
    let rounded = format!("{value:.6}");
    rounded
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[derive(Default)]
struct ClearSelectionSpec {
    all: bool,
    clear_globals: bool,
    clear_functions: bool,
    names: BTreeSet<String>,
    regexes: Vec<String>,
}

struct ClearvarsSpec {
    targets: ClearSelectionSpec,
    keep: BTreeSet<String>,
    keep_regex: Vec<String>,
}

fn parse_clear_spec(
    args: &[Value],
    builtin_name: &str,
) -> Result<ClearSelectionSpec, RuntimeError> {
    let mut spec = ClearSelectionSpec::default();
    let mut regex_mode = false;
    for argument in args {
        let text = text_value(argument)?;
        if text.eq_ignore_ascii_case("-regexp") {
            regex_mode = true;
            continue;
        }
        if text.starts_with('-') {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently supports variable names and `-regexp` only"
            )));
        }
        if !regex_mode
            && (text.eq_ignore_ascii_case("all") || text.eq_ignore_ascii_case("variables"))
        {
            spec.all = true;
            spec.clear_globals = true;
            spec.clear_functions = true;
            continue;
        }
        if !regex_mode && text.eq_ignore_ascii_case("global") {
            spec.clear_globals = true;
            continue;
        }
        if !regex_mode && text.eq_ignore_ascii_case("functions") {
            spec.clear_functions = true;
            continue;
        }
        if regex_mode {
            spec.regexes.push(text.to_string());
        } else {
            spec.names.insert(text.to_string());
        }
    }
    Ok(spec)
}

struct SaveSpec {
    path: PathBuf,
    names: Option<BTreeSet<String>>,
    regexes: Vec<String>,
    struct_name: Option<String>,
    append: bool,
}

fn parse_save_spec(args: &[Value]) -> Result<SaveSpec, RuntimeError> {
    let Some((filename, names)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(format!(
            "save currently expects a filename"
        )));
    };
    let filename = text_value(filename)?;
    let path = normalize_workspace_snapshot_path(filename);
    let mut append = false;
    let mut struct_name = None;
    let mut regexes = Vec::new();
    let mut variables = BTreeSet::new();
    let mut regex_mode = false;
    let mut index = 0usize;
    while index < names.len() {
        let text = text_value(&names[index])?;
        if text.eq_ignore_ascii_case("-append") {
            append = true;
            index += 1;
            continue;
        }
        if matches!(text, "-mat" | "-v6" | "-v7") {
            index += 1;
            continue;
        }
        if text.eq_ignore_ascii_case("-regexp") {
            regex_mode = true;
            index += 1;
            continue;
        }
        if text.eq_ignore_ascii_case("-struct") {
            regex_mode = false;
            let Some(next) = names.get(index + 1).map(text_value).transpose()? else {
                return Err(RuntimeError::Unsupported(
                    "save -struct currently expects a struct variable name".to_string(),
                ));
            };
            struct_name = Some(next.to_string());
            index += 2;
            continue;
        }
        if text.starts_with('-') {
            return Err(RuntimeError::Unsupported(
                "save currently supports variable names, `-append`, `-regexp`, `-struct`, `-mat`, `-v6`, and `-v7` only"
                    .to_string(),
            ));
        }
        if regex_mode {
            regexes.push(text.to_string());
        } else {
            variables.insert(text.to_string());
        }
        index += 1;
    }
    Ok(SaveSpec {
        path,
        names: if variables.is_empty() {
            None
        } else {
            Some(variables)
        },
        regexes,
        struct_name,
        append,
    })
}

struct LoadSpec {
    path: PathBuf,
    names: BTreeSet<String>,
    regexes: Vec<String>,
}

#[derive(Default)]
struct DisplayFormatUpdate {
    numeric: Option<NumericDisplayFormat>,
    spacing: Option<DisplaySpacingMode>,
    reset_default: bool,
}

fn parse_load_arguments(args: &[Value]) -> Result<LoadSpec, RuntimeError> {
    let Some((filename, names)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(
            "load currently expects a filename".to_string(),
        ));
    };
    let filename = text_value(filename)?;
    let path = normalize_workspace_snapshot_path(filename);
    let (names, regexes) = parse_query_filters(names, "load")?;
    Ok(LoadSpec {
        path,
        names,
        regexes,
    })
}

fn parse_format_arguments(args: &[Value]) -> Result<DisplayFormatUpdate, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "format currently expects one or more format options".to_string(),
        ));
    }

    let mut tokens = Vec::new();
    for argument in args {
        let text = text_value(argument)?;
        tokens.extend(
            text.split_whitespace()
                .filter(|part| !part.is_empty())
                .map(|part| part.to_ascii_lowercase()),
        );
    }
    if tokens.is_empty() {
        return Err(RuntimeError::Unsupported(
            "format currently expects one or more format options".to_string(),
        ));
    }

    let mut update = DisplayFormatUpdate::default();
    let mut index = 0usize;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "short" => {
                match tokens.get(index + 1).map(String::as_str) {
                    Some("g") => {
                        update.numeric = Some(NumericDisplayFormat::ShortG);
                        index += 2;
                    }
                    Some("e") => {
                        update.numeric = Some(NumericDisplayFormat::ShortE);
                        index += 2;
                    }
                    _ => {
                        update.numeric = Some(NumericDisplayFormat::Short);
                        index += 1;
                    }
                }
            }
            "shortg" => {
                update.numeric = Some(NumericDisplayFormat::ShortG);
                index += 1;
            }
            "shorte" => {
                update.numeric = Some(NumericDisplayFormat::ShortE);
                index += 1;
            }
            "long" => {
                match tokens.get(index + 1).map(String::as_str) {
                    Some("g") => {
                        update.numeric = Some(NumericDisplayFormat::LongG);
                        index += 2;
                    }
                    Some("e") => {
                        update.numeric = Some(NumericDisplayFormat::LongE);
                        index += 2;
                    }
                    _ => {
                        update.numeric = Some(NumericDisplayFormat::Long);
                        index += 1;
                    }
                }
            }
            "longg" => {
                update.numeric = Some(NumericDisplayFormat::LongG);
                index += 1;
            }
            "longe" => {
                update.numeric = Some(NumericDisplayFormat::LongE);
                index += 1;
            }
            "default" => {
                update = DisplayFormatUpdate {
                    reset_default: true,
                    ..DisplayFormatUpdate::default()
                };
                index += 1;
            }
            "bank" => {
                update.numeric = Some(NumericDisplayFormat::Bank);
                index += 1;
            }
            "compact" => {
                update.spacing = Some(DisplaySpacingMode::Compact);
                index += 1;
            }
            "loose" => {
                update.spacing = Some(DisplaySpacingMode::Loose);
                index += 1;
            }
            other => {
                return Err(RuntimeError::Unsupported(format!(
                    "format currently supports `default`, `short`, `long`, `short g`, `long g`, `short e`, `long e`, `shortG`, `longG`, `shortE`, `longE`, `bank`, `compact`, and `loose`, not `{other}`"
                )))
            }
        }
    }

    Ok(update)
}

fn normalize_workspace_snapshot_path(filename: &str) -> PathBuf {
    let path = PathBuf::from(filename);
    if path.extension().is_some() {
        path
    } else {
        path.with_extension("mat")
    }
}

fn workspace_snapshot_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(extension) if extension.eq_ignore_ascii_case("matws")
    )
}

struct FPlotSpec {
    function: Value,
    interval: (f64, f64),
    style: Option<Value>,
    property_pairs: Vec<Value>,
    sample_count: usize,
}

struct FPlot3Spec {
    x_function: Value,
    y_function: Value,
    z_function: Value,
    interval: (f64, f64),
    style: Option<Value>,
    property_pairs: Vec<Value>,
    sample_count: usize,
}

struct FSurfSpec {
    function: Value,
    domain: (f64, f64, f64, f64),
    property_pairs: Vec<Value>,
    sample_count: usize,
}

struct FContourSpec {
    function: Value,
    domain: (f64, f64, f64, f64),
    levels: Option<Value>,
    sample_count: usize,
}

struct FImplicitSpec {
    function: Value,
    domain: (f64, f64, f64, f64),
    sample_count: usize,
}

struct FunctionPlotRenderOptions {
    style: Option<Value>,
    property_pairs: Vec<Value>,
    sample_count: usize,
}

fn parse_fplot_spec(args: &[Value]) -> Result<FPlotSpec, RuntimeError> {
    let Some((function, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(
            "fplot currently expects a function handle or function name".to_string(),
        ));
    };
    let mut interval = (-5.0, 5.0);
    let mut next_index = 0usize;
    if let Some(candidate) = rest.first() {
        if let Ok(bounds) = numeric_interval_pair(candidate, "fplot") {
            interval = bounds;
            next_index = 1;
        }
    }
    let render_options = parse_function_plot_render_options(&rest[next_index..], "fplot")?;
    if !interval.0.is_finite() || !interval.1.is_finite() || interval.0 == interval.1 {
        return Err(RuntimeError::ShapeError(
            "fplot currently expects a finite interval with distinct endpoints".to_string(),
        ));
    }
    Ok(FPlotSpec {
        function: function.clone(),
        interval,
        style: render_options.style,
        property_pairs: render_options.property_pairs,
        sample_count: render_options.sample_count,
    })
}

fn parse_fplot3_spec(args: &[Value]) -> Result<FPlot3Spec, RuntimeError> {
    let [x_function, y_function, z_function, rest @ ..] = args else {
        return Err(RuntimeError::Unsupported(
            "fplot3 currently expects X, Y, and Z function handles or function names".to_string(),
        ));
    };
    let mut interval = (-5.0, 5.0);
    let mut next_index = 0usize;
    if let Some(candidate) = rest.first() {
        if let Ok(bounds) = numeric_interval_pair(candidate, "fplot3") {
            interval = bounds;
            next_index = 1;
        }
    }
    let render_options = parse_function_plot_render_options(&rest[next_index..], "fplot3")?;
    if !interval.0.is_finite() || !interval.1.is_finite() || interval.0 == interval.1 {
        return Err(RuntimeError::ShapeError(
            "fplot3 currently expects a finite interval with distinct endpoints".to_string(),
        ));
    }
    Ok(FPlot3Spec {
        x_function: x_function.clone(),
        y_function: y_function.clone(),
        z_function: z_function.clone(),
        interval,
        style: render_options.style,
        property_pairs: render_options.property_pairs,
        sample_count: render_options.sample_count,
    })
}

fn parse_fsurf_spec(args: &[Value], builtin_name: &str) -> Result<FSurfSpec, RuntimeError> {
    let Some((function, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects a function handle or function name"
        )));
    };
    let mut domain = (-5.0, 5.0, -5.0, 5.0);
    let mut next_index = 0usize;
    if let Some(candidate) = rest.first() {
        if let Ok(bounds) = numeric_surface_domain(candidate, builtin_name) {
            domain = bounds;
            next_index = 1;
        }
    }
    let property_pairs = &rest[next_index..];
    if property_pairs.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects trailing graphics properties as property/value pairs"
        )));
    }
    let (property_pairs, sample_count) =
        split_function_plot_property_pairs(property_pairs, builtin_name)?;
    if !domain.0.is_finite()
        || !domain.1.is_finite()
        || !domain.2.is_finite()
        || !domain.3.is_finite()
        || domain.0 == domain.1
        || domain.2 == domain.3
    {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects finite domains with distinct bounds"
        )));
    }
    Ok(FSurfSpec {
        function: function.clone(),
        domain,
        property_pairs,
        sample_count: if sample_count == 401 {
            35
        } else {
            sample_count.max(2)
        },
    })
}

fn parse_fcontour_spec(args: &[Value], builtin_name: &str) -> Result<FContourSpec, RuntimeError> {
    let Some((function, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects a function handle or function name"
        )));
    };
    let mut domain = (-5.0, 5.0, -5.0, 5.0);
    let mut next_index = 0usize;
    if let Some(candidate) = rest.first() {
        if let Ok(bounds) = numeric_surface_domain(candidate, builtin_name) {
            domain = bounds;
            next_index = 1;
        }
    }
    let mut levels = None;
    if let Some(candidate) = rest.get(next_index) {
        let is_property_name = matches!(candidate, Value::CharArray(text) | Value::String(text) if is_fplot_property_name(text));
        if !is_property_name {
            levels = Some(candidate.clone());
            next_index += 1;
        }
    }
    let (property_pairs, sample_count) =
        split_function_plot_property_pairs(&rest[next_index..], builtin_name)?;
    if !property_pairs.is_empty() {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports only the `MeshDensity` name/value option"
        )));
    }
    if !domain.0.is_finite()
        || !domain.1.is_finite()
        || !domain.2.is_finite()
        || !domain.3.is_finite()
        || domain.0 == domain.1
        || domain.2 == domain.3
    {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects finite domains with distinct bounds"
        )));
    }
    Ok(FContourSpec {
        function: function.clone(),
        domain,
        levels,
        sample_count: if sample_count == 401 {
            35
        } else {
            sample_count.max(2)
        },
    })
}

fn parse_fimplicit_spec(args: &[Value]) -> Result<FImplicitSpec, RuntimeError> {
    let Some((function, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(
            "fimplicit currently expects a function handle or function name".to_string(),
        ));
    };
    let mut domain = (-5.0, 5.0, -5.0, 5.0);
    let mut next_index = 0usize;
    if let Some(candidate) = rest.first() {
        if let Ok(bounds) = numeric_surface_domain(candidate, "fimplicit") {
            domain = bounds;
            next_index = 1;
        }
    }
    let (property_pairs, sample_count) =
        split_function_plot_property_pairs(&rest[next_index..], "fimplicit")?;
    if !property_pairs.is_empty() {
        return Err(RuntimeError::Unsupported(
            "fimplicit currently supports only the `MeshDensity` name/value option".to_string(),
        ));
    }
    if !domain.0.is_finite()
        || !domain.1.is_finite()
        || !domain.2.is_finite()
        || !domain.3.is_finite()
        || domain.0 == domain.1
        || domain.2 == domain.3
    {
        return Err(RuntimeError::ShapeError(
            "fimplicit currently expects finite domains with distinct bounds".to_string(),
        ));
    }
    Ok(FImplicitSpec {
        function: function.clone(),
        domain,
        sample_count: if sample_count == 401 {
            35
        } else {
            sample_count.max(2)
        },
    })
}

fn parse_function_plot_render_options(
    args: &[Value],
    builtin_name: &str,
) -> Result<FunctionPlotRenderOptions, RuntimeError> {
    let mut style = None;
    let mut next_index = 0usize;
    if let Some(candidate) = args.first() {
        if is_fplot_style_arg(candidate, args.get(1))? {
            style = Some(candidate.clone());
            next_index += 1;
        }
    }
    let property_pairs = &args[next_index..];
    if property_pairs.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects trailing graphics properties as property/value pairs"
        )));
    }
    let (property_pairs, sample_count) =
        split_function_plot_property_pairs(property_pairs, builtin_name)?;
    Ok(FunctionPlotRenderOptions {
        style,
        property_pairs,
        sample_count,
    })
}

fn split_function_plot_property_pairs(
    property_pairs: &[Value],
    builtin_name: &str,
) -> Result<(Vec<Value>, usize), RuntimeError> {
    let mut filtered_pairs = Vec::new();
    let mut sample_count = 401usize;
    for pair in property_pairs.chunks_exact(2) {
        if is_function_plot_mesh_density_property(&pair[0])? {
            sample_count = parse_function_plot_mesh_density(&pair[1], builtin_name)?;
        } else {
            filtered_pairs.push(pair[0].clone());
            filtered_pairs.push(pair[1].clone());
        }
    }
    Ok((filtered_pairs, sample_count))
}

fn is_function_plot_mesh_density_property(value: &Value) -> Result<bool, RuntimeError> {
    match value {
        Value::CharArray(text) | Value::String(text) => {
            Ok(text.eq_ignore_ascii_case("meshdensity"))
        }
        _ => Ok(false),
    }
}

fn parse_function_plot_mesh_density(
    value: &Value,
    builtin_name: &str,
) -> Result<usize, RuntimeError> {
    let mesh_density = finite_scalar_value(value, builtin_name)?;
    let rounded = mesh_density.round();
    if mesh_density < 2.0 || (mesh_density - rounded).abs() > 1e-9 {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects MeshDensity to be an integer greater than or equal to 2"
        )));
    }
    Ok(rounded as usize)
}

fn normalize_fplot_function_arg(value: &Value, builtin_name: &str) -> Result<Value, RuntimeError> {
    match value {
        Value::FunctionHandle(_) => Ok(value.clone()),
        Value::CharArray(text) | Value::String(text) => {
            Ok(Value::FunctionHandle(FunctionHandleValue {
                display_name: text.clone(),
                target: FunctionHandleTarget::Named(text.clone()),
            }))
        }
        other => Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects a function handle or function name, found {}",
            other.kind_name()
        ))),
    }
}

fn numeric_interval_pair(value: &Value, builtin_name: &str) -> Result<(f64, f64), RuntimeError> {
    let values = fplot_numeric_output_values(value, 2, builtin_name)?;
    if values.len() != 2 {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects the interval as a numeric vector with exactly two elements"
        )));
    }
    Ok((values[0], values[1]))
}

fn numeric_surface_domain(
    value: &Value,
    builtin_name: &str,
) -> Result<(f64, f64, f64, f64), RuntimeError> {
    let values = fplot_numeric_output_values(value, 4, builtin_name)?;
    match values.as_slice() {
        [xmin, xmax] => Ok((*xmin, *xmax, *xmin, *xmax)),
        [xmin, xmax, ymin, ymax] => Ok((*xmin, *xmax, *ymin, *ymax)),
        _ => Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects the domain as a numeric vector with two or four elements"
        ))),
    }
}

fn is_fplot_style_arg(value: &Value, next_value: Option<&Value>) -> Result<bool, RuntimeError> {
    let text = match value {
        Value::CharArray(text) | Value::String(text) => text,
        _ => return Ok(false),
    };
    if is_fplot_property_name(text) && next_value.is_some() {
        return Ok(false);
    }
    Ok(true)
}

fn is_fplot_property_name(text: &str) -> bool {
    matches!(
        text.to_ascii_lowercase().as_str(),
        "color"
            | "displayname"
            | "visible"
            | "linewidth"
            | "linestyle"
            | "marker"
            | "markersize"
            | "markeredgecolor"
            | "markerfacecolor"
            | "maximumnumpoints"
            | "meshdensity"
    )
}

fn sampled_fplot_x_values(interval: (f64, f64), sample_count: usize) -> Vec<f64> {
    let count = sample_count.max(2);
    let step = (interval.1 - interval.0) / (count.saturating_sub(1) as f64);
    (0..count)
        .map(|index| interval.0 + step * index as f64)
        .collect()
}

fn sampled_surface_axis_values(bounds: (f64, f64), sample_count: usize) -> Vec<f64> {
    sampled_fplot_x_values(bounds, sample_count)
}

fn fplot_vector_value(values: &[f64]) -> Result<Value, RuntimeError> {
    Ok(Value::Matrix(MatrixValue::new(
        1,
        values.len(),
        values.iter().copied().map(Value::Scalar).collect(),
    )?))
}

fn surface_matrix_value(rows: usize, cols: usize, values: &[f64]) -> Result<Value, RuntimeError> {
    Ok(Value::Matrix(MatrixValue::new(
        rows,
        cols,
        values.iter().copied().map(Value::Scalar).collect(),
    )?))
}

fn sampled_surface_grid(
    domain: (f64, f64, f64, f64),
    sample_count: usize,
) -> (usize, usize, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let cols = sample_count.max(2);
    let rows = sample_count.max(2);
    let x_values = sampled_surface_axis_values((domain.0, domain.1), cols);
    let y_values = sampled_surface_axis_values((domain.2, domain.3), rows);
    let mut x_grid = Vec::with_capacity(rows * cols);
    let mut y_grid = Vec::with_capacity(rows * cols);
    for y in &y_values {
        for x in &x_values {
            x_grid.push(*x);
            y_grid.push(*y);
        }
    }
    (rows, cols, x_values, y_values, x_grid, y_grid)
}

fn fplot_numeric_output_values(
    value: &Value,
    expected_len: usize,
    builtin_name: &str,
) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(vec![*number; expected_len]),
        Value::Logical(flag) => Ok(vec![if *flag { 1.0 } else { 0.0 }; expected_len]),
        Value::Matrix(matrix) => {
            let values = matrix
                .elements
                .iter()
                .map(|entry| match entry {
                    Value::Scalar(number) => Ok(*number),
                    Value::Logical(flag) => Ok(if *flag { 1.0 } else { 0.0 }),
                    other => Err(RuntimeError::TypeError(format!(
                        "{builtin_name} currently expects the sampled function output to be numeric/logical, found {}",
                        other.kind_name()
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if values.len() == 1 {
                Ok(vec![values[0]; expected_len])
            } else if values.len() == expected_len {
                Ok(values)
            } else {
                Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} currently expects the sampled function output to be scalar or match the sample count {}, found {} values",
                    expected_len,
                    values.len()
                )))
            }
        }
        other => Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects numeric/logical function outputs, found {}",
            other.kind_name()
        ))),
    }
}

fn fsurf_numeric_output_values(
    value: &Value,
    rows: usize,
    cols: usize,
    builtin_name: &str,
) -> Result<Vec<f64>, RuntimeError> {
    let expected_len = rows * cols;
    match value {
        Value::Scalar(number) => Ok(vec![*number; expected_len]),
        Value::Logical(flag) => Ok(vec![if *flag { 1.0 } else { 0.0 }; expected_len]),
        Value::Matrix(matrix) => {
            let values = matrix
                .elements
                .iter()
                .map(|entry| match entry {
                    Value::Scalar(number) => Ok(*number),
                    Value::Logical(flag) => Ok(if *flag { 1.0 } else { 0.0 }),
                    other => Err(RuntimeError::TypeError(format!(
                        "{builtin_name} currently expects the sampled function output to be numeric/logical, found {}",
                        other.kind_name()
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if values.len() == 1 {
                Ok(vec![values[0]; expected_len])
            } else if matrix.rows == rows && matrix.cols == cols {
                Ok(values)
            } else if values.len() == expected_len {
                Ok(values)
            } else {
                Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} currently expects the sampled function output to be scalar or match the sampled surface shape {}x{}, found {} values",
                    rows,
                    cols,
                    values.len()
                )))
            }
        }
        other => Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects numeric/logical function outputs, found {}",
            other.kind_name()
        ))),
    }
}

fn workspace_struct_value(workspace: Workspace) -> Value {
    let mut fields = BTreeMap::new();
    for (name, value) in workspace {
        fields.insert(name, value);
    }
    Value::Struct(StructValue::from_fields(fields))
}

fn parse_clearvars_spec(args: &[Value]) -> Result<ClearvarsSpec, RuntimeError> {
    let mut targets = ClearSelectionSpec::default();
    let mut keep = BTreeSet::new();
    let mut keep_regex = Vec::new();
    let mut except_mode = false;
    let mut regex_mode = false;
    for argument in args {
        let text = text_value(argument)?;
        if text.eq_ignore_ascii_case("-except") {
            except_mode = true;
            regex_mode = false;
            continue;
        }
        if text.eq_ignore_ascii_case("-regexp") {
            regex_mode = true;
            continue;
        }
        if text.starts_with('-') {
            return Err(RuntimeError::Unsupported(
                "clearvars currently supports variable names, `-regexp`, and `-except` only"
                    .to_string(),
            ));
        }
        if except_mode {
            if regex_mode {
                keep_regex.push(text.to_string());
            } else {
                keep.insert(text.to_string());
            }
        } else {
            if regex_mode {
                targets.regexes.push(text.to_string());
            } else if text.eq_ignore_ascii_case("all") || text.eq_ignore_ascii_case("variables") {
                targets.all = true;
            } else {
                targets.names.insert(text.to_string());
            }
        }
    }

    Ok(ClearvarsSpec {
        targets,
        keep,
        keep_regex,
    })
}

fn select_workspace_names(
    visible_names: Vec<String>,
    spec: &ClearSelectionSpec,
) -> Result<BTreeSet<String>, RuntimeError> {
    let mut names = BTreeSet::new();
    if spec.all || (spec.names.is_empty() && spec.regexes.is_empty()) {
        names.extend(visible_names.iter().cloned());
    }
    for name in &spec.names {
        names.insert(name.clone());
    }
    for pattern in &spec.regexes {
        for name in &visible_names {
            if matlab_regexp_is_match(pattern, name)? {
                names.insert(name.clone());
            }
        }
    }
    Ok(names)
}

fn apply_clearvars_keep_filters(
    names_to_clear: &mut BTreeSet<String>,
    visible_names: &[String],
    keep: &BTreeSet<String>,
    keep_regex: &[String],
) -> Result<(), RuntimeError> {
    for name in keep {
        names_to_clear.remove(name);
    }
    for pattern in keep_regex {
        for name in visible_names {
            if matlab_regexp_is_match(pattern, name)? {
                names_to_clear.remove(name);
            }
        }
    }
    Ok(())
}

fn clear_global_state(
    frame: &mut Frame<'_>,
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

fn clear_persistent_state(frame: &mut Frame<'_>, shared_state: &Rc<RefCell<SharedRuntimeState>>) {
    let names = frame.persistent_names.clone();
    frame.clear_workspace_names(&names);
    shared_state.borrow_mut().persistents.clear();
}

fn select_name_filters(
    visible_names: Vec<String>,
    names: &BTreeSet<String>,
    regexes: &[String],
) -> Result<BTreeSet<String>, RuntimeError> {
    let mut selected = BTreeSet::new();
    for name in names {
        if has_wildcard_pattern(name) {
            for visible in &visible_names {
                if matlab_wildcard_is_match(name, visible) {
                    selected.insert(visible.clone());
                }
            }
        } else {
            selected.insert(name.clone());
        }
    }
    for pattern in regexes {
        for visible in &visible_names {
            if matlab_regexp_is_match(pattern, visible)? {
                selected.insert(visible.clone());
            }
        }
    }
    Ok(selected)
}

fn has_wildcard_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

fn matlab_wildcard_is_match(pattern: &str, text: &str) -> bool {
    wildcard_match_here(
        &pattern.chars().collect::<Vec<_>>(),
        &text.chars().collect::<Vec<_>>(),
    )
}

fn wildcard_match_here(pattern: &[char], text: &[char]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    match pattern[0] {
        '*' => {
            for offset in 0..=text.len() {
                if wildcard_match_here(&pattern[1..], &text[offset..]) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if text.is_empty() {
                false
            } else {
                wildcard_match_here(&pattern[1..], &text[1..])
            }
        }
        ch => {
            if text.first().copied() != Some(ch) {
                false
            } else {
                wildcard_match_here(&pattern[1..], &text[1..])
            }
        }
    }
}

fn matlab_regexp_is_match(pattern: &str, text: &str) -> Result<bool, RuntimeError> {
    if pattern.is_empty() {
        return Ok(true);
    }
    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$') && !pattern.ends_with("\\$");
    let core = pattern
        .strip_prefix('^')
        .unwrap_or(pattern)
        .strip_suffix('$')
        .unwrap_or(pattern.strip_prefix('^').unwrap_or(pattern));
    if anchored_start {
        return Ok(simple_regexp_match_here(core, text, anchored_end));
    }
    for start in text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
    {
        if simple_regexp_match_here(core, &text[start..], anchored_end) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn simple_regexp_match_here(pattern: &str, text: &str, anchored_end: bool) -> bool {
    if pattern.is_empty() {
        return !anchored_end || text.is_empty();
    }
    let mut chars = pattern.chars();
    let ch = chars.next().unwrap_or_default();
    let rest = chars.as_str();
    if let Some(star_rest) = rest.strip_prefix('*') {
        return simple_regexp_match_star(ch, star_rest, text, anchored_end);
    }
    let Some(first) = text.chars().next() else {
        return false;
    };
    if ch != '.' && ch != first {
        return false;
    }
    simple_regexp_match_here(rest, &text[first.len_utf8()..], anchored_end)
}

fn simple_regexp_match_star(pattern_ch: char, rest: &str, text: &str, anchored_end: bool) -> bool {
    let mut cursor = text;
    loop {
        if simple_regexp_match_here(rest, cursor, anchored_end) {
            return true;
        }
        let Some(first) = cursor.chars().next() else {
            return false;
        };
        if pattern_ch != '.' && pattern_ch != first {
            return false;
        }
        cursor = &cursor[first.len_utf8()..];
    }
}

fn figure_backend_from_env() -> Option<FigureBackendState> {
    let session_dir = env::var_os("MATC_FIGURE_BACKEND_DIR").map(PathBuf::from)?;
    let title =
        env::var("MATC_FIGURE_BACKEND_TITLE").unwrap_or_else(|_| "MATC Figure Viewer".to_string());
    Some(FigureBackendState {
        session_dir,
        title,
        known_handles: BTreeSet::new(),
    })
}

fn figure_svg_file_name(handle: u32) -> String {
    format!("figure-{handle}.svg")
}

fn figure_html_file_name(handle: u32) -> String {
    format!("figure-{handle}.html")
}

fn host_figure_html_file_name(handle: u32) -> String {
    format!("host-figure-{handle}.html")
}

fn consume_figure_backend_events(state: &mut SharedRuntimeState, backend: &FigureBackendState) {
    let before_resize = figure_resize_callback_snapshot(&state.graphics);
    let Ok(entries) = fs::read_dir(&backend.session_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if let Some(handle_text) = name
            .strip_prefix("event-close-")
            .and_then(|value| value.strip_suffix(".txt"))
        {
            if let Ok(handle) = handle_text.parse::<u32>() {
                if before_resize.contains_key(&handle) {
                    state.pending_host_close_events.insert(handle);
                }
            }
            let _ = fs::remove_file(path);
            continue;
        }
        if let Some(handle_text) = name
            .strip_prefix("event-position-")
            .and_then(|value| value.strip_suffix(".txt"))
        {
            if let Ok(handle) = handle_text.parse::<u32>() {
                if let Ok(raw) = fs::read_to_string(&path) {
                    let values = raw
                        .trim()
                        .split(',')
                        .filter_map(|part| part.trim().parse::<f64>().ok())
                        .collect::<Vec<_>>();
                    if values.len() == 4 && values[2] > 0.0 && values[3] > 0.0 {
                        let next_position = [values[0], values[1], values[2], values[3]];
                        let changed = before_resize
                            .get(&handle)
                            .map(|(position, _)| *position != next_position)
                            .unwrap_or(false);
                        let _ = apply_backend_figure_position(
                            &mut state.graphics,
                            handle,
                            next_position,
                        );
                        if changed {
                            state.pending_host_resize_events.insert(handle);
                        }
                    }
                }
            }
            let _ = fs::remove_file(path);
        }
    }
}

fn session_manifest_json(title: &str, figures: &[RenderedFigure], revision: u128) -> String {
    let handles = figures
        .iter()
        .map(|figure| figure.handle.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let visible_count = figures.iter().filter(|figure| figure.visible).count();
    let figure_entries = figures
        .iter()
        .map(|figure| {
            format!(
                "{{\"handle\":{},\"title\":\"{}\",\"visible\":{},\"window_style\":\"{}\",\"position\":[{},{},{},{}],\"page\":\"{}\",\"browser_page\":\"{}\",\"host_page\":\"{}\",\"svg\":\"{}\"}}",
                figure.handle,
                json_escape(&figure.title),
                if figure.visible { "true" } else { "false" },
                json_escape(&figure.window_style),
                figure.position[0],
                figure.position[1],
                figure.position[2],
                figure.position[3],
                json_escape(&figure_html_file_name(figure.handle)),
                json_escape(&figure_html_file_name(figure.handle)),
                json_escape(&host_figure_html_file_name(figure.handle)),
                json_escape(&figure_svg_file_name(figure.handle)),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"title\":\"{}\",\"figure_count\":{},\"visible_figure_count\":{},\"revision\":{},\"handles\":[{}],\"figures\":[{}]}}",
        json_escape(title),
        figures.len(),
        visible_count,
        revision,
        handles,
        figure_entries
    )
}

fn flush_figure_backend(shared_state: &Rc<RefCell<SharedRuntimeState>>) {
    let mut state = shared_state.borrow_mut();
    let Some(mut backend) = state.figure_backend.clone() else {
        return;
    };

    if fs::create_dir_all(&backend.session_dir).is_err() {
        return;
    }

    consume_figure_backend_events(&mut state, &backend);

    let figures = rendered_figures(&state.graphics);
    let visible_figures = figures
        .iter()
        .filter(|figure| figure.visible)
        .cloned()
        .collect::<Vec<_>>();
    let docked_figures = visible_figures
        .iter()
        .filter(|figure| figure.window_style.eq_ignore_ascii_case("docked"))
        .cloned()
        .collect::<Vec<_>>();
    let current_handles = figures
        .iter()
        .map(|figure| figure.handle)
        .collect::<BTreeSet<_>>();
    let revision = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    for figure in &figures {
        let path = backend
            .session_dir
            .join(figure_svg_file_name(figure.handle));
        if fs::write(&path, &figure.svg).is_err() {
            return;
        }
        let page =
            render_figure_backend_index(&figure.title, std::slice::from_ref(figure), revision);
        if fs::write(
            backend
                .session_dir
                .join(figure_html_file_name(figure.handle)),
            page,
        )
        .is_err()
        {
            return;
        }
        let host_page =
            render_native_figure_host_index(&figure.title, std::slice::from_ref(figure), revision);
        if fs::write(
            backend
                .session_dir
                .join(host_figure_html_file_name(figure.handle)),
            host_page,
        )
        .is_err()
        {
            return;
        }
    }

    for stale in backend
        .known_handles
        .iter()
        .filter(|handle| !current_handles.contains(handle))
    {
        let _ = fs::remove_file(backend.session_dir.join(figure_svg_file_name(*stale)));
        let _ = fs::remove_file(backend.session_dir.join(figure_html_file_name(*stale)));
        let _ = fs::remove_file(backend.session_dir.join(host_figure_html_file_name(*stale)));
    }

    let html = render_figure_backend_index(&backend.title, &visible_figures, revision);
    if fs::write(backend.session_dir.join("index.html"), html).is_err() {
        return;
    }
    let host_html = render_native_figure_host_index(&backend.title, &docked_figures, revision);
    if fs::write(backend.session_dir.join("host_index.html"), &host_html).is_err() {
        return;
    }
    let _ = fs::write(backend.session_dir.join("dock_index.html"), &host_html);

    let _ = fs::write(
        backend.session_dir.join("session.json"),
        session_manifest_json(&backend.title, &figures, revision),
    );

    backend.known_handles = current_handles;
    state.figure_backend = Some(backend);
}

fn render_figure_backend_index(title: &str, figures: &[RenderedFigure], revision: u128) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    html.push_str("<meta http-equiv=\"X-UA-Compatible\" content=\"IE=edge\">");
    html.push_str(&format!("<title>{}</title>", html_escape(title)));
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    html.push_str("<style>");
    html.push_str(":root{color-scheme:light;--bg:#d7dde7;--panel:#f5f7fb;--panel-edge:#b8c2d1;--canvas:#ffffff;--toolbar:#e8edf5;--toolbar-edge:#c1cad7;--text:#15202c;--muted:#526273;--accent:#1f77b4;}");
    html.push_str("*{box-sizing:border-box;}html,body{margin:0;padding:0;background:var(--bg);color:var(--text);font-family:Segoe UI,Arial,sans-serif;}body{min-height:100vh;}");
    html.push_str("header{display:flex;align-items:center;justify-content:space-between;gap:16px;padding:14px 18px;border-bottom:1px solid var(--panel-edge);background:linear-gradient(180deg,#f8fafe 0%,#dfe6f1 100%);box-shadow:0 1px 0 rgba(255,255,255,.8) inset;}header strong{font-size:15px;}header small{color:var(--muted);font-size:12px;}");
    html.push_str("main{max-width:1480px;margin:0 auto;padding:20px;display:grid;gap:18px;}");
    html.push_str(".matc-empty{padding:40px;border:1px solid var(--panel-edge);border-radius:12px;background:var(--panel);text-align:center;color:var(--muted);box-shadow:0 12px 28px rgba(31,47,71,.08);}");
    html.push_str(".matc-figure{border:1px solid var(--panel-edge);border-radius:12px;background:var(--panel);overflow:hidden;box-shadow:0 14px 36px rgba(31,47,71,.12);transition:border-color .14s ease, box-shadow .14s ease;}");
    html.push_str(
        ".matc-figure.active{border-color:#4f89c7;box-shadow:0 16px 40px rgba(31,119,180,.18);}",
    );
    html.push_str(".matc-figure-head{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:10px 14px;border-bottom:1px solid var(--toolbar-edge);background:linear-gradient(180deg,#f7f9fd 0%,#e4eaf3 100%);}");
    html.push_str(".matc-figure-head h2{margin:0;font-size:14px;font-weight:600;}");
    html.push_str(".matc-toolbar{display:flex;flex-wrap:wrap;gap:8px;}");
    html.push_str(".matc-toolbar button{border:1px solid #adb9ca;border-radius:7px;padding:7px 12px;background:linear-gradient(180deg,#ffffff 0%,#e7edf6 100%);color:var(--text);font-size:12px;font-weight:600;cursor:pointer;box-shadow:0 1px 0 rgba(255,255,255,.85) inset;}");
    html.push_str(".matc-toolbar button:hover{border-color:#8fa4bd;background:linear-gradient(180deg,#ffffff 0%,#dbe6f5 100%);}");
    html.push_str(".matc-toolbar button:active{transform:translateY(1px);}");
    html.push_str(".matc-toolbar button.active{border-color:#4f89c7;background:linear-gradient(180deg,#ffffff 0%,#d6e7fb 100%);color:#143e67;}");
    html.push_str(".matc-figure-stage{padding:16px;background:linear-gradient(180deg,#dfe5ee 0%,#ced6e2 100%);}");
    html.push_str(".matc-figure-canvas{min-height:680px;border:1px solid #c6cfdb;border-radius:8px;background:var(--canvas);overflow:hidden;display:flex;align-items:center;justify-content:center;cursor:default;box-shadow:inset 0 1px 2px rgba(17,24,39,.06);}");
    html.push_str(".matc-figure-canvas.dragging{cursor:grabbing;}.matc-figure-canvas.brush-mode{cursor:crosshair;}.matc-figure-canvas.rotate-mode{cursor:grab;}.matc-figure-canvas svg{display:block;max-width:100%;width:100%;height:auto;user-select:none;touch-action:none;}");
    html.push_str(".matc-meta{display:flex;justify-content:space-between;gap:12px;padding:10px 14px;border-top:1px solid var(--toolbar-edge);background:#f8fafc;color:var(--muted);font-size:12px;}");
    html.push_str(".matc-status-readout{font-variant-numeric:tabular-nums;}");
    html.push_str(".matc-tip line{stroke:#334155;stroke-width:1.15;}.matc-tip circle{fill:#1f77b4;stroke:#ffffff;stroke-width:1.2;}.matc-tip rect{fill:#fffdf7;stroke:#526273;stroke-width:0.9;rx:6;ry:6;}.matc-tip text{fill:#1e2937;font-size:11px;font-family:Consolas,'Courier New',monospace;}");
    html.push_str(".matc-brush-box rect{fill:#4f89c7;fill-opacity:0.12;stroke:#275a92;stroke-width:1.1;stroke-dasharray:6 4;}.matc-brush-hit circle{fill:#f59e0b;stroke:#7c2d12;stroke-width:1.2;fill-opacity:0.88;}");
    html.push_str("@media (max-width: 900px){main{padding:12px;}.matc-figure-stage{padding:10px;}.matc-figure-canvas{min-height:420px;}}");
    html.push_str("</style>");
    html.push_str("</head><body>");
    html.push_str(&format!(
        "<header><strong>{}</strong><small>revision {} | figures {}</small></header><main>",
        html_escape(title),
        revision,
        figures.len()
    ));
    render_interactive_figure_sections(&mut html, figures);
    html.push_str("</main><script>");
    append_interactive_figure_script(&mut html);
    html.push_str("</script></body></html>\n");
    html
}

fn render_native_figure_host_index(
    title: &str,
    figures: &[RenderedFigure],
    revision: u128,
) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    html.push_str("<meta http-equiv=\"X-UA-Compatible\" content=\"IE=edge\">");
    html.push_str(&format!("<title>{}</title>", html_escape(title)));
    html.push_str("<style>");
    html.push_str("html,body{margin:0;padding:0;background:#dde4ef;color:#17202a;font-family:Segoe UI,Arial,sans-serif;}body{min-height:100vh;}");
    html.push_str("header{padding:12px 16px;border-bottom:1px solid #b8c2d1;background:linear-gradient(180deg,#f8fafe 0%,#dfe6f1 100%);}header strong{font-size:15px;}header small{margin-left:12px;color:#536274;font-size:12px;}");
    html.push_str("header{display:block;padding:12px 16px;border-bottom:1px solid #b8c2d1;background:linear-gradient(180deg,#f8fafe 0%,#dfe6f1 100%);}main{display:block;padding:18px;}.matc-empty{display:block;padding:28px;border:1px solid #bac6d7;background:#f5f7fb;color:#576678;}.matc-figure{display:block;width:100%;box-sizing:border-box;margin:0 0 18px 0;border:1px solid #bac6d7;background:#f5f7fb;}.matc-figure.active{border-color:#4f89c7;}.matc-figure-head{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:10px 14px;border-bottom:1px solid #c6cfdb;background:#edf2f9;}.matc-figure-head h2{margin:0;font-size:14px;font-weight:600;}.matc-toolbar{display:flex;flex-wrap:wrap;gap:8px;}.matc-toolbar button{border:1px solid #adb9ca;border-radius:6px;padding:6px 11px;background:linear-gradient(180deg,#ffffff 0%,#e7edf6 100%);color:#17202a;font-size:12px;font-weight:600;cursor:pointer;}.matc-toolbar button.active{border-color:#4f89c7;background:linear-gradient(180deg,#ffffff 0%,#d6e7fb 100%);color:#143e67;}.matc-native-host .matc-toolbar{display:flex;}.matc-figure-stage{display:block;width:100%;box-sizing:border-box;padding:14px;overflow:hidden;background:#dbe3ef;}.matc-figure-canvas{display:block;width:100%;box-sizing:border-box;min-height:520px;height:520px;background:#ffffff;border:1px solid #cbd3df;cursor:default;overflow:hidden;}.matc-figure-canvas.dragging{cursor:grabbing;}.matc-figure-canvas svg{display:block;width:100%;height:100%;max-width:none;}.matc-meta{display:flex;justify-content:space-between;gap:12px;padding:8px 14px;border-top:1px solid #c6cfdb;background:#f8fafc;color:#576678;font-size:12px;}.matc-status-readout{font-variant-numeric:tabular-nums;}.matc-tip line{stroke:#334155;stroke-width:1.15;}.matc-tip circle{fill:#1f77b4;stroke:#ffffff;stroke-width:1.2;}.matc-tip rect{fill:#fffdf7;stroke:#526273;stroke-width:0.9;rx:6;ry:6;}.matc-tip text{fill:#1e2937;font-size:11px;font-family:Consolas,'Courier New',monospace;}");
    html.push_str(
        "</style></head><body class=\"matc-native-host\" data-matc-surface=\"native-host\">",
    );
    html.push_str(&format!(
        "<header><strong>{}</strong><small>revision {} | figures {}</small></header><main>",
        html_escape(title),
        revision,
        figures.len()
    ));
    render_interactive_figure_sections(&mut html, figures);
    html.push_str("</main><script>");
    append_interactive_figure_script(&mut html);
    html.push_str("</script></body></html>\n");
    html
}

fn render_interactive_figure_sections(html: &mut String, figures: &[RenderedFigure]) {
    if figures.is_empty() {
        html.push_str("<section class=\"matc-empty\">Waiting for figure output...</section>");
        return;
    }

    for figure in figures {
        html.push_str(&format!(
            "<section class=\"matc-figure\" data-handle=\"{}\" data-window-style=\"{}\" data-visible=\"{}\">",
            figure.handle,
            html_escape(&figure.window_style),
            if figure.visible { "true" } else { "false" }
        ));
        html.push_str(&format!(
            "<div class=\"matc-figure-head\"><h2>{}</h2><div class=\"matc-toolbar\"><button type=\"button\" data-action=\"pan\">Pan</button><button type=\"button\" data-action=\"rotate\">Rotate</button><button type=\"button\" data-action=\"brush\">Brush</button><button type=\"button\" data-action=\"clear-brush\">Clear Brush</button><button type=\"button\" data-action=\"datatip\">Data Tips</button><button type=\"button\" data-action=\"clear-tips\">Clear Tips</button><button type=\"button\" data-action=\"zoom-in\">Zoom In</button><button type=\"button\" data-action=\"zoom-out\">Zoom Out</button><button type=\"button\" data-action=\"reset\">Reset View</button><button type=\"button\" data-action=\"save\">Save SVG</button></div></div>",
            html_escape(&figure.title),
        ));
        html.push_str("<div class=\"matc-figure-stage\">");
        html.push_str(&format!(
            "<div class=\"matc-figure-canvas\" data-handle=\"{}\">",
            figure.handle
        ));
        html.push_str(&figure.svg);
        html.push_str("</div></div>");
        html.push_str(&format!(
            "<div class=\"matc-meta\"><span class=\"matc-status-readout\">Inspect mode | hover a curve or marker</span><span>Handle {}</span></div>",
            figure.handle
        ));
        html.push_str("</section>");
    }
}

fn append_interactive_figure_script(html: &mut String) {
    html.push_str(r##"var matcActiveHandle=null;
function hasClass(node,className){if(!node){return false;}var current='';if(node.getAttribute){current=node.getAttribute('class')||'';}else if(node.className){current=node.className;}return (' '+current+' ').indexOf(' '+className+' ')>=0;}
function findAncestor(node,className){while(node&&node!==document){if(hasClass(node,className)){return node;}node=node.parentNode;}return null;}
function findButton(node){while(node&&node!==document){if(node.tagName==='BUTTON'){return node;}node=node.parentNode;}return null;}
function getPanels(){return document.querySelectorAll('.matc-figure');}
function setActiveFigure(handle){matcActiveHandle=String(handle);var panels=getPanels();for(var i=0;i<panels.length;i++){var panel=panels[i];panel.className=(panel.getAttribute('data-handle')===matcActiveHandle)?'matc-figure active':'matc-figure';}}
function getActivePanel(){var panels=getPanels();if(!panels.length){return null;}if(matcActiveHandle===null){setActiveFigure(panels[0].getAttribute('data-handle'));return panels[0];}for(var i=0;i<panels.length;i++){if(panels[i].getAttribute('data-handle')===String(matcActiveHandle)){return panels[i];}}setActiveFigure(panels[0].getAttribute('data-handle'));return panels[0];}
function panelCanvas(panel){return panel?panel.querySelector('.matc-figure-canvas'):null;}
function defaultReadoutForMode(mode){switch(mode){case 'pan':return 'Pan mode | drag to move axis limits';case 'brush':return 'Brush mode | drag to select points';case 'datatip':return 'Data Tips mode | click a curve or marker to inspect values';default:return 'Inspect mode | hover a curve or marker';}}
function setPanelReadout(panel,text){if(!panel){return;}var label=panel.querySelector('.matc-status-readout');if(label){label.innerText=text;}}
function panelMode(panel){return panel&&panel._matcMode?panel._matcMode:'inspect';}
function syncPanelMode(panel){if(!panel){return;}var mode=panelMode(panel);var buttons=panel.querySelectorAll('button[data-action]');for(var i=0;i<buttons.length;i++){var button=buttons[i];var action=button.getAttribute('data-action');var active=(action==='pan'&&mode==='pan')||(action==='brush'&&mode==='brush')||(action==='datatip'&&mode==='datatip');button.className=active?'active':'';}var canvas=panelCanvas(panel);if(canvas){canvas.className=(mode==='brush')?'matc-figure-canvas brush-mode':'matc-figure-canvas';}setPanelReadout(panel,defaultReadoutForMode(mode));}
function setPanelMode(panel,mode){if(!panel){return;}panel._matcMode=mode;syncPanelMode(panel);}
function togglePanelMode(panel,mode){if(!panel){return;}setPanelMode(panel,panelMode(panel)===mode?'inspect':mode);}
function parseViewBox(svg){var raw=svg.getAttribute('viewBox');if(raw){var parts=raw.replace(/^\s+|\s+$/g,'').split(/\s+/);if(parts.length===4){var nums=[Number(parts[0]),Number(parts[1]),Number(parts[2]),Number(parts[3])];if(isFinite(nums[0])&&isFinite(nums[1])&&isFinite(nums[2])&&isFinite(nums[3])){return nums;}}}var width=Number(svg.getAttribute('width'))||1200;var height=Number(svg.getAttribute('height'))||800;return [0,0,width,height];}
function setViewBox(state,next){state.current=next;state.svg.setAttribute('viewBox',next.join(' '));}
function createSvgElement(name){return document.createElementNS('http://www.w3.org/2000/svg',name);}
function ensureState(canvas){if(canvas._matcState){return canvas._matcState;}var svg=canvas.getElementsByTagName('svg')[0];var original=parseViewBox(svg);svg.setAttribute('viewBox',original.join(' '));svg.removeAttribute('width');svg.removeAttribute('height');svg.style.width='100%';svg.style.height=(document.body&&hasClass(document.body,'matc-native-host'))?'100%':'auto';svg.style.maxWidth='none';var overlay=svg.querySelector('g.matc-overlay');if(!overlay){overlay=createSvgElement('g');overlay.setAttribute('class','matc-overlay');svg.appendChild(overlay);}var tipsLayer=createSvgElement('g');tipsLayer.setAttribute('class','matc-tips-layer');var brushLayer=createSvgElement('g');brushLayer.setAttribute('class','matc-brush-layer');var brushBoxLayer=createSvgElement('g');brushBoxLayer.setAttribute('class','matc-brush-box-layer');overlay.appendChild(brushLayer);overlay.appendChild(brushBoxLayer);overlay.appendChild(tipsLayer);var state={svg:svg,overlay:overlay,tipsLayer:tipsLayer,brushLayer:brushLayer,brushBoxLayer:brushBoxLayer,original:original.slice(0),current:original.slice(0)};canvas._matcState=state;return state;}
function zoomCanvas(canvas,factor,anchorX,anchorY){var state=ensureState(canvas);var current=state.current.slice(0);var nextWidth=current[2]*factor;var nextHeight=current[3]*factor;var focusX=current[0]+current[2]*anchorX;var focusY=current[1]+current[3]*anchorY;var nextX=focusX-nextWidth*anchorX;var nextY=focusY-nextHeight*anchorY;setViewBox(state,[nextX,nextY,nextWidth,nextHeight]);}
function resetCanvas(canvas){var state=ensureState(canvas);setViewBox(state,state.original.slice(0));}
function clearGroup(node){while(node.firstChild){node.removeChild(node.firstChild);}}
function clearCanvasTips(canvas){clearGroup(ensureState(canvas).tipsLayer);}
function clearPanelTips(panel){var canvas=panelCanvas(panel);if(canvas){clearCanvasTips(canvas);}}
function clearCanvasBrush(canvas){var state=ensureState(canvas);clearGroup(state.brushLayer);clearGroup(state.brushBoxLayer);}
function clearPanelBrush(panel){var canvas=panelCanvas(panel);if(canvas){clearCanvasBrush(canvas);}}
function parseSerializedPoints(raw){var out=[];if(!raw){return out;}var entries=raw.split(';');for(var i=0;i<entries.length;i++){if(!entries[i]){continue;}var parts=entries[i].split(',');var point=[];for(var j=0;j<parts.length;j++){point.push(Number(parts[j]));}out.push(point);}return out;}
function candidateDataPoints(candidate){if(!candidate._matcDataPoints){candidate._matcDataPoints=parseSerializedPoints(candidate.getAttribute('data-matc-data'));}return candidate._matcDataPoints;}
function candidateScreenPoints(candidate){if(!candidate._matcScreenPoints){candidate._matcScreenPoints=parseSerializedPoints(candidate.getAttribute('data-matc-screen'));}return candidate._matcScreenPoints;}
function svgPointFromEvent(canvas,event){var state=ensureState(canvas);var rect=canvas.getBoundingClientRect();if(!rect.width||!rect.height){return {x:state.current[0],y:state.current[1]};}var relX=(event.clientX-rect.left)/rect.width;var relY=(event.clientY-rect.top)/rect.height;return {x:state.current[0]+state.current[2]*relX,y:state.current[1]+state.current[3]*relY};}
function findDataCandidate(node){while(node&&node!==document){if(hasClass(node,'matc-series-path')||hasClass(node,'matc-datapoint')){return node;}node=node.parentNode;}return null;}
function nearestCandidatePoint(candidate,canvas,event){var data=candidateDataPoints(candidate);var screen=candidateScreenPoints(candidate);if(!data.length||data.length!==screen.length){return null;}var point=svgPointFromEvent(canvas,event);var bestIndex=0;var bestDistance=Infinity;for(var i=0;i<screen.length;i++){var dx=screen[i][0]-point.x;var dy=screen[i][1]-point.y;var distance=dx*dx+dy*dy;if(distance<bestDistance){bestDistance=distance;bestIndex=i;}}return {index:bestIndex,data:data[bestIndex],screen:screen[bestIndex]};}
function formatViewerNumber(value){if(value===null||value===undefined){return '';}if(!isFinite(value)){return String(value);}var abs=Math.abs(value);if((abs>=10000)||(abs>0&&abs<0.001)){return value.toExponential(4).replace(/0+e/,'e').replace(/\.e/,'e');}var rounded=Math.round(value*1000000)/1000000;var text=String(rounded);return text==='-0'?'0':text;}
function describeDataPoint(info){var text='X: '+formatViewerNumber(info.data[0])+'  Y: '+formatViewerNumber(info.data[1]);if(info.data.length>2){text+='  Z: '+formatViewerNumber(info.data[2]);}return text;}
function buildDataTipLines(info){var lines=['X '+formatViewerNumber(info.data[0]),'Y '+formatViewerNumber(info.data[1])];if(info.data.length>2){lines.push('Z '+formatViewerNumber(info.data[2]));}return lines;}
function addDataTip(panel,info){var canvas=panelCanvas(panel);if(!canvas){return;}var state=ensureState(canvas);var lines=buildDataTipLines(info);var anchorX=info.screen[0];var anchorY=info.screen[1];var boxWidth=12;for(var i=0;i<lines.length;i++){boxWidth=Math.max(boxWidth,lines[i].length*7+12);}var boxHeight=lines.length*16+8;var boxX=anchorX+18;var boxY=anchorY-boxHeight-18;var rightLimit=state.current[0]+state.current[2]-8;var bottomLimit=state.current[1]+state.current[3]-8;if(boxX+boxWidth>rightLimit){boxX=anchorX-boxWidth-18;}if(boxY<state.current[1]+8){boxY=anchorY+18;}if(boxY+boxHeight>bottomLimit){boxY=bottomLimit-boxHeight;}var pointerX=boxX>anchorX?boxX:boxX+boxWidth;var pointerY=boxY+14;var group=createSvgElement('g');group.setAttribute('class','matc-tip');var leader=createSvgElement('line');leader.setAttribute('x1',anchorX);leader.setAttribute('y1',anchorY);leader.setAttribute('x2',pointerX);leader.setAttribute('y2',pointerY);group.appendChild(leader);var dot=createSvgElement('circle');dot.setAttribute('cx',anchorX);dot.setAttribute('cy',anchorY);dot.setAttribute('r',4.2);group.appendChild(dot);var rect=createSvgElement('rect');rect.setAttribute('x',boxX);rect.setAttribute('y',boxY);rect.setAttribute('width',boxWidth);rect.setAttribute('height',boxHeight);group.appendChild(rect);var text=createSvgElement('text');text.setAttribute('x',boxX+8);text.setAttribute('y',boxY+15);for(var lineIndex=0;lineIndex<lines.length;lineIndex++){var tspan=createSvgElement('tspan');tspan.setAttribute('x',boxX+8);if(lineIndex>0){tspan.setAttribute('dy',14);}tspan.appendChild(document.createTextNode(lines[lineIndex]));text.appendChild(tspan);}group.appendChild(text);state.tipsLayer.appendChild(group);}
function drawBrushBox(canvas,start,end){var state=ensureState(canvas);clearGroup(state.brushBoxLayer);var rect=createSvgElement('rect');var left=Math.min(start.x,end.x);var top=Math.min(start.y,end.y);var width=Math.abs(end.x-start.x);var height=Math.abs(end.y-start.y);var group=createSvgElement('g');group.setAttribute('class','matc-brush-box');rect.setAttribute('x',left);rect.setAttribute('y',top);rect.setAttribute('width',width);rect.setAttribute('height',height);group.appendChild(rect);state.brushBoxLayer.appendChild(group);}
function brushPointsInRect(panel,canvas,start,end){var state=ensureState(canvas);clearGroup(state.brushLayer);var minX=Math.min(start.x,end.x);var maxX=Math.max(start.x,end.x);var minY=Math.min(start.y,end.y);var maxY=Math.max(start.y,end.y);var seen={};var count=0;var candidates=panel.querySelectorAll('.matc-series-path,.matc-datapoint');for(var i=0;i<candidates.length;i++){var candidate=candidates[i];var data=candidateDataPoints(candidate);var screen=candidateScreenPoints(candidate);for(var j=0;j<screen.length;j++){var point=screen[j];if(point[0]>=minX&&point[0]<=maxX&&point[1]>=minY&&point[1]<=maxY){var key=point[0]+','+point[1];if(seen[key]){continue;}seen[key]=true;count+=1;var group=createSvgElement('g');group.setAttribute('class','matc-brush-hit');var dot=createSvgElement('circle');dot.setAttribute('cx',point[0]);dot.setAttribute('cy',point[1]);dot.setAttribute('r',5.2);group.appendChild(dot);state.brushLayer.appendChild(group);}}}return count;}
function saveCanvas(panel){var svg=panel.getElementsByTagName('svg')[0];if(!svg||!window.Blob||!window.URL||!URL.createObjectURL){return;}var source='<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n'+svg.outerHTML;var blob=new Blob([source],{type:'image/svg+xml;charset=utf-8'});var url=URL.createObjectURL(blob);var link=document.createElement('a');link.href=url;link.download='figure-'+panel.getAttribute('data-handle')+'.svg';document.body.appendChild(link);link.click();document.body.removeChild(link);window.setTimeout(function(){URL.revokeObjectURL(url);},0);}
function panelHas2DAxes(panel){return matcAxes2DGroups(panel).length>0;}
function matcActive2DGroup(panel){var groups=matcAxes2DGroups(panel);return groups.length?groups[0]:null;}
function matcSet2DLimitReadout(panel,group){setPanelReadout(panel,'Pan mode | X ['+formatViewerNumber(matcGroupLimitTuple(group,'x')[0])+', '+formatViewerNumber(matcGroupLimitTuple(group,'x')[1])+']  Y ['+formatViewerNumber(matcGroupLimitTuple(group,'y')[0])+', '+formatViewerNumber(matcGroupLimitTuple(group,'y')[1])+']');}
function matcZoomPanel(panel,factor){var canvas=panelCanvas(panel);if(!canvas){return;}var group=matcActive2DGroup(panel);if(group){var frame=axesPlotFrame(group);if(frame.length>=4){matcApplyAxisLimits(panel,group,'x',matcZoomLimits(matcScaleName(group,'x'),matcGroupLimitTuple(group,'x'),0.5,factor));matcApplyAxisLimits(panel,group,'y',matcZoomLimits(matcScaleName(group,'y'),matcGroupLimitTuple(group,'y'),0.5,factor));clearPanelTips(panel);clearPanelBrush(panel);matcSet2DLimitReadout(panel,group);return;}}zoomCanvas(canvas,factor,0.5,0.5);}
function matcResetPanel2D(panel){var groups=matcAxes2DGroups(panel);for(var i=0;i<groups.length;i++){matcSetGroupLimitTuple(groups[i],'x',matcBaseGroupLimitTuple(groups[i],'x'));matcSetGroupLimitTuple(groups[i],'y',matcBaseGroupLimitTuple(groups[i],'y'));matcReproject2DGroup(groups[i]);}}
function matcZoomInActiveFigure(){var panel=getActivePanel();if(panel){matcZoomPanel(panel,0.88);}}
function matcZoomOutActiveFigure(){var panel=getActivePanel();if(panel){matcZoomPanel(panel,1.14);}}
function matcResetActiveFigure(){var panel=getActivePanel();var canvas=panelCanvas(panel);if(!panel||!canvas){return;}if(panelHas2DAxes(panel)){matcResetPanel2D(panel);}else{resetCanvas(canvas);}setPanelReadout(panel,defaultReadoutForMode(panelMode(panel)));}
function matcSaveActiveFigure(){var panel=getActivePanel();if(panel){saveCanvas(panel);}}
function matcToggleActiveFigurePanMode(){var panel=getActivePanel();if(panel){togglePanelMode(panel,'pan');}}
function matcToggleActiveFigureBrushMode(){var panel=getActivePanel();if(panel){togglePanelMode(panel,'brush');}}
function matcClearActiveFigureBrush(){var panel=getActivePanel();if(panel){clearPanelBrush(panel);setPanelReadout(panel,defaultReadoutForMode(panelMode(panel)));}}
function matcToggleActiveFigureDataTips(){var panel=getActivePanel();if(panel){togglePanelMode(panel,'datatip');}}
function matcClearActiveFigureDataTips(){var panel=getActivePanel();if(panel){clearPanelTips(panel);setPanelReadout(panel,defaultReadoutForMode(panelMode(panel)));}}
var canvases=document.querySelectorAll('.matc-figure-canvas');for(var i=0;i<canvases.length;i++){(function(canvas){ensureState(canvas);var drag=null;var brushDrag=null;var panel=findAncestor(canvas,'matc-figure');if(panel&&!panel._matcMode){panel._matcMode='inspect';syncPanelMode(panel);}canvas.addEventListener('wheel',function(event){var owningPanel=findAncestor(canvas,'matc-figure');if(!owningPanel||panelHas2DAxes(owningPanel)){return;}if(event.preventDefault){event.preventDefault();}var rect=canvas.getBoundingClientRect();var x=(event.clientX-rect.left)/rect.width;var y=(event.clientY-rect.top)/rect.height;zoomCanvas(canvas,event.deltaY<0?0.88:1.14,x,y);},false);canvas.addEventListener('mousedown',function(event){var owningPanel=findAncestor(canvas,'matc-figure');if(owningPanel){setActiveFigure(owningPanel.getAttribute('data-handle'));}if(event.button!==0){return;}if(panelMode(owningPanel)==='pan'){if(panelHas2DAxes(owningPanel)){return;}drag={x:event.clientX,y:event.clientY,viewBox:ensureState(canvas).current.slice(0)};canvas.className='matc-figure-canvas dragging';if(event.preventDefault){event.preventDefault();}return;}if(panelMode(owningPanel)==='brush'){var start=svgPointFromEvent(canvas,event);brushDrag={start:start,current:start};clearCanvasBrush(canvas);drawBrushBox(canvas,start,start);setPanelReadout(owningPanel,'Brush mode | dragging selection');if(event.preventDefault){event.preventDefault();}}});canvas.addEventListener('mousemove',function(event){var owningPanel=findAncestor(canvas,'matc-figure');if(!owningPanel){return;}if(brushDrag){brushDrag.current=svgPointFromEvent(canvas,event);drawBrushBox(canvas,brushDrag.start,brushDrag.current);return;}if(!owningPanel||drag){return;}var candidate=findDataCandidate(event.target);if(candidate&&findAncestor(candidate,'matc-figure')===owningPanel){var info=nearestCandidatePoint(candidate,canvas,event);if(info){setPanelReadout(owningPanel,describeDataPoint(info));return;}}setPanelReadout(owningPanel,defaultReadoutForMode(panelMode(owningPanel)));});canvas.addEventListener('mouseleave',function(){var owningPanel=findAncestor(canvas,'matc-figure');if(owningPanel&&!drag&&!brushDrag){setPanelReadout(owningPanel,defaultReadoutForMode(panelMode(owningPanel)));}});document.addEventListener('mousemove',function(event){if(!drag){return;}var rect=canvas.getBoundingClientRect();var dx=(event.clientX-drag.x)/rect.width*drag.viewBox[2];var dy=(event.clientY-drag.y)/rect.height*drag.viewBox[3];setViewBox(ensureState(canvas),[drag.viewBox[0]-dx,drag.viewBox[1]-dy,drag.viewBox[2],drag.viewBox[3]]);});document.addEventListener('mouseup',function(){if(drag){drag=null;syncPanelMode(findAncestor(canvas,'matc-figure'));var owningPanel=findAncestor(canvas,'matc-figure');if(owningPanel){setPanelReadout(owningPanel,defaultReadoutForMode(panelMode(owningPanel)));}return;}if(!brushDrag){return;}var owningPanel=findAncestor(canvas,'matc-figure');var count=brushPointsInRect(owningPanel,canvas,brushDrag.start,brushDrag.current);clearGroup(ensureState(canvas).brushBoxLayer);brushDrag=null;if(owningPanel){setPanelReadout(owningPanel,'Brushed '+count+' point(s)');}});canvas.addEventListener('dblclick',function(){var owningPanel=findAncestor(canvas,'matc-figure');if(!owningPanel||panelHas2DAxes(owningPanel)){return;}resetCanvas(canvas);setPanelReadout(owningPanel,defaultReadoutForMode(panelMode(owningPanel)));});canvas.addEventListener('click',function(event){var owningPanel=findAncestor(canvas,'matc-figure');if(owningPanel){setActiveFigure(owningPanel.getAttribute('data-handle'));}if(!owningPanel||panelMode(owningPanel)!=='datatip'){return;}var candidate=findDataCandidate(event.target);if(!candidate){return;}var info=nearestCandidatePoint(candidate,canvas,event);if(info){addDataTip(owningPanel,info);setPanelReadout(owningPanel,describeDataPoint(info));if(event.preventDefault){event.preventDefault();}}});})(canvases[i]);}
document.addEventListener('click',function(event){var button=findButton(event.target);if(!button||!button.getAttribute('data-action')){return;}var panel=findAncestor(button,'matc-figure');if(panel){setActiveFigure(panel.getAttribute('data-handle'));}switch(button.getAttribute('data-action')){case 'pan':matcToggleActiveFigurePanMode();break;case 'brush':matcToggleActiveFigureBrushMode();break;case 'clear-brush':matcClearActiveFigureBrush();break;case 'datatip':matcToggleActiveFigureDataTips();break;case 'clear-tips':matcClearActiveFigureDataTips();break;case 'zoom-in':matcZoomInActiveFigure();break;case 'zoom-out':matcZoomOutActiveFigure();break;case 'reset':matcResetActiveFigure();break;case 'save':matcSaveActiveFigure();break;}});if(document.querySelector('.matc-figure')){var panel=document.querySelector('.matc-figure');setActiveFigure(panel.getAttribute('data-handle'));syncPanelMode(panel);}"##);
    html.push_str(r##"
function panelAxesGroups(panel){return panel?panel.querySelectorAll('g.matc-axes[data-matc-3d=\"true\"]'):[];}
function panelHasRotatableAxes(panel){return panelAxesGroups(panel).length>0;}
function parseNumberTuple(raw){if(!raw){return [];}var parts=String(raw).split(',');var values=[];for(var i=0;i<parts.length;i++){values.push(Number(parts[i]));}return values;}
function serializePoints(points){var out=[];for(var i=0;i<points.length;i++){var point=[];for(var j=0;j<points[i].length;j++){point.push(formatViewerNumber(points[i][j]));}out.push(point.join(','));}return out.join(';');}
function setCandidateScreenPoints(candidate,points){candidate.setAttribute('data-matc-screen',serializePoints(points));candidate._matcScreenPoints=points;}
function ensureBaseScreenPoints(candidate){if(!candidate._matcBaseScreenPoints){candidate._matcBaseScreenPoints=parseSerializedPoints(candidate.getAttribute('data-matc-base-screen')||candidate.getAttribute('data-matc-screen'));candidate.setAttribute('data-matc-base-screen',serializePoints(candidate._matcBaseScreenPoints));}return candidate._matcBaseScreenPoints;}
function axesPlotFrame(group){if(!group._matcPlotFrame){group._matcPlotFrame=parseNumberTuple(group.getAttribute('data-matc-plot-frame'));}return group._matcPlotFrame;}
function axesThreeDRange(group){if(!group._matcThreeDRange){group._matcThreeDRange=parseNumberTuple(group.getAttribute('data-matc-3d-range'));}return group._matcThreeDRange;}
function axesView(group){if(!group._matcView){group._matcView=parseNumberTuple(group.getAttribute('data-matc-view'));}return group._matcView;}
function axesBaseView(group){if(!group._matcBaseView){group._matcBaseView=parseNumberTuple(group.getAttribute('data-matc-base-view'));}return group._matcBaseView;}
function setAxesView(group,view){group._matcView=[view[0],view[1]];group.setAttribute('data-matc-view',formatViewerNumber(view[0])+','+formatViewerNumber(view[1]));}
function projectThreeDPoint(point,range,view){var azimuth=view[0]*Math.PI/180;var elevation=(view[1]-90)*Math.PI/180;var zMid=(range[4]+range[5])/2;var xSpan=Math.abs(range[1]-range[0]);var ySpan=Math.abs(range[3]-range[2]);var zSpan=Math.abs(range[5]-range[4]);if(!isFinite(xSpan)||xSpan<=0){xSpan=1;}if(!isFinite(ySpan)||ySpan<=0){ySpan=1;}if(!isFinite(zSpan)||zSpan<=0){zSpan=1;}var zScale=0.8*((xSpan+ySpan)/2)/zSpan;var centeredZ=(point[2]-zMid)*zScale;var x1=point[0]*Math.cos(azimuth)+point[1]*Math.sin(azimuth);var y1=-point[0]*Math.sin(azimuth)+point[1]*Math.cos(azimuth);return{x:x1,y:y1*Math.cos(elevation)-centeredZ*Math.sin(elevation),depth:y1*Math.sin(elevation)+centeredZ*Math.cos(elevation)};}
function scaleProjected(value,min,max,start,span,invert){if(!isFinite(value)||!isFinite(min)||!isFinite(max)||Math.abs(max-min)<=1e-9){return start+span/2;}var normalized=(value-min)/(max-min);if(invert){normalized=1-normalized;}return start+normalized*span;}
function projectedRangeForAxes(group){var range=axesThreeDRange(group);var view=axesView(group);var nodes=group.querySelectorAll('.matc-series-path[data-matc-dim=\"3\"],.matc-datapoint[data-matc-dim=\"3\"],.matc-3d-stem,.matc-3d-arrow,.matc-3d-patch');var minX=Infinity,maxX=-Infinity,minY=Infinity,maxY=-Infinity;for(var i=0;i<nodes.length;i++){var points=candidateDataPoints(nodes[i]);for(var j=0;j<points.length;j++){if(points[j].length<3){continue;}var projected=projectThreeDPoint(points[j],range,view);minX=Math.min(minX,projected.x);maxX=Math.max(maxX,projected.x);minY=Math.min(minY,projected.y);maxY=Math.max(maxY,projected.y);}}if(!isFinite(minX)||!isFinite(maxX)||!isFinite(minY)||!isFinite(maxY)){return null;}if(Math.abs(maxX-minX)<=1e-9){minX-=0.5;maxX+=0.5;}if(Math.abs(maxY-minY)<=1e-9){minY-=0.5;maxY+=0.5;}return[minX,maxX,minY,maxY];}
function projectScreenPoints(group,points){var limits=projectedRangeForAxes(group);var frame=axesPlotFrame(group);var range=axesThreeDRange(group);var view=axesView(group);var screen=[];var depths=[];if(!limits||frame.length<4){return{screen:screen,depths:depths};}for(var i=0;i<points.length;i++){var projected=projectThreeDPoint(points[i],range,view);screen.push([scaleProjected(projected.x,limits[0],limits[1],frame[0],frame[2],false),scaleProjected(projected.y,limits[2],limits[3],frame[1],frame[3],true)]);depths.push(projected.depth);}return{screen:screen,depths:depths};}
function updatePointGroupTransform(node,screenPoint){var base=ensureBaseScreenPoints(node);if(!base.length||!screenPoint){return;}node.setAttribute('transform','translate('+formatViewerNumber(screenPoint[0]-base[0][0])+' '+formatViewerNumber(screenPoint[1]-base[0][1])+')');}
function updateArrowGroup(node,screenPoints){var color=node.getAttribute('data-matc-color')||'#1f77b4';while(node.firstChild){node.removeChild(node.firstChild);}if(screenPoints.length<2){return;}var x1=screenPoints[0][0];var y1=screenPoints[0][1];var x2=screenPoints[1][0];var y2=screenPoints[1][1];var dx=x2-x1;var dy=y2-y1;var length=Math.sqrt(dx*dx+dy*dy);if(length<=1e-9){var dot=createSvgElement('circle');dot.setAttribute('cx',formatViewerNumber(x1));dot.setAttribute('cy',formatViewerNumber(y1));dot.setAttribute('r','2.5');dot.setAttribute('fill',color);dot.setAttribute('fill-opacity','0.85');node.appendChild(dot);return;}var shaft=createSvgElement('line');shaft.setAttribute('class','matc-arrow-shaft');shaft.setAttribute('x1',formatViewerNumber(x1));shaft.setAttribute('y1',formatViewerNumber(y1));shaft.setAttribute('x2',formatViewerNumber(x2));shaft.setAttribute('y2',formatViewerNumber(y2));shaft.setAttribute('stroke',color);shaft.setAttribute('stroke-width','2.2');shaft.setAttribute('stroke-linecap','round');node.appendChild(shaft);var headLength=Math.max(6,Math.min(12,length*0.22));var headAngle=26*Math.PI/180;var unitX=dx/length;var unitY=dy/length;var backX=-unitX;var backY=-unitY;var cosAngle=Math.cos(headAngle);var sinAngle=Math.sin(headAngle);var leftX=x2+headLength*(backX*cosAngle-backY*sinAngle);var leftY=y2+headLength*(backX*sinAngle+backY*cosAngle);var rightX=x2+headLength*(backX*cosAngle+backY*sinAngle);var rightY=y2+headLength*(-backX*sinAngle+backY*cosAngle);var left=createSvgElement('line');left.setAttribute('class','matc-arrow-head-left');left.setAttribute('x1',formatViewerNumber(x2));left.setAttribute('y1',formatViewerNumber(y2));left.setAttribute('x2',formatViewerNumber(leftX));left.setAttribute('y2',formatViewerNumber(leftY));left.setAttribute('stroke',color);left.setAttribute('stroke-width','2.2');left.setAttribute('stroke-linecap','round');node.appendChild(left);var right=createSvgElement('line');right.setAttribute('class','matc-arrow-head-right');right.setAttribute('x1',formatViewerNumber(x2));right.setAttribute('y1',formatViewerNumber(y2));right.setAttribute('x2',formatViewerNumber(rightX));right.setAttribute('y2',formatViewerNumber(rightY));right.setAttribute('stroke',color);right.setAttribute('stroke-width','2.2');right.setAttribute('stroke-linecap','round');node.appendChild(right);}
function reprojectAxesGroup(group){var nodes=group.querySelectorAll('.matc-series-path[data-matc-dim=\"3\"],.matc-datapoint[data-matc-dim=\"3\"],.matc-3d-stem,.matc-3d-arrow,.matc-3d-patch');for(var i=0;i<nodes.length;i++){var node=nodes[i];var projection=projectScreenPoints(group,candidateDataPoints(node));setCandidateScreenPoints(node,projection.screen);if(hasClass(node,'matc-series-path')){var parts=[];for(var j=0;j<projection.screen.length;j++){parts.push(formatViewerNumber(projection.screen[j][0])+','+formatViewerNumber(projection.screen[j][1]));}node.setAttribute('points',parts.join(' '));continue;}if(hasClass(node,'matc-datapoint')){if(projection.screen.length){updatePointGroupTransform(node,projection.screen[0]);}continue;}if(hasClass(node,'matc-3d-stem')){if(projection.screen.length>=2){node.setAttribute('x1',formatViewerNumber(projection.screen[0][0]));node.setAttribute('y1',formatViewerNumber(projection.screen[0][1]));node.setAttribute('x2',formatViewerNumber(projection.screen[1][0]));node.setAttribute('y2',formatViewerNumber(projection.screen[1][1]));}continue;}if(hasClass(node,'matc-3d-arrow')){updateArrowGroup(node,projection.screen);continue;}if(hasClass(node,'matc-3d-patch')){var patchParts=[];for(var patchIndex=0;patchIndex<projection.screen.length;patchIndex++){patchParts.push(formatViewerNumber(projection.screen[patchIndex][0])+','+formatViewerNumber(projection.screen[patchIndex][1]));}node.setAttribute('points',patchParts.join(' '));var depthSum=0;for(var depthIndex=0;depthIndex<projection.depths.length;depthIndex++){depthSum+=projection.depths[depthIndex];}node.setAttribute('data-matc-depth',String(depthSum/Math.max(1,projection.depths.length)));}}var patches=group.querySelectorAll('.matc-3d-patch');var ordered=[];for(var orderedIndex=0;orderedIndex<patches.length;orderedIndex++){ordered.push(patches[orderedIndex]);}ordered.sort(function(left,right){return Number(left.getAttribute('data-matc-depth')||0)-Number(right.getAttribute('data-matc-depth')||0);});for(var patchOrder=0;patchOrder<ordered.length;patchOrder++){ordered[patchOrder].parentNode.appendChild(ordered[patchOrder]);}}
function reprojectPanel3D(panel){var groups=panelAxesGroups(panel);for(var i=0;i<groups.length;i++){reprojectAxesGroup(groups[i]);}}
function resetPanel3D(panel){var groups=panelAxesGroups(panel);for(var i=0;i<groups.length;i++){setAxesView(groups[i],axesBaseView(groups[i]).slice(0));reprojectAxesGroup(groups[i]);}}
function axesGroupForEvent(panel,canvas,event){var groups=panelAxesGroups(panel);if(!groups.length){return null;}var point=svgPointFromEvent(canvas,event);for(var i=0;i<groups.length;i++){var frame=axesPlotFrame(groups[i]);if(frame.length>=4&&point.x>=frame[0]&&point.x<=frame[0]+frame[2]&&point.y>=frame[1]&&point.y<=frame[1]+frame[3]){return groups[i];}}return groups[0];}
function defaultReadoutForMode(mode,panel){var targetPanel=panel||getActivePanel();switch(mode){case 'pan':return 'Pan mode | drag to move axis limits';case 'brush':return 'Brush mode | drag to select points';case 'datatip':return 'Data Tips mode | click a curve or marker to inspect values';case 'rotate':return panelHasRotatableAxes(targetPanel)?'Rotate mode | drag to orbit 3D axes':'Rotate mode | no 3D axes available';default:return 'Inspect mode | hover a curve or marker';}}
function syncPanelMode(panel){if(!panel){return;}var mode=panelMode(panel);var buttons=panel.querySelectorAll('button[data-action]');for(var i=0;i<buttons.length;i++){var button=buttons[i];var action=button.getAttribute('data-action');var active=(action==='pan'&&mode==='pan')||(action==='rotate'&&mode==='rotate')||(action==='brush'&&mode==='brush')||(action==='datatip'&&mode==='datatip');button.className=active?'active':'';}var canvas=panelCanvas(panel);if(canvas){canvas.className=(mode==='brush')?'matc-figure-canvas brush-mode':(mode==='rotate'?'matc-figure-canvas rotate-mode':'matc-figure-canvas');}setPanelReadout(panel,defaultReadoutForMode(mode,panel));}
function matcToggleActiveFigureRotateMode(){var panel=getActivePanel();if(panel){togglePanelMode(panel,'rotate');}}
var matcRotateDrag=null;
var matcRotateCanvases=document.querySelectorAll('.matc-figure-canvas');for(var rotateCanvasIndex=0;rotateCanvasIndex<matcRotateCanvases.length;rotateCanvasIndex++){(function(canvas){canvas.addEventListener('mousedown',function(event){var panel=findAncestor(canvas,'matc-figure');if(!panel||event.button!==0||panelMode(panel)!=='rotate'){return;}var axesGroup=axesGroupForEvent(panel,canvas,event);if(!axesGroup){setPanelReadout(panel,'Rotate mode | no 3D axes available');return;}matcRotateDrag={canvas:canvas,panel:panel,axes:axesGroup,startX:event.clientX,startY:event.clientY,startView:axesView(axesGroup).slice(0)};canvas.className='matc-figure-canvas dragging rotate-mode';if(event.preventDefault){event.preventDefault();}},false);})(matcRotateCanvases[rotateCanvasIndex]);}
document.addEventListener('mousemove',function(event){if(!matcRotateDrag){return;}var nextView=[matcRotateDrag.startView[0]+(event.clientX-matcRotateDrag.startX)*0.6,Math.max(-89,Math.min(89,matcRotateDrag.startView[1]-(event.clientY-matcRotateDrag.startY)*0.45))];setAxesView(matcRotateDrag.axes,nextView);reprojectAxesGroup(matcRotateDrag.axes);clearPanelTips(matcRotateDrag.panel);clearPanelBrush(matcRotateDrag.panel);setPanelReadout(matcRotateDrag.panel,'Rotate mode | Az '+formatViewerNumber(nextView[0])+'°  El '+formatViewerNumber(nextView[1])+'°');},false);
document.addEventListener('mouseup',function(){if(!matcRotateDrag){return;}var panel=matcRotateDrag.panel;matcRotateDrag=null;syncPanelMode(panel);setPanelReadout(panel,defaultReadoutForMode(panelMode(panel),panel));},false);
var previousMatcResetActiveFigure=matcResetActiveFigure;matcResetActiveFigure=function(){var panel=getActivePanel();previousMatcResetActiveFigure();if(panel){resetPanel3D(panel);clearPanelTips(panel);clearPanelBrush(panel);setPanelReadout(panel,defaultReadoutForMode(panelMode(panel),panel));}};
document.addEventListener('click',function(event){var button=findButton(event.target);if(!button||button.getAttribute('data-action')!=='rotate'){return;}var panel=findAncestor(button,'matc-figure');if(panel){setActiveFigure(panel.getAttribute('data-handle'));matcToggleActiveFigureRotateMode();}},false);
var matcPanels=getPanels();for(var panelIndex=0;panelIndex<matcPanels.length;panelIndex++){reprojectPanel3D(matcPanels[panelIndex]);syncPanelMode(matcPanels[panelIndex]);}"##);
    html.push_str(r##"
function matcAxes2DGroups(panel){return panel?panel.querySelectorAll('g.matc-axes[data-matc-xlim]:not([data-matc-3d=\"true\"])'):[];}
function matcGroupLimitTuple(group,axis){var key=(axis==='x')?'_matcXLim':'_matcYLim';if(!group[key]){group[key]=parseNumberTuple(group.getAttribute(axis==='x'?'data-matc-xlim':'data-matc-ylim'));}return group[key].slice(0);}
function matcBaseGroupLimitTuple(group,axis){var key=(axis==='x')?'_matcBaseXLim':'_matcBaseYLim';if(!group[key]){group[key]=parseNumberTuple(group.getAttribute(axis==='x'?'data-matc-base-xlim':'data-matc-base-ylim'));}return group[key].slice(0);}
function matcSetGroupLimitTuple(group,axis,limits){var key=(axis==='x')?'_matcXLim':'_matcYLim';group[key]=[limits[0],limits[1]];group.setAttribute(axis==='x'?'data-matc-xlim':'data-matc-ylim',formatViewerNumber(limits[0])+','+formatViewerNumber(limits[1]));}
function matcScaleName(group,axis){return String(group.getAttribute(axis==='x'?'data-matc-xscale':'data-matc-yscale')||'linear').toLowerCase();}
function matcScaleForward(scale,value){if(scale==='log'){return Math.log(value)/Math.LN10;}return value;}
function matcScaleInverse(scale,value){if(scale==='log'){return Math.pow(10,value);}return value;}
function matcTicksForScale(scale,lower,upper,count){if(!isFinite(lower)||!isFinite(upper)||count<=0){return [];}if(Math.abs(upper-lower)<=1e-9){var fixed=[];for(var fixedIndex=0;fixedIndex<count;fixedIndex++){fixed.push(lower);}return fixed;}if(scale==='log'&&lower>0&&upper>0){var logLower=matcScaleForward(scale,lower);var logUpper=matcScaleForward(scale,upper);var logTicks=[];for(var logIndex=0;logIndex<count;logIndex++){var logFraction=(count===1)?0:(logIndex/(count-1));logTicks.push(matcScaleInverse(scale,logLower+logFraction*(logUpper-logLower)));}return logTicks;}var linearTicks=[];for(var linearIndex=0;linearIndex<count;linearIndex++){var linearFraction=(count===1)?0:(linearIndex/(count-1));linearTicks.push(lower+linearFraction*(upper-lower));}return linearTicks;}
function matcProject2DPoint(group,dataPoint){var frame=axesPlotFrame(group);var xLimits=matcGroupLimitTuple(group,'x');var yLimits=matcGroupLimitTuple(group,'y');var xScale=matcScaleName(group,'x');var yScale=matcScaleName(group,'y');return [scaleProjected(matcScaleForward(xScale,dataPoint[0]),matcScaleForward(xScale,xLimits[0]),matcScaleForward(xScale,xLimits[1]),frame[0],frame[2],false),scaleProjected(matcScaleForward(yScale,dataPoint[1]),matcScaleForward(yScale,yLimits[0]),matcScaleForward(yScale,yLimits[1]),frame[1],frame[3],true)];}
function matcUpdate2DTicks(group,axis){var tickNodes=group.querySelectorAll((axis==='x')?'.matc-x-tick-label':'.matc-y-tick-label');var gridNodes=group.querySelectorAll((axis==='x')?'.matc-x-grid':'.matc-y-grid');var limits=matcGroupLimitTuple(group,axis);var scale=matcScaleName(group,axis);var ticks=matcTicksForScale(scale,limits[0],limits[1],tickNodes.length||5);var frame=axesPlotFrame(group);for(var index=0;index<tickNodes.length;index++){var tick=ticks[Math.min(index,ticks.length-1)];var label=(tick===undefined)?'':formatViewerNumber(tick);var node=tickNodes[index];if(axis==='x'){var x=scaleProjected(matcScaleForward(scale,tick),matcScaleForward(scale,limits[0]),matcScaleForward(scale,limits[1]),frame[0],frame[2],false);node.setAttribute('x',formatViewerNumber(x));node.setAttribute('y',formatViewerNumber(frame[1]+frame[3]+20));node.textContent=label;}else{var y=scaleProjected(matcScaleForward(scale,tick),matcScaleForward(scale,limits[0]),matcScaleForward(scale,limits[1]),frame[1],frame[3],true);node.setAttribute('x',formatViewerNumber(frame[0]-8));node.setAttribute('y',formatViewerNumber(y+4));node.textContent=label;}}for(var gridIndex=0;gridIndex<gridNodes.length;gridIndex++){var gridTick=ticks[Math.min(gridIndex,ticks.length-1)];var gridNode=gridNodes[gridIndex];if(axis==='x'){var gridX=scaleProjected(matcScaleForward(scale,gridTick),matcScaleForward(scale,limits[0]),matcScaleForward(scale,limits[1]),frame[0],frame[2],false);gridNode.setAttribute('x1',formatViewerNumber(gridX));gridNode.setAttribute('x2',formatViewerNumber(gridX));gridNode.setAttribute('y1',formatViewerNumber(frame[1]+frame[3]));gridNode.setAttribute('y2',formatViewerNumber(frame[1]));}else{var gridY=scaleProjected(matcScaleForward(scale,gridTick),matcScaleForward(scale,limits[0]),matcScaleForward(scale,limits[1]),frame[1],frame[3],true);gridNode.setAttribute('x1',formatViewerNumber(frame[0]));gridNode.setAttribute('x2',formatViewerNumber(frame[0]+frame[2]));gridNode.setAttribute('y1',formatViewerNumber(gridY));gridNode.setAttribute('y2',formatViewerNumber(gridY));}}}
function matcReproject2DGroup(group){var nodes=group.querySelectorAll('.matc-series-path[data-matc-dim=\"2\"],.matc-datapoint[data-matc-dim=\"2\"]');for(var i=0;i<nodes.length;i++){var node=nodes[i];var dataPoints=candidateDataPoints(node);var screen=[];for(var pointIndex=0;pointIndex<dataPoints.length;pointIndex++){screen.push(matcProject2DPoint(group,dataPoints[pointIndex]));}setCandidateScreenPoints(node,screen);if(hasClass(node,'matc-series-path')){var parts=[];for(var screenIndex=0;screenIndex<screen.length;screenIndex++){parts.push(formatViewerNumber(screen[screenIndex][0])+','+formatViewerNumber(screen[screenIndex][1]));}node.setAttribute('points',parts.join(' '));}else if(hasClass(node,'matc-datapoint')&&screen.length){updatePointGroupTransform(node,screen[0]);}}matcUpdate2DTicks(group,'x');matcUpdate2DTicks(group,'y');}
function matcAxes2DGroupForEvent(panel,canvas,event){var groups=matcAxes2DGroups(panel);if(!groups.length){return null;}var point=svgPointFromEvent(canvas,event);for(var i=0;i<groups.length;i++){var frame=axesPlotFrame(groups[i]);if(frame.length>=4&&point.x>=frame[0]&&point.x<=frame[0]+frame[2]&&point.y>=frame[1]&&point.y<=frame[1]+frame[3]){return groups[i];}}return null;}
function matcLinkedPeers(panel,group,axis){var groupId=group.getAttribute('data-matc-link-group');var mode=String(group.getAttribute('data-matc-link-mode')||'').toLowerCase();if(!groupId||mode.indexOf(axis)<0){return [group];}var peers=[];var groups=matcAxes2DGroups(panel);for(var i=0;i<groups.length;i++){if(groups[i].getAttribute('data-matc-link-group')===groupId){peers.push(groups[i]);}}return peers.length?peers:[group];}
function matcApplyAxisLimits(panel,group,axis,limits){var peers=matcLinkedPeers(panel,group,axis);for(var i=0;i<peers.length;i++){matcSetGroupLimitTuple(peers[i],axis,limits);matcReproject2DGroup(peers[i]);}}
function matcPanLimits(scale,limits,deltaFraction){var start=matcScaleForward(scale,limits[0]);var end=matcScaleForward(scale,limits[1]);var span=end-start;return [matcScaleInverse(scale,start-deltaFraction*span),matcScaleInverse(scale,end-deltaFraction*span)];}
function matcZoomLimits(scale,limits,anchorFraction,factor){var start=matcScaleForward(scale,limits[0]);var end=matcScaleForward(scale,limits[1]);var span=end-start;var nextSpan=span*factor;var focus=start+span*anchorFraction;var nextStart=focus-nextSpan*anchorFraction;return [matcScaleInverse(scale,nextStart),matcScaleInverse(scale,nextStart+nextSpan)];}
var matcLinked2DDrag=null;
var matcLinked2DCanvases=document.querySelectorAll('.matc-figure-canvas');for(var canvasIndex=0;canvasIndex<matcLinked2DCanvases.length;canvasIndex++){(function(canvas){canvas.addEventListener('mousedown',function(event){var panel=findAncestor(canvas,'matc-figure');if(!panel||event.button!==0||panelMode(panel)!=='pan'){return;}var group=matcAxes2DGroupForEvent(panel,canvas,event);if(!group){return;}matcLinked2DDrag={panel:panel,group:group,startX:event.clientX,startY:event.clientY,xlim:matcGroupLimitTuple(group,'x'),ylim:matcGroupLimitTuple(group,'y')};if(event.preventDefault){event.preventDefault();}if(event.stopImmediatePropagation){event.stopImmediatePropagation();}},true);canvas.addEventListener('wheel',function(event){var panel=findAncestor(canvas,'matc-figure');if(!panel){return;}var group=matcAxes2DGroupForEvent(panel,canvas,event);if(!group){return;}var frame=axesPlotFrame(group);if(frame.length<4){return;}var point=svgPointFromEvent(canvas,event);var xAnchor=(point.x-frame[0])/frame[2];var yAnchor=(point.y-frame[1])/frame[3];var factor=(event.deltaY<0)?0.88:1.14;matcApplyAxisLimits(panel,group,'x',matcZoomLimits(matcScaleName(group,'x'),matcGroupLimitTuple(group,'x'),xAnchor,factor));matcApplyAxisLimits(panel,group,'y',matcZoomLimits(matcScaleName(group,'y'),matcGroupLimitTuple(group,'y'),1-yAnchor,factor));clearPanelTips(panel);clearPanelBrush(panel);matcSet2DLimitReadout(panel,group);if(event.preventDefault){event.preventDefault();}if(event.stopImmediatePropagation){event.stopImmediatePropagation();}},true);canvas.addEventListener('dblclick',function(event){var panel=findAncestor(canvas,'matc-figure');if(!panel){return;}var group=matcAxes2DGroupForEvent(panel,canvas,event);if(!group){return;}matcApplyAxisLimits(panel,group,'x',matcBaseGroupLimitTuple(group,'x'));matcApplyAxisLimits(panel,group,'y',matcBaseGroupLimitTuple(group,'y'));clearPanelTips(panel);clearPanelBrush(panel);setPanelReadout(panel,defaultReadoutForMode(panelMode(panel),panel));if(event.preventDefault){event.preventDefault();}if(event.stopImmediatePropagation){event.stopImmediatePropagation();}},true);})(matcLinked2DCanvases[canvasIndex]);}
document.addEventListener('mousemove',function(event){if(!matcLinked2DDrag){return;}var group=matcLinked2DDrag.group;var frame=axesPlotFrame(group);if(frame.length<4){return;}matcApplyAxisLimits(matcLinked2DDrag.panel,group,'x',matcPanLimits(matcScaleName(group,'x'),matcLinked2DDrag.xlim,(event.clientX-matcLinked2DDrag.startX)/frame[2]));matcApplyAxisLimits(matcLinked2DDrag.panel,group,'y',matcPanLimits(matcScaleName(group,'y'),matcLinked2DDrag.ylim,(event.clientY-matcLinked2DDrag.startY)/frame[3]));clearPanelTips(matcLinked2DDrag.panel);clearPanelBrush(matcLinked2DDrag.panel);matcSet2DLimitReadout(matcLinked2DDrag.panel,group);if(event.preventDefault){event.preventDefault();}},true);
document.addEventListener('mouseup',function(){if(!matcLinked2DDrag){return;}var panel=matcLinked2DDrag.panel;matcLinked2DDrag=null;syncPanelMode(panel);setPanelReadout(panel,defaultReadoutForMode(panelMode(panel),panel));},true);
var previousMatcResetFor2D=matcResetActiveFigure;matcResetActiveFigure=function(){var panel=getActivePanel();previousMatcResetFor2D();if(panel&&panelHas2DAxes(panel)){matcResetPanel2D(panel);setPanelReadout(panel,defaultReadoutForMode(panelMode(panel),panel));}};
var matcAllPanelsFor2D=getPanels();for(var panel2DIndex=0;panel2DIndex<matcAllPanelsFor2D.length;panel2DIndex++){var groups=matcAxes2DGroups(matcAllPanelsFor2D[panel2DIndex]);for(var groupIndex=0;groupIndex<groups.length;groupIndex++){matcReproject2DGroup(groups[groupIndex]);}}
function matcHostStatePayload(panel){var active=panel||getActivePanel();return {type:'matc-host-state',activeHandle:active?Number(active.getAttribute('data-handle')):null,readout:active?(active.querySelector('.matc-status-readout')?active.querySelector('.matc-status-readout').innerText:''):'',mode:active?panelMode(active):'inspect'};}
function matcPostHostState(panel){try{if(window.ipc&&window.ipc.postMessage){window.ipc.postMessage(JSON.stringify(matcHostStatePayload(panel)));}}catch(error){}}
var previousMatcSetActiveFigure=setActiveFigure;setActiveFigure=function(handle){previousMatcSetActiveFigure(handle);matcPostHostState(getActivePanel());};
var previousMatcSetPanelReadout=setPanelReadout;setPanelReadout=function(panel,text){previousMatcSetPanelReadout(panel,text);matcPostHostState(panel);};
var previousMatcSyncPanelMode=syncPanelMode;syncPanelMode=function(panel){previousMatcSyncPanelMode(panel);matcPostHostState(panel);};
var initialMatcHostPanel=getActivePanel();if(initialMatcHostPanel){matcPostHostState(initialMatcHostPanel);}"##);
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
}

fn json_escape(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn close_status_outputs(
    status: bool,
    output_arity: usize,
    builtin_name: &str,
) -> Result<Vec<Value>, RuntimeError> {
    match output_arity {
        0 => Ok(Vec::new()),
        1 => Ok(vec![Value::Scalar(if status { 1.0 } else { 0.0 })]),
        _ => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports at most one output"
        ))),
    }
}

fn changed_resize_callbacks(
    before: &BTreeMap<u32, ([f64; 4], Option<Value>)>,
    after: &BTreeMap<u32, ([f64; 4], Option<Value>)>,
    active: &BTreeSet<u32>,
) -> Vec<(u32, Value)> {
    after
        .iter()
        .filter_map(|(handle, (position, callback))| {
            if active.contains(handle) {
                return None;
            }
            let callback = callback.clone()?;
            match before.get(handle) {
                Some((before_position, _)) if before_position == position => None,
                _ => Some((*handle, callback)),
            }
        })
        .collect()
}

fn resize_callback_for_handle(state: &SharedRuntimeState, handle: u32) -> Option<Value> {
    figure_resize_callback_snapshot(&state.graphics)
        .get(&handle)
        .and_then(|(_, callback)| callback.clone())
}

fn render_workspace_with_format(workspace: &Workspace, format: DisplayFormatState) -> String {
    if format == DisplayFormatState::default() {
        return render_workspace(workspace);
    }

    let mut out = String::from("workspace\n");
    for (index, (name, value)) in workspace.iter().enumerate() {
        if index > 0 && matches!(format.spacing, Some(DisplaySpacingMode::Loose)) {
            out.push('\n');
        }
        render_named_value_with_format(&mut out, "  ", name, value, format);
    }
    out
}

fn render_named_value_with_format(
    out: &mut String,
    indent: &str,
    name: &str,
    value: &Value,
    format: DisplayFormatState,
) {
    if format == DisplayFormatState::default() {
        render_named_value(out, indent, name, value);
        return;
    }

    match value {
        Value::Matrix(matrix) => {
            if let Some(page_dims) = display_paged_tail_dimensions(&matrix.dims) {
                if page_dims.iter().product::<usize>() == 0 {
                    out.push_str(&format!("{indent}{name} = []\n"));
                    return;
                }
                for page in 0..page_dims.iter().product::<usize>() {
                    let page_index = display_column_major_multi_index(page, &page_dims);
                    let label = page_index
                        .iter()
                        .map(|index| (index + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "{indent}{name}(:,:,{label}) = {}\n",
                        render_matrix_inline_with_format(matrix, Some(&page_index), format)
                    ));
                }
            } else {
                out.push_str(&format!(
                    "{indent}{name} = {}\n",
                    render_matrix_inline_with_format(matrix, None, format)
                ));
            }
        }
        Value::Cell(cell) => {
            if let Some(page_dims) = display_paged_tail_dimensions(&cell.dims) {
                if page_dims.iter().product::<usize>() == 0 {
                    out.push_str(&format!("{indent}{name} = {{}}\n"));
                    return;
                }
                for page in 0..page_dims.iter().product::<usize>() {
                    let page_index = display_column_major_multi_index(page, &page_dims);
                    let label = page_index
                        .iter()
                        .map(|index| (index + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "{indent}{name}(:,:,{label}) = {}\n",
                        render_cell_inline_with_format(cell, Some(&page_index), format)
                    ));
                }
            } else {
                out.push_str(&format!(
                    "{indent}{name} = {}\n",
                    render_cell_inline_with_format(cell, None, format)
                ));
            }
        }
        _ => out.push_str(&format!(
            "{indent}{name} = {}\n",
            render_value_with_format(value, format)
        )),
    }
}

fn render_value_with_format(value: &Value, format: DisplayFormatState) -> String {
    if format == DisplayFormatState::default() {
        return render_value(value);
    }

    match value {
        Value::Scalar(number) => render_number_with_format(*number, format.numeric),
        Value::Complex(number) => render_complex_with_format(number, format.numeric),
        Value::Logical(flag) => {
            if *flag {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::CharArray(text) => render_quoted_text_with_delimiter(text, '\''),
        Value::String(text) => render_quoted_text_with_delimiter(text, '"'),
        Value::Matrix(matrix) => render_matrix_inline_with_format(matrix, None, format),
        Value::Cell(cell) => render_cell_inline_with_format(cell, None, format),
        Value::Struct(struct_value) => {
            let fields = struct_value
                .fields
                .iter()
                .map(|(name, value)| format!("{name}={}", render_value_with_format(value, format)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("struct{{{fields}}}")
        }
        Value::Object(object) => format!(
            "{} with properties {{{}}}",
            object.class.class_name,
            object
                .properties()
                .ordered_entries()
                .map(|(name, value)| format!("{name}={}", render_value_with_format(value, format)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::FunctionHandle(handle) => format!("@{}", handle.display_name),
    }
}

fn render_matrix_inline_with_format(
    matrix: &MatrixValue,
    tail_index: Option<&[usize]>,
    format: DisplayFormatState,
) -> String {
    let rows = matrix.dims.first().copied().unwrap_or(matrix.rows);
    let cols = matrix.dims.get(1).copied().unwrap_or(matrix.cols);
    let rows = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let mut index = vec![row, col];
                    if let Some(tail_index) = tail_index {
                        index.extend_from_slice(tail_index);
                    }
                    let linear = row_major_linear_index(&index, &matrix.dims);
                    render_value_with_format(&matrix.elements[linear], format)
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect::<Vec<_>>()
        .join(" ; ");
    format!("[{rows}]")
}

fn render_cell_inline_with_format(
    cell: &CellValue,
    tail_index: Option<&[usize]>,
    format: DisplayFormatState,
) -> String {
    let rows = cell.dims.first().copied().unwrap_or(cell.rows);
    let cols = cell.dims.get(1).copied().unwrap_or(cell.cols);
    let rows = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let mut index = vec![row, col];
                    if let Some(tail_index) = tail_index {
                        index.extend_from_slice(tail_index);
                    }
                    let linear = row_major_linear_index(&index, &cell.dims);
                    render_value_with_format(&cell.elements[linear], format)
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect::<Vec<_>>()
        .join(" ; ");
    format!("{{{rows}}}")
}

fn display_paged_tail_dimensions(dims: &[usize]) -> Option<Vec<usize>> {
    let mut canonical = dims.to_vec();
    while canonical.len() > 2 && canonical.last() == Some(&1) {
        canonical.pop();
    }
    (canonical.len() > 2).then(|| canonical[2..].to_vec())
}

fn display_column_major_multi_index(mut linear: usize, dims: &[usize]) -> Vec<usize> {
    let mut index = vec![0usize; dims.len()];
    for axis in 0..dims.len() {
        let dim = dims[axis].max(1);
        index[axis] = linear % dim;
        linear /= dim;
    }
    index
}

fn render_complex_with_format(number: &ComplexValue, format: NumericDisplayFormat) -> String {
    if number.imag == 0.0 {
        return render_number_with_format(number.real, format);
    }
    if number.real == 0.0 {
        return format!("{}i", render_number_with_format(number.imag, format));
    }

    let sign = if number.imag.is_sign_negative() {
        "-"
    } else {
        "+"
    };
    let imag = render_number_with_format(number.imag.abs(), format);
    format!(
        "{} {} {}i",
        render_number_with_format(number.real, format),
        sign,
        imag
    )
}

fn render_number_with_format(number: f64, format: NumericDisplayFormat) -> String {
    match format {
        NumericDisplayFormat::Legacy => {
            if number.fract() == 0.0 {
                format!("{number:.0}")
            } else {
                number.to_string()
            }
        }
        NumericDisplayFormat::Short => format_trimmed_fixed(number, 4),
        NumericDisplayFormat::Long => format_trimmed_fixed(number, 15),
        NumericDisplayFormat::ShortE => format_scientific(number, 4),
        NumericDisplayFormat::LongE => format_scientific(number, 15),
        NumericDisplayFormat::ShortG => format_trimmed_fixed(number, 5),
        NumericDisplayFormat::LongG => format_trimmed_fixed(number, 15),
        NumericDisplayFormat::Bank => normalize_negative_zero(format!("{number:.2}")),
    }
}

fn format_trimmed_fixed(number: f64, precision: usize) -> String {
    normalize_negative_zero(
        format!("{number:.precision$}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string(),
    )
}

fn format_scientific(number: f64, precision: usize) -> String {
    if !number.is_finite() {
        return number.to_string();
    }
    let rendered = format!("{number:.precision$e}");
    let Some((mantissa, exponent)) = rendered.split_once('e') else {
        return rendered;
    };
    let exponent = exponent.parse::<i32>().unwrap_or(0);
    let sign = if exponent < 0 { '-' } else { '+' };
    let exponent_abs = exponent.abs();
    let exponent_text = if exponent_abs < 10 {
        format!("0{exponent_abs}")
    } else {
        exponent_abs.to_string()
    };
    format!(
        "{}e{}{exponent_text}",
        normalize_negative_zero(mantissa.to_string()),
        sign
    )
}

fn normalize_negative_zero(text: String) -> String {
    match text.as_str() {
        "-0" | "-0.0" | "-0.00" | "-0.000" | "-0.0000" | "-0.00000" | "-0.000000000000000" => {
            text.trim_start_matches('-').to_string()
        }
        _ => text,
    }
}

fn render_quoted_text_with_delimiter(text: &str, delimiter: char) -> String {
    let mut out = String::new();
    out.push(delimiter);
    for ch in text.chars() {
        if ch == delimiter {
            out.push(delimiter);
        }
        out.push(ch);
    }
    out.push(delimiter);
    out
}

fn matlab_named_output_uses_blank_separator(format: DisplayFormatState) -> bool {
    !matches!(format.spacing, Some(DisplaySpacingMode::Compact))
}

fn format_display_builtin_value(value: &Value, format: DisplayFormatState) -> String {
    match value {
        Value::CharArray(text) | Value::String(text) => text.clone(),
        _ => render_value_with_format(value, format),
    }
}

fn finite_scalar_value(value: &Value, builtin_name: &str) -> Result<f64, RuntimeError> {
    let number = value.as_scalar()?;
    if !number.is_finite() {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects a finite numeric scalar"
        )));
    }
    Ok(number)
}

fn set_warnings_enabled(shared_state: &Rc<RefCell<SharedRuntimeState>>, enabled: bool) {
    shared_state.borrow_mut().warnings_enabled = enabled;
}

fn warning_is_enabled(shared_state: &Rc<RefCell<SharedRuntimeState>>, identifier: &str) -> bool {
    let state = shared_state.borrow();
    if let Some(enabled) = warning_override_for(&state, identifier) {
        enabled
    } else {
        state.warnings_enabled
    }
}

fn set_warning_override(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    identifier: &str,
    enabled: bool,
) {
    shared_state
        .borrow_mut()
        .warning_overrides
        .insert(identifier.to_ascii_lowercase(), enabled);
}

fn clear_warning_overrides(shared_state: &Rc<RefCell<SharedRuntimeState>>) {
    shared_state.borrow_mut().warning_overrides.clear();
}

fn warning_override_for(state: &SharedRuntimeState, identifier: &str) -> Option<bool> {
    if identifier.is_empty() || identifier.eq_ignore_ascii_case("all") {
        None
    } else {
        state
            .warning_overrides
            .get(&identifier.to_ascii_lowercase())
            .copied()
    }
}

fn store_last_warning(shared_state: &Rc<RefCell<SharedRuntimeState>>, warning: WarningState) {
    shared_state.borrow_mut().last_warning =
        if warning.message.is_empty() && warning.identifier.is_empty() {
            None
        } else {
            Some(warning)
        };
}

fn render_warning_outputs(
    warning: &WarningState,
    output_arity: usize,
    builtin_name: &str,
) -> Result<Vec<Value>, RuntimeError> {
    match output_arity {
        0 => Ok(Vec::new()),
        1 => Ok(vec![Value::CharArray(warning.message.clone())]),
        2 => Ok(vec![
            Value::CharArray(warning.message.clone()),
            Value::CharArray(warning.identifier.clone()),
        ]),
        _ => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports at most two outputs"
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WarningControl {
    On { identifier: String },
    Off { identifier: String },
    Query { identifier: String },
}

fn parse_warning_control(args: &[Value]) -> Result<Option<WarningControl>, RuntimeError> {
    let Some(first) = args.first() else {
        return Ok(None);
    };
    let first_text = text_value(first)?.to_ascii_lowercase();
    let identifier = match args {
        [_] => "all".to_string(),
        [_, identifier] => text_value(identifier)?.to_string(),
        _ => return Ok(None),
    };

    let control = match first_text.as_str() {
        "on" => WarningControl::On { identifier },
        "off" => WarningControl::Off { identifier },
        "query" => WarningControl::Query { identifier },
        _ => return Ok(None),
    };
    Ok(Some(control))
}

fn apply_warning_control(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    control: WarningControl,
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    match control {
        WarningControl::On { identifier } => {
            let enabled = set_warning_control_state(shared_state, &identifier, true);
            render_warning_query_outputs(&identifier, enabled, output_arity, "warning")
        }
        WarningControl::Off { identifier } => {
            let enabled = set_warning_control_state(shared_state, &identifier, false);
            render_warning_query_outputs(&identifier, enabled, output_arity, "warning")
        }
        WarningControl::Query { identifier } => render_warning_query_outputs(
            &identifier,
            warning_is_enabled(shared_state, &identifier),
            output_arity,
            "warning",
        ),
    }
}

fn set_warning_control_state(
    shared_state: &Rc<RefCell<SharedRuntimeState>>,
    identifier: &str,
    enabled: bool,
) -> bool {
    if identifier.eq_ignore_ascii_case("all") {
        set_warnings_enabled(shared_state, enabled);
        clear_warning_overrides(shared_state);
        enabled
    } else {
        set_warning_override(shared_state, identifier, enabled);
        enabled
    }
}

fn render_warning_query_outputs(
    identifier: &str,
    enabled: bool,
    output_arity: usize,
    builtin_name: &str,
) -> Result<Vec<Value>, RuntimeError> {
    match output_arity {
        0 => Ok(Vec::new()),
        1 => Ok(vec![warning_query_value(identifier, enabled)]),
        _ => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} control/query forms currently support at most one output"
        ))),
    }
}

fn warning_query_value(identifier: &str, enabled: bool) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(
        "identifier".to_string(),
        Value::CharArray(identifier.to_string()),
    );
    fields.insert(
        "state".to_string(),
        Value::CharArray(if enabled { "on" } else { "off" }.to_string()),
    );
    Value::Struct(StructValue::from_fields(fields))
}

fn parse_warning_arguments(args: &[Value]) -> Result<WarningState, RuntimeError> {
    let (identifier, format, format_values) = match args {
        [] => {
            return Err(RuntimeError::Unsupported(
                "warning currently expects a message or identifier/message pair".to_string(),
            ))
        }
        [message] => (String::new(), text_value(message)?.to_string(), &[][..]),
        [first, rest @ ..] => {
            let first_text = text_value(first)?;
            if looks_like_warning_identifier(first_text) {
                let Some((message, values)) = rest.split_first() else {
                    return Err(RuntimeError::Unsupported(
                        "warning currently expects a message or format after the identifier"
                            .to_string(),
                    ));
                };
                (
                    first_text.to_string(),
                    text_value(message)?.to_string(),
                    values,
                )
            } else {
                (String::new(), first_text.to_string(), rest)
            }
        }
    };

    let message = format_text_with_values_for_builtin(&format, format_values, "warning")?;
    Ok(WarningState {
        message,
        identifier,
    })
}

fn runtime_error_value_with_stack_fallback(
    error: &RuntimeError,
    current_stack: &[RuntimeStackFrame],
    fill_empty_stack: bool,
) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(
        "cause".to_string(),
        runtime_error_cause_value(error.causes(), current_stack),
    );
    fields.insert(
        "identifier".to_string(),
        Value::String(error.identifier().to_string()),
    );
    fields.insert(
        "message".to_string(),
        Value::String(error.message().to_string()),
    );
    fields.insert(
        "stack".to_string(),
        runtime_error_stack_value(if fill_empty_stack && error.stack().is_empty() {
            current_stack
        } else {
            error.stack()
        }),
    );
    Value::Struct(StructValue::from_fields(fields))
}

fn empty_error_cause_value() -> Value {
    Value::Cell(CellValue {
        rows: 0,
        cols: 0,
        dims: vec![0, 0],
        elements: Vec::new(),
    })
}

fn runtime_error_cause_value(
    causes: &[RuntimeError],
    current_stack: &[RuntimeStackFrame],
) -> Value {
    if causes.is_empty() {
        return empty_error_cause_value();
    }

    Value::Cell(CellValue {
        rows: 1,
        cols: causes.len(),
        dims: vec![1, causes.len()],
        elements: causes
            .iter()
            .map(|cause| runtime_error_value_with_stack_fallback(cause, current_stack, false))
            .collect(),
    })
}

fn runtime_error_stack_value(stack: &[RuntimeStackFrame]) -> Value {
    Value::Matrix(MatrixValue {
        rows: 1,
        cols: stack.len(),
        dims: vec![1, stack.len()],
        elements: stack
            .iter()
            .rev()
            .map(|frame| {
                let mut fields = BTreeMap::new();
                fields.insert("file".to_string(), Value::String(frame.file.clone()));
                fields.insert("line".to_string(), Value::Scalar(frame.line as f64));
                fields.insert("name".to_string(), Value::String(frame.name.clone()));
                Value::Struct(StructValue::from_fields(fields))
            })
            .collect(),
    })
}

struct Interpreter<'a> {
    module: &'a HirModule,
    module_identity: String,
    module_functions: BTreeMap<String, &'a HirFunction>,
    anonymous_functions: HashMap<String, AnonymousClosure<'a>>,
    shared_state: Rc<RefCell<SharedRuntimeState>>,
    call_stack: Vec<RuntimeStackFrame>,
    next_anonymous_id: u32,
}

impl<'a> Interpreter<'a> {
    fn new(module: &'a HirModule) -> Self {
        Self::with_shared_state(
            module,
            "<root>".to_string(),
            Rc::new(RefCell::new(SharedRuntimeState::default())),
            Vec::new(),
        )
    }

    fn with_shared_state(
        module: &'a HirModule,
        module_identity: String,
        shared_state: Rc<RefCell<SharedRuntimeState>>,
        call_stack: Vec<RuntimeStackFrame>,
    ) -> Self {
        let module_functions = module
            .items
            .iter()
            .filter_map(|item| match item {
                HirItem::Function(function) => Some((function.name.clone(), function)),
                HirItem::Statement(_) => None,
            })
            .collect();

        Self {
            module,
            module_identity,
            module_functions,
            anonymous_functions: HashMap::new(),
            shared_state,
            call_stack,
            next_anonymous_id: 0,
        }
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
        let script_frame = self.make_stack_frame("<script>");
        self.with_stack_frame(script_frame, |this| {
            let mut frame = Frame::new(this.module_functions.clone());
            if let Some(ans) = &this.module.implicit_ans {
                frame.declare_binding(ans)?;
            }
            for item in &this.module.items {
                match item {
                    HirItem::Statement(statement) => {
                        if let Some(control) = this.execute_statement(&mut frame, statement)? {
                            return Err(RuntimeError::Unsupported(format!(
                                "top-level control flow `{}` is not supported",
                                control_name(control)
                            )));
                        }
                    }
                    HirItem::Function(_) => {}
                }
            }

            flush_figure_backend(&this.shared_state);
            Ok(ExecutionResult {
                workspace: frame.export_workspace()?,
                displayed_outputs: take_displayed_outputs(&this.shared_state),
                figures: rendered_figures(&this.shared_state.borrow().graphics),
                display_format: current_display_format(&this.shared_state),
            })
        })
    }

    fn execute_function_file(&mut self, args: &[Value]) -> Result<ExecutionResult, RuntimeError> {
        let workspace = self
            .invoke_primary_function(args)?
            .into_iter()
            .collect::<Workspace>();
        flush_figure_backend(&self.shared_state);
        Ok(ExecutionResult {
            workspace,
            displayed_outputs: take_displayed_outputs(&self.shared_state),
            figures: rendered_figures(&self.shared_state.borrow().graphics),
            display_format: current_display_format(&self.shared_state),
        })
    }

    fn invoke_primary_function(
        &mut self,
        args: &[Value],
    ) -> Result<Vec<(String, Value)>, RuntimeError> {
        let primary = self
            .module
            .items
            .iter()
            .find_map(|item| match item {
                HirItem::Function(function) => Some(function),
                HirItem::Statement(_) => None,
            })
            .ok_or_else(|| {
                RuntimeError::Unsupported(
                    "function file execution requires at least one function definition".to_string(),
                )
            })?;

        let output_values = self.invoke_function(primary, args, None)?;
        Ok(primary
            .outputs
            .iter()
            .zip(output_values)
            .map(|(binding, value)| (binding.name.clone(), value))
            .collect())
    }

    fn invoke_function(
        &mut self,
        function: &'a HirFunction,
        args: &[Value],
        caller: Option<&Frame<'a>>,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.invoke_function_with_prebound_outputs(function, args, caller, &[])
    }

    fn invoke_function_with_prebound_outputs(
        &mut self,
        function: &'a HirFunction,
        args: &[Value],
        caller: Option<&Frame<'a>>,
        prebound_outputs: &[(String, Value)],
    ) -> Result<Vec<Value>, RuntimeError> {
        let stack_frame = self.make_stack_frame(function.name.clone());
        self.with_stack_frame(stack_frame, |this| {
            if args.len() != function.inputs.len() {
                return Err(RuntimeError::Unsupported(format!(
                    "function `{}` expects {} input(s), got {}",
                    function.name,
                    function.inputs.len(),
                    args.len()
                )));
            }

            let mut visible_functions = this.module_functions.clone();
            if let Some(caller) = caller {
                visible_functions.extend(caller.visible_functions.clone());
            }
            for local in &function.local_functions {
                visible_functions.insert(local.name.clone(), local);
            }

            let mut frame = Frame::new(visible_functions);
            if let Some(caller) = caller {
                frame.inherit_hidden_cells_from(caller);
                for capture in &function.captures {
                    let cell = caller.cell(capture.binding_id).ok_or_else(|| {
                        RuntimeError::MissingVariable(format!(
                            "captured binding `{}` is not available when calling `{}`",
                            capture.name, function.name
                        ))
                    })?;
                    frame.bind_existing(capture.binding_id, &capture.name, cell);
                }
            }

            for (binding, value) in function.inputs.iter().zip(args.iter()) {
                frame.assign_binding(binding, value.clone())?;
            }
            if let Some(ans) = &function.implicit_ans {
                frame.declare_binding(ans)?;
            }
            for output in &function.outputs {
                frame.declare_binding(output)?;
            }
            for (name, value) in prebound_outputs {
                if let Some(binding) = function.outputs.iter().find(|binding| binding.name == *name) {
                    frame.assign_binding(binding, value.clone())?;
                }
            }

            let _ = this.execute_block(&mut frame, &function.body)?;

            function
                .outputs
                .iter()
                .map(|binding| frame.read_binding(binding))
                .collect()
        })
    }

    fn find_module_function<'b>(
        &'b self,
        module: &'b HirModule,
        name: &str,
    ) -> Option<&'b HirFunction> {
        module.items.iter().find_map(|item| match item {
            HirItem::Function(function) if function.name == name => Some(function),
            HirItem::Statement(_) => None,
            HirItem::Function(_) => None,
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
        let inline_methods = class.inline_methods.iter().cloned().collect::<BTreeSet<_>>();
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
                inline_methods,
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
        let loaded = load_class_module_from_path(path)?;
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
            let loaded = load_class_module_from_path(source_path)?;
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

    fn invoke_anonymous(
        &mut self,
        closure: AnonymousClosure<'a>,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let stack_frame = self.make_stack_frame("@anonymous");
        self.with_stack_frame(stack_frame, |this| {
            if args.len() != closure.function.params.len() {
                return Err(RuntimeError::Unsupported(format!(
                    "anonymous function expects {} input(s), got {}",
                    closure.function.params.len(),
                    args.len()
                )));
            }

            let mut frame = Frame::new(closure.visible_functions.clone());
            for (binding_id, cell) in closure.captured_cells {
                frame.bind_existing(binding_id, "<capture>", cell);
            }
            for (param, value) in closure.function.params.iter().zip(args.iter()) {
                frame.assign_binding(param, value.clone())?;
            }

            this.evaluate_expression(&mut frame, &closure.function.body)
        })
    }

    fn execute_statement_builtin_call(
        &mut self,
        frame: &mut Frame<'a>,
        target: &HirCallTarget,
        args: &[HirIndexArgument],
    ) -> Result<(), RuntimeError> {
        let HirCallTarget::Callable(reference) = target else {
            return Err(RuntimeError::Unsupported(
                "statement builtin dispatch expects a callable target".to_string(),
            ));
        };
        let evaluated_args = self.evaluate_function_arguments(frame, args)?;
        if reference.name == "fplot" {
            self.invoke_fplot_builtin_outputs(frame, &evaluated_args, 0)
                .map(|_| ())?;
            return Ok(());
        }
        if reference.name == "fplot3" {
            self.invoke_fplot3_builtin_outputs(frame, &evaluated_args, 0)
                .map(|_| ())?;
            return Ok(());
        }
        if reference.name == "fsurf" {
            self.invoke_fsurf_builtin_outputs(frame, &evaluated_args, 0)
                .map(|_| ())?;
            return Ok(());
        }
        if reference.name == "fmesh" {
            self.invoke_fmesh_builtin_outputs(frame, &evaluated_args, 0)
                .map(|_| ())?;
            return Ok(());
        }
        if reference.name == "fimplicit" {
            self.invoke_fimplicit_builtin_outputs(frame, &evaluated_args, 0)
                .map(|_| ())?;
            return Ok(());
        }
        if reference.name == "fcontour" {
            self.invoke_fcontour_builtin_outputs(frame, &evaluated_args, 0)
                .map(|_| ())?;
            return Ok(());
        }
        if reference.name == "fcontour3" {
            self.invoke_fcontour3_builtin_outputs(frame, &evaluated_args, 0)
                .map(|_| ())?;
            return Ok(());
        }
        if matches!(
            reference.name.as_str(),
            "clear" | "clearvars" | "save" | "load"
        ) {
            let shared_state = Rc::clone(&self.shared_state);
            self.invoke_workspace_builtin_outputs(
                frame,
                &shared_state,
                &reference.name,
                &evaluated_args,
                0,
            )
            .map(|_| ())?;
            return Ok(());
        }
        invoke_runtime_builtin_outputs(
            &self.shared_state,
            Some(frame),
            &reference.name,
            &evaluated_args,
            0,
        )
        .map(|_| ())
    }

    fn execute_statement(
        &mut self,
        frame: &mut Frame<'a>,
        statement: &HirStatement,
    ) -> Result<Option<ControlFlow>, RuntimeError> {
        let result = match statement {
            HirStatement::Assignment {
                targets,
                value,
                list_assignment,
                display_suppressed,
            } => {
                if let Some(field) =
                    self.unsupported_multi_struct_field_subindex_target(frame, targets)?
                {
                    return Err(unsupported_multi_struct_field_subindexing_error(&field));
                }
                if !*list_assignment
                    && matches!(
                        targets.as_slice(),
                        [HirAssignmentTarget::Field { target, .. }]
                            if field_target_requires_list_assignment(target)
                    )
                {
                    if let Some(count) =
                        self.simple_struct_field_assignment_target_count(frame, targets)?
                    {
                        return Err(unsupported_simple_csl_assignment_error(count));
                    }
                }
                if self.try_execute_undefined_root_colon_brace_csl_assignment(
                    frame, targets, value,
                )? {
                    return Ok(None);
                }
                if self.try_execute_undefined_cell_receiver_struct_field_brace_csl_assignment(
                    frame, targets, value,
                )? {
                    return Ok(None);
                }
                if self.try_execute_undefined_cell_struct_field_csl_assignment(
                    frame, targets, value,
                )? {
                    return Ok(None);
                }
                if self.try_execute_undefined_struct_field_brace_csl_assignment(
                    frame, targets, value,
                )? {
                    return Ok(None);
                }
                if self.try_execute_undefined_indexed_struct_field_brace_csl_assignment(
                    frame, targets, value,
                )? {
                    return Ok(None);
                }
                if *list_assignment
                    && self.try_execute_undefined_scalar_struct_field_csl_assignment(
                        frame, targets, value,
                    )?
                {
                    return Ok(None);
                }
                if self.try_execute_undefined_indexed_struct_field_csl_assignment(
                    frame, targets, value,
                )? {
                    return Ok(None);
                }
                let values = self.evaluate_assignment_values(
                    frame,
                    targets,
                    value,
                    *list_assignment,
                )?;
                if !display_suppressed && targets.len() == 1 {
                    if let HirAssignmentTarget::Binding(binding) = &targets[0] {
                        if let Some(display_value) = values.first().cloned() {
                            push_named_displayed_output(
                                &self.shared_state,
                                binding.name.clone(),
                                display_value,
                            );
                        }
                    }
                }
                for (target, assigned_value) in targets.iter().zip(values.into_iter()) {
                    self.assign_target(frame, target, assigned_value)?;
                }
                Ok(None)
            }
            HirStatement::Expression {
                expression,
                display_suppressed,
            } => {
                if let Some(value) = first_statement_value(self, frame, expression)? {
                    if let Some(ans) = frame.implicit_binding("ans") {
                        frame.assign_binding(&ans, value.clone())?;
                    }
                    if !display_suppressed {
                        push_named_displayed_output(&self.shared_state, "ans".to_string(), value);
                    }
                }
                Ok(None)
            }
            HirStatement::If {
                branches,
                else_body,
            } => {
                for HirConditionalBranch { condition, body } in branches {
                    if self.evaluate_expression(frame, condition)?.truthy()? {
                        return self.execute_block(frame, body);
                    }
                }
                self.execute_block(frame, else_body)
            }
            HirStatement::Switch {
                expression,
                cases,
                otherwise_body,
            } => {
                let switch_value = self.evaluate_expression(frame, expression)?;
                for HirSwitchCase { matcher, body } in cases {
                    if values_equal(&switch_value, &self.evaluate_expression(frame, matcher)?)? {
                        return self.execute_block(frame, body);
                    }
                }
                self.execute_block(frame, otherwise_body)
            }
            HirStatement::Try {
                body,
                catch_binding,
                catch_body,
            } => match self.execute_block(frame, body) {
                Ok(control) => Ok(control),
                Err(error) => {
                    if let Some(binding) = catch_binding {
                        self.bind_storage_binding(frame, binding)?;
                        frame.assign_binding(
                            binding,
                            runtime_error_value(&error, &self.call_stack),
                        )?;
                    }
                    self.execute_block(frame, catch_body)
                }
            },
            HirStatement::For {
                variable,
                iterable,
                body,
            } => {
                let values = iteration_values(&self.evaluate_expression(frame, iterable)?)?;
                for value in values {
                    frame.assign_binding(variable, value)?;
                    if let Some(control) = self.execute_block(frame, body)? {
                        match control {
                            ControlFlow::Continue => continue,
                            ControlFlow::Break => break,
                            ControlFlow::Return => return Ok(Some(ControlFlow::Return)),
                        }
                    }
                }
                Ok(None)
            }
            HirStatement::While { condition, body } => {
                while self.evaluate_expression(frame, condition)?.truthy()? {
                    if let Some(control) = self.execute_block(frame, body)? {
                        match control {
                            ControlFlow::Continue => continue,
                            ControlFlow::Break => break,
                            ControlFlow::Return => return Ok(Some(ControlFlow::Return)),
                        }
                    }
                }
                Ok(None)
            }
            HirStatement::Break => Ok(Some(ControlFlow::Break)),
            HirStatement::Continue => Ok(Some(ControlFlow::Continue)),
            HirStatement::Return => Ok(Some(ControlFlow::Return)),
            HirStatement::Global(bindings) => {
                for binding in bindings {
                    self.bind_storage_binding(frame, binding)?;
                }
                Ok(None)
            }
            HirStatement::Persistent(bindings) => {
                for binding in bindings {
                    self.bind_storage_binding(frame, binding)?;
                }
                Ok(None)
            }
        };
        if result.is_ok() {
            self.drain_pending_host_figure_events(frame)?;
        }
        result
    }

    fn try_execute_undefined_root_colon_brace_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
    ) -> Result<bool, RuntimeError> {
        let [HirAssignmentTarget::CellIndex { target, indices }] = targets else {
            return Ok(false);
        };
        if !indices
            .iter()
            .any(|argument| matches!(argument, HirIndexArgument::FullSlice))
        {
            return Ok(false);
        }

        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        if cell.borrow().is_some() || !projections.is_empty() {
            return Ok(false);
        }

        let values = match value {
            HirExpression::Call {
                target: HirCallTarget::Callable(reference),
                args,
            } if reference.name == "deal" => {
                self.evaluate_call_outputs(
                    frame,
                    &HirCallTarget::Callable(reference.clone()),
                    args,
                    Some(args.len().max(1)),
                )?
            }
            _ if expression_supports_list_expansion(value) => {
                self.evaluate_expression_outputs(frame, value)?
            }
            _ => {
                let direct = self.evaluate_expression(frame, value)?;
                let Some(values) = direct_multi_value_values_ordered(&direct)? else {
                    return Ok(false);
                };
                values
            }
        };

        if values.is_empty() {
            return Ok(false);
        }

        let Some(materialized) =
            self.materialize_undefined_root_brace_csl_assignment(frame, indices, values)?
        else {
            return Ok(false);
        };
        *cell.borrow_mut() = Some(Value::Cell(materialized));
        Ok(true)
    }

    fn materialize_undefined_root_brace_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        indices: &[HirIndexArgument],
        values: Vec<Value>,
    ) -> Result<Option<CellValue>, RuntimeError> {
        let output_count = values.len();
        if output_count == 0 {
            return Ok(None);
        }
        if indices.iter().any(|argument| match argument {
            HirIndexArgument::FullSlice => false,
            HirIndexArgument::End => true,
            HirIndexArgument::Expression(expression) => expression_contains_end_keyword(expression),
        }) {
            return Ok(None);
        }

        let full_slice_axes = indices
            .iter()
            .enumerate()
            .filter_map(|(axis, argument)| {
                matches!(argument, HirIndexArgument::FullSlice).then_some(axis)
            })
            .collect::<Vec<_>>();
        if full_slice_axes.len() > 1 {
            return Ok(None);
        }

        let empty = empty_cell_value();
        let Value::Cell(empty_cell) = &empty else {
            unreachable!("empty cell helper should return a cell value");
        };
        let evaluated_on_empty = indices
            .iter()
            .enumerate()
            .map(|(axis, argument)| {
                self.evaluate_index_argument(frame, &empty, argument, axis, indices.len())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let target_dims = if full_slice_axes.is_empty() {
            let plan = cell_assignment_plan(empty_cell, &evaluated_on_empty)?;
            if plan.selection.positions.len() != output_count {
                return Ok(None);
            }
            plan.target_dims
        } else {
            let effective_dims = indexing_dimensions_from_dims(empty_cell.dims(), indices.len());
            let mut target_dims = effective_dims.clone();
            let mut known_selection_product = 1usize;
            for (axis, argument) in indices.iter().enumerate() {
                if matches!(argument, HirIndexArgument::FullSlice) {
                    continue;
                }
                let label = format!("dimension {}", axis + 1);
                let (selected, target_extent) = assignment_dimension_indices(
                    &evaluated_on_empty[axis],
                    effective_dims[axis],
                    &label,
                    "cell array",
                )?;
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
            if indices.len() == 1 {
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
        let evaluated_indices = self.evaluate_index_arguments(frame, &current_value, indices)?;
        let selection = cell_selection(&current, &evaluated_indices)?;
        if selection.positions.len() != output_count {
            return Ok(None);
        }
        let rhs = Value::Cell(CellValue::with_dimensions(
            selection.rows,
            selection.cols,
            selection.dims.clone(),
            values,
        )?);
        Ok(Some(assign_cell_content_index(
            current,
            &evaluated_indices,
            rhs,
        )?))
    }

    fn try_execute_undefined_scalar_struct_field_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
    ) -> Result<bool, RuntimeError> {
        let [HirAssignmentTarget::Field { target, field }] = targets else {
            return Ok(false);
        };

        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        if cell.borrow().is_some()
            || projections
                .iter()
                .any(|projection| !matches!(projection, LValueProjection::Field(_)))
        {
            return Ok(false);
        }

        let values = match value {
            HirExpression::Call {
                target: HirCallTarget::Callable(reference),
                args,
            } if reference.name == "deal" => {
                self.evaluate_call_outputs(
                    frame,
                    &HirCallTarget::Callable(reference.clone()),
                    args,
                    Some(args.len()),
                )?
            }
            _ if expression_supports_list_expansion(value) => self.evaluate_expression_outputs(frame, value)?,
            _ => {
                let direct = self.evaluate_expression(frame, value)?;
                let Some(values) = direct_multi_value_values_ordered(&direct)? else {
                    return Ok(false);
                };
                values
            }
        };

        if values.is_empty() {
            return Ok(false);
        }

        let nested = if values.len() == 1 {
            assign_struct_path(
                Value::Struct(StructValue::default()),
                std::slice::from_ref(field),
                values.into_iter().next().expect("single scalar struct assignment value"),
            )?
        } else {
            let assigned_values = Value::Matrix(MatrixValue::with_dimensions(
                1,
                values.len(),
                vec![1, values.len()],
                values,
            )?);
            assign_struct_path(
                empty_struct_assignment_target_row(values_len(&assigned_values)?)?,
                std::slice::from_ref(field),
                assigned_values,
            )?
        };
        let updated = if projections.is_empty() {
            nested
        } else {
            assign_struct_path(
                Value::Struct(StructValue::default()),
                &projections
                    .iter()
                    .map(|projection| match projection {
                        LValueProjection::Field(field) => field.clone(),
                        _ => unreachable!("guard restricted to field projections"),
                    })
                    .collect::<Vec<_>>(),
                nested,
            )?
        };
        *cell.borrow_mut() = Some(updated);
        Ok(true)
    }

    fn try_execute_undefined_indexed_struct_field_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
    ) -> Result<bool, RuntimeError> {
        let [HirAssignmentTarget::Field { target, field }] = targets else {
            return Ok(false);
        };

        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        let mut prefix_fields = Vec::new();
        let mut paren_index = None;
        for (index, projection) in projections.iter().enumerate() {
            match projection {
                LValueProjection::Field(field) if paren_index.is_none() => {
                    prefix_fields.push(field.clone());
                }
                LValueProjection::Paren(_) if paren_index.is_none() => paren_index = Some(index),
                _ => {}
            }
        }
        let Some(paren_index) = paren_index else {
            return Ok(false);
        };
        let LValueProjection::Paren(receiver_indices) = &projections[paren_index] else {
            unreachable!("paren index identified a paren projection");
        };
        let field_projections = &projections[paren_index + 1..];
        if cell.borrow().is_some()
            || field_projections
                .iter()
                .any(|projection| !matches!(projection, LValueProjection::Field(_)))
            || projections.iter().any(|projection| {
                !matches!(projection, LValueProjection::Field(_) | LValueProjection::Paren(_))
            })
        {
            return Ok(false);
        }

        let (output_count, assigned) = match value {
            HirExpression::Call {
                target: HirCallTarget::Callable(reference),
                args,
            } if reference.name == "deal" => {
                let values = self.evaluate_call_outputs(
                    frame,
                    &HirCallTarget::Callable(reference.clone()),
                    args,
                    Some(args.len().max(1)),
                )?;
                if values.is_empty() {
                    return Ok(false);
                }
                (
                    values.len(),
                    Value::Matrix(MatrixValue::with_dimensions(
                        1,
                        values.len(),
                        vec![1, values.len()],
                        values,
                    )?),
                )
            }
            _ if expression_supports_list_expansion(value) => {
                let values = self.evaluate_expression_outputs(frame, value)?;
                if values.is_empty() {
                    return Ok(false);
                }
                (
                    values.len(),
                    Value::Matrix(MatrixValue::with_dimensions(
                        1,
                        values.len(),
                        vec![1, values.len()],
                        values,
                    )?),
                )
            }
            _ => {
                let direct = self.evaluate_expression(frame, value)?;
                let Some(count) = direct_multi_value_count(&direct) else {
                    return Ok(false);
                };
                (count, direct)
            }
        };

        let Some(receivers) = self.materialize_undefined_indexed_struct_receivers(
            frame,
            receiver_indices,
            output_count,
        )? else {
            return Ok(false);
        };
        let receiver_count = match &receivers {
            Value::Struct(_) => 1,
            Value::Matrix(matrix) if matrix_is_struct_array(matrix) => matrix.elements.len(),
            _ => return Ok(false),
        };
        if receiver_count != output_count {
            return Ok(false);
        }

        let mut field_path = field_projections
            .iter()
            .map(|projection| match projection {
                LValueProjection::Field(field) => field.clone(),
                _ => unreachable!("guard restricted to field projections"),
            })
            .collect::<Vec<_>>();
        field_path.push(field.clone());
        let mut updated = assign_struct_path(receivers, &field_path, assigned)?;
        if !prefix_fields.is_empty() {
            updated =
                assign_struct_path(Value::Struct(StructValue::default()), &prefix_fields, updated)?;
        }
        *cell.borrow_mut() = Some(updated);
        Ok(true)
    }

    fn materialize_undefined_indexed_struct_receivers(
        &mut self,
        frame: &mut Frame<'a>,
        receiver_indices: &[HirIndexArgument],
        output_count: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        if output_count == 0 {
            return Ok(None);
        }

        let receiver_target = empty_matrix_value();
        let Value::Matrix(empty_receiver) = &receiver_target else {
            unreachable!("empty matrix helper should return a matrix value");
        };
        let evaluated_receiver_indices =
            self.evaluate_index_arguments(frame, &receiver_target, receiver_indices)?;
        if let Some(receivers) =
            default_struct_selection_value_for_index_update(&receiver_target, &evaluated_receiver_indices)?
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

        if receiver_indices
            .iter()
            .any(index_argument_contains_end_keyword)
        {
            return Ok(None);
        }

        let full_slice_axes = receiver_indices
            .iter()
            .enumerate()
            .filter_map(|(axis, argument)| {
                matches!(argument, HirIndexArgument::FullSlice).then_some(axis)
            })
            .collect::<Vec<_>>();
        if full_slice_axes.len() != 1 {
            return Ok(None);
        }

        let effective_dims = indexing_dimensions_from_dims(empty_receiver.dims(), receiver_indices.len());
        let mut target_dims = effective_dims.clone();
        let mut known_selection_product = 1usize;
        for (axis, argument) in evaluated_receiver_indices.iter().enumerate() {
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
        if receiver_indices.len() == 1 {
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

    fn try_execute_undefined_cell_struct_field_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
    ) -> Result<bool, RuntimeError> {
        let [HirAssignmentTarget::Field { target, field }] = targets else {
            return Ok(false);
        };

        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        let mut prefix_fields = Vec::new();
        let mut brace_index = None;
        for (index, projection) in projections.iter().enumerate() {
            match projection {
                LValueProjection::Field(field) if brace_index.is_none() => {
                    prefix_fields.push(field.clone());
                }
                LValueProjection::Brace(_) if brace_index.is_none() => brace_index = Some(index),
                _ => {}
            }
        }
        let Some(brace_index) = brace_index else {
            return Ok(false);
        };
        let LValueProjection::Brace(receiver_indices) = &projections[brace_index] else {
            unreachable!("brace index identified a brace projection");
        };
        let field_projections = &projections[brace_index + 1..];
        if cell.borrow().is_some()
            || projections.iter().any(|projection| {
                !matches!(projection, LValueProjection::Field(_) | LValueProjection::Brace(_))
            })
            || field_projections
                .iter()
                .any(|projection| !matches!(projection, LValueProjection::Field(_)))
        {
            return Ok(false);
        }

        let (receiver_count, assigned_value) = match value {
            HirExpression::Call {
                target: HirCallTarget::Callable(reference),
                args,
            } if reference.name == "deal" => {
                let values = self.evaluate_call_outputs(
                    frame,
                    &HirCallTarget::Callable(reference.clone()),
                    args,
                    Some(args.len().max(1)),
                )?;
                if values.is_empty() {
                    return Ok(false);
                }
                (
                    values.len(),
                    Value::Matrix(MatrixValue::with_dimensions(
                        1,
                        values.len(),
                        vec![1, values.len()],
                        values,
                    )?),
                )
            }
            _ if expression_supports_list_expansion(value) => {
                let values = self.evaluate_expression_outputs(frame, value)?;
                if values.is_empty() {
                    return Ok(false);
                }
                (
                    values.len(),
                    Value::Matrix(MatrixValue::with_dimensions(
                        1,
                        values.len(),
                        vec![1, values.len()],
                        values,
                    )?),
                )
            }
            _ => {
                let direct = self.evaluate_expression(frame, value)?;
                let Some(count) = direct_multi_value_count(&direct) else {
                    return Ok(false);
                };
                (count, direct)
            }
        };
        if receiver_count == 0 {
            return Ok(false);
        }

        let receiver_defaults = vec![Value::Struct(StructValue::default()); receiver_count];
        let Some(receivers) = self.materialize_undefined_root_brace_csl_assignment(
            frame,
            receiver_indices,
            receiver_defaults,
        )? else {
            return Ok(false);
        };

        let mut field_path = field_projections
            .iter()
            .map(|projection| match projection {
                LValueProjection::Field(field) => field.clone(),
                _ => unreachable!("guard restricted to field projections"),
            })
            .collect::<Vec<_>>();
        field_path.push(field.clone());
        let mut updated = assign_struct_path(
            Value::Cell(receivers),
            &field_path,
            assigned_value,
        )?;
        if !prefix_fields.is_empty() {
            updated = assign_struct_path(Value::Struct(StructValue::default()), &prefix_fields, updated)?;
        }
        *cell.borrow_mut() = Some(updated);
        Ok(true)
    }

    fn try_execute_undefined_cell_receiver_struct_field_brace_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
    ) -> Result<bool, RuntimeError> {
        let [HirAssignmentTarget::CellIndex { target, indices }] = targets else {
            return Ok(false);
        };

        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        let mut prefix_fields = Vec::new();
        let mut brace_index = None;
        for (index, projection) in projections.iter().enumerate() {
            match projection {
                LValueProjection::Field(field) if brace_index.is_none() => {
                    prefix_fields.push(field.clone());
                }
                LValueProjection::Brace(_) if brace_index.is_none() => brace_index = Some(index),
                _ => {}
            }
        }
        let Some(brace_index) = brace_index else {
            return Ok(false);
        };
        let LValueProjection::Brace(receiver_indices) = &projections[brace_index] else {
            unreachable!("brace index identified a brace projection");
        };
        let field_projections = &projections[brace_index + 1..];
        if cell.borrow().is_some()
            || field_projections.is_empty()
            || projections.iter().any(|projection| {
                !matches!(projection, LValueProjection::Field(_) | LValueProjection::Brace(_))
            })
            || field_projections
                .iter()
                .any(|projection| !matches!(projection, LValueProjection::Field(_)))
            || receiver_indices
                .iter()
                .any(|argument| matches!(argument, HirIndexArgument::End))
        {
            return Ok(false);
        }

        let values = match value {
            HirExpression::Call {
                target: HirCallTarget::Callable(reference),
                args,
            } if reference.name == "deal" => {
                self.evaluate_call_outputs(
                    frame,
                    &HirCallTarget::Callable(reference.clone()),
                    args,
                    Some(args.len().max(1)),
                )?
            }
            _ if expression_supports_list_expansion(value) => {
                self.evaluate_expression_outputs(frame, value)?
            }
            _ => {
                let direct = self.evaluate_expression(frame, value)?;
                let Some(values) = direct_multi_value_values_ordered(&direct)? else {
                    return Ok(false);
                };
                values
            }
        };
        if values.is_empty() {
            return Ok(false);
        }

        let empty = empty_cell_value();
        let Value::Cell(empty_cell) = &empty else {
            unreachable!("empty cell helper should return a cell value");
        };
        let evaluated_receiver_indices =
            self.evaluate_index_arguments(frame, &empty, receiver_indices)?;
        let receiver_plan = cell_assignment_plan(empty_cell, &evaluated_receiver_indices)?;
        let receiver_count = if receiver_plan.selection.positions.is_empty() {
            match value {
                HirExpression::Call { .. } if expression_supports_list_expansion(value) => 0,
                HirExpression::Call { .. } => 0,
                _ => infer_missing_root_receiver_count_from_value(
                    receiver_indices,
                    &self.evaluate_expression(frame, value)?,
                )
                .unwrap_or(0),
            }
        } else {
            receiver_plan.selection.positions.len()
        };
        if receiver_count == 0 || values.len() % receiver_count != 0 {
            return Ok(false);
        }

        let receiver_defaults = vec![Value::Struct(StructValue::default()); receiver_count];
        let Some(receivers) = self.materialize_undefined_root_brace_csl_assignment(
            frame,
            receiver_indices,
            receiver_defaults,
        )? else {
            return Ok(false);
        };

        let field_path = field_projections
            .iter()
            .map(|projection| match projection {
                LValueProjection::Field(field) => field.clone(),
                _ => unreachable!("guard restricted to field projections"),
            })
            .collect::<Vec<_>>();
        let chunk_len = values.len() / receiver_count;
        let mut chunks = values.chunks(chunk_len);
        let mut elements = Vec::with_capacity(receivers.elements.len());
        for element in receivers.elements {
            let chunk = chunks.next().expect("chunk per cell receiver").to_vec();
            let Some(materialized) =
                self.materialize_undefined_root_brace_csl_assignment(frame, indices, chunk)?
            else {
                return Ok(false);
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
        *cell.borrow_mut() = Some(updated);
        Ok(true)
    }

    fn try_execute_undefined_struct_field_brace_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
    ) -> Result<bool, RuntimeError> {
        let [HirAssignmentTarget::CellIndex { target, indices }] = targets else {
            return Ok(false);
        };

        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        if cell.borrow().is_some()
            || projections
                .iter()
                .any(|projection| !matches!(projection, LValueProjection::Field(_)))
        {
            return Ok(false);
        }

        let values = match value {
            HirExpression::Call {
                target: HirCallTarget::Callable(reference),
                args,
            } if reference.name == "deal" => {
                self.evaluate_call_outputs(
                    frame,
                    &HirCallTarget::Callable(reference.clone()),
                    args,
                    Some(args.len().max(1)),
                )?
            }
            _ if expression_supports_list_expansion(value) => self.evaluate_expression_outputs(frame, value)?,
            _ => {
                let direct = self.evaluate_expression(frame, value)?;
                let Some(values) = direct_multi_value_values_ordered(&direct)? else {
                    return Ok(false);
                };
                values
            }
        };
        if values.is_empty() {
            return Ok(false);
        }

        let Some(materialized) =
            self.materialize_undefined_root_brace_csl_assignment(frame, indices, values)?
        else {
            return Ok(false);
        };
        let updated = assign_struct_path(
            Value::Struct(StructValue::default()),
            &projections
                .iter()
                .map(|projection| match projection {
                    LValueProjection::Field(field) => field.clone(),
                    _ => unreachable!("guard restricted to field projections"),
                })
                .collect::<Vec<_>>(),
            Value::Cell(materialized),
        )?;
        *cell.borrow_mut() = Some(updated);
        Ok(true)
    }

    fn try_execute_undefined_indexed_struct_field_brace_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
    ) -> Result<bool, RuntimeError> {
        let [HirAssignmentTarget::CellIndex { target, indices }] = targets else {
            return Ok(false);
        };

        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        let mut prefix_fields = Vec::new();
        let mut paren_index = None;
        for (index, projection) in projections.iter().enumerate() {
            match projection {
                LValueProjection::Field(field) if paren_index.is_none() => {
                    prefix_fields.push(field.clone());
                }
                LValueProjection::Paren(_) if paren_index.is_none() => paren_index = Some(index),
                _ => {}
            }
        }
        let Some(paren_index) = paren_index else {
            return Ok(false);
        };
        let LValueProjection::Paren(receiver_indices) = &projections[paren_index] else {
            unreachable!("paren index identified a paren projection");
        };
        let field_projections = &projections[paren_index + 1..];
        if cell.borrow().is_some()
            || field_projections.is_empty()
            || projections
                .iter()
                .any(|projection| !matches!(projection, LValueProjection::Field(_) | LValueProjection::Paren(_)))
            || field_projections
                .iter()
                .any(|projection| !matches!(projection, LValueProjection::Field(_)))
        {
            return Ok(false);
        }

        let values = match value {
            HirExpression::Call {
                target: HirCallTarget::Callable(reference),
                args,
            } if reference.name == "deal" => {
                self.evaluate_call_outputs(
                    frame,
                    &HirCallTarget::Callable(reference.clone()),
                    args,
                    Some(args.len().max(1)),
                )?
            }
            _ if expression_supports_list_expansion(value) => {
                self.evaluate_expression_outputs(frame, value)?
            }
            _ => {
                let direct = self.evaluate_expression(frame, value)?;
                let Some(values) = direct_multi_value_values_ordered(&direct)? else {
                    return Ok(false);
                };
                values
            }
        };
        if values.is_empty() {
            return Ok(false);
        }

        let Some(receivers) = self.materialize_undefined_indexed_struct_brace_receivers(
            frame,
            receiver_indices,
            indices,
            values.len(),
        )? else {
            return Ok(false);
        };
        let updated = self.materialize_undefined_indexed_struct_field_brace_csl_assignment(
            frame,
            receivers,
            field_projections,
            indices,
            values,
        )?;
        let Some(mut updated) = updated else {
            return Ok(false);
        };
        if !prefix_fields.is_empty() {
            updated = assign_struct_path(Value::Struct(StructValue::default()), &prefix_fields, updated)?;
        }
        *cell.borrow_mut() = Some(updated);
        Ok(true)
    }

    fn materialize_undefined_indexed_struct_brace_receivers(
        &mut self,
        frame: &mut Frame<'a>,
        receiver_indices: &[HirIndexArgument],
        indices: &[HirIndexArgument],
        output_count: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        let receiver_target = empty_matrix_value();
        let evaluated_receiver_indices =
            self.evaluate_index_arguments(frame, &receiver_target, receiver_indices)?;
        if let Some(receivers) = default_struct_selection_value_for_index_update(
            &receiver_target,
            &evaluated_receiver_indices,
        )? {
            let receiver_count = match &receivers {
                Value::Struct(_) => 1,
                Value::Matrix(matrix) if matrix_is_struct_array(matrix) => matrix.elements.len(),
                _ => 0,
            };
            if receiver_count > 0 {
                return Ok(Some(receivers));
            }
        }

        let Some(receiver_count) = self.infer_missing_root_indexed_struct_brace_receiver_count(
            frame,
            indices,
            output_count,
        )? else {
            return Ok(None);
        };
        self.materialize_undefined_indexed_struct_receivers(frame, receiver_indices, receiver_count)
    }

    fn infer_missing_root_indexed_struct_brace_receiver_count(
        &mut self,
        frame: &mut Frame<'a>,
        indices: &[HirIndexArgument],
        output_count: usize,
    ) -> Result<Option<usize>, RuntimeError> {
        if output_count == 0 || indices.iter().any(index_argument_contains_end_keyword) {
            return Ok(None);
        }

        let empty = empty_cell_value();
        let Value::Cell(empty_cell) = &empty else {
            unreachable!("empty cell helper should return a cell value");
        };
        let evaluated_indices = self.evaluate_index_arguments(frame, &empty, indices)?;
        if evaluated_indices
            .iter()
            .any(|argument| matches!(argument, EvaluatedIndexArgument::FullSlice))
        {
            return Ok(None);
        }

        let plan = cell_assignment_plan(empty_cell, &evaluated_indices)?;
        let per_receiver_count = plan.selection.positions.len();
        if per_receiver_count == 0 || output_count % per_receiver_count != 0 {
            return Ok(None);
        }
        Ok(Some(output_count / per_receiver_count))
    }

    fn materialize_undefined_indexed_struct_field_brace_csl_assignment(
        &mut self,
        frame: &mut Frame<'a>,
        receivers: Value,
        field_projections: &[LValueProjection],
        indices: &[HirIndexArgument],
        values: Vec<Value>,
    ) -> Result<Option<Value>, RuntimeError> {
        let receiver_count = match &receivers {
            Value::Struct(_) => 1,
            Value::Matrix(matrix) if matrix_is_struct_array(matrix) => matrix.elements.len(),
            _ => return Ok(None),
        };
        if receiver_count == 0 || values.len() % receiver_count != 0 {
            return Ok(None);
        }

        let chunk_len = values.len() / receiver_count;
        let field_path = field_projections
            .iter()
            .map(|projection| match projection {
                LValueProjection::Field(field) => field.clone(),
                _ => unreachable!("guard restricted to field projections"),
            })
            .collect::<Vec<_>>();

        let mut chunks = values.chunks(chunk_len);
        match receivers {
            Value::Struct(receiver) => {
                let chunk = chunks.next().expect("single receiver chunk").to_vec();
                let Some(materialized) =
                    self.materialize_undefined_root_brace_csl_assignment(frame, indices, chunk)?
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
                    let Some(materialized) =
                        self.materialize_undefined_root_brace_csl_assignment(frame, indices, chunk)?
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
        }
    }

    fn evaluate_assignment_values(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
        value: &HirExpression,
        list_assignment: bool,
    ) -> Result<Vec<Value>, RuntimeError> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }

        if targets.len() == 1 {
            if let HirAssignmentTarget::CellIndex { target, indices } = &targets[0] {
                let (_name, cell, projections) = frame.lvalue_root(target)?;
                if cell.borrow().is_none()
                    && undefined_brace_assignment_with_colon(
                        &projections,
                        &LValueLeaf::Index {
                            kind: IndexAssignmentKind::Brace,
                            indices: indices.clone(),
                            value: empty_matrix_value(),
                        },
                    )
                {
                    return Err(RuntimeError::Unsupported(
                        "comma-separated list assignment to a nonexistent variable is not supported when any index is a colon"
                            .to_string(),
                    ));
                }
                if let Some(output_count) =
                    self.list_expanded_cell_assignment_target_count(frame, target, indices)?
                {
                    match value {
                        HirExpression::Call { target, args } => {
                            let values = self.evaluate_call_outputs(
                                frame,
                                target,
                                args,
                                Some(output_count),
                            )?;
                            if values.len() < output_count {
                                return Err(RuntimeError::Unsupported(format!(
                                    "assignment requests {} output(s), but rhs only produced {}",
                                    output_count,
                                    values.len()
                                )));
                            }
                            return Ok(vec![Value::Cell(CellValue::new(
                                1,
                                output_count,
                                values.into_iter().take(output_count).collect(),
                            )?)]);
                        }
                        _ => {
                            let direct = match self.evaluate_expression(frame, value) {
                                Ok(direct) => Some(direct),
                                Err(error) if is_single_output_dot_or_brace_result_error(&error) => {
                                    None
                                }
                                Err(error) => return Err(error),
                            };
                            if let Some(direct) = direct {
                                if distributed_cell_assignment_values(&direct, output_count).is_ok() {
                                    return Ok(vec![direct]);
                                }
                            }
                            let values = self.evaluate_expression_outputs(frame, value)?;
                            if values.len() < output_count {
                                return Err(RuntimeError::Unsupported(format!(
                                    "assignment requests {} output(s), but rhs only produced {}",
                                    output_count,
                                    values.len()
                                )));
                            }
                            return Ok(vec![Value::Cell(CellValue::new(
                                1,
                                output_count,
                                values.into_iter().take(output_count).collect(),
                            )?)]);
                        }
                    }
                }
                if matches!(&**target, HirExpression::FieldAccess { .. }) {
                    if let Some(receiver_count) =
                        self.list_expanded_struct_assignment_target_count(frame, target)?
                    {
                        if receiver_count > 1 {
                            match value {
                                HirExpression::Call {
                                    target: HirCallTarget::Callable(reference),
                                    args,
                                } if reference.name == "deal" => {
                                    let values = self.evaluate_call_outputs(
                                        frame,
                                        &HirCallTarget::Callable(reference.clone()),
                                        args,
                                        Some(args.len()),
                                    )?;
                                    return Ok(vec![Value::Cell(CellValue::new(
                                        1,
                                        values.len(),
                                        values,
                                    )?)]);
                                }
                                _ => {
                                    let direct = match self.evaluate_expression(frame, value) {
                                        Ok(direct) => Some(direct),
                                        Err(error)
                                            if is_single_output_dot_or_brace_result_error(&error) =>
                                        {
                                            None
                                        }
                                        Err(error) => return Err(error),
                                    };
                                    if let Some(direct) = direct {
                                        if matches!(direct, Value::Cell(_)) {
                                            return Ok(vec![direct]);
                                        }
                                    }
                                    let values = self.evaluate_expression_outputs(frame, value)?;
                                    if values.len() > receiver_count {
                                        return Ok(vec![Value::Cell(CellValue::new(
                                            1,
                                            values.len(),
                                            values,
                                        )?)]);
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(&**target, HirExpression::FieldAccess { .. }) {
                    if let Some(receiver_count) =
                        self.list_expanded_struct_assignment_target_count(frame, target)?
                    {
                        if receiver_count > 1 {
                            match value {
                                HirExpression::Call {
                                    target: HirCallTarget::Callable(reference),
                                    args,
                                } if reference.name == "deal" => {
                                    let values = self.evaluate_call_outputs(
                                        frame,
                                        &HirCallTarget::Callable(reference.clone()),
                                        args,
                                        Some(args.len()),
                                    )?;
                                    return Ok(vec![Value::Cell(CellValue::new(
                                        1,
                                        values.len(),
                                        values,
                                    )?)]);
                                }
                                _ => {
                                    let direct = match self.evaluate_expression(frame, value) {
                                        Ok(direct) => Some(direct),
                                        Err(error)
                                            if is_single_output_dot_or_brace_result_error(&error) =>
                                        {
                                            None
                                        }
                                        Err(error) => return Err(error),
                                    };
                                    if let Some(direct) = direct {
                                        if matches!(direct, Value::Cell(_)) {
                                            return Ok(vec![direct]);
                                        }
                                    }
                                    let values = self.evaluate_expression_outputs(frame, value)?;
                                    if values.len() > receiver_count {
                                        return Ok(vec![Value::Cell(CellValue::new(
                                            1,
                                            values.len(),
                                            values,
                                        )?)]);
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(value, HirExpression::Call { .. })
                    || expression_supports_list_expansion(value)
                {
                    match self.evaluate_expression(frame, target) {
                        Ok(target_value) => {
                            let evaluated_indices = match self.evaluate_index_arguments(
                                frame,
                                &target_value,
                                indices,
                            ) {
                                Ok(indices) => Some(indices),
                                Err(RuntimeError::InvalidIndex(_)) => None,
                                Err(error) => return Err(error),
                            };
                            let Value::Cell(cell) = &target_value else {
                                return Err(RuntimeError::TypeError(format!(
                                    "cell-content indexing is only defined for cell values, found {}",
                                    target_value.kind_name()
                                )));
                            };
                            if let Some(evaluated_indices) = evaluated_indices {
                                let selection = cell_selection(cell, &evaluated_indices)?;
                                if selection.positions.len() > 1 {
                                    let values = match value {
                                        HirExpression::Call { target, args } => self
                                            .evaluate_call_outputs(
                                                frame,
                                                target,
                                                args,
                                                Some(selection.positions.len()),
                                            )?,
                                        _ => self.evaluate_expression_outputs(frame, value)?,
                                    };
                                    if values.len() < selection.positions.len() {
                                        return Err(RuntimeError::Unsupported(format!(
                                            "assignment requests {} output(s), but rhs only produced {}",
                                            selection.positions.len(),
                                            values.len()
                                        )));
                                    }
                                    return Ok(vec![Value::Cell(CellValue::new(
                                        1,
                                        selection.positions.len(),
                                        values
                                            .into_iter()
                                            .take(selection.positions.len())
                                            .collect(),
                                    )?)]);
                                }
                            }
                        }
                        Err(RuntimeError::MissingVariable(_)) => {
                            if let Some(selection_count) = self
                                .undefined_explicit_cell_assignment_target_count(
                                    frame, target, indices,
                                )?
                            {
                                let values = match value {
                                    HirExpression::Call { target, args } => self
                                        .evaluate_call_outputs(
                                            frame,
                                            target,
                                            args,
                                            Some(selection_count),
                                        )?,
                                    _ => self.evaluate_expression_outputs(frame, value)?,
                                };
                                if values.len() < selection_count {
                                    return Err(RuntimeError::Unsupported(format!(
                                        "assignment requests {} output(s), but rhs only produced {}",
                                        selection_count,
                                        values.len()
                                    )));
                                }
                                return Ok(vec![Value::Cell(CellValue::new(
                                    1,
                                    selection_count,
                                    values.into_iter().take(selection_count).collect(),
                                )?)]);
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            if let HirAssignmentTarget::Field { target, .. } = &targets[0] {
                if (list_assignment || field_target_requires_list_assignment(target))
                    && (matches!(value, HirExpression::Call { .. })
                        || expression_supports_list_expansion(value))
                {
                    if self.undefined_struct_field_list_assignment_requires_index(frame, target)? {
                        return Err(RuntimeError::Unsupported(
                            "comma-separated list assignment to a nonexistent struct array requires an explicit indexed receiver"
                                .to_string(),
                        ));
                    }
                    if let Some(output_count) =
                        self.list_expanded_struct_assignment_target_count(frame, target)?
                    {
                        let values = match value {
                            HirExpression::Call { target, args } => {
                                self.evaluate_call_outputs(frame, target, args, Some(output_count))?
                            }
                            _ => self.evaluate_expression_outputs(frame, value)?,
                        };
                        if values.len() >= output_count && values.len() > 1 {
                            let matrix = MatrixValue::with_dimensions(
                                1,
                                output_count,
                                vec![1, output_count],
                                values.into_iter().take(output_count).collect(),
                            )?;
                            return Ok(vec![Value::Matrix(matrix)]);
                        }
                    }
                    if let Some(output_count) =
                        self.undefined_explicit_struct_assignment_target_count(frame, target)?
                    {
                        let values = match value {
                            HirExpression::Call { target, args } => {
                                self.evaluate_call_outputs(frame, target, args, Some(output_count))?
                            }
                            _ => self.evaluate_expression_outputs(frame, value)?,
                        };
                        if values.len() >= output_count && values.len() > 1 {
                            let matrix = MatrixValue::with_dimensions(
                                1,
                                output_count,
                                vec![1, output_count],
                                values.into_iter().take(output_count).collect(),
                            )?;
                            return Ok(vec![Value::Matrix(matrix)]);
                        }
                    }
                    if expression_supports_list_expansion(target) {
                        match value {
                            HirExpression::Call {
                                target: HirCallTarget::Callable(reference),
                                args,
                            } if reference.name == "deal" => {
                                let values = self.evaluate_call_outputs(
                                    frame,
                                    &HirCallTarget::Callable(reference.clone()),
                                    args,
                                    Some(args.len()),
                                )?;
                                if values.len() > 1 {
                                    let matrix = MatrixValue::with_dimensions(
                                        1,
                                        values.len(),
                                        vec![1, values.len()],
                                        values,
                                    )?;
                                    return Ok(vec![Value::Matrix(matrix)]);
                                }
                            }
                            _ => {
                                let direct = match self.evaluate_expression(frame, value) {
                                    Ok(direct) => Some(direct),
                                    Err(error)
                                        if is_single_output_dot_or_brace_result_error(&error) =>
                                    {
                                        None
                                    }
                                    Err(error) => return Err(error),
                                };
                                if let Some(direct) = direct {
                                    if matches!(&direct, Value::Cell(cell) if cell.elements.len() > 1)
                                    {
                                        return Ok(vec![direct]);
                                    }
                                }
                                let values = self.evaluate_expression_outputs(frame, value)?;
                                if values.len() > 1 {
                                    let matrix = MatrixValue::with_dimensions(
                                        1,
                                        values.len(),
                                        vec![1, values.len()],
                                        values,
                                    )?;
                                    return Ok(vec![Value::Matrix(matrix)]);
                                }
                            }
                        }
                    }
                    match self.evaluate_expression(frame, target) {
                        Ok(Value::Matrix(matrix))
                            if matrix_is_struct_array(&matrix) && matrix.elements.len() > 1 =>
                        {
                            let output_count = matrix.elements.len();
                            let values = match value {
                                HirExpression::Call { target, args } => self
                                    .evaluate_call_outputs(
                                        frame,
                                        target,
                                        args,
                                        Some(output_count),
                                    )?,
                                _ => self.evaluate_expression_outputs(frame, value)?,
                            };
                            if values.len() >= output_count && values.len() > 1 {
                                let matrix = MatrixValue::with_dimensions(
                                    matrix.rows,
                                    matrix.cols,
                                    matrix.dims.clone(),
                                    values.into_iter().take(output_count).collect(),
                                )?;
                                return Ok(vec![Value::Matrix(matrix)]);
                            }
                        }
                        Ok(Value::Cell(cell))
                            if cell.elements.len() > 1
                                && cell.elements.iter().all(value_is_struct_assignment_target) =>
                        {
                            let output_count = cell.elements.len();
                            let values = match value {
                                HirExpression::Call { target, args } => self
                                    .evaluate_call_outputs(
                                        frame,
                                        target,
                                        args,
                                        Some(output_count),
                                    )?,
                                _ => self.evaluate_expression_outputs(frame, value)?,
                            };
                            if values.len() >= output_count && values.len() > 1 {
                                let matrix = MatrixValue::with_dimensions(
                                    cell.rows,
                                    cell.cols,
                                    cell.dims.clone(),
                                    values.into_iter().take(output_count).collect(),
                                )?;
                                return Ok(vec![Value::Matrix(matrix)]);
                            }
                        }
                        Ok(_)
                        | Err(RuntimeError::MissingVariable(_))
                        | Err(RuntimeError::InvalidIndex(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            if targets.len() == 1 {
                return Ok(vec![self.evaluate_expression(frame, value)?]);
            }
        }

        let values = match value {
            HirExpression::Call { target, args } => {
                self.evaluate_call_outputs(frame, target, args, Some(targets.len()))?
            }
            _ => self.evaluate_expression_outputs(frame, value)?,
        };

        if values.len() < targets.len() {
            return Err(RuntimeError::Unsupported(format!(
                "assignment requests {} output(s), but rhs only produced {}",
                targets.len(),
                values.len()
            )));
        }

        Ok(values.into_iter().take(targets.len()).collect())
    }

    fn execute_block(
        &mut self,
        frame: &mut Frame<'a>,
        statements: &[HirStatement],
    ) -> Result<Option<ControlFlow>, RuntimeError> {
        for statement in statements {
            if let Some(control) = self.execute_statement(frame, statement)? {
                return Ok(Some(control));
            }
        }
        Ok(None)
    }

    fn assign_target(
        &mut self,
        frame: &mut Frame<'a>,
        target: &HirAssignmentTarget,
        value: Value,
    ) -> Result<(), RuntimeError> {
        match target {
            HirAssignmentTarget::Binding(binding) => {
                self.bind_storage_binding(frame, binding)?;
                frame.assign_binding(binding, value)
            }
            HirAssignmentTarget::Index { target, indices } => self.assign_lvalue_target(
                frame,
                target,
                LValueLeaf::Index {
                    kind: IndexAssignmentKind::Paren,
                    indices: indices.clone(),
                    value,
                },
            ),
            HirAssignmentTarget::CellIndex { target, indices } => self.assign_lvalue_target(
                frame,
                target,
                LValueLeaf::Index {
                    kind: IndexAssignmentKind::Brace,
                    indices: indices.clone(),
                    value,
                },
            ),
            HirAssignmentTarget::Field { target, field } => self.assign_lvalue_target(
                frame,
                target,
                LValueLeaf::Field {
                    field: field.clone(),
                    value,
                },
            ),
        }
    }

    fn evaluate_expression(
        &mut self,
        frame: &mut Frame<'a>,
        expression: &HirExpression,
    ) -> Result<Value, RuntimeError> {
        match expression {
            HirExpression::ValueRef(reference) => {
                if reference.resolution == ReferenceResolution::BuiltinValue {
                    builtin_value_from_name(&reference.name)
                } else {
                    frame.read_reference(reference.binding_id, &reference.name)
                }
            }
            HirExpression::NumberLiteral(text) => parse_numeric_literal(text),
            HirExpression::CharLiteral(text) => {
                Ok(Value::CharArray(decode_text_literal(text, '\'')?))
            }
            HirExpression::StringLiteral(text) => {
                Ok(Value::String(decode_text_literal(text, '"')?))
            }
            HirExpression::MatrixLiteral(rows) => {
                let rows = self.evaluate_literal_rows(frame, rows)?;
                Ok(Value::Matrix(MatrixValue::from_rows(rows)?))
            }
            HirExpression::CellLiteral(rows) => {
                let rows = self.evaluate_literal_rows(frame, rows)?;
                Ok(Value::Cell(CellValue::from_rows(rows)?))
            }
            HirExpression::FunctionHandle(reference) => {
                Ok(Value::FunctionHandle(FunctionHandleValue {
                    display_name: reference.name.clone(),
                    target: match &reference.final_resolution {
                        Some(FinalReferenceResolution::ResolvedPath { path, .. }) => {
                            FunctionHandleTarget::ResolvedPath(path.clone())
                        }
                        _ => FunctionHandleTarget::Named(reference.name.clone()),
                    },
                }))
            }
            HirExpression::EndKeyword => Err(RuntimeError::Unsupported(
                "`end` execution is not implemented outside indexing".to_string(),
            )),
            HirExpression::Unary { op, rhs } => {
                let rhs = self.evaluate_expression(frame, rhs)?;
                match op {
                    matlab_frontend::ast::UnaryOp::Plus => map_numeric_unary(&rhs, |value| value),
                    matlab_frontend::ast::UnaryOp::Minus => {
                        map_numeric_unary(&rhs, |value| NumericComplexParts {
                            real: -value.real,
                            imag: -value.imag,
                        })
                    }
                    matlab_frontend::ast::UnaryOp::LogicalNot => {
                        map_numeric_unary_logical(&rhs, |value| value == 0.0)
                    }
                    matlab_frontend::ast::UnaryOp::DotTranspose => transpose_value(&rhs, false),
                    matlab_frontend::ast::UnaryOp::Transpose => transpose_value(&rhs, true),
                }
            }
            HirExpression::Binary { op, lhs, rhs } => {
                let lhs = self.evaluate_expression(frame, lhs)?;
                let rhs = self.evaluate_expression(frame, rhs)?;
                match op {
                    matlab_frontend::ast::BinaryOp::Add => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| lhs.plus(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::Subtract => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| lhs.minus(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::Multiply => matrix_multiply(&lhs, &rhs),
                    matlab_frontend::ast::BinaryOp::ElementwiseMultiply => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| lhs.times(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::MatrixRightDivide => {
                        matrix_right_divide(&lhs, &rhs)
                    }
                    matlab_frontend::ast::BinaryOp::ElementwiseRightDivide => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| lhs.rdivide(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::MatrixLeftDivide => {
                        matrix_left_divide(&lhs, &rhs)
                    }
                    matlab_frontend::ast::BinaryOp::ElementwiseLeftDivide => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| rhs.rdivide(lhs))
                    }
                    matlab_frontend::ast::BinaryOp::Power => matrix_power(&lhs, &rhs),
                    matlab_frontend::ast::BinaryOp::ElementwisePower => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| {
                            normalize_numeric_complex_parts(lhs.pow(rhs))
                        })
                    }
                    matlab_frontend::ast::BinaryOp::GreaterThan => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs > rhs)
                    }
                    matlab_frontend::ast::BinaryOp::GreaterThanOrEqual => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs >= rhs)
                    }
                    matlab_frontend::ast::BinaryOp::LessThan => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs < rhs)
                    }
                    matlab_frontend::ast::BinaryOp::LessThanOrEqual => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs <= rhs)
                    }
                    matlab_frontend::ast::BinaryOp::Equal => {
                        map_numeric_binary_equality(&lhs, &rhs, |lhs, rhs| lhs.exact_eq(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::NotEqual => {
                        map_numeric_binary_equality(&lhs, &rhs, |lhs, rhs| lhs.exact_ne(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::LogicalAnd => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs != 0.0 && rhs != 0.0)
                    }
                    matlab_frontend::ast::BinaryOp::LogicalOr => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs != 0.0 || rhs != 0.0)
                    }
                    matlab_frontend::ast::BinaryOp::ShortCircuitAnd => {
                        Ok(logical_value(lhs.truthy()? && rhs.truthy()?))
                    }
                    matlab_frontend::ast::BinaryOp::ShortCircuitOr => {
                        Ok(logical_value(lhs.truthy()? || rhs.truthy()?))
                    }
                    _ => Err(RuntimeError::Unsupported(format!(
                        "binary operator {op:?} is not implemented in the current interpreter"
                    ))),
                }
            }
            HirExpression::Range { start, step, end } => {
                let start = self.evaluate_expression(frame, start)?.as_scalar()?;
                let step = match step {
                    Some(step) => self.evaluate_expression(frame, step)?.as_scalar()?,
                    None => 1.0,
                };
                let end = self.evaluate_expression(frame, end)?.as_scalar()?;
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
            HirExpression::Call { target, args } => Ok(first_output_or_unit(
                self.evaluate_call_outputs(frame, target, args, Some(1))?,
            )),
            HirExpression::CellIndex { target, indices } => {
                let target = self.evaluate_expression(frame, target)?;
                let indices = self.evaluate_index_arguments(frame, &target, indices)?;
                evaluate_cell_content_index(&target, &indices)
            }
            HirExpression::FieldAccess { target, field } => {
                let target = self.evaluate_expression(frame, target)?;
                read_field_value(&target, field)
            }
            HirExpression::AnonymousFunction(anonymous) => {
                let handle_name = format!(
                    "<anonymous:{}:{}:{}>",
                    anonymous.scope_id.0, anonymous.workspace_id.0, self.next_anonymous_id
                );
                self.next_anonymous_id += 1;
                let captured_cells = anonymous
                    .captures
                    .iter()
                    .map(|capture| {
                        let cell = frame.cell(capture.binding_id).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "captured binding `{}` is not available",
                                capture.name
                            ))
                        })?;
                        Ok((capture.binding_id, cell))
                    })
                    .collect::<Result<HashMap<_, _>, RuntimeError>>()?;
                self.anonymous_functions.insert(
                    handle_name.clone(),
                    AnonymousClosure {
                        function: anonymous.clone(),
                        captured_cells,
                        visible_functions: frame.visible_functions.clone(),
                    },
                );
                Ok(Value::FunctionHandle(FunctionHandleValue {
                    display_name: handle_name.clone(),
                    target: FunctionHandleTarget::Named(handle_name),
                }))
            }
        }
    }

    fn evaluate_expression_outputs(
        &mut self,
        frame: &mut Frame<'a>,
        expression: &HirExpression,
    ) -> Result<Vec<Value>, RuntimeError> {
        match expression {
            HirExpression::Call { target, args }
                if matches!(
                    target,
                    HirCallTarget::Expression(expression)
                        if expression_supports_list_expansion(expression)
                ) =>
            {
                self.evaluate_call_outputs(frame, target, args, None)
            }
            HirExpression::CellIndex { target, indices } => {
                let targets = self.evaluate_expression_outputs(frame, target)?;
                let mut values = Vec::new();
                for target in targets {
                    let evaluated_indices =
                        self.evaluate_index_arguments(frame, &target, indices)?;
                    values.extend(evaluate_cell_content_outputs(&target, &evaluated_indices)?);
                }
                Ok(values)
            }
            HirExpression::FieldAccess { target, field } => {
                let targets = self.evaluate_expression_outputs(frame, target)?;
                read_field_outputs_from_values(&targets, field)
            }
            _ => Ok(vec![self.evaluate_expression(frame, expression)?]),
        }
    }

    fn evaluate_literal_rows(
        &mut self,
        frame: &mut Frame<'a>,
        rows: &[Vec<HirExpression>],
    ) -> Result<Vec<Vec<Value>>, RuntimeError> {
        rows.iter()
            .map(|row| {
                let mut values = Vec::new();
                for expression in row {
                    values.extend(self.evaluate_expression_outputs(frame, expression)?);
                }
                Ok(values)
            })
            .collect()
    }

    fn list_expanded_struct_assignment_target_count(
        &mut self,
        frame: &mut Frame<'a>,
        expression: &HirExpression,
    ) -> Result<Option<usize>, RuntimeError> {
        if !expression_supports_list_expansion(expression) {
            match self.evaluate_expression(frame, expression) {
                Ok(value) => {
                    return Ok(
                        nested_struct_assignment_target_count(&value).filter(|&count| count > 1)
                    )
                }
                Err(RuntimeError::MissingVariable(_)) | Err(RuntimeError::InvalidIndex(_)) => {}
                Err(error) if is_single_output_dot_or_brace_result_error(&error) => {}
                Err(error) => return Err(error),
            }
            if let HirExpression::FieldAccess { target, .. } = expression {
                return self.list_expanded_struct_assignment_target_count(frame, target);
            }
            return Ok(None);
        }

        match self.evaluate_expression_outputs(frame, expression) {
            Ok(values) if values.len() > 1 => Ok(Some(
                values
                    .iter()
                    .map(|value| nested_struct_assignment_target_count(value).unwrap_or(1))
                    .sum(),
            )),
            Ok(_) | Err(RuntimeError::MissingVariable(_)) | Err(RuntimeError::InvalidIndex(_)) => {
                match self.evaluate_expression(frame, expression) {
                    Ok(value) => Ok(
                        nested_struct_assignment_target_count(&value).filter(|&count| count > 1)
                    ),
                    Err(RuntimeError::MissingVariable(_))
                    | Err(RuntimeError::InvalidIndex(_))
                    | Err(RuntimeError::TypeError(_)) => {
                        if let HirExpression::FieldAccess { target, .. } = expression {
                            self.list_expanded_struct_assignment_target_count(frame, target)
                        } else {
                            Ok(None)
                        }
                    }
                    Err(error) if is_single_output_dot_or_brace_result_error(&error) => {
                        if let HirExpression::FieldAccess { target, .. } = expression {
                            self.list_expanded_struct_assignment_target_count(frame, target)
                        } else {
                            Ok(None)
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) if is_single_output_dot_or_brace_result_error(&error) => {
                if let HirExpression::FieldAccess { target, .. } = expression {
                    self.list_expanded_struct_assignment_target_count(frame, target)
                } else {
                    Ok(None)
                }
            }
            Err(error) => Err(error),
        }
    }

    fn list_expanded_cell_assignment_target_count(
        &mut self,
        frame: &mut Frame<'a>,
        target: &HirExpression,
        indices: &[HirIndexArgument],
    ) -> Result<Option<usize>, RuntimeError> {
        let target_values = match self.evaluate_expression_outputs(frame, target) {
            Ok(values) => values,
            Err(RuntimeError::MissingVariable(_))
            | Err(RuntimeError::InvalidIndex(_))
            | Err(RuntimeError::TypeError(_)) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };

        if target_values.len() <= 1 {
            return Ok(None);
        }

        let mut output_count = 0;
        for target_value in &target_values {
            let evaluated_indices = self.evaluate_index_arguments(frame, target_value, indices)?;
            let Value::Cell(cell) = target_value else {
                return Err(RuntimeError::TypeError(format!(
                    "cell-content indexing is only defined for cell values, found {}",
                    target_value.kind_name()
                )));
            };
            output_count += cell_selection(cell, &evaluated_indices)?.positions.len();
        }
        Ok(Some(output_count))
    }

    fn undefined_explicit_cell_assignment_target_count(
        &mut self,
        frame: &mut Frame<'a>,
        target: &HirExpression,
        indices: &[HirIndexArgument],
    ) -> Result<Option<usize>, RuntimeError> {
        if indices.iter().any(|argument| match argument {
            HirIndexArgument::FullSlice | HirIndexArgument::End => true,
            HirIndexArgument::Expression(expression) => expression_contains_end_keyword(expression),
        }) {
            return Ok(None);
        }

        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        if cell.borrow().is_some() || !projections.is_empty() {
            return Ok(None);
        }

        let empty = empty_cell_value();
        let Value::Cell(cell_value) = &empty else {
            unreachable!("empty cell helper should return a cell value");
        };
        let evaluated_indices = self.evaluate_index_arguments(frame, &empty, indices)?;
        let plan = cell_assignment_plan(cell_value, &evaluated_indices)?;
        Ok(Some(plan.selection.positions.len()))
    }

    fn undefined_explicit_struct_assignment_target_count(
        &mut self,
        frame: &mut Frame<'a>,
        target: &HirExpression,
    ) -> Result<Option<usize>, RuntimeError> {
        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        if cell.borrow().is_some() || projections.is_empty() {
            return Ok(None);
        }

        let mut current = empty_matrix_value();
        for projection in &projections {
            match projection {
                LValueProjection::Paren(indices) => {
                    let evaluated = self.evaluate_index_arguments(frame, &current, indices)?;
                    let Some(selected) =
                        default_struct_selection_value_for_index_update(&current, &evaluated)?
                    else {
                        return Ok(None);
                    };
                    current = selected;
                }
                LValueProjection::Field(field) => {
                    let leaf = LValueLeaf::Field {
                        field: field.clone(),
                        value: empty_matrix_value(),
                    };
                    let Some(selected) = default_field_projection_value(&current, &[], &leaf) else {
                        return Ok(None);
                    };
                    current = selected;
                }
                LValueProjection::Brace(_) => return Ok(None),
            }
        }

        Ok(nested_struct_assignment_target_count(&current).filter(|&count| count > 1))
    }

    fn undefined_struct_field_list_assignment_requires_index(
        &mut self,
        frame: &mut Frame<'a>,
        target: &HirExpression,
    ) -> Result<bool, RuntimeError> {
        let (_name, cell, projections) = match frame.lvalue_root(target) {
            Ok(root) => root,
            Err(RuntimeError::MissingVariable(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(cell.borrow().is_none()
            && !projections
                .iter()
                .any(|projection| matches!(projection, LValueProjection::Paren(_))))
    }

    fn simple_struct_field_assignment_target_count(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
    ) -> Result<Option<usize>, RuntimeError> {
        let [HirAssignmentTarget::Field { target, .. }] = targets else {
            return Ok(None);
        };
        if let Some(count) = self.list_expanded_struct_assignment_target_count(frame, target)? {
            if count > 1 {
                return Ok(Some(count));
            }
        }
        self.undefined_explicit_struct_assignment_target_count(frame, target)
    }

    fn unsupported_multi_struct_field_subindex_target(
        &mut self,
        frame: &mut Frame<'a>,
        targets: &[HirAssignmentTarget],
    ) -> Result<Option<String>, RuntimeError> {
        let target = match targets {
            [HirAssignmentTarget::Index { target, .. }] => target,
            _ => return Ok(None),
        };
        let HirExpression::FieldAccess { target: receiver, field } = &**target else {
            return Ok(None);
        };
        let receiver = match self.evaluate_expression(frame, receiver) {
            Ok(receiver) => receiver,
            Err(RuntimeError::MissingVariable(_))
            | Err(RuntimeError::InvalidIndex(_))
            | Err(RuntimeError::TypeError(_)) => return Ok(None),
            Err(error) if is_single_output_dot_or_brace_result_error(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        match receiver {
            Value::Matrix(matrix)
                if matrix_is_struct_array(&matrix) && matrix.element_count() > 1 =>
            {
                Ok(Some(field.clone()))
            }
            _ => Ok(None),
        }
    }

    fn evaluate_index_literal_rows(
        &mut self,
        frame: &mut Frame<'a>,
        target: &Value,
        rows: &[Vec<HirExpression>],
        position: usize,
        total_arguments: usize,
    ) -> Result<Vec<Vec<Value>>, RuntimeError> {
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|expression| {
                        self.evaluate_index_expression_value(
                            frame,
                            target,
                            expression,
                            position,
                            total_arguments,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect()
    }

    fn evaluate_function_arguments(
        &mut self,
        frame: &mut Frame<'a>,
        arguments: &[HirIndexArgument],
    ) -> Result<Vec<Value>, RuntimeError> {
        let mut values = Vec::new();
        for argument in arguments {
            values.extend(self.evaluate_function_argument_outputs(frame, argument)?);
        }
        Ok(values)
    }

    fn evaluate_function_argument_outputs(
        &mut self,
        frame: &mut Frame<'a>,
        argument: &HirIndexArgument,
    ) -> Result<Vec<Value>, RuntimeError> {
        match argument {
            HirIndexArgument::Expression(expression) => {
                self.evaluate_expression_outputs(frame, expression)
            }
            HirIndexArgument::FullSlice => Err(RuntimeError::Unsupported(
                "slice-style indexing arguments are not implemented in the current interpreter"
                    .to_string(),
            )),
            HirIndexArgument::End => Err(RuntimeError::Unsupported(
                "`end` is not implemented for function-call arguments in the current interpreter"
                    .to_string(),
            )),
        }
    }

    fn evaluate_index_arguments(
        &mut self,
        frame: &mut Frame<'a>,
        target: &Value,
        arguments: &[HirIndexArgument],
    ) -> Result<Vec<EvaluatedIndexArgument>, RuntimeError> {
        arguments
            .iter()
            .enumerate()
            .map(|(position, argument)| {
                self.evaluate_index_argument(frame, target, argument, position, arguments.len())
            })
            .collect()
    }

    fn evaluate_index_argument(
        &mut self,
        frame: &mut Frame<'a>,
        target: &Value,
        argument: &HirIndexArgument,
        position: usize,
        total_arguments: usize,
    ) -> Result<EvaluatedIndexArgument, RuntimeError> {
        match argument {
            HirIndexArgument::Expression(expression) => {
                evaluated_index_argument(self.evaluate_index_expression_value(
                    frame,
                    target,
                    expression,
                    position,
                    total_arguments,
                )?)
            }
            HirIndexArgument::FullSlice => Ok(EvaluatedIndexArgument::FullSlice),
            HirIndexArgument::End => Ok(EvaluatedIndexArgument::Numeric {
                values: vec![end_index_extent(target, position, total_arguments)? as f64],
                rows: 1,
                cols: 1,
                dims: vec![1, 1],
                logical: false,
            }),
        }
    }

    fn evaluate_index_expression_value(
        &mut self,
        frame: &mut Frame<'a>,
        target: &Value,
        expression: &HirExpression,
        position: usize,
        total_arguments: usize,
    ) -> Result<Value, RuntimeError> {
        if !expression_contains_end_keyword(expression) {
            return self.evaluate_expression(frame, expression);
        }

        match expression {
            HirExpression::EndKeyword => {
                Ok(Value::Scalar(end_index_extent(target, position, total_arguments)? as f64))
            }
            HirExpression::Unary { op, rhs } => {
                let rhs =
                    self.evaluate_index_expression_value(frame, target, rhs, position, total_arguments)?;
                match op {
                    matlab_frontend::ast::UnaryOp::Plus => map_numeric_unary(&rhs, |value| value),
                    matlab_frontend::ast::UnaryOp::Minus => {
                        map_numeric_unary(&rhs, |value| NumericComplexParts {
                            real: -value.real,
                            imag: -value.imag,
                        })
                    }
                    matlab_frontend::ast::UnaryOp::LogicalNot => {
                        map_numeric_unary_logical(&rhs, |value| value == 0.0)
                    }
                    matlab_frontend::ast::UnaryOp::DotTranspose => transpose_value(&rhs, false),
                    matlab_frontend::ast::UnaryOp::Transpose => transpose_value(&rhs, true),
                }
            }
            HirExpression::Binary { op, lhs, rhs } => {
                let lhs =
                    self.evaluate_index_expression_value(frame, target, lhs, position, total_arguments)?;
                let rhs =
                    self.evaluate_index_expression_value(frame, target, rhs, position, total_arguments)?;
                match op {
                    matlab_frontend::ast::BinaryOp::Add => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| lhs.plus(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::Subtract => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| lhs.minus(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::Multiply => matrix_multiply(&lhs, &rhs),
                    matlab_frontend::ast::BinaryOp::ElementwiseMultiply => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| lhs.times(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::MatrixRightDivide => {
                        matrix_right_divide(&lhs, &rhs)
                    }
                    matlab_frontend::ast::BinaryOp::ElementwiseRightDivide => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| lhs.rdivide(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::MatrixLeftDivide => {
                        matrix_left_divide(&lhs, &rhs)
                    }
                    matlab_frontend::ast::BinaryOp::ElementwiseLeftDivide => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| rhs.rdivide(lhs))
                    }
                    matlab_frontend::ast::BinaryOp::Power => matrix_power(&lhs, &rhs),
                    matlab_frontend::ast::BinaryOp::ElementwisePower => {
                        map_numeric_binary(&lhs, &rhs, |lhs, rhs| {
                            normalize_numeric_complex_parts(lhs.pow(rhs))
                        })
                    }
                    matlab_frontend::ast::BinaryOp::GreaterThan => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs > rhs)
                    }
                    matlab_frontend::ast::BinaryOp::GreaterThanOrEqual => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs >= rhs)
                    }
                    matlab_frontend::ast::BinaryOp::LessThan => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs < rhs)
                    }
                    matlab_frontend::ast::BinaryOp::LessThanOrEqual => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs <= rhs)
                    }
                    matlab_frontend::ast::BinaryOp::Equal => {
                        map_numeric_binary_equality(&lhs, &rhs, |lhs, rhs| lhs.exact_eq(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::NotEqual => {
                        map_numeric_binary_equality(&lhs, &rhs, |lhs, rhs| lhs.exact_ne(rhs))
                    }
                    matlab_frontend::ast::BinaryOp::LogicalAnd => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs != 0.0 && rhs != 0.0)
                    }
                    matlab_frontend::ast::BinaryOp::LogicalOr => {
                        map_numeric_binary_logical(&lhs, &rhs, |lhs, rhs| lhs != 0.0 || rhs != 0.0)
                    }
                    matlab_frontend::ast::BinaryOp::ShortCircuitAnd => {
                        Ok(logical_value(lhs.truthy()? && rhs.truthy()?))
                    }
                    matlab_frontend::ast::BinaryOp::ShortCircuitOr => {
                        Ok(logical_value(lhs.truthy()? || rhs.truthy()?))
                    }
                    _ => Err(RuntimeError::Unsupported(format!(
                        "binary operator {op:?} is not implemented in the current interpreter"
                    ))),
                }
            }
            HirExpression::Range { start, step, end } => {
                let start = self
                    .evaluate_index_expression_value(frame, target, start, position, total_arguments)?
                    .as_scalar()?;
                let step = match step {
                    Some(step) => self
                        .evaluate_index_expression_value(frame, target, step, position, total_arguments)?
                        .as_scalar()?,
                    None => 1.0,
                };
                let end = self
                    .evaluate_index_expression_value(frame, target, end, position, total_arguments)?
                    .as_scalar()?;
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
            HirExpression::MatrixLiteral(rows) => {
                let rows = self.evaluate_index_literal_rows(
                    frame,
                    target,
                    rows,
                    position,
                    total_arguments,
                )?;
                Ok(Value::Matrix(MatrixValue::from_rows(rows)?))
            }
            HirExpression::CellLiteral(rows) => {
                let rows = self.evaluate_index_literal_rows(
                    frame,
                    target,
                    rows,
                    position,
                    total_arguments,
                )?;
                Ok(Value::Cell(CellValue::from_rows(rows)?))
            }
            HirExpression::Call { target: call_target, args } => match call_target {
                HirCallTarget::Callable(reference)
                    if reference.semantic_resolution == ReferenceResolution::WorkspaceValue =>
                {
                    let call_target =
                        frame.read_reference(reference.binding_id, &reference.name)?;
                    if matches!(call_target, Value::FunctionHandle(_)) {
                        let args = self.evaluate_index_function_arguments(
                            frame,
                            target,
                            args,
                            position,
                            total_arguments,
                        )?;
                        Ok(first_output_or_unit(self.call_function_value_outputs(
                            frame,
                            &call_target,
                            &args,
                            Some(1),
                        )?))
                    } else {
                        let args = self.evaluate_index_arguments(frame, &call_target, args)?;
                        evaluate_expression_call(&call_target, &args)
                    }
                }
                HirCallTarget::Callable(reference) => {
                    let args = self.evaluate_index_function_arguments(
                        frame,
                        target,
                        args,
                        position,
                        total_arguments,
                    )?;
                    Ok(first_output_or_unit(self.call_callable_reference_outputs(
                        frame,
                        reference,
                        &args,
                        Some(1),
                    )?))
                }
                HirCallTarget::Expression(call_target_expression) => {
                    let call_target = self.evaluate_index_expression_value(
                        frame,
                        target,
                        call_target_expression,
                        position,
                        total_arguments,
                    )?;
                    if matches!(call_target, Value::FunctionHandle(_)) {
                        let args = self.evaluate_index_function_arguments(
                            frame,
                            target,
                            args,
                            position,
                            total_arguments,
                        )?;
                        Ok(first_output_or_unit(self.call_function_value_outputs(
                            frame,
                            &call_target,
                            &args,
                            Some(1),
                        )?))
                    } else {
                        let args = self.evaluate_index_arguments(frame, &call_target, args)?;
                        evaluate_expression_call(&call_target, &args)
                    }
                }
            },
            HirExpression::CellIndex {
                target: cell_target,
                indices,
            } => {
                let cell_target = self.evaluate_index_expression_value(
                    frame,
                    target,
                    cell_target,
                    position,
                    total_arguments,
                )?;
                let indices = self.evaluate_index_arguments(frame, &cell_target, indices)?;
                evaluate_cell_content_index(&cell_target, &indices)
            }
            HirExpression::FieldAccess {
                target: field_target,
                field,
            } => {
                let field_target = self.evaluate_index_expression_value(
                    frame,
                    target,
                    field_target,
                    position,
                    total_arguments,
                )?;
                read_field_value(&field_target, field)
            }
            _ => self.evaluate_expression(frame, expression),
        }
    }

    fn evaluate_index_function_arguments(
        &mut self,
        frame: &mut Frame<'a>,
        target: &Value,
        arguments: &[HirIndexArgument],
        position: usize,
        total_arguments: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let mut values = Vec::new();
        for argument in arguments {
            match argument {
                HirIndexArgument::Expression(expression) => values.push(
                    self.evaluate_index_expression_value(
                        frame,
                        target,
                        expression,
                        position,
                        total_arguments,
                    )?,
                ),
                HirIndexArgument::FullSlice => {
                    return Err(RuntimeError::Unsupported(
                        "slice-style indexing arguments are not implemented in the current interpreter"
                            .to_string(),
                    ))
                }
                HirIndexArgument::End => values.push(Value::Scalar(
                    end_index_extent(target, position, total_arguments)? as f64,
                )),
            }
        }
        Ok(values)
    }

    fn bind_storage_binding(
        &mut self,
        frame: &mut Frame<'a>,
        binding: &HirBinding,
    ) -> Result<(), RuntimeError> {
        match binding.storage {
            Some(BindingStorage::Global) => self.bind_global(frame, binding),
            Some(BindingStorage::Persistent) => self.bind_persistent(frame, binding),
            None | Some(BindingStorage::Local) => Ok(()),
        }
    }

    fn bind_global(
        &mut self,
        frame: &mut Frame<'a>,
        binding: &HirBinding,
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
        frame: &mut Frame<'a>,
        binding: &HirBinding,
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

    fn assign_lvalue_target(
        &mut self,
        frame: &mut Frame<'a>,
        target: &HirExpression,
        leaf: LValueLeaf,
    ) -> Result<(), RuntimeError> {
        let (name, cell, projections) = frame.lvalue_root(target)?;
        let current = match cell.borrow().clone() {
            Some(current) => current,
            None if undefined_brace_assignment_with_colon(&projections, &leaf) => {
                return Err(RuntimeError::Unsupported(
                    "comma-separated list assignment to a nonexistent variable is not supported when any index is a colon"
                        .to_string(),
                ));
            }
            None => default_lvalue_root_value(&projections, &leaf).ok_or_else(|| {
                RuntimeError::MissingVariable(format!(
                    "variable `{name}` is declared but has no runtime value"
                ))
            })?,
        };
        let updated = self.assign_lvalue_path(frame, current, &projections, false, leaf)?;
        *cell.borrow_mut() = Some(updated);
        Ok(())
    }

    fn assign_lvalue_path(
        &mut self,
        frame: &mut Frame<'a>,
        current: Value,
        projections: &[LValueProjection],
        nested_context: bool,
        leaf: LValueLeaf,
    ) -> Result<Value, RuntimeError> {
        let Some((projection, rest)) = projections.split_first() else {
            return self.apply_lvalue_leaf(frame, current, nested_context, leaf);
        };

        match projection {
            LValueProjection::Field(field) => {
                if matches!(
                    &current,
                    Value::Matrix(matrix)
                        if matrix_is_struct_array(matrix)
                            && matrix.element_count() > 1
                            && (field_subindexing_requires_single_struct(rest)
                                || matches!(leaf, LValueLeaf::Index { kind: IndexAssignmentKind::Paren, .. }))
                ) {
                    return Err(unsupported_multi_struct_field_subindexing_error(field));
                }
                if let Some(updated) = self.try_assign_distributed_struct_field_projection(
                    frame, &current, field, rest, &leaf,
                )? {
                    return Ok(updated);
                }
                let next = read_field_lvalue_value_for_assignment(&current, field, rest, &leaf)?;
                let updated_next = self.assign_lvalue_path(frame, next, rest, true, leaf)?;
                assign_struct_path(current, std::slice::from_ref(field), updated_next)
            }
            LValueProjection::Paren(indices) => {
                let evaluated_indices = self.evaluate_index_arguments(frame, &current, indices)?;
                let next = match evaluate_expression_call(&current, &evaluated_indices) {
                    Ok(next) => next,
                    Err(RuntimeError::InvalidIndex(_)) => match &leaf {
                        LValueLeaf::Field { .. } => default_struct_selection_value_for_index_update(
                            &current,
                            &evaluated_indices,
                        )?
                        .or_else(|| default_nested_lvalue_value(rest, &leaf))
                        .ok_or_else(|| {
                            RuntimeError::InvalidIndex(
                                "indexed assignment path could not synthesize a missing intermediate value"
                                    .to_string(),
                            )
                        })?,
                        _ => default_nested_lvalue_value(rest, &leaf).ok_or_else(|| {
                            RuntimeError::InvalidIndex(
                                "indexed assignment path could not synthesize a missing intermediate value"
                                    .to_string(),
                            )
                        })?,
                    },
                    Err(error) => return Err(error),
                };
                let updated_next = self.assign_lvalue_path(frame, next, rest, true, leaf)?;
                apply_index_update(
                    current,
                    &evaluated_indices,
                    updated_next,
                    IndexAssignmentKind::Paren,
                )
            }
            LValueProjection::Brace(indices) => {
                let evaluated_indices = self.evaluate_index_arguments(frame, &current, indices)?;
                let next = match materialize_cell_content_index(&current, &evaluated_indices) {
                    Ok(next) => next,
                    Err(RuntimeError::InvalidIndex(_)) => {
                        default_nested_lvalue_value(rest, &leaf).ok_or_else(|| {
                            RuntimeError::InvalidIndex(
                                "cell assignment path could not synthesize a missing intermediate value"
                                    .to_string(),
                            )
                        })?
                    }
                    Err(error) => return Err(error),
                };
                if rest.is_empty() {
                    if let LValueLeaf::Index {
                        kind: IndexAssignmentKind::Brace,
                        indices: leaf_indices,
                        value,
                    } = &leaf
                    {
                        if let Some(updated_next) = self
                            .try_assign_nested_list_expanded_cell_contents(
                                frame,
                                &next,
                                leaf_indices,
                                value,
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
                let updated_next = self.assign_lvalue_path(frame, next, rest, true, leaf)?;
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
        frame: &mut Frame<'a>,
        current: Value,
        nested_context: bool,
        leaf: LValueLeaf,
    ) -> Result<Value, RuntimeError> {
        match leaf {
            LValueLeaf::Field { field, value } => {
                if let Some(updated) =
                    try_assign_field_to_nested_cell_containers(&current, &field, &value)?
                {
                    return Ok(updated);
                }
                assign_struct_path(current, std::slice::from_ref(&field), value)
            }
            LValueLeaf::Index {
                kind,
                indices,
                value,
            } => {
                let evaluated_indices = self.evaluate_index_arguments(frame, &current, &indices)?;
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
        frame: &mut Frame<'a>,
        current: &Value,
        indices: &[HirIndexArgument],
        value: &Value,
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
            let evaluated_indices = self.evaluate_index_arguments(frame, element, indices)?;
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

    fn evaluate_call_outputs(
        &mut self,
        frame: &mut Frame<'a>,
        target: &HirCallTarget,
        args: &[HirIndexArgument],
        requested_outputs: Option<usize>,
    ) -> Result<Vec<Value>, RuntimeError> {
        if let HirCallTarget::Expression(expression) = target {
            if let HirExpression::FieldAccess {
                target: receiver_expr,
                field,
            } = expression.as_ref()
            {
                if !expression_supports_list_expansion(receiver_expr) {
                    let receiver = self.evaluate_expression(frame, receiver_expr)?;
                    if let Some(method) = mexception_method_builtin_name(&receiver, field) {
                        let mut evaluated_args = Vec::with_capacity(args.len() + 1);
                        evaluated_args.push(receiver);
                        evaluated_args.extend(self.evaluate_function_arguments(frame, args)?);
                        return invoke_runtime_builtin_outputs(
                            &self.shared_state,
                            Some(frame),
                            method,
                            &evaluated_args,
                            requested_outputs.unwrap_or(1),
                        );
                    }
                }
            }
        }

        match target {
            HirCallTarget::Callable(reference)
                if reference.semantic_resolution == ReferenceResolution::WorkspaceValue =>
            {
                let target = frame.read_reference(reference.binding_id, &reference.name)?;
                self.call_runtime_value_outputs(frame, &target, args, requested_outputs)
            }
            HirCallTarget::Callable(reference) => {
                let args = self.evaluate_function_arguments(frame, args)?;
                self.call_callable_reference_outputs(frame, reference, &args, requested_outputs)
            }
            HirCallTarget::Expression(target) => {
                let targets = self.evaluate_expression_outputs(frame, target)?;
                let mut outputs = Vec::new();
                for target in targets {
                    outputs.extend(self.call_runtime_value_outputs(
                        frame,
                        &target,
                        args,
                        requested_outputs,
                    )?);
                }
                Ok(outputs)
            }
        }
    }

    fn call_runtime_value_outputs(
        &mut self,
        frame: &mut Frame<'a>,
        target: &Value,
        args: &[HirIndexArgument],
        requested_outputs: Option<usize>,
    ) -> Result<Vec<Value>, RuntimeError> {
        match target {
            Value::FunctionHandle(_) => {
                let args = self.evaluate_function_arguments(frame, args)?;
                self.call_function_value_outputs(frame, target, &args, requested_outputs)
            }
            _ => {
                let args = self.evaluate_index_arguments(frame, target, args)?;
                Ok(vec![evaluate_expression_call(target, &args)?])
            }
        }
    }

    fn call_callable_reference_outputs(
        &mut self,
        frame: &mut Frame<'a>,
        reference: &HirCallableRef,
        args: &[Value],
        requested_outputs: Option<usize>,
    ) -> Result<Vec<Value>, RuntimeError> {
        let output_arity = requested_outputs.unwrap_or(1);

        match reference.semantic_resolution {
            ReferenceResolution::WorkspaceValue => {
                let target = frame.read_reference(reference.binding_id, &reference.name)?;
                self.call_function_value_outputs(frame, &target, args, requested_outputs)
            }
            ReferenceResolution::BuiltinFunction
                if matches!(
                    reference.final_resolution,
                    None | Some(FinalReferenceResolution::BuiltinFunction)
                ) =>
            {
                if matches!(reference.name.as_str(), "figure" | "set") {
                    return self.invoke_graphics_builtin_with_resize_callbacks(
                        frame,
                        &reference.name,
                        args,
                        output_arity,
                    );
                }
                if reference.name == "fplot" {
                    return self.invoke_fplot_builtin_outputs(frame, args, output_arity);
                }
                if reference.name == "fplot3" {
                    return self.invoke_fplot3_builtin_outputs(frame, args, output_arity);
                }
                if reference.name == "fsurf" {
                    return self.invoke_fsurf_builtin_outputs(frame, args, output_arity);
                }
                if reference.name == "fmesh" {
                    return self.invoke_fmesh_builtin_outputs(frame, args, output_arity);
                }
                if reference.name == "fimplicit" {
                    return self.invoke_fimplicit_builtin_outputs(frame, args, output_arity);
                }
                if reference.name == "fcontour" {
                    return self.invoke_fcontour_builtin_outputs(frame, args, output_arity);
                }
                if reference.name == "fcontour3" {
                    return self.invoke_fcontour3_builtin_outputs(frame, args, output_arity);
                }
                if reference.name == "arrayfun" {
                    return execute_arrayfun_builtin_outputs(args, output_arity, |callback, call_args, requested| {
                        self.call_function_value_outputs(frame, callback, call_args, Some(requested))
                    });
                }
                if reference.name == "cellfun" {
                    return execute_cellfun_builtin_outputs(args, output_arity, |callback, call_args, requested| {
                        self.call_function_value_outputs(frame, callback, call_args, Some(requested))
                    });
                }
                if reference.name == "structfun" {
                    return execute_structfun_builtin_outputs(args, output_arity, |callback, call_args, requested| {
                        self.call_function_value_outputs(frame, callback, call_args, Some(requested))
                    });
                }
                if matches!(
                    reference.name.as_str(),
                    "clear" | "clearvars" | "save" | "load"
                ) {
                    let shared_state = Rc::clone(&self.shared_state);
                    return self.invoke_workspace_builtin_outputs(
                        frame,
                        &shared_state,
                        &reference.name,
                        args,
                        output_arity,
                    );
                }
                if reference.name == "close" {
                    return self.invoke_close_builtin_outputs(frame, args, output_arity);
                }
                invoke_runtime_builtin_outputs(
                    &self.shared_state,
                    Some(frame),
                    &reference.name,
                    args,
                    output_arity,
                )
            }
            ReferenceResolution::FileFunction | ReferenceResolution::NestedFunction => {
                let function = self.resolve_named_function(frame, &reference.name)?;
                self.invoke_function(function, args, Some(frame))
            }
            _ if matches!(
                reference.final_resolution,
                Some(FinalReferenceResolution::ResolvedPath { .. })
            ) =>
            {
                let Some(FinalReferenceResolution::ResolvedPath { path, kind, .. }) =
                    reference.final_resolution.as_ref()
                else {
                    unreachable!("guard ensured resolved path final resolution");
                };
                if path_resolution_is_class(*kind) {
                    self.construct_class_from_path(path, args)
                } else {
                    self.load_and_invoke_external_function(path, args)
                }
            }
            _ => {
                if let Some(Value::Object(object)) = args.first() {
                    if object_has_method(object, &reference.name) {
                        return self.invoke_object_method_outputs(object, &reference.name, &args[1..]);
                    }
                }
                invoke_runtime_builtin_outputs(
                    &self.shared_state,
                    Some(frame),
                    &reference.name,
                    args,
                    output_arity,
                )
                .map_err(|_| {
                    RuntimeError::Unsupported(format!(
                        "call target `{}` is not executable in the current interpreter",
                        reference.name
                    ))
                })
            }
        }
    }

    fn invoke_workspace_builtin_outputs(
        &mut self,
        frame: &mut Frame<'a>,
        shared_state: &Rc<RefCell<SharedRuntimeState>>,
        name: &str,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        match name {
            "clear" => invoke_clear_builtin_outputs(frame, shared_state, args, output_arity),
            "clearvars" => invoke_clearvars_builtin_outputs(frame, args, output_arity),
            "save" => invoke_save_builtin_outputs(frame, shared_state, args, output_arity),
            "load" => invoke_load_builtin_outputs(frame, shared_state, args, output_arity),
            _ => Err(RuntimeError::Unsupported(format!(
                "workspace builtin `{name}` is not implemented in the current interpreter"
            ))),
        }
    }

    fn invoke_close_builtin_outputs(
        &mut self,
        frame: &Frame<'a>,
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

    fn invoke_close_function_handle_outputs(
        &mut self,
        frame: &Frame<'a>,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.invoke_close_builtin_outputs(frame, args, output_arity)
    }

    fn invoke_figure_close_callback(
        &mut self,
        frame: &Frame<'a>,
        callback: &Value,
    ) -> Result<(), RuntimeError> {
        if matches!(callback, Value::CharArray(text) | Value::String(text) if text.eq_ignore_ascii_case("close"))
            || matches!(callback, Value::FunctionHandle(handle) if handle.display_name.eq_ignore_ascii_case("close"))
        {
            return Err(RuntimeError::Unsupported(
                "figure `CloseRequestFcn` callbacks should use `closereq`, not `close`, to avoid recursive close handling"
                    .to_string(),
            ));
        }
        let callback_value = match callback {
            Value::FunctionHandle(_) => callback.clone(),
            Value::CharArray(text) | Value::String(text) => {
                if text.eq_ignore_ascii_case("close") {
                    return Err(RuntimeError::Unsupported(
                        "figure `CloseRequestFcn` callbacks should use `closereq`, not `close`, to avoid recursive close handling"
                            .to_string(),
                    ));
                }
                Value::FunctionHandle(FunctionHandleValue {
                    display_name: text.clone(),
                    target: FunctionHandleTarget::Named(text.clone()),
                })
            }
            _ => return Err(RuntimeError::Unsupported(
                "figure close callbacks currently support function handles or text function names"
                    .to_string(),
            )),
        };
        self.call_function_value_outputs(frame, &callback_value, &[], Some(0))?;
        Ok(())
    }

    fn invoke_graphics_builtin_with_resize_callbacks(
        &mut self,
        frame: &Frame<'a>,
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
            let callback_result = self.invoke_figure_resize_callback(frame, handle, &callback);
            {
                let mut state = self.shared_state.borrow_mut();
                state.active_resize_callbacks.remove(&handle);
            }
            callback_result?;
        }
        flush_figure_backend(&self.shared_state);
        Ok(result)
    }

    fn drain_pending_host_figure_events(&mut self, frame: &Frame<'a>) -> Result<(), RuntimeError> {
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
                    let callback_result =
                        self.invoke_figure_resize_callback(frame, handle, &callback);
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
        frame: &Frame<'a>,
        _handle: u32,
        callback: &Value,
    ) -> Result<(), RuntimeError> {
        let callback_value = match callback {
            Value::FunctionHandle(handle) => Value::FunctionHandle(handle.clone()),
            Value::CharArray(text) | Value::String(text) => {
                Value::FunctionHandle(FunctionHandleValue {
                    display_name: text.clone(),
                    target: FunctionHandleTarget::Named(text.clone()),
                })
            }
            _ => return Err(RuntimeError::Unsupported(
                "figure resize callbacks currently support function handles or text function names"
                    .to_string(),
            )),
        };
        self.call_function_value_outputs(frame, &callback_value, &[], Some(0))?;
        Ok(())
    }

    fn invoke_fplot_builtin_outputs(
        &mut self,
        frame: &Frame<'a>,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let spec = parse_fplot_spec(args)?;
        let x_values = sampled_fplot_x_values(spec.interval, spec.sample_count);
        let x_arg = fplot_vector_value(&x_values)?;
        let function_value = normalize_fplot_function_arg(&spec.function, "fplot")?;
        let mut outputs =
            self.call_function_value_outputs(frame, &function_value, &[x_arg.clone()], Some(1))?;
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
        frame: &Frame<'a>,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let spec = parse_fplot3_spec(args)?;
        let x_values = sampled_fplot_x_values(spec.interval, spec.sample_count);
        let x_arg = fplot_vector_value(&x_values)?;
        let x_function = normalize_fplot_function_arg(&spec.x_function, "fplot3")?;
        let y_function = normalize_fplot_function_arg(&spec.y_function, "fplot3")?;
        let z_function = normalize_fplot_function_arg(&spec.z_function, "fplot3")?;

        let y_values = {
            let mut outputs =
                self.call_function_value_outputs(frame, &y_function, &[x_arg.clone()], Some(1))?;
            let y_value = outputs.pop().ok_or_else(|| {
                RuntimeError::Unsupported("fplot3 Y function did not produce an output".to_string())
            })?;
            fplot_numeric_output_values(&y_value, x_values.len(), "fplot3")?
        };
        let z_values = {
            let mut outputs =
                self.call_function_value_outputs(frame, &z_function, &[x_arg.clone()], Some(1))?;
            let z_value = outputs.pop().ok_or_else(|| {
                RuntimeError::Unsupported("fplot3 Z function did not produce an output".to_string())
            })?;
            fplot_numeric_output_values(&z_value, x_values.len(), "fplot3")?
        };

        let mut plot_args = vec![
            {
                let mut outputs = self.call_function_value_outputs(
                    frame,
                    &x_function,
                    &[x_arg.clone()],
                    Some(1),
                )?;
                let x_value = outputs.pop().ok_or_else(|| {
                    RuntimeError::Unsupported(
                        "fplot3 X function did not produce an output".to_string(),
                    )
                })?;
                fplot_vector_value(&fplot_numeric_output_values(
                    &x_value,
                    x_values.len(),
                    "fplot3",
                )?)?
            },
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
        frame: &Frame<'a>,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.invoke_function_surface_builtin_outputs(frame, args, output_arity, "fsurf", "surf")
    }

    fn invoke_fmesh_builtin_outputs(
        &mut self,
        frame: &Frame<'a>,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.invoke_function_surface_builtin_outputs(frame, args, output_arity, "fmesh", "mesh")
    }

    fn invoke_fimplicit_builtin_outputs(
        &mut self,
        frame: &Frame<'a>,
        args: &[Value],
        output_arity: usize,
    ) -> Result<Vec<Value>, RuntimeError> {
        let spec = parse_fimplicit_spec(args)?;
        let (rows, cols, x_values, y_values, x_grid, y_grid) =
            sampled_surface_grid(spec.domain, spec.sample_count);
        let x_arg = surface_matrix_value(rows, cols, &x_grid)?;
        let y_arg = surface_matrix_value(rows, cols, &y_grid)?;
        let function_value = normalize_fplot_function_arg(&spec.function, "fimplicit")?;
        let mut outputs =
            self.call_function_value_outputs(frame, &function_value, &[x_arg, y_arg], Some(1))?;
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
        frame: &Frame<'a>,
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
        frame: &Frame<'a>,
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
        frame: &Frame<'a>,
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
        let mut outputs =
            self.call_function_value_outputs(frame, &function_value, &[x_arg, y_arg], Some(1))?;
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
        frame: &Frame<'a>,
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
        let mut outputs =
            self.call_function_value_outputs(frame, &function_value, &[x_arg, y_arg], Some(1))?;
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

    fn call_function_value_outputs(
        &mut self,
        frame: &Frame<'a>,
        target: &Value,
        args: &[Value],
        requested_outputs: Option<usize>,
    ) -> Result<Vec<Value>, RuntimeError> {
        match target {
            Value::FunctionHandle(handle) => match &handle.target {
                FunctionHandleTarget::Named(name) => {
                    if let Some(closure) = self.anonymous_functions.get(name).cloned() {
                        Ok(vec![self.invoke_anonymous(closure, args)?])
                    } else if let Some(function) = frame.visible_functions.get(name).copied() {
                        self.invoke_function(function, args, Some(frame))
                    } else {
                        if name == "close" {
                            return self.invoke_close_function_handle_outputs(
                                frame,
                                args,
                                requested_outputs.unwrap_or(1),
                            );
                        }
                        if name == "arrayfun" {
                            return execute_arrayfun_builtin_outputs(
                                args,
                                requested_outputs.unwrap_or(1),
                                |callback, call_args, requested| {
                                    self.call_function_value_outputs(
                                        frame,
                                        callback,
                                        call_args,
                                        Some(requested),
                                    )
                                },
                            );
                        }
                        if name == "cellfun" {
                            return execute_cellfun_builtin_outputs(
                                args,
                                requested_outputs.unwrap_or(1),
                                |callback, call_args, requested| {
                                    self.call_function_value_outputs(
                                        frame,
                                        callback,
                                        call_args,
                                        Some(requested),
                                    )
                                },
                            );
                        }
                        if name == "structfun" {
                            return execute_structfun_builtin_outputs(
                                args,
                                requested_outputs.unwrap_or(1),
                                |callback, call_args, requested| {
                                    self.call_function_value_outputs(
                                        frame,
                                        callback,
                                        call_args,
                                        Some(requested),
                                    )
                                },
                            );
                        }
                        if matches!(name.as_str(), "figure" | "set") {
                            return self.invoke_graphics_builtin_with_resize_callbacks(
                                frame,
                                name,
                                args,
                                requested_outputs.unwrap_or(1),
                            );
                        }
                        invoke_runtime_builtin_outputs(
                            &self.shared_state,
                            Some(frame),
                            name,
                            args,
                            requested_outputs.unwrap_or(1),
                        ).map_err(
                            |_| {
                                RuntimeError::Unsupported(format!(
                                "function handle `{}` is not executable in the current interpreter",
                                handle.display_name
                            ))
                            },
                        )
                    }
                }
                FunctionHandleTarget::ResolvedPath(path) => {
                    self.load_and_invoke_external_function(path, args)
                }
                FunctionHandleTarget::BundleModule(module_id) => Err(RuntimeError::Unsupported(
                    format!(
                        "bundle-backed function handle `{}` ({module_id}) is only executable in the bytecode VM",
                        handle.display_name
                    ),
                )),
                FunctionHandleTarget::BoundMethod { receiver, method_name, .. } => {
                    let Value::Object(object) = receiver.as_ref() else {
                        return Err(RuntimeError::Unsupported(format!(
                            "bound method handle `{}` does not carry an object receiver",
                            handle.display_name
                        )));
                    };
                    self.invoke_object_method_outputs(object, method_name, args)
                }
            },
            _ => Err(RuntimeError::Unsupported(format!(
                "value `{}` is not invocable as a function in the current interpreter",
                target.kind_name()
            ))),
        }
    }

    fn resolve_named_function(
        &self,
        frame: &Frame<'a>,
        name: &str,
    ) -> Result<&'a HirFunction, RuntimeError> {
        frame.visible_functions.get(name).copied().ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "function `{name}` is not available in the current runtime scope"
            ))
        })
    }

    fn load_and_invoke_external_function(
        &mut self,
        path: &Path,
        args: &[Value],
    ) -> Result<Vec<Value>, RuntimeError> {
        let source = fs::read_to_string(path).map_err(|error| {
            RuntimeError::Unsupported(format!(
                "failed to read external function `{}`: {error}",
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
        let mut interpreter = Interpreter::with_shared_state(
            &hir,
            path.display().to_string(),
            Rc::clone(&self.shared_state),
            self.call_stack.clone(),
        );
        Ok(interpreter
            .invoke_primary_function(args)?
            .into_iter()
            .map(|(_, value)| value)
            .collect())
    }
}

fn values_len(value: &Value) -> Result<usize, RuntimeError> {
    match value {
        Value::Matrix(matrix) => Ok(matrix.element_count()),
        other => Err(RuntimeError::TypeError(format!(
            "expected matrix assignment payload, found {}",
            other.kind_name()
        ))),
    }
}

fn empty_struct_assignment_target_row(count: usize) -> Result<Value, RuntimeError> {
    Ok(Value::Matrix(MatrixValue::with_dimensions(
        1,
        count,
        vec![1, count],
        vec![Value::Struct(StructValue::default()); count],
    )?))
}

impl<'a> Frame<'a> {
    fn new(visible_functions: BTreeMap<String, &'a HirFunction>) -> Self {
        Self {
            cells: HashMap::new(),
            names: BTreeMap::new(),
            global_names: BTreeSet::new(),
            persistent_names: BTreeSet::new(),
            visible_functions,
        }
    }

    fn declare_binding(&mut self, binding: &HirBinding) -> Result<(), RuntimeError> {
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

    fn inherit_hidden_cells_from(&mut self, caller: &Frame<'a>) {
        for (binding_id, cell) in &caller.cells {
            self.cells
                .entry(*binding_id)
                .or_insert_with(|| cell.clone());
        }
    }

    fn assign_binding(&mut self, binding: &HirBinding, value: Value) -> Result<(), RuntimeError> {
        self.declare_binding(binding)?;
        let binding_id = binding
            .binding_id
            .expect("declare_binding checked binding id");
        if let Some(cell) = self.cells.get(&binding_id) {
            *cell.borrow_mut() = Some(value);
        }
        Ok(())
    }

    fn read_binding(&self, binding: &HirBinding) -> Result<Value, RuntimeError> {
        self.read_reference(binding.binding_id, &binding.name)
    }

    fn implicit_binding(&self, name: &str) -> Option<HirBinding> {
        self.names.get(name).copied().map(|binding_id| HirBinding {
            name: name.to_string(),
            symbol_kind: matlab_semantics::symbols::SymbolKind::Variable,
            binding_id: Some(binding_id),
            storage: Some(BindingStorage::Local),
        })
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

    fn ensure_reference_cell(&mut self, binding_id: Option<BindingId>, name: &str) -> Option<Cell> {
        if let Some(binding_id) = binding_id {
            self.names.entry(name.to_string()).or_insert(binding_id);
            return Some(
                self.cells
                    .entry(binding_id)
                    .or_insert_with(|| Rc::new(RefCell::new(None)))
                    .clone(),
            );
        }

        self.cell_for_reference(binding_id, name)
    }

    fn lvalue_root(
        &mut self,
        target: &HirExpression,
    ) -> Result<(String, Cell, Vec<LValueProjection>), RuntimeError> {
        match target {
            HirExpression::ValueRef(reference) => self
                .ensure_reference_cell(reference.binding_id, &reference.name)
                .map(|cell| (reference.name.clone(), cell, Vec::new()))
                .ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "variable `{}` is not defined",
                        reference.name
                    ))
                }),
            HirExpression::FieldAccess { target, field } => {
                let (name, cell, mut projections) = self.lvalue_root(target)?;
                projections.push(LValueProjection::Field(field.clone()));
                Ok((name, cell, projections))
            }
            HirExpression::CellIndex { target, indices } => {
                let (name, cell, mut projections) = self.lvalue_root(target)?;
                projections.push(LValueProjection::Brace(indices.clone()));
                Ok((name, cell, projections))
            }
            HirExpression::Call { target, args } => match target {
                HirCallTarget::Callable(reference)
                    if reference.semantic_resolution == ReferenceResolution::WorkspaceValue =>
                {
                    self.ensure_reference_cell(reference.binding_id, &reference.name)
                        .map(|cell| {
                            (
                                reference.name.clone(),
                                cell,
                                vec![LValueProjection::Paren(args.clone())],
                            )
                        })
                        .ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "variable `{}` is not defined",
                                reference.name
                            ))
                        })
                }
                HirCallTarget::Expression(expression) => {
                    let (name, cell, mut projections) = self.lvalue_root(expression)?;
                    projections.push(LValueProjection::Paren(args.clone()));
                    Ok((name, cell, projections))
                }
                HirCallTarget::Callable(reference) => Err(RuntimeError::Unsupported(format!(
                    "call target `{}` is not assignable in the current interpreter",
                    reference.name
                ))),
            },
            _ => Err(RuntimeError::Unsupported(
                "assignment currently requires a binding, indexable value, or nested field target"
                    .to_string(),
            )),
        }
    }
}

fn iteration_values(value: &Value) -> Result<Vec<Value>, RuntimeError> {
    match value {
        Value::Scalar(_)
        | Value::Complex(_)
        | Value::Logical(_)
        | Value::CharArray(_)
        | Value::String(_)
        | Value::Struct(_)
        | Value::Object(_)
        | Value::FunctionHandle(_) => Ok(vec![value.clone()]),
        Value::Matrix(matrix) => Ok(matrix.iter().cloned().collect()),
        Value::Cell(cell) => Ok(cell.iter().cloned().collect()),
    }
}

fn map_numeric_unary(
    value: &Value,
    op: impl Fn(NumericComplexParts) -> NumericComplexParts,
) -> Result<Value, RuntimeError> {
    let operand = numeric_or_complex_operand(value)?;
    if operand.rows == 1 && operand.cols == 1 {
        return Ok(value_from_numeric_complex_parts(op(operand.values[0])));
    }

    Ok(Value::Matrix(MatrixValue::new(
        operand.rows,
        operand.cols,
        operand
            .values
            .into_iter()
            .map(|value| value_from_numeric_complex_parts(op(value)))
            .collect(),
    )?))
}

fn map_numeric_unary_logical(
    value: &Value,
    op: impl Fn(f64) -> bool,
) -> Result<Value, RuntimeError> {
    let operand = numeric_operand(value)?;
    if operand.rows == 1 && operand.cols == 1 {
        return Ok(logical_value(op(operand.values[0])));
    }

    Ok(Value::Matrix(MatrixValue::new(
        operand.rows,
        operand.cols,
        operand
            .values
            .into_iter()
            .map(|value| logical_value(op(value)))
            .collect(),
    )?))
}

fn map_numeric_binary(
    lhs: &Value,
    rhs: &Value,
    op: impl Fn(NumericComplexParts, NumericComplexParts) -> NumericComplexParts,
) -> Result<Value, RuntimeError> {
    let lhs = numeric_or_complex_operand(lhs)?;
    let rhs = numeric_or_complex_operand(rhs)?;
    let (rows, cols) = broadcast_numeric_or_complex_shape(&lhs, &rhs)?;
    let element_count = rows * cols;

    let values = (0..element_count)
        .map(|offset| {
            value_from_numeric_complex_parts(op(
                lhs.values[numeric_or_complex_offset(&lhs, rows, cols, offset)],
                rhs.values[numeric_or_complex_offset(&rhs, rows, cols, offset)],
            ))
        })
        .collect::<Vec<_>>();

    if rows == 1 && cols == 1 {
        Ok(values
            .into_iter()
            .next()
            .expect("scalar result has one element"))
    } else {
        Ok(Value::Matrix(MatrixValue::new(rows, cols, values)?))
    }
}

fn map_numeric_binary_equality(
    lhs: &Value,
    rhs: &Value,
    op: impl Fn(NumericComplexParts, NumericComplexParts) -> bool,
) -> Result<Value, RuntimeError> {
    let lhs = numeric_or_complex_operand(lhs)?;
    let rhs = numeric_or_complex_operand(rhs)?;
    let (rows, cols) = broadcast_numeric_or_complex_shape(&lhs, &rhs)?;
    let element_count = rows * cols;

    let values = (0..element_count)
        .map(|offset| {
            let lhs_value = lhs.values[numeric_or_complex_offset(&lhs, rows, cols, offset)];
            let rhs_value = rhs.values[numeric_or_complex_offset(&rhs, rows, cols, offset)];
            logical_value(op(lhs_value, rhs_value))
        })
        .collect::<Vec<_>>();

    if rows == 1 && cols == 1 {
        Ok(values
            .into_iter()
            .next()
            .expect("scalar result has one element"))
    } else {
        Ok(Value::Matrix(MatrixValue::new(rows, cols, values)?))
    }
}

fn map_numeric_binary_logical(
    lhs: &Value,
    rhs: &Value,
    op: impl Fn(f64, f64) -> bool,
) -> Result<Value, RuntimeError> {
    let lhs = numeric_operand(lhs)?;
    let rhs = numeric_operand(rhs)?;
    let (rows, cols) = broadcast_numeric_shape(&lhs, &rhs)?;
    let element_count = rows * cols;

    let values = (0..element_count)
        .map(|offset| {
            logical_value(op(
                lhs.values[lhs_offset(&lhs, rows, cols, offset)],
                rhs.values[lhs_offset(&rhs, rows, cols, offset)],
            ))
        })
        .collect::<Vec<_>>();

    if rows == 1 && cols == 1 {
        Ok(values
            .into_iter()
            .next()
            .expect("scalar result has one element"))
    } else {
        Ok(Value::Matrix(MatrixValue::new(rows, cols, values)?))
    }
}

#[derive(Debug, Clone)]
struct NumericOperand {
    rows: usize,
    cols: usize,
    values: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
struct NumericComplexParts {
    real: f64,
    imag: f64,
}

impl NumericComplexParts {
    fn zero() -> Self {
        Self {
            real: 0.0,
            imag: 0.0,
        }
    }

    fn plus(self, rhs: Self) -> Self {
        Self {
            real: self.real + rhs.real,
            imag: self.imag + rhs.imag,
        }
    }

    fn minus(self, rhs: Self) -> Self {
        Self {
            real: self.real - rhs.real,
            imag: self.imag - rhs.imag,
        }
    }

    fn times(self, rhs: Self) -> Self {
        Self {
            real: (self.real * rhs.real) - (self.imag * rhs.imag),
            imag: (self.real * rhs.imag) + (self.imag * rhs.real),
        }
    }

    fn rdivide(self, rhs: Self) -> Self {
        let denominator = (rhs.real * rhs.real) + (rhs.imag * rhs.imag);
        Self {
            real: ((self.real * rhs.real) + (self.imag * rhs.imag)) / denominator,
            imag: ((self.imag * rhs.real) - (self.real * rhs.imag)) / denominator,
        }
    }

    fn conjugate(self) -> Self {
        Self {
            real: self.real,
            imag: -self.imag,
        }
    }

    fn conjugate_if(self, conjugate: bool) -> Self {
        if conjugate {
            self.conjugate()
        } else {
            self
        }
    }

    fn magnitude(self) -> f64 {
        self.real.hypot(self.imag)
    }

    fn argument(self) -> f64 {
        self.imag.atan2(self.real)
    }

    fn exp(self) -> Self {
        let scale = self.real.exp();
        Self {
            real: scale * self.imag.cos(),
            imag: scale * self.imag.sin(),
        }
    }

    fn ln(self) -> Self {
        Self {
            real: self.magnitude().ln(),
            imag: self.argument(),
        }
    }

    fn pow(self, rhs: Self) -> Self {
        if self.real == 0.0 && self.imag == 0.0 {
            if rhs.real == 0.0 && rhs.imag == 0.0 {
                return Self {
                    real: 1.0,
                    imag: 0.0,
                };
            }
            if rhs.imag == 0.0 && rhs.real > 0.0 {
                return Self::zero();
            }
        }

        rhs.times(self.ln()).exp()
    }

    fn exact_eq(self, rhs: Self) -> bool {
        self.real == rhs.real && self.imag == rhs.imag
    }

    fn exact_ne(self, rhs: Self) -> bool {
        !self.exact_eq(rhs)
    }
}

#[derive(Debug, Clone)]
struct NumericOrComplexOperand {
    rows: usize,
    cols: usize,
    values: Vec<NumericComplexParts>,
}

fn numeric_operand(value: &Value) -> Result<NumericOperand, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(NumericOperand {
            rows: 1,
            cols: 1,
            values: vec![*number],
        }),
        Value::Logical(flag) => Ok(NumericOperand {
            rows: 1,
            cols: 1,
            values: vec![truth_number(*flag)],
        }),
        Value::Matrix(matrix) => Ok(NumericOperand {
            rows: matrix.rows,
            cols: matrix.cols,
            values: matrix
                .elements
                .iter()
                .map(Value::as_scalar)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        other => Err(RuntimeError::TypeError(format!(
            "numeric operation expects scalar or matrix input, found {}",
            other.kind_name()
        ))),
    }
}

fn numeric_or_complex_scalar(value: &Value) -> Result<NumericComplexParts, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(NumericComplexParts {
            real: *number,
            imag: 0.0,
        }),
        Value::Logical(flag) => Ok(NumericComplexParts {
            real: truth_number(*flag),
            imag: 0.0,
        }),
        Value::Complex(number) => Ok(NumericComplexParts {
            real: number.real,
            imag: number.imag,
        }),
        other => Err(RuntimeError::TypeError(format!(
            "numeric operation expects numeric, logical, or complex scalar/matrix input, found {}",
            other.kind_name()
        ))),
    }
}

fn numeric_or_complex_operand(value: &Value) -> Result<NumericOrComplexOperand, RuntimeError> {
    match value {
        Value::Scalar(_) | Value::Logical(_) | Value::Complex(_) => Ok(NumericOrComplexOperand {
            rows: 1,
            cols: 1,
            values: vec![numeric_or_complex_scalar(value)?],
        }),
        Value::Matrix(matrix) => Ok(NumericOrComplexOperand {
            rows: matrix.rows,
            cols: matrix.cols,
            values: matrix
                .iter()
                .map(numeric_or_complex_scalar)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        other => Err(RuntimeError::TypeError(format!(
            "numeric operation expects numeric, logical, or complex scalar/matrix input, found {}",
            other.kind_name()
        ))),
    }
}

fn broadcast_numeric_shape(
    lhs: &NumericOperand,
    rhs: &NumericOperand,
) -> Result<(usize, usize), RuntimeError> {
    if lhs.rows == rhs.rows && lhs.cols == rhs.cols {
        Ok((lhs.rows, lhs.cols))
    } else if lhs.rows == 1 && lhs.cols == 1 {
        Ok((rhs.rows, rhs.cols))
    } else if rhs.rows == 1 && rhs.cols == 1 {
        Ok((lhs.rows, lhs.cols))
    } else {
        Err(RuntimeError::ShapeError(format!(
            "binary numeric operation requires matching shapes or scalar expansion, found {}x{} and {}x{}",
            lhs.rows, lhs.cols, rhs.rows, rhs.cols
        )))
    }
}

fn broadcast_numeric_or_complex_shape(
    lhs: &NumericOrComplexOperand,
    rhs: &NumericOrComplexOperand,
) -> Result<(usize, usize), RuntimeError> {
    if lhs.rows == rhs.rows && lhs.cols == rhs.cols {
        Ok((lhs.rows, lhs.cols))
    } else if lhs.rows == 1 && lhs.cols == 1 {
        Ok((rhs.rows, rhs.cols))
    } else if rhs.rows == 1 && rhs.cols == 1 {
        Ok((lhs.rows, lhs.cols))
    } else {
        Err(RuntimeError::ShapeError(format!(
            "binary numeric operation requires matching shapes or scalar expansion, found {}x{} and {}x{}",
            lhs.rows, lhs.cols, rhs.rows, rhs.cols
        )))
    }
}

fn lhs_offset(operand: &NumericOperand, rows: usize, cols: usize, offset: usize) -> usize {
    if operand.rows == 1 && operand.cols == 1 {
        0
    } else {
        debug_assert_eq!(operand.rows, rows);
        debug_assert_eq!(operand.cols, cols);
        offset
    }
}

fn numeric_or_complex_offset(
    operand: &NumericOrComplexOperand,
    rows: usize,
    cols: usize,
    offset: usize,
) -> usize {
    if operand.rows == 1 && operand.cols == 1 {
        0
    } else {
        debug_assert_eq!(operand.rows, rows);
        debug_assert_eq!(operand.cols, cols);
        offset
    }
}

fn value_from_numeric_complex_parts(value: NumericComplexParts) -> Value {
    if value.imag == 0.0 {
        Value::Scalar(value.real)
    } else {
        Value::Complex(ComplexValue {
            real: value.real,
            imag: value.imag,
        })
    }
}

fn build_numeric_or_complex_matrix_result(
    rows: usize,
    cols: usize,
    values: Vec<NumericComplexParts>,
) -> Result<Value, RuntimeError> {
    if rows == 1 && cols == 1 {
        return Ok(value_from_numeric_complex_parts(
            values
                .into_iter()
                .next()
                .expect("scalar result has one element"),
        ));
    }

    Ok(Value::Matrix(MatrixValue::new(
        rows,
        cols,
        values
            .into_iter()
            .map(value_from_numeric_complex_parts)
            .collect(),
    )?))
}

fn numeric_or_complex_operand_to_value(
    value: &NumericOrComplexOperand,
) -> Result<Value, RuntimeError> {
    build_numeric_or_complex_matrix_result(value.rows, value.cols, value.values.clone())
}

fn transpose_value(value: &Value, conjugate: bool) -> Result<Value, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(Value::Scalar(*number)),
        Value::Logical(flag) => Ok(Value::Logical(*flag)),
        Value::Complex(number) => Ok(value_from_numeric_complex_parts(
            NumericComplexParts {
                real: number.real,
                imag: number.imag,
            }
            .conjugate_if(conjugate),
        )),
        Value::Matrix(matrix) => transpose_matrix_value(matrix, conjugate),
        Value::Cell(cell) => transpose_cell_value(cell),
        other => Err(RuntimeError::TypeError(format!(
            "transpose expects scalar, logical, complex, matrix, or cell input, found {}",
            other.kind_name()
        ))),
    }
}

fn ensure_transpose_supported(dims: &[usize], conjugate: bool) -> Result<(), RuntimeError> {
    let mut canonical = dims.to_vec();
    while canonical.len() > 2 && canonical.last() == Some(&1) {
        canonical.pop();
    }
    if canonical.len() > 2 {
        return Err(RuntimeError::Unsupported(if conjugate {
            "TRANSPOSE does not support N-D arrays. Use PAGETRANSPOSE/PAGECTRANSPOSE to transpose pages or PERMUTE to reorder dimensions of N-D arrays."
                .to_string()
        } else {
            "TRANSPOSE does not support N-D arrays. Use PAGETRANSPOSE/PAGECTRANSPOSE to transpose pages or PERMUTE to reorder dimensions of N-D arrays."
                .to_string()
        }));
    }
    Ok(())
}

fn matrix_multiply(lhs: &Value, rhs: &Value) -> Result<Value, RuntimeError> {
    let lhs = numeric_or_complex_operand(lhs)?;
    let rhs = numeric_or_complex_operand(rhs)?;

    if lhs.rows == 1 && lhs.cols == 1 {
        return build_numeric_or_complex_matrix_result(
            rhs.rows,
            rhs.cols,
            rhs.values
                .into_iter()
                .map(|value| lhs.values[0].times(value))
                .collect(),
        );
    }

    if rhs.rows == 1 && rhs.cols == 1 {
        return build_numeric_or_complex_matrix_result(
            lhs.rows,
            lhs.cols,
            lhs.values
                .into_iter()
                .map(|value| value.times(rhs.values[0]))
                .collect(),
        );
    }

    if lhs.cols != rhs.rows {
        return Err(RuntimeError::ShapeError(format!(
            "matrix multiply requires inner dimensions to agree, found {}x{} and {}x{}",
            lhs.rows, lhs.cols, rhs.rows, rhs.cols
        )));
    }

    let mut values = Vec::with_capacity(lhs.rows * rhs.cols);
    for row in 0..lhs.rows {
        for col in 0..rhs.cols {
            let mut total = NumericComplexParts::zero();
            for inner in 0..lhs.cols {
                total = total.plus(
                    lhs.values[row * lhs.cols + inner].times(rhs.values[inner * rhs.cols + col]),
                );
            }
            values.push(total);
        }
    }

    build_numeric_or_complex_matrix_result(lhs.rows, rhs.cols, values)
}

fn matrix_power(lhs: &Value, rhs: &Value) -> Result<Value, RuntimeError> {
    let lhs = numeric_or_complex_operand(lhs)?;
    let rhs = numeric_or_complex_operand(rhs)?;

    if lhs.rows == 1 && lhs.cols == 1 {
        if rhs.rows != 1 || rhs.cols != 1 {
            return Err(RuntimeError::ShapeError(
                "scalar base power currently requires a scalar exponent".to_string(),
            ));
        }
        return Ok(value_from_numeric_complex_parts(
            normalize_numeric_complex_parts(lhs.values[0].pow(rhs.values[0])),
        ));
    }

    if rhs.rows != 1 || rhs.cols != 1 {
        return Err(RuntimeError::ShapeError(
            "matrix power currently requires a scalar exponent".to_string(),
        ));
    }

    if lhs.rows != lhs.cols {
        return Err(RuntimeError::ShapeError(format!(
            "matrix power currently requires a square base, found {}x{}",
            lhs.rows, lhs.cols
        )));
    }

    let exponent = rhs.values[0];
    if exponent.imag != 0.0 || exponent.real.fract() != 0.0 {
        return Err(RuntimeError::Unsupported(
            "matrix power currently requires an integer real exponent for matrix bases".to_string(),
        ));
    }

    let powered = matrix_power_operand(&lhs, exponent.real as i64)?;
    numeric_or_complex_operand_to_value(&powered)
}

fn matrix_left_divide(lhs: &Value, rhs: &Value) -> Result<Value, RuntimeError> {
    let lhs = numeric_or_complex_operand(lhs)?;
    let rhs = numeric_or_complex_operand(rhs)?;

    if lhs.rows == 1 && lhs.cols == 1 {
        return build_numeric_or_complex_matrix_result(
            rhs.rows,
            rhs.cols,
            rhs.values
                .into_iter()
                .map(|value| value.rdivide(lhs.values[0]))
                .collect(),
        );
    }

    if lhs.rows != rhs.rows {
        return Err(RuntimeError::ShapeError(format!(
            "matrix left divide requires matching row counts, found {}x{} and {}x{}",
            lhs.rows, lhs.cols, rhs.rows, rhs.cols
        )));
    }

    let solution = matrix_left_divide_operands(&lhs, &rhs, "matrix left divide")?;
    numeric_or_complex_operand_to_value(&solution)
}

fn matrix_right_divide(lhs: &Value, rhs: &Value) -> Result<Value, RuntimeError> {
    let lhs = numeric_or_complex_operand(lhs)?;
    let rhs = numeric_or_complex_operand(rhs)?;

    if rhs.rows == 1 && rhs.cols == 1 {
        return build_numeric_or_complex_matrix_result(
            lhs.rows,
            lhs.cols,
            lhs.values
                .into_iter()
                .map(|value| value.rdivide(rhs.values[0]))
                .collect(),
        );
    }

    if lhs.cols != rhs.cols {
        return Err(RuntimeError::ShapeError(format!(
            "matrix right divide requires matching column counts, found {}x{} and {}x{}",
            lhs.rows, lhs.cols, rhs.rows, rhs.cols
        )));
    }

    let rhs_transposed = transpose_numeric_or_complex_operand(&rhs, false);
    let lhs_transposed = transpose_numeric_or_complex_operand(&lhs, false);
    let solution =
        matrix_left_divide_operands(&rhs_transposed, &lhs_transposed, "matrix right divide")?;
    numeric_or_complex_operand_to_value(&transpose_numeric_or_complex_operand(&solution, false))
}

fn matrix_left_divide_operands(
    lhs: &NumericOrComplexOperand,
    rhs: &NumericOrComplexOperand,
    context: &str,
) -> Result<NumericOrComplexOperand, RuntimeError> {
    if lhs.rows == lhs.cols {
        if let Ok(solution) = solve_square_linear_system(lhs, rhs, context) {
            return Ok(solution);
        }
        if let Ok(solution) = solve_square_or_singular_system_basic(lhs, rhs, context) {
            return Ok(solution);
        }
    }

    let lhs_conjugate_transpose = transpose_numeric_or_complex_operand(lhs, true);
    if lhs.rows > lhs.cols {
        let normal_lhs = matrix_multiply_operands(&lhs_conjugate_transpose, lhs, context)?;
        let normal_rhs = matrix_multiply_operands(&lhs_conjugate_transpose, rhs, context)?;
        if let Ok(solution) = solve_square_linear_system(&normal_lhs, &normal_rhs, context) {
            return Ok(solution);
        }
    } else {
        let normal_lhs = matrix_multiply_operands(lhs, &lhs_conjugate_transpose, context)?;
        if let Ok(reduced_rhs) = solve_square_linear_system(&normal_lhs, rhs, context) {
            return matrix_multiply_operands(&lhs_conjugate_transpose, &reduced_rhs, context);
        }
    }

    least_squares_basic_solution(lhs, rhs, context)
}

fn transpose_matrix_value(matrix: &MatrixValue, conjugate: bool) -> Result<Value, RuntimeError> {
    ensure_transpose_supported(&matrix.dims, conjugate)?;
    let mut elements = Vec::with_capacity(matrix.elements.len());
    for row in 0..matrix.cols {
        for col in 0..matrix.rows {
            elements.push(transpose_element_value(matrix.get(col, row), conjugate)?);
        }
    }
    Ok(Value::Matrix(MatrixValue::new(
        matrix.cols,
        matrix.rows,
        elements,
    )?))
}

fn transpose_numeric_or_complex_operand(
    value: &NumericOrComplexOperand,
    conjugate: bool,
) -> NumericOrComplexOperand {
    let mut values = Vec::with_capacity(value.values.len());
    for col in 0..value.cols {
        for row in 0..value.rows {
            values.push(value.values[row * value.cols + col].conjugate_if(conjugate));
        }
    }
    NumericOrComplexOperand {
        rows: value.cols,
        cols: value.rows,
        values,
    }
}

fn transpose_cell_value(cell: &CellValue) -> Result<Value, RuntimeError> {
    ensure_transpose_supported(&cell.dims, false)?;
    let mut elements = Vec::with_capacity(cell.elements.len());
    for row in 0..cell.cols {
        for col in 0..cell.rows {
            elements.push(cell.get(col, row).clone());
        }
    }
    Ok(Value::Cell(CellValue::new(cell.cols, cell.rows, elements)?))
}

fn transpose_element_value(value: &Value, conjugate: bool) -> Result<Value, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(Value::Scalar(*number)),
        Value::Logical(flag) => Ok(Value::Logical(*flag)),
        Value::Complex(number) => Ok(value_from_numeric_complex_parts(
            NumericComplexParts {
                real: number.real,
                imag: number.imag,
            }
            .conjugate_if(conjugate),
        )),
        other => Ok(other.clone()),
    }
}

fn solve_square_linear_system(
    lhs: &NumericOrComplexOperand,
    rhs: &NumericOrComplexOperand,
    context: &str,
) -> Result<NumericOrComplexOperand, RuntimeError> {
    let n = lhs.rows;
    let m = rhs.cols;
    let mut a = lhs.values.clone();
    let mut b = rhs.values.clone();

    for pivot in 0..n {
        let mut pivot_row = pivot;
        let mut pivot_norm = a[pivot * n + pivot].real.abs() + a[pivot * n + pivot].imag.abs();
        for row in (pivot + 1)..n {
            let candidate = a[row * n + pivot];
            let candidate_norm = candidate.real.abs() + candidate.imag.abs();
            if candidate_norm > pivot_norm {
                pivot_row = row;
                pivot_norm = candidate_norm;
            }
        }

        if pivot_norm == 0.0 {
            return Err(RuntimeError::ShapeError(format!(
                "{context} encountered a singular matrix"
            )));
        }

        if pivot_row != pivot {
            for col in 0..n {
                a.swap(pivot * n + col, pivot_row * n + col);
            }
            for col in 0..m {
                b.swap(pivot * m + col, pivot_row * m + col);
            }
        }

        let diagonal = a[pivot * n + pivot];
        for row in (pivot + 1)..n {
            let factor = a[row * n + pivot].rdivide(diagonal);
            a[row * n + pivot] = NumericComplexParts::zero();
            for col in (pivot + 1)..n {
                a[row * n + col] = a[row * n + col].minus(factor.times(a[pivot * n + col]));
            }
            for col in 0..m {
                b[row * m + col] = b[row * m + col].minus(factor.times(b[pivot * m + col]));
            }
        }
    }

    let mut x = vec![NumericComplexParts::zero(); n * m];
    for row in (0..n).rev() {
        let diagonal = a[row * n + row];
        if diagonal.real == 0.0 && diagonal.imag == 0.0 {
            return Err(RuntimeError::ShapeError(format!(
                "{context} encountered a singular matrix"
            )));
        }
        for col in 0..m {
            let mut total = b[row * m + col];
            for inner in (row + 1)..n {
                total = total.minus(a[row * n + inner].times(x[inner * m + col]));
            }
            x[row * m + col] = normalize_numeric_complex_parts(total.rdivide(diagonal));
        }
    }

    Ok(NumericOrComplexOperand {
        rows: n,
        cols: m,
        values: x,
    })
}

#[derive(Debug, Clone)]
struct ReducedRowEchelonForm {
    values: Vec<NumericComplexParts>,
    pivot_columns: Vec<usize>,
}

fn solve_square_or_singular_system_basic(
    lhs: &NumericOrComplexOperand,
    rhs: &NumericOrComplexOperand,
    context: &str,
) -> Result<NumericOrComplexOperand, RuntimeError> {
    let tolerance = default_rref_tolerance(lhs).max(1e-12);
    let augmented = augmented_numeric_or_complex_operand(lhs, rhs);
    let reduced = reduced_row_echelon_form(&augmented, tolerance);
    for row in 0..lhs.rows {
        let lhs_zero = (0..lhs.cols)
            .all(|col| reduced.values[row * augmented.cols + col].magnitude() <= tolerance);
        let rhs_nonzero = (0..rhs.cols).any(|col| {
            reduced.values[row * augmented.cols + lhs.cols + col].magnitude() > tolerance
        });
        if lhs_zero && rhs_nonzero {
            return Err(RuntimeError::ShapeError(format!(
                "{context} encountered an inconsistent singular system"
            )));
        }
    }

    let mut values = vec![NumericComplexParts::zero(); lhs.cols * rhs.cols];
    for (pivot_row, &pivot_col) in reduced
        .pivot_columns
        .iter()
        .enumerate()
        .filter(|(_, pivot_col)| **pivot_col < lhs.cols)
    {
        for col in 0..rhs.cols {
            values[pivot_col * rhs.cols + col] = normalize_numeric_complex_parts(
                reduced.values[pivot_row * augmented.cols + lhs.cols + col],
            );
        }
    }

    Ok(NumericOrComplexOperand {
        rows: lhs.cols,
        cols: rhs.cols,
        values,
    })
}

fn least_squares_basic_solution(
    lhs: &NumericOrComplexOperand,
    rhs: &NumericOrComplexOperand,
    context: &str,
) -> Result<NumericOrComplexOperand, RuntimeError> {
    let lhs_conjugate_transpose = transpose_numeric_or_complex_operand(lhs, true);
    if lhs.rows >= lhs.cols {
        let normal_lhs = matrix_multiply_operands(&lhs_conjugate_transpose, lhs, context)?;
        let normal_rhs = matrix_multiply_operands(&lhs_conjugate_transpose, rhs, context)?;
        solve_square_or_singular_system_basic(&normal_lhs, &normal_rhs, context)
    } else {
        let normal_lhs = matrix_multiply_operands(lhs, &lhs_conjugate_transpose, context)?;
        let reduced_rhs = solve_square_or_singular_system_basic(&normal_lhs, rhs, context)?;
        matrix_multiply_operands(&lhs_conjugate_transpose, &reduced_rhs, context)
    }
}

fn augmented_numeric_or_complex_operand(
    lhs: &NumericOrComplexOperand,
    rhs: &NumericOrComplexOperand,
) -> NumericOrComplexOperand {
    let mut values = Vec::with_capacity(lhs.rows * (lhs.cols + rhs.cols));
    for row in 0..lhs.rows {
        values.extend_from_slice(&lhs.values[row * lhs.cols..(row + 1) * lhs.cols]);
        values.extend_from_slice(&rhs.values[row * rhs.cols..(row + 1) * rhs.cols]);
    }
    NumericOrComplexOperand {
        rows: lhs.rows,
        cols: lhs.cols + rhs.cols,
        values,
    }
}

fn reduced_row_echelon_form(
    value: &NumericOrComplexOperand,
    tolerance: f64,
) -> ReducedRowEchelonForm {
    let mut values = value.values.clone();
    let mut pivot_columns = Vec::new();
    let mut pivot_row = 0usize;

    for pivot_col in 0..value.cols {
        if pivot_row >= value.rows {
            break;
        }

        let mut best_row = pivot_row;
        let mut best_norm = values[pivot_row * value.cols + pivot_col].magnitude();
        for row in (pivot_row + 1)..value.rows {
            let candidate = values[row * value.cols + pivot_col].magnitude();
            if candidate > best_norm {
                best_row = row;
                best_norm = candidate;
            }
        }

        if best_norm <= tolerance {
            continue;
        }

        if best_row != pivot_row {
            for col in 0..value.cols {
                values.swap(pivot_row * value.cols + col, best_row * value.cols + col);
            }
        }

        let pivot = values[pivot_row * value.cols + pivot_col];
        for col in pivot_col..value.cols {
            values[pivot_row * value.cols + col] = normalize_numeric_complex_parts(
                values[pivot_row * value.cols + col].rdivide(pivot),
            );
        }

        for row in 0..value.rows {
            if row == pivot_row {
                continue;
            }
            let factor = values[row * value.cols + pivot_col];
            if factor.magnitude() <= tolerance {
                values[row * value.cols + pivot_col] = NumericComplexParts::zero();
                continue;
            }
            for col in pivot_col..value.cols {
                values[row * value.cols + col] = normalize_numeric_complex_parts(
                    values[row * value.cols + col]
                        .minus(factor.times(values[pivot_row * value.cols + col])),
                );
            }
        }

        pivot_columns.push(pivot_col);
        pivot_row += 1;
    }

    ReducedRowEchelonForm {
        values,
        pivot_columns,
    }
}

fn default_rref_tolerance(value: &NumericOrComplexOperand) -> f64 {
    if value.rows == 0 || value.cols == 0 {
        return 0.0;
    }
    (value.rows.max(value.cols) as f64) * f64::EPSILON * matrix_infinity_norm(value)
}

fn matrix_infinity_norm(value: &NumericOrComplexOperand) -> f64 {
    (0..value.rows)
        .map(|row| {
            (0..value.cols)
                .map(|col| value.values[row * value.cols + col].magnitude())
                .sum::<f64>()
        })
        .fold(0.0, f64::max)
}

fn matrix_power_operand(
    base: &NumericOrComplexOperand,
    exponent: i64,
) -> Result<NumericOrComplexOperand, RuntimeError> {
    if exponent == 0 {
        return Ok(identity_numeric_or_complex_operand(base.rows));
    }

    if exponent < 0 {
        let inverse = solve_square_linear_system(
            base,
            &identity_numeric_or_complex_operand(base.rows),
            "matrix power",
        )?;
        return matrix_power_operand(&inverse, -exponent);
    }

    let mut result = identity_numeric_or_complex_operand(base.rows);
    let mut power = base.clone();
    let mut exponent = exponent as u64;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = matrix_multiply_operands(&result, &power, "matrix power")?;
        }
        exponent >>= 1;
        if exponent > 0 {
            power = matrix_multiply_operands(&power, &power, "matrix power")?;
        }
    }
    Ok(result)
}

fn matrix_multiply_operands(
    lhs: &NumericOrComplexOperand,
    rhs: &NumericOrComplexOperand,
    context: &str,
) -> Result<NumericOrComplexOperand, RuntimeError> {
    if lhs.cols != rhs.rows {
        return Err(RuntimeError::ShapeError(format!(
            "{context} requires inner dimensions to agree, found {}x{} and {}x{}",
            lhs.rows, lhs.cols, rhs.rows, rhs.cols
        )));
    }

    let mut values = Vec::with_capacity(lhs.rows * rhs.cols);
    for row in 0..lhs.rows {
        for col in 0..rhs.cols {
            let mut total = NumericComplexParts::zero();
            for inner in 0..lhs.cols {
                total = total.plus(
                    lhs.values[row * lhs.cols + inner].times(rhs.values[inner * rhs.cols + col]),
                );
            }
            values.push(normalize_numeric_complex_parts(total));
        }
    }

    Ok(NumericOrComplexOperand {
        rows: lhs.rows,
        cols: rhs.cols,
        values,
    })
}

fn identity_numeric_or_complex_operand(size: usize) -> NumericOrComplexOperand {
    let mut values = vec![NumericComplexParts::zero(); size * size];
    for index in 0..size {
        values[index * size + index] = NumericComplexParts {
            real: 1.0,
            imag: 0.0,
        };
    }
    NumericOrComplexOperand {
        rows: size,
        cols: size,
        values,
    }
}

fn normalize_numeric_complex_parts(value: NumericComplexParts) -> NumericComplexParts {
    NumericComplexParts {
        real: normalize_execution_number(value.real),
        imag: normalize_execution_number(value.imag),
    }
}

fn normalize_execution_number(value: f64) -> f64 {
    if value.abs() <= 1e-12 {
        return 0.0;
    }

    let integer = value.round();
    if (value - integer).abs() <= 1e-12 {
        return integer;
    }

    (value * 1e12).round() / 1e12
}

fn values_equal(lhs: &Value, rhs: &Value) -> Result<bool, RuntimeError> {
    if let (Ok(lhs_numeric), Ok(rhs_numeric)) = (
        numeric_or_complex_scalar(lhs),
        numeric_or_complex_scalar(rhs),
    ) {
        return Ok(lhs_numeric.exact_eq(rhs_numeric));
    }

    match (lhs, rhs) {
        (Value::CharArray(lhs), Value::CharArray(rhs))
        | (Value::CharArray(lhs), Value::String(rhs))
        | (Value::String(lhs), Value::CharArray(rhs))
        | (Value::String(lhs), Value::String(rhs)) => Ok(lhs == rhs),
        (Value::FunctionHandle(lhs), Value::FunctionHandle(rhs)) => Ok(lhs == rhs),
        (Value::Matrix(lhs), Value::Matrix(rhs))
            if lhs.rows == rhs.rows && lhs.cols == rhs.cols =>
        {
            for (lhs, rhs) in lhs.iter().zip(rhs.iter()) {
                if !values_equal(lhs, rhs)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (Value::Cell(lhs), Value::Cell(rhs)) if lhs.rows == rhs.rows && lhs.cols == rhs.cols => {
            for (lhs, rhs) in lhs.iter().zip(rhs.iter()) {
                if !values_equal(lhs, rhs)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (Value::Struct(lhs), Value::Struct(rhs)) => {
            if lhs.fields.len() != rhs.fields.len() {
                return Ok(false);
            }
            for (name, lhs_value) in &lhs.fields {
                let Some(rhs_value) = rhs.fields.get(name) else {
                    return Ok(false);
                };
                if !values_equal(lhs_value, rhs_value)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn evaluate_expression_call(
    target: &Value,
    args: &[EvaluatedIndexArgument],
) -> Result<Value, RuntimeError> {
    match target {
        Value::CharArray(text) => read_char_array_selection(text, args),
        Value::Matrix(matrix) => read_matrix_selection(matrix, args),
        Value::Cell(cell) => read_cell_selection(cell, args),
        Value::Struct(struct_value) => read_matrix_selection(
            &MatrixValue::new(1, 1, vec![Value::Struct(struct_value.clone())])
                .expect("single struct matrix is valid"),
            args,
        ),
        Value::Object(_) => Err(RuntimeError::Unsupported(
            "expression-call execution is not defined for object values".to_string(),
        )),
        _ => Err(RuntimeError::Unsupported(format!(
            "expression-call execution is not defined for {} values",
            target.kind_name()
        ))),
    }
}

fn end_index_extent(
    target: &Value,
    position: usize,
    total_arguments: usize,
) -> Result<usize, RuntimeError> {
    let (dims, kind) = match target {
        Value::CharArray(text) => (vec![1, text.chars().count()], "char array"),
        Value::Matrix(matrix) => (matrix.dims().to_vec(), "matrix"),
        Value::Cell(cell) => (cell.dims().to_vec(), "cell array"),
        Value::Struct(_) => (vec![1, 1], "struct array"),
        _ => {
            return Err(RuntimeError::TypeError(format!(
                "`end` indexing is only defined for matrix, char, cell, and current struct-array values in the current interpreter, found {}",
                target.kind_name()
            )))
        }
    };

    match total_arguments {
        0 => Err(RuntimeError::InvalidIndex(
            "indexing requires at least one argument".to_string(),
        )),
        1 => Ok(dims.iter().product()),
        _ => {
            let indexing_dims = indexing_dimensions_from_dims(&dims, total_arguments);
            indexing_dims.get(position).copied().ok_or_else(|| {
                RuntimeError::InvalidIndex(format!(
                    "`end` position {position} is out of bounds for {kind} indexing"
                ))
            })
        }
    }
}

fn assign_matrix_index(
    matrix: MatrixValue,
    args: &[EvaluatedIndexArgument],
    value: Value,
) -> Result<MatrixValue, RuntimeError> {
    if is_empty_matrix_value(&value) {
        return delete_matrix_index(matrix, args);
    }

    let plan = matrix_assignment_plan(&matrix, args)?;
    let mut matrix = resize_matrix_to_dims(matrix, &plan.target_dims, Value::Scalar(0.0));
    let values = matrix_assignment_values(&value, &plan.selection)?;
    for (position, value) in plan.selection.positions.into_iter().zip(values.into_iter()) {
        matrix.elements_mut()[position] = value;
    }
    Ok(matrix)
}

fn assign_char_array_index(
    text: String,
    args: &[EvaluatedIndexArgument],
    value: Value,
) -> Result<String, RuntimeError> {
    if is_empty_matrix_value(&value) {
        return delete_char_array_index(text, args);
    }

    let plan = char_array_assignment_plan(&text, args)?;
    if plan.target_rows > 1 {
        return Err(RuntimeError::Unsupported(
            "char-array assignment currently only supports row-vector results".to_string(),
        ));
    }

    let mut chars = text.chars().collect::<Vec<_>>();
    chars.resize(plan.target_cols, ' ');
    let values = char_array_assignment_values(&value, &plan.selection)?;
    for (position, value) in plan.selection.positions.into_iter().zip(values.into_iter()) {
        chars[position] = value;
    }
    Ok(chars.into_iter().collect())
}

fn assign_cell_content_index(
    cell: CellValue,
    args: &[EvaluatedIndexArgument],
    value: Value,
) -> Result<CellValue, RuntimeError> {
    let plan = cell_assignment_plan(&cell, args)?;
    let mut cell = resize_cell_to_dims(cell, &plan.target_dims, empty_matrix_value());
    if plan.selection.positions.len() == 1 {
        let position = plan.selection.positions[0];
        cell.elements_mut()[position] = value;
        return Ok(cell);
    }

    let values = cell_content_assignment_values(&value, &plan.selection)?;
    for (position, value) in plan.selection.positions.into_iter().zip(values.into_iter()) {
        cell.elements_mut()[position] = value;
    }
    Ok(cell)
}

fn assign_cell_index(
    cell: CellValue,
    args: &[EvaluatedIndexArgument],
    value: Value,
) -> Result<CellValue, RuntimeError> {
    if is_empty_matrix_value(&value) {
        return delete_cell_index(cell, args);
    }

    let plan = cell_assignment_plan(&cell, args)?;
    let mut cell = resize_cell_to_dims(cell, &plan.target_dims, empty_matrix_value());
    let values = cell_assignment_values(&value, &plan.selection)?;
    for (position, value) in plan.selection.positions.into_iter().zip(values.into_iter()) {
        cell.elements_mut()[position] = value;
    }
    Ok(cell)
}

fn evaluate_cell_content_index(
    target: &Value,
    args: &[EvaluatedIndexArgument],
) -> Result<Value, RuntimeError> {
    let Value::Cell(cell) = target else {
        return Err(RuntimeError::TypeError(format!(
            "cell-content indexing is only defined for cell values, found {}",
            target.kind_name()
        )));
    };

    let selection = cell_selection(cell, args)?;
    match selection.positions.len() {
        0 => Ok(empty_cell_value()),
        1 => Ok(cell.elements()[selection.positions[0]].clone()),
        count => Err(single_output_dot_or_brace_result_error(count)),
    }
}

fn materialize_cell_content_index(
    target: &Value,
    args: &[EvaluatedIndexArgument],
) -> Result<Value, RuntimeError> {
    let Value::Cell(cell) = target else {
        return Err(RuntimeError::TypeError(format!(
            "cell-content indexing is only defined for cell values, found {}",
            target.kind_name()
        )));
    };

    let selection = cell_selection(cell, args)?;
    let values = selection
        .positions
        .iter()
        .map(|&position| cell.elements()[position].clone())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(empty_cell_value());
    }

    if values.iter().all(|value| {
        matches!(
            value,
            Value::Scalar(_) | Value::Logical(_) | Value::Complex(_)
        )
    }) {
        return Ok(Value::Matrix(MatrixValue::with_dimensions(
            selection.rows,
            selection.cols,
            selection.dims.clone(),
            values,
        )?));
    }

    Ok(Value::Cell(CellValue::with_dimensions(
        selection.rows,
        selection.cols,
        selection.dims,
        values,
    )?))
}

fn evaluate_cell_content_outputs(
    target: &Value,
    args: &[EvaluatedIndexArgument],
) -> Result<Vec<Value>, RuntimeError> {
    let Value::Cell(cell) = target else {
        return Err(RuntimeError::TypeError(format!(
            "cell-content indexing is only defined for cell values, found {}",
            target.kind_name()
        )));
    };

    let selection = cell_selection(cell, args)?;
    Ok(selection
        .positions
        .into_iter()
        .map(|position| cell.elements()[position].clone())
        .collect())
}

fn read_matrix_selection(
    matrix: &MatrixValue,
    args: &[EvaluatedIndexArgument],
) -> Result<Value, RuntimeError> {
    let selection = matrix_selection(matrix, args)?;
    if selection.rows == 1 && selection.cols == 1 {
        return Ok(matrix.elements()[selection.positions[0]].clone());
    }

    let rows = selection.rows;
    let cols = selection.cols;
    let elements = selection
        .positions
        .into_iter()
        .map(|position| matrix.elements()[position].clone())
        .collect();
    Ok(Value::Matrix(MatrixValue::with_dimensions(
        rows,
        cols,
        selection.dims,
        elements,
    )?))
}

fn read_cell_selection(
    cell: &CellValue,
    args: &[EvaluatedIndexArgument],
) -> Result<Value, RuntimeError> {
    let selection = cell_selection(cell, args)?;
    let rows = selection.rows;
    let cols = selection.cols;
    let elements = selection
        .positions
        .into_iter()
        .map(|position| cell.elements()[position].clone())
        .collect();
    Ok(Value::Cell(CellValue::with_dimensions(
        rows,
        cols,
        selection.dims,
        elements,
    )?))
}

fn read_char_array_selection(
    text: &str,
    args: &[EvaluatedIndexArgument],
) -> Result<Value, RuntimeError> {
    let selection = char_array_selection(text, args)?;
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(selection.positions.len());
    for position in selection.positions {
        out.push(chars[position]);
    }
    Ok(Value::CharArray(out))
}

fn matrix_selection(
    matrix: &MatrixValue,
    args: &[EvaluatedIndexArgument],
) -> Result<IndexSelection, RuntimeError> {
    match args {
        [index] => linear_selection(index, matrix.rows, matrix.cols, matrix.dims(), "matrix"),
        [row, col] => row_col_selection(row, col, matrix.rows, matrix.cols, "matrix"),
        _ => nd_selection(
            args,
            &indexing_dimensions_from_dims(matrix.dims(), args.len()),
            "matrix",
        ),
    }
}

fn cell_selection(
    cell: &CellValue,
    args: &[EvaluatedIndexArgument],
) -> Result<IndexSelection, RuntimeError> {
    match args {
        [index] => linear_selection(index, cell.rows, cell.cols, cell.dims(), "cell array"),
        [row, col] => row_col_selection(row, col, cell.rows, cell.cols, "cell array"),
        _ => nd_selection(
            args,
            &indexing_dimensions_from_dims(cell.dims(), args.len()),
            "cell array",
        ),
    }
}

fn matrix_assignment_plan(
    matrix: &MatrixValue,
    args: &[EvaluatedIndexArgument],
) -> Result<AssignmentPlan, RuntimeError> {
    match args {
        [_] | [_, _] => assignment_plan(matrix.dims(), "matrix", args),
        _ => nd_assignment_plan(matrix.dims(), "matrix", args),
    }
}

fn cell_assignment_plan(
    cell: &CellValue,
    args: &[EvaluatedIndexArgument],
) -> Result<AssignmentPlan, RuntimeError> {
    match args {
        [_] | [_, _] => assignment_plan(cell.dims(), "cell array", args),
        _ => nd_assignment_plan(cell.dims(), "cell array", args),
    }
}

fn char_array_assignment_plan(
    text: &str,
    args: &[EvaluatedIndexArgument],
) -> Result<AssignmentPlan, RuntimeError> {
    assignment_plan(&[1, text.chars().count()], "char array", args)
}

fn assignment_plan(
    current_dims: &[usize],
    kind: &str,
    args: &[EvaluatedIndexArgument],
) -> Result<AssignmentPlan, RuntimeError> {
    let effective_dims = indexing_dimensions_from_dims(current_dims, args.len());
    match args {
        [argument] => linear_assignment_plan(argument, current_dims, kind),
        [row, col] => row_col_assignment_plan(row, col, current_dims, &effective_dims, kind),
        _ => Err(RuntimeError::Unsupported(format!(
            "{kind} indexed assignment currently supports one or two indices with scalar or `:` selectors"
        ))),
    }
}

fn char_array_selection(
    text: &str,
    args: &[EvaluatedIndexArgument],
) -> Result<IndexSelection, RuntimeError> {
    let len = text.chars().count();
    match args {
        [index] => linear_selection(index, 1, len, &[1, len], "char array"),
        [row, col] => row_col_selection(row, col, 1, len, "char array"),
        _ => Err(RuntimeError::Unsupported(
            "char-array indexing currently supports one or two indices with scalar or `:` selectors"
                .to_string(),
        )),
    }
}

fn linear_assignment_plan(
    argument: &EvaluatedIndexArgument,
    current_dims: &[usize],
    kind: &str,
) -> Result<AssignmentPlan, RuntimeError> {
    let (current_rows, current_cols) = storage_shape_from_dimensions(current_dims);
    let allow_growth = current_dims.len() <= 2 || current_dims.iter().skip(2).all(|dim| *dim == 1);
    let current_extent = current_dims.iter().product::<usize>();
    let stable_target_dims = || {
        if allow_growth {
            let mut dims = vec![current_rows, current_cols];
            if current_dims.len() > 2 {
                dims.extend(current_dims.iter().skip(2).copied());
            }
            dims
        } else {
            current_dims.to_vec()
        }
    };

    let selector =
        plan_linear_selector(argument, current_rows, current_cols, current_dims, kind, SelectorPlanMode::Assignment)?;

    match selector.source {
        SelectorSource::FullSlice
        | SelectorSource::LogicalMask
        | SelectorSource::ScalarLogical => Ok(AssignmentPlan {
            selection: IndexSelection {
                positions: selector
                    .indices
                    .iter()
                    .copied()
                    .map(|index| {
                        linear_position_for_dimensions(
                            index,
                            current_rows,
                            current_cols,
                            current_dims,
                            kind,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                rows: selector.output_rows,
                cols: selector.output_cols,
                dims: selector.output_dims,
                linear: true,
            },
            target_rows: current_rows,
            target_cols: current_cols,
            target_dims: stable_target_dims(),
        }),
        SelectorSource::Numeric => {
            let max_index = selector.indices.iter().copied().max().unwrap_or(0);
            let collapsed_growth = current_dims.len() > 2 && max_index > current_extent;
            if !allow_growth && max_index > current_rows * current_cols && !collapsed_growth {
                return Err(RuntimeError::Unsupported(format!(
                    "{kind} indexed assignment currently does not grow arrays when fewer indices than dimensions are provided"
                )));
            }

            let (target_rows, target_cols, target_dims) = if collapsed_growth {
                let extent = max_index.max(1);
                (extent, 1, vec![extent, 1])
            } else {
                let (target_rows, target_cols) = if current_rows == 0 || current_cols == 0 {
                    (1, max_index.max(1))
                } else if current_rows == 1 && max_index > current_cols {
                    (1, max_index)
                } else if current_cols == 1 && max_index > current_rows {
                    (max_index, 1)
                } else if max_index <= current_rows * current_cols {
                    (current_rows, current_cols)
                } else {
                    (
                        current_rows,
                        ((max_index + current_rows - 1) / current_rows).max(current_cols),
                    )
                };
                let target_dims = if allow_growth {
                    let mut dims = vec![target_rows, target_cols];
                    if current_dims.len() > 2 {
                        dims.extend(current_dims.iter().skip(2).copied());
                    }
                    dims
                } else {
                    current_dims.to_vec()
                };
                (target_rows, target_cols, target_dims)
            };

            Ok(AssignmentPlan {
                selection: IndexSelection {
                    positions: selector
                        .indices
                        .iter()
                        .copied()
                        .map(|index| {
                            linear_position_for_dimensions(
                                index,
                                target_rows,
                                target_cols,
                                &target_dims,
                                kind,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    rows: selector.output_rows,
                    cols: selector.output_cols,
                    dims: selector.output_dims,
                    linear: true,
                },
                target_rows,
                target_cols,
                target_dims,
            })
        }
    }
}

fn row_col_assignment_plan(
    row_argument: &EvaluatedIndexArgument,
    col_argument: &EvaluatedIndexArgument,
    current_dims: &[usize],
    effective_dims: &[usize],
    kind: &str,
) -> Result<AssignmentPlan, RuntimeError> {
    let current_rows = effective_dims.first().copied().unwrap_or(1);
    let current_cols = effective_dims.get(1).copied().unwrap_or(1);
    let omitted_trailing_dims_are_singleton = current_dims.iter().skip(2).all(|dim| *dim == 1);
    let allow_growth =
        current_dims.len() <= 2 || omitted_trailing_dims_are_singleton || current_dims.len() > 2;
    let (row_indices, target_rows) =
        assignment_dimension_indices(row_argument, current_rows, "row", kind)?;
    let (col_indices, target_cols) =
        assignment_dimension_indices(col_argument, current_cols, "column", kind)?;
    if !allow_growth && (target_rows != current_rows || target_cols != current_cols) {
        return Err(RuntimeError::Unsupported(format!(
            "{kind} indexed assignment currently does not grow arrays when fewer indices than dimensions are provided"
        )));
    }
    let mut positions = Vec::with_capacity(row_indices.len() * col_indices.len());
    for row in &row_indices {
        for col in &col_indices {
            positions.push(row_col_position(
                *row,
                *col,
                target_rows,
                target_cols,
                kind,
            )?);
        }
    }
    Ok(AssignmentPlan {
        selection: IndexSelection {
            positions,
            rows: row_indices.len(),
            cols: col_indices.len(),
            dims: vec![row_indices.len(), col_indices.len()],
            linear: false,
        },
        target_rows,
        target_cols,
        target_dims: if allow_growth {
            if target_rows == current_rows && target_cols == current_cols {
                current_dims.to_vec()
            } else {
                let mut dims = vec![target_rows, target_cols];
                if current_dims.len() > 2 && omitted_trailing_dims_are_singleton {
                    dims.extend(current_dims.iter().skip(2).copied());
                }
                dims
            }
        } else {
            current_dims.to_vec()
        },
    })
}

fn plan_dimension_selector(
    argument: &EvaluatedIndexArgument,
    extent: usize,
    dimension: &str,
    kind: &str,
    mode: SelectorPlanMode,
) -> Result<SelectorPlan, RuntimeError> {
    match argument {
        EvaluatedIndexArgument::Numeric {
            values,
            rows,
            cols,
            dims,
            logical,
        } => {
            if *logical && is_logical_selector(values) && values.len() == extent {
                let indices = logical_selector_indices(values, *rows, *cols, dims);
                return Ok(SelectorPlan {
                    source: SelectorSource::LogicalMask,
                    output_rows: indices.len(),
                    output_cols: 1,
                    output_dims: vec![indices.len()],
                    target_extent: extent,
                    indices,
                });
            }
            if *logical && is_logical_selector(values) {
                let indices = bounded_logical_selector_indices(
                    values, *rows, *cols, dims, extent, dimension, kind,
                )?;
                return Ok(SelectorPlan {
                    source: SelectorSource::LogicalMask,
                    output_rows: indices.len(),
                    output_cols: 1,
                    output_dims: vec![indices.len()],
                    target_extent: extent,
                    indices,
                });
            }
            if is_scalar_logical_selector(values, *logical) {
                let indices = scalar_logical_selection_positions(values[0]);
                return Ok(SelectorPlan {
                    source: SelectorSource::ScalarLogical,
                    output_rows: indices.len(),
                    output_cols: 1,
                    output_dims: vec![indices.len()],
                    target_extent: extent,
                    indices,
                });
            }

            let indices = numeric_selector_indices(values)?;
            if mode == SelectorPlanMode::Selection {
                if let Some(index) = indices.iter().copied().find(|index| *index > extent) {
                    return Err(RuntimeError::InvalidIndex(format!(
                        "{dimension} index {index} is out of bounds for {kind} with extent {extent}"
                    )));
                }
            }

            let target_extent = if mode == SelectorPlanMode::Assignment {
                indices.iter().copied().max().map_or(extent, |index| extent.max(index))
            } else {
                extent
            };
            Ok(SelectorPlan {
                source: SelectorSource::Numeric,
                output_rows: indices.len(),
                output_cols: 1,
                output_dims: vec![indices.len()],
                target_extent,
                indices,
            })
        }
        EvaluatedIndexArgument::FullSlice => Ok(SelectorPlan {
            source: SelectorSource::FullSlice,
            indices: (1..=extent).collect(),
            output_rows: extent,
            output_cols: 1,
            output_dims: vec![extent],
            target_extent: extent,
        }),
    }
}

fn plan_linear_selector(
    argument: &EvaluatedIndexArgument,
    rows: usize,
    cols: usize,
    _dims: &[usize],
    kind: &str,
    mode: SelectorPlanMode,
) -> Result<SelectorPlan, RuntimeError> {
    let current_extent = rows * cols;
    match argument {
        EvaluatedIndexArgument::Numeric {
            values,
            rows: selection_rows,
            cols: selection_cols,
            dims: selector_dims,
            logical,
        } => {
            if *logical && is_logical_selector(values) && values.len() == current_extent {
                let indices =
                    logical_selector_indices(values, *selection_rows, *selection_cols, selector_dims);
                let (output_rows, output_cols) =
                    logical_linear_result_shape(rows, cols, indices.len());
                return Ok(SelectorPlan {
                    source: SelectorSource::LogicalMask,
                    output_rows,
                    output_cols,
                    output_dims: vec![output_rows, output_cols],
                    target_extent: current_extent,
                    indices,
                });
            }
            if *logical && is_logical_selector(values) {
                let indices = bounded_logical_selector_indices(
                    values,
                    *selection_rows,
                    *selection_cols,
                    selector_dims,
                    current_extent,
                    "linear",
                    kind,
                )?;
                let (output_rows, output_cols) =
                    logical_linear_result_shape(rows, cols, indices.len());
                return Ok(SelectorPlan {
                    source: SelectorSource::LogicalMask,
                    output_rows,
                    output_cols,
                    output_dims: vec![output_rows, output_cols],
                    target_extent: current_extent,
                    indices,
                });
            }
            if is_scalar_logical_selector(values, *logical) {
                let indices = scalar_logical_selection_positions(values[0]);
                let (output_rows, output_cols) =
                    logical_linear_result_shape(rows, cols, indices.len());
                return Ok(SelectorPlan {
                    source: SelectorSource::ScalarLogical,
                    output_rows,
                    output_cols,
                    output_dims: vec![output_rows, output_cols],
                    target_extent: current_extent,
                    indices,
                });
            }

            let indices = numeric_selector_indices(values)?;
            let target_extent = if mode == SelectorPlanMode::Assignment {
                indices
                    .iter()
                    .copied()
                    .max()
                    .map_or(current_extent, |index| current_extent.max(index))
            } else {
                current_extent
            };
            Ok(SelectorPlan {
                source: SelectorSource::Numeric,
                output_rows: *selection_rows,
                output_cols: *selection_cols,
                output_dims: vec![*selection_rows, *selection_cols],
                target_extent,
                indices,
            })
        }
        EvaluatedIndexArgument::FullSlice => Ok(SelectorPlan {
            source: SelectorSource::FullSlice,
            indices: (1..=current_extent).collect(),
            output_rows: current_extent,
            output_cols: 1,
            output_dims: vec![current_extent, 1],
            target_extent: current_extent,
        }),
    }
}

fn assignment_dimension_indices(
    argument: &EvaluatedIndexArgument,
    current_extent: usize,
    dimension: &str,
    kind: &str,
) -> Result<(Vec<usize>, usize), RuntimeError> {
    let plan = plan_dimension_selector(
        argument,
        current_extent,
        dimension,
        kind,
        SelectorPlanMode::Assignment,
    )?;
    Ok((plan.indices, plan.target_extent))
}

fn linear_selection(
    argument: &EvaluatedIndexArgument,
    rows: usize,
    cols: usize,
    dims: &[usize],
    kind: &str,
) -> Result<IndexSelection, RuntimeError> {
    let plan = plan_linear_selector(argument, rows, cols, dims, kind, SelectorPlanMode::Selection)?;
    Ok(IndexSelection {
        positions: plan
            .indices
            .iter()
            .copied()
            .map(|index| linear_position_for_dimensions(index, rows, cols, dims, kind))
            .collect::<Result<Vec<_>, _>>()?,
        rows: plan.output_rows,
        cols: plan.output_cols,
        dims: plan.output_dims,
        linear: true,
    })
}

fn row_col_selection(
    row_argument: &EvaluatedIndexArgument,
    col_argument: &EvaluatedIndexArgument,
    rows: usize,
    cols: usize,
    kind: &str,
) -> Result<IndexSelection, RuntimeError> {
    let row_indices = dimension_indices(row_argument, rows, "row", kind)?;
    let col_indices = dimension_indices(col_argument, cols, "column", kind)?;
    let mut positions = Vec::with_capacity(row_indices.len() * col_indices.len());
    for row in &row_indices {
        for col in &col_indices {
            positions.push(row_col_position(*row, *col, rows, cols, kind)?);
        }
    }
    Ok(IndexSelection {
        positions,
        rows: row_indices.len(),
        cols: col_indices.len(),
        dims: vec![row_indices.len(), col_indices.len()],
        linear: false,
    })
}

fn nd_selection(
    args: &[EvaluatedIndexArgument],
    dims: &[usize],
    kind: &str,
) -> Result<IndexSelection, RuntimeError> {
    let mut selectors = Vec::with_capacity(args.len());
    let mut result_dims = Vec::with_capacity(args.len());
    for (axis, argument) in args.iter().enumerate() {
        let label = format!("dimension {}", axis + 1);
        let indices = dimension_indices(argument, dims[axis], &label, kind)?;
        result_dims.push(indices.len());
        selectors.push(indices);
    }
    let positions = nd_positions(&selectors, dims);
    let (rows, cols) = storage_shape_from_dimensions(&result_dims);
    Ok(IndexSelection {
        positions,
        rows,
        cols,
        dims: result_dims,
        linear: false,
    })
}

fn nd_positions(selectors: &[Vec<usize>], dims: &[usize]) -> Vec<usize> {
    fn gather(
        axis: usize,
        selectors: &[Vec<usize>],
        dims: &[usize],
        current: &mut Vec<usize>,
        positions: &mut Vec<usize>,
    ) {
        if axis == selectors.len() {
            let zero_based = current
                .iter()
                .map(|index| index.saturating_sub(1))
                .collect::<Vec<_>>();
            positions.push(row_major_linear_index(&zero_based, dims));
            return;
        }

        for &index in &selectors[axis] {
            current.push(index);
            gather(axis + 1, selectors, dims, current, positions);
            current.pop();
        }
    }

    if selectors.iter().any(|selector| selector.is_empty()) {
        return Vec::new();
    }

    let mut positions = Vec::with_capacity(selectors.iter().map(Vec::len).product());
    gather(
        0,
        selectors,
        dims,
        &mut Vec::with_capacity(selectors.len()),
        &mut positions,
    );
    positions
}

fn nd_assignment_plan(
    current_dims: &[usize],
    kind: &str,
    args: &[EvaluatedIndexArgument],
) -> Result<AssignmentPlan, RuntimeError> {
    let effective_dims = indexing_dimensions_from_dims(current_dims, args.len());
    let allow_growth = args.len() >= current_dims.len()
        || current_dims.iter().skip(args.len()).all(|dim| *dim == 1);
    let mut target_dims = effective_dims.clone();
    let mut selectors = Vec::with_capacity(args.len());
    let mut selection_dims = Vec::with_capacity(args.len());

    for (axis, argument) in args.iter().enumerate() {
        let label = format!("dimension {}", axis + 1);
        let (indices, target_extent) =
            assignment_dimension_indices(argument, effective_dims[axis], &label, kind)?;
        if !allow_growth && target_extent != effective_dims[axis] {
            return Err(RuntimeError::Unsupported(format!(
                "{kind} indexed assignment currently does not grow arrays when fewer indices than dimensions are provided"
            )));
        }
        target_dims[axis] = target_extent;
        selection_dims.push(indices.len());
        selectors.push(indices);
    }

    let positions = nd_positions(&selectors, &target_dims);
    let (target_rows, target_cols) = storage_shape_from_dimensions(&target_dims);
    let (rows, cols) = storage_shape_from_dimensions(&selection_dims);
    Ok(AssignmentPlan {
        selection: IndexSelection {
            positions,
            rows,
            cols,
            dims: selection_dims,
            linear: false,
        },
        target_rows,
        target_cols,
        target_dims,
    })
}

fn dimension_indices(
    argument: &EvaluatedIndexArgument,
    extent: usize,
    dimension: &str,
    kind: &str,
) -> Result<Vec<usize>, RuntimeError> {
    Ok(
        plan_dimension_selector(argument, extent, dimension, kind, SelectorPlanMode::Selection)?
            .indices,
    )
}

fn matrix_assignment_values(
    value: &Value,
    selection: &IndexSelection,
) -> Result<Vec<Value>, RuntimeError> {
    match value {
        Value::Matrix(matrix) => expand_matrix_assignment_values(matrix, selection),
        other => Ok(vec![other.clone(); selection.positions.len()]),
    }
}

fn expand_matrix_assignment_values(
    matrix: &MatrixValue,
    selection: &IndexSelection,
) -> Result<Vec<Value>, RuntimeError> {
    let count = selection.positions.len();
    if matrix.rows * matrix.cols == 1 {
        return Ok(vec![matrix.elements()[0].clone(); count]);
    }

    if selection.linear {
        if matrix.rows * matrix.cols != count {
            return Err(RuntimeError::ShapeError(format!(
                "linear matrix assignment expects {} rhs element(s), found {}",
                count,
                matrix.rows * matrix.cols
            )));
        }
        return linearized_matrix_elements(matrix);
    }

    if equivalent_dimensions(matrix.dims(), &selection.dims) {
        return Ok(matrix.elements().to_vec());
    }

    if matrix.element_count() == count {
        return Ok(reorder_matlab_linear_values_to_row_major(
            linearized_matrix_elements(matrix)?,
            &selection.dims,
        ));
    }

    Err(RuntimeError::ShapeError(format!(
        "matrix assignment expects rhs dimensions {:?}, found {:?}",
        selection.dims,
        matrix.dims()
    )))
}

fn char_array_assignment_values(
    value: &Value,
    selection: &IndexSelection,
) -> Result<Vec<char>, RuntimeError> {
    let rhs = match value {
        Value::CharArray(text) | Value::String(text) => text.chars().collect::<Vec<_>>(),
        other => {
            return Err(RuntimeError::TypeError(format!(
                "char-array assignment expects char or string rhs, found {}",
                other.kind_name()
            )))
        }
    };

    if rhs.len() == 1 {
        return Ok(vec![rhs[0]; selection.positions.len()]);
    }

    if rhs.len() == selection.positions.len() {
        return Ok(rhs);
    }

    Err(RuntimeError::ShapeError(format!(
        "char-array assignment expects 1 or {} character(s), found {}",
        selection.positions.len(),
        rhs.len()
    )))
}

fn cell_assignment_values(
    value: &Value,
    selection: &IndexSelection,
) -> Result<Vec<Value>, RuntimeError> {
    let Value::Cell(cell) = value else {
        return Err(RuntimeError::TypeError(format!(
            "cell array `()` assignment expects a cell rhs, found {}",
            value.kind_name()
        )));
    };

    expand_cell_assignment_values(cell, selection)
}

fn expand_cell_assignment_values(
    cell: &CellValue,
    selection: &IndexSelection,
) -> Result<Vec<Value>, RuntimeError> {
    let count = selection.positions.len();
    if cell.rows == 1 && cell.cols == 1 {
        return Ok(vec![cell.elements()[0].clone(); count]);
    }

    if selection.linear {
        if cell.rows * cell.cols != count {
            return Err(RuntimeError::ShapeError(format!(
                "linear cell assignment expects {} rhs element(s), found {}",
                count,
                cell.rows * cell.cols
            )));
        }
        return linearized_cell_elements(cell);
    }

    if equivalent_dimensions(cell.dims(), &selection.dims) {
        return Ok(cell.elements().to_vec());
    }

    if cell.element_count() == count {
        return Ok(reorder_matlab_linear_values_to_row_major(
            linearized_cell_elements(cell)?,
            &selection.dims,
        ));
    }

    Err(RuntimeError::ShapeError(format!(
        "cell assignment expects rhs dimensions {:?}, found {:?}",
        selection.dims,
        cell.dims()
    )))
}

fn distributed_cell_assignment_values(
    value: &Value,
    count: usize,
) -> Result<Vec<Value>, RuntimeError> {
    match value {
        Value::Cell(cell) => {
            if count == 0 {
                return Ok(Vec::new());
            }
            if cell.rows == 1 && cell.cols == 1 {
                return Ok(vec![cell.elements()[0].clone(); count]);
            }
            if cell.element_count() == count {
                return linearized_cell_elements(cell);
            }

            Err(RuntimeError::ShapeError(format!(
                "cell-content assignment expects {} rhs element(s), found {}",
                count,
                cell.element_count()
            )))
        }
        Value::Matrix(matrix) => {
            if count == 0 {
                return Ok(Vec::new());
            }
            if matrix.rows == 1 && matrix.cols == 1 {
                return Ok(vec![matrix.elements()[0].clone(); count]);
            }
            if matrix.element_count() == count {
                return linearized_matrix_elements(matrix);
            }

            Err(RuntimeError::ShapeError(format!(
                "cell-content assignment expects {} rhs element(s), found {}",
                count,
                matrix.element_count()
            )))
        }
        other => Err(RuntimeError::Unsupported(format!(
            "cell-content assignment with multiple targets currently expects a cell or matrix rhs, found {}",
            other.kind_name()
        ))),
    }
}

fn cell_content_assignment_values(
    value: &Value,
    selection: &IndexSelection,
) -> Result<Vec<Value>, RuntimeError> {
    match value {
        Value::Cell(_) => cell_assignment_values(value, selection),
        Value::Matrix(matrix) => {
            let count = selection.positions.len();
            if count == 0 {
                return Ok(Vec::new());
            }
            if matrix.element_count() == count {
                return linearized_matrix_elements(matrix);
            }
            Err(RuntimeError::ShapeError(format!(
                "cell-content assignment expects {} rhs element(s), found {}",
                count,
                matrix.element_count()
            )))
        }
        other => Err(RuntimeError::Unsupported(format!(
            "cell-content assignment with multiple targets currently expects a cell or matrix rhs, found {}",
            other.kind_name()
        ))),
    }
}

fn pack_cell_assignment_rhs(
    selection: &IndexSelection,
    values: Vec<Value>,
) -> Result<Value, RuntimeError> {
    if selection.positions.len() == 1 {
        return Ok(values.into_iter().next().unwrap_or_else(empty_matrix_value));
    }

    Ok(Value::Cell(CellValue::with_dimensions(
        selection.rows,
        selection.cols,
        selection.dims.clone(),
        values,
    )?))
}

fn try_assign_selected_nested_cell_contents(
    current: &Value,
    indices: &[EvaluatedIndexArgument],
    value: &Value,
) -> Result<Option<Value>, RuntimeError> {
    let Value::Cell(outer_cell) = current else {
        return Ok(None);
    };
    let outer_selection = match cell_selection(outer_cell, indices) {
        Ok(selection) => selection,
        Err(RuntimeError::InvalidIndex(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if outer_selection.positions.len() <= 1 {
        return Ok(None);
    }

    let mut inner_cells = Vec::with_capacity(outer_selection.positions.len());
    let mut total_count = 0;
    for &position in &outer_selection.positions {
        let Value::Cell(inner_cell) = &outer_cell.elements[position] else {
            return Ok(None);
        };
        total_count += inner_cell.elements.len();
        inner_cells.push((position, inner_cell.clone()));
    }

    let rhs_values = if total_count > 0 {
        match distributed_cell_assignment_values(value, total_count) {
            Ok(values) => values,
            Err(_) => return Ok(None),
        }
    } else {
        let Value::Cell(rhs_cell) = value else {
            return Ok(None);
        };
        if rhs_cell.elements.is_empty()
            || rhs_cell.elements.len() % outer_selection.positions.len() != 0
        {
            return Ok(None);
        }
        rhs_cell.elements.clone()
    };

    let mut rhs_iter = rhs_values.into_iter();
    let mut elements = outer_cell.elements.clone();
    let selected_count = inner_cells.len();
    for (selection_index, (position, inner_cell)) in inner_cells.into_iter().enumerate() {
        let chunk_len = if inner_cell.elements.is_empty() {
            rhs_iter.len() / (selected_count - selection_index)
        } else {
            inner_cell.elements.len()
        };
        let chunk = rhs_iter.by_ref().take(chunk_len).collect::<Vec<_>>();
        elements[position] = Value::Cell(if inner_cell.elements.is_empty() {
            CellValue::new(1, chunk.len(), chunk)?
        } else {
            let reordered = reorder_matlab_linear_values_to_row_major(chunk, &inner_cell.dims);
            CellValue::with_dimensions(
                inner_cell.rows,
                inner_cell.cols,
                inner_cell.dims.clone(),
                reordered,
            )?
        });
    }

    Ok(Some(Value::Cell(CellValue::with_dimensions(
        outer_cell.rows,
        outer_cell.cols,
        outer_cell.dims.clone(),
        elements,
    )?)))
}

fn try_assign_field_to_nested_cell_containers(
    current: &Value,
    field: &str,
    value: &Value,
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

    let rhs_values = match value {
        Value::Matrix(matrix) => matrix.elements.clone(),
        Value::Cell(cell) => cell.elements.clone(),
        other => vec![other.clone()],
    };
    if rhs_values.is_empty() {
        return Ok(None);
    }

    let mut known_total = 0usize;
    let mut unknown_count = 0usize;
    let mut counts = Vec::with_capacity(outer_cell.elements.len());
    for element in &outer_cell.elements {
        let Value::Cell(inner_cell) = element else {
            unreachable!("guard ensured nested cell containers");
        };
        let count = if inner_cell.elements.is_empty() {
            None
        } else {
            match nested_struct_assignment_target_count(element) {
                Some(nested) => Some(nested),
                None => return Ok(None),
            }
        };
        match count {
            Some(count) => known_total += count,
            None => unknown_count += 1,
        }
        counts.push(count);
    }

    if rhs_values.len() < known_total {
        return Ok(None);
    }
    if unknown_count > 0 && (rhs_values.len() - known_total) % unknown_count != 0 {
        return Ok(None);
    }
    let unknown_chunk_len = if unknown_count > 0 {
        (rhs_values.len() - known_total) / unknown_count
    } else {
        0
    };

    let mut rhs_iter = rhs_values.into_iter();
    let mut elements = outer_cell.elements.clone();
    let field_name = field.to_string();
    for (index, count) in counts.into_iter().enumerate() {
        let Value::Cell(inner_cell) = &outer_cell.elements[index] else {
            unreachable!("guard ensured nested cell containers");
        };
        let chunk_len = count.unwrap_or(unknown_chunk_len);
        let chunk = rhs_iter.by_ref().take(chunk_len).collect::<Vec<_>>();
        elements[index] = if inner_cell.elements.is_empty() {
            let structs = chunk
                .into_iter()
                .map(|assigned| {
                    let mut struct_value = StructValue::default();
                    struct_value.insert_field(field_name.clone(), assigned);
                    Value::Struct(struct_value)
                })
                .collect::<Vec<_>>();
            Value::Cell(CellValue::new(1, structs.len(), structs)?)
        } else {
            let rhs = pack_struct_assignment_chunk(&outer_cell.elements[index], chunk)?;
            assign_struct_path(
                outer_cell.elements[index].clone(),
                std::slice::from_ref(&field_name),
                rhs,
            )?
        };
    }

    Ok(Some(Value::Cell(CellValue::with_dimensions(
        outer_cell.rows,
        outer_cell.cols,
        outer_cell.dims.clone(),
        elements,
    )?)))
}

impl<'a> Interpreter<'a> {
    fn try_assign_distributed_struct_field_projection(
        &mut self,
        frame: &mut Frame<'a>,
        current: &Value,
        field: &String,
        rest: &[LValueProjection],
        leaf: &LValueLeaf,
    ) -> Result<Option<Value>, RuntimeError> {
        let Some(LValueProjection::Brace(_)) = rest.first() else {
            return Ok(None);
        };

        match current {
            Value::Matrix(matrix) if matrix_is_struct_array(matrix) => {
                let mut elements = Vec::with_capacity(matrix.elements.len());
                for element in &matrix.elements {
                    let next = match read_field_value(element, field) {
                        Ok(value) => value,
                        Err(RuntimeError::MissingVariable(_)) => {
                            default_nested_lvalue_value(rest, leaf).ok_or_else(|| {
                                RuntimeError::MissingVariable(format!(
                                    "struct field `{field}` is not defined"
                                ))
                            })?
                        }
                        Err(error) => return Err(error),
                    };
                    let updated_next =
                        self.assign_lvalue_path(frame, next, rest, true, leaf.clone())?;
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
                            default_nested_lvalue_value(rest, leaf).ok_or_else(|| {
                                RuntimeError::MissingVariable(format!(
                                    "struct field `{field}` is not defined"
                                ))
                            })?
                        }
                        Err(error) => return Err(error),
                    };
                    let updated_next =
                        self.assign_lvalue_path(frame, next, rest, true, leaf.clone())?;
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

fn delete_matrix_index(
    matrix: MatrixValue,
    args: &[EvaluatedIndexArgument],
) -> Result<MatrixValue, RuntimeError> {
    let rows = matrix.rows;
    let cols = matrix.cols;
    match args {
        [argument] => delete_vector_matrix_elements(matrix, argument),
        [row_argument, EvaluatedIndexArgument::FullSlice] => {
            delete_matrix_rows(matrix, &dimension_indices(row_argument, rows, "row", "matrix")?)
        }
        [EvaluatedIndexArgument::FullSlice, col_argument] => delete_matrix_cols(
            matrix,
            &dimension_indices(col_argument, cols, "column", "matrix")?,
        ),
        _ if args.len() > 2 => delete_matrix_nd_axis(matrix, args),
        _ => Err(RuntimeError::Unsupported(
            "matrix deletion currently supports removing full rows, full columns, or linear elements"
                .to_string(),
        )),
    }
}

fn delete_char_array_index(
    text: String,
    args: &[EvaluatedIndexArgument],
) -> Result<String, RuntimeError> {
    let len = text.chars().count();
    match args {
        [argument] => delete_vector_text_elements(text, argument),
        [row_argument, EvaluatedIndexArgument::FullSlice] => {
            let rows = dimension_indices(row_argument, 1, "row", "char array")?;
            if rows.contains(&1) {
                Ok(String::new())
            } else {
                Ok(text)
            }
        }
        [EvaluatedIndexArgument::FullSlice, col_argument] => {
            let indices = dimension_indices(col_argument, len, "column", "char array")?;
            let removed = removal_mask(len, &indices);
            Ok(text
                .chars()
                .enumerate()
                .filter_map(|(offset, ch)| (!removed[offset]).then_some(ch))
                .collect())
        }
        _ => Err(RuntimeError::Unsupported(
            "char-array deletion currently supports removing full rows, full columns, or linear elements"
                .to_string(),
        )),
    }
}

fn delete_cell_index(
    cell: CellValue,
    args: &[EvaluatedIndexArgument],
) -> Result<CellValue, RuntimeError> {
    let rows = cell.rows;
    let cols = cell.cols;
    match args {
        [argument] => delete_vector_cell_elements(cell, argument),
        [row_argument, EvaluatedIndexArgument::FullSlice] => {
            delete_cell_rows(cell, &dimension_indices(row_argument, rows, "row", "cell array")?)
        }
        [EvaluatedIndexArgument::FullSlice, col_argument] => delete_cell_cols(
            cell,
            &dimension_indices(col_argument, cols, "column", "cell array")?,
        ),
        _ if args.len() > 2 => delete_cell_nd_axis(cell, args),
        _ => Err(RuntimeError::Unsupported(
            "cell array deletion currently supports removing full rows, full columns, or linear elements"
                .to_string(),
        )),
    }
}

fn delete_vector_text_elements(
    text: String,
    argument: &EvaluatedIndexArgument,
) -> Result<String, RuntimeError> {
    let len = text.chars().count();
    let indices = dimension_indices(argument, len, "linear", "char vector")?;
    let removed = removal_mask(len, &indices);
    Ok(text
        .chars()
        .enumerate()
        .filter_map(|(offset, ch)| (!removed[offset]).then_some(ch))
        .collect())
}

fn delete_vector_matrix_elements(
    matrix: MatrixValue,
    argument: &EvaluatedIndexArgument,
) -> Result<MatrixValue, RuntimeError> {
    let indices = dimension_indices(
        argument,
        matrix.rows * matrix.cols,
        "linear",
        "matrix",
    )?;
    let removed = removal_mask(matrix.rows * matrix.cols, &indices);
    let elements = linearized_matrix_elements(&matrix)?
        .into_iter()
        .enumerate()
        .filter_map(|(offset, value)| (!removed[offset]).then_some(value))
        .collect::<Vec<_>>();
    if matrix.rows == 1 {
        MatrixValue::new(1, elements.len(), elements)
    } else {
        MatrixValue::new(elements.len(), 1, elements)
    }
}

fn delete_vector_cell_elements(
    cell: CellValue,
    argument: &EvaluatedIndexArgument,
) -> Result<CellValue, RuntimeError> {
    let indices = dimension_indices(argument, cell.rows * cell.cols, "linear", "cell array")?;
    let removed = removal_mask(cell.rows * cell.cols, &indices);
    let elements = linearized_cell_elements(&cell)?
        .into_iter()
        .enumerate()
        .filter_map(|(offset, value)| (!removed[offset]).then_some(value))
        .collect::<Vec<_>>();
    if cell.rows == 1 {
        CellValue::new(1, elements.len(), elements)
    } else {
        CellValue::new(elements.len(), 1, elements)
    }
}

fn delete_matrix_rows(
    matrix: MatrixValue,
    rows_to_remove: &[usize],
) -> Result<MatrixValue, RuntimeError> {
    let removed = removal_mask(matrix.rows, rows_to_remove);
    let kept_rows = (1..=matrix.rows)
        .filter(|row| !removed[row - 1])
        .collect::<Vec<_>>();
    let mut elements = Vec::with_capacity(kept_rows.len() * matrix.cols);
    for row in kept_rows {
        for col in 1..=matrix.cols {
            elements.push(
                matrix.elements[row_col_position(row, col, matrix.rows, matrix.cols, "matrix")?]
                    .clone(),
            );
        }
    }
    MatrixValue::new(
        matrix.rows
            - rows_to_remove
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
        matrix.cols,
        elements,
    )
}

fn delete_matrix_cols(
    matrix: MatrixValue,
    cols_to_remove: &[usize],
) -> Result<MatrixValue, RuntimeError> {
    let removed = removal_mask(matrix.cols, cols_to_remove);
    let kept_cols = (1..=matrix.cols)
        .filter(|col| !removed[col - 1])
        .collect::<Vec<_>>();
    let mut elements = Vec::with_capacity(matrix.rows * kept_cols.len());
    for row in 1..=matrix.rows {
        for col in &kept_cols {
            elements.push(
                matrix.elements[row_col_position(row, *col, matrix.rows, matrix.cols, "matrix")?]
                    .clone(),
            );
        }
    }
    MatrixValue::new(
        matrix.rows,
        matrix.cols
            - cols_to_remove
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
        elements,
    )
}

fn delete_cell_rows(cell: CellValue, rows_to_remove: &[usize]) -> Result<CellValue, RuntimeError> {
    let removed = removal_mask(cell.rows, rows_to_remove);
    let kept_rows = (1..=cell.rows)
        .filter(|row| !removed[row - 1])
        .collect::<Vec<_>>();
    let mut elements = Vec::with_capacity(kept_rows.len() * cell.cols);
    for row in kept_rows {
        for col in 1..=cell.cols {
            elements.push(
                cell.elements[row_col_position(row, col, cell.rows, cell.cols, "cell array")?]
                    .clone(),
            );
        }
    }
    CellValue::new(
        cell.rows
            - rows_to_remove
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
        cell.cols,
        elements,
    )
}

fn delete_cell_cols(cell: CellValue, cols_to_remove: &[usize]) -> Result<CellValue, RuntimeError> {
    let removed = removal_mask(cell.cols, cols_to_remove);
    let kept_cols = (1..=cell.cols)
        .filter(|col| !removed[col - 1])
        .collect::<Vec<_>>();
    let mut elements = Vec::with_capacity(cell.rows * kept_cols.len());
    for row in 1..=cell.rows {
        for col in &kept_cols {
            elements.push(
                cell.elements[row_col_position(row, *col, cell.rows, cell.cols, "cell array")?]
                    .clone(),
            );
        }
    }
    CellValue::new(
        cell.rows,
        cell.cols
            - cols_to_remove
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
        elements,
    )
}

fn delete_matrix_nd_axis(
    matrix: MatrixValue,
    args: &[EvaluatedIndexArgument],
) -> Result<MatrixValue, RuntimeError> {
    let dims = indexing_dimensions_from_dims(&matrix.dims, args.len());
    if args.len() < matrix.dims.len() {
        let collapsed = MatrixValue::with_dimensions(
            matrix.rows,
            matrix.cols,
            dims.clone(),
            reshape_elements_column_major(&matrix.elements, &matrix.dims, &dims),
        )?;
        return delete_matrix_index(collapsed, args);
    }
    let Some(axis) = single_indexed_nd_axis(args) else {
        return Err(RuntimeError::Unsupported(
            "N-D matrix deletion currently supports deleting along exactly one indexed dimension while all others are `:`"
                .to_string(),
        ));
    };
    let axis_indices = dimension_indices(
        &args[axis],
        dims[axis],
        &format!("dimension {}", axis + 1),
        "matrix",
    )?;
    let removed = removal_mask(dims[axis], &axis_indices);
    let mut target_dims = dims.clone();
    target_dims[axis] = target_dims[axis].saturating_sub(
        axis_indices
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
    );
    let elements = matrix
        .elements
        .into_iter()
        .enumerate()
        .filter_map(|(linear, value)| {
            let index = row_major_multi_index(linear, &dims);
            (!removed[index[axis]]).then_some(value)
        })
        .collect::<Vec<_>>();
    let (rows, cols) = storage_shape_from_dimensions(&target_dims);
    MatrixValue::with_dimensions(rows, cols, target_dims, elements)
}

fn delete_cell_nd_axis(
    cell: CellValue,
    args: &[EvaluatedIndexArgument],
) -> Result<CellValue, RuntimeError> {
    let dims = indexing_dimensions_from_dims(&cell.dims, args.len());
    if args.len() < cell.dims.len() {
        let collapsed = CellValue::with_dimensions(
            cell.rows,
            cell.cols,
            dims.clone(),
            reshape_elements_column_major(&cell.elements, &cell.dims, &dims),
        )?;
        return delete_cell_index(collapsed, args);
    }
    let Some(axis) = single_indexed_nd_axis(args) else {
        return Err(RuntimeError::Unsupported(
            "N-D cell deletion currently supports deleting along exactly one indexed dimension while all others are `:`"
                .to_string(),
        ));
    };
    let axis_indices = dimension_indices(
        &args[axis],
        dims[axis],
        &format!("dimension {}", axis + 1),
        "cell array",
    )?;
    let removed = removal_mask(dims[axis], &axis_indices);
    let mut target_dims = dims.clone();
    target_dims[axis] = target_dims[axis].saturating_sub(
        axis_indices
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
    );
    let elements = cell
        .elements
        .into_iter()
        .enumerate()
        .filter_map(|(linear, value)| {
            let index = row_major_multi_index(linear, &dims);
            (!removed[index[axis]]).then_some(value)
        })
        .collect::<Vec<_>>();
    let (rows, cols) = storage_shape_from_dimensions(&target_dims);
    CellValue::with_dimensions(rows, cols, target_dims, elements)
}

fn removal_mask(extent: usize, indices: &[usize]) -> Vec<bool> {
    let mut removed = vec![false; extent];
    for index in indices {
        if *index != 0 && *index <= extent {
            removed[index - 1] = true;
        }
    }
    removed
}

fn single_indexed_nd_axis(args: &[EvaluatedIndexArgument]) -> Option<usize> {
    let mut axis = None;
    for (position, argument) in args.iter().enumerate() {
        if matches!(argument, EvaluatedIndexArgument::FullSlice) {
            continue;
        }
        if axis.is_some() {
            return None;
        }
        axis = Some(position);
    }
    axis
}

fn resize_matrix_to_dims(matrix: MatrixValue, target_dims: &[usize], fill: Value) -> MatrixValue {
    let (target_rows, target_cols) = storage_shape_from_dimensions(target_dims);
    if equivalent_dimensions(&matrix.dims, target_dims) {
        return MatrixValue {
            rows: target_rows,
            cols: target_cols,
            dims: target_dims.to_vec(),
            elements: matrix.elements,
        };
    }

    if matrix.dims.len() > 2 && target_dims.len() == 2 {
        if target_dims[1] == 1 {
            let mut elements = vec![fill; target_rows * target_cols];
            for (offset, value) in linearized_matrix_elements(&matrix)
                .expect("matrix linearization should succeed")
                .into_iter()
                .enumerate()
            {
                if offset < elements.len() {
                    elements[offset] = value;
                }
            }
            return MatrixValue {
                rows: target_rows,
                cols: target_cols,
                dims: target_dims.to_vec(),
                elements,
            };
        }

        let collapsed = MatrixValue::with_dimensions(
            matrix.rows,
            matrix.cols,
            vec![matrix.rows, matrix.cols],
            matrix.elements,
        )
        .expect("collapsed matrix view should preserve folded storage order");
        return resize_matrix_to_dims(collapsed, target_dims, fill);
    }

    let (source_dims, source_elements) =
        resize_source_layout(&matrix.dims, &matrix.elements, target_dims);

    let mut elements = vec![fill; target_rows * target_cols];
    for linear in 0..source_elements.len() {
        let source_index = row_major_multi_index(linear, &source_dims);
        let destination = row_major_linear_index(&source_index, target_dims);
        elements[destination] = source_elements[linear].clone();
    }

    MatrixValue {
        rows: target_rows,
        cols: target_cols,
        dims: target_dims.to_vec(),
        elements,
    }
}

fn resize_cell_to_dims(cell: CellValue, target_dims: &[usize], fill: Value) -> CellValue {
    let (target_rows, target_cols) = storage_shape_from_dimensions(target_dims);
    if equivalent_dimensions(&cell.dims, target_dims) {
        return CellValue {
            rows: target_rows,
            cols: target_cols,
            dims: target_dims.to_vec(),
            elements: cell.elements,
        };
    }

    if cell.dims.len() > 2 && target_dims.len() == 2 {
        if target_dims[1] == 1 {
            let mut elements = vec![fill; target_rows * target_cols];
            for (offset, value) in linearized_cell_elements(&cell)
                .expect("cell linearization should succeed")
                .into_iter()
                .enumerate()
            {
                if offset < elements.len() {
                    elements[offset] = value;
                }
            }
            return CellValue {
                rows: target_rows,
                cols: target_cols,
                dims: target_dims.to_vec(),
                elements,
            };
        }

        let collapsed = CellValue::with_dimensions(
            cell.rows,
            cell.cols,
            vec![cell.rows, cell.cols],
            cell.elements,
        )
        .expect("collapsed cell view should preserve folded storage order");
        return resize_cell_to_dims(collapsed, target_dims, fill);
    }

    let (source_dims, source_elements) =
        resize_source_layout(&cell.dims, &cell.elements, target_dims);

    let mut elements = vec![fill; target_rows * target_cols];
    for linear in 0..source_elements.len() {
        let source_index = row_major_multi_index(linear, &source_dims);
        let destination = row_major_linear_index(&source_index, target_dims);
        elements[destination] = source_elements[linear].clone();
    }

    CellValue {
        rows: target_rows,
        cols: target_cols,
        dims: target_dims.to_vec(),
        elements,
    }
}

fn resize_source_dims(source_dims: &[usize], target_len: usize) -> Vec<usize> {
    let mut normalized = source_dims.to_vec();
    while normalized.len() > target_len && normalized.last() == Some(&1) {
        normalized.pop();
    }
    while normalized.len() < target_len {
        normalized.push(1);
    }
    normalized
}

fn resize_source_layout<T: Clone>(
    source_dims: &[usize],
    source_elements: &[T],
    target_dims: &[usize],
) -> (Vec<usize>, Vec<T>) {
    let target_len = target_dims.len();
    if source_dims.len() > target_len {
        let collapsed = if target_len == 2 && target_dims.get(1) == Some(&1) {
            vec![source_dims.iter().product::<usize>(), 1]
        } else {
            indexing_dimensions_from_dims(source_dims, target_len)
        };
        (
            collapsed.clone(),
            reshape_elements_column_major(source_elements, source_dims, &collapsed),
        )
    } else {
        (
            resize_source_dims(source_dims, target_len),
            source_elements.to_vec(),
        )
    }
}

fn reshape_elements_column_major<T: Clone>(
    elements: &[T],
    source_dims: &[usize],
    target_dims: &[usize],
) -> Vec<T> {
    if elements.is_empty() {
        return Vec::new();
    }

    let mut normalized_source_dims = source_dims.to_vec();
    while normalized_source_dims.len() < target_dims.len() {
        normalized_source_dims.push(1);
    }

    let element_count = target_dims.iter().product::<usize>();
    let mut output = vec![elements[0].clone(); element_count];
    for linear in 0..element_count {
        let source_index = column_major_multi_index(linear, &normalized_source_dims);
        let source_offset = row_major_linear_index(&source_index, &normalized_source_dims);
        let target_index = column_major_multi_index(linear, target_dims);
        let target_offset = row_major_linear_index(&target_index, target_dims);
        output[target_offset] = elements[source_offset].clone();
    }
    output
}

fn empty_matrix_value() -> Value {
    Value::Matrix(MatrixValue::new(0, 0, Vec::new()).expect("empty matrix is valid"))
}

fn empty_cell_value() -> Value {
    Value::Cell(CellValue::new(0, 0, Vec::new()).expect("empty cell is valid"))
}

fn is_empty_matrix_value(value: &Value) -> bool {
    matches!(value, Value::Matrix(matrix) if matrix.rows == 0 && matrix.cols == 0)
}

fn linearized_matrix_elements(matrix: &MatrixValue) -> Result<Vec<Value>, RuntimeError> {
    (1..=matrix.element_count())
        .map(|index| {
            Ok(matrix.elements()[linear_position_for_dimensions(
                index,
                matrix.rows,
                matrix.cols,
                matrix.dims(),
                "matrix",
            )?]
            .clone())
        })
        .collect()
}

fn linearized_cell_elements(cell: &CellValue) -> Result<Vec<Value>, RuntimeError> {
    (1..=cell.element_count())
        .map(|index| {
            Ok(cell.elements()[linear_position_for_dimensions(
                index,
                cell.rows,
                cell.cols,
                cell.dims(),
                "cell array",
            )?]
            .clone())
        })
        .collect()
}

fn reorder_matlab_linear_values_to_row_major<T>(values: Vec<T>, dims: &[usize]) -> Vec<T> {
    if values.is_empty() {
        return values;
    }

    let mut reordered = (0..values.len()).map(|_| None).collect::<Vec<Option<T>>>();
    for (linear, value) in values.into_iter().enumerate() {
        let index = column_major_multi_index(linear, dims);
        let destination = row_major_linear_index(&index, dims);
        reordered[destination] = Some(value);
    }
    reordered
        .into_iter()
        .map(|value| value.expect("every destination index should be initialized"))
        .collect()
}

fn linear_column_major_position(
    index: usize,
    rows: usize,
    cols: usize,
    kind: &str,
) -> Result<usize, RuntimeError> {
    let element_count = rows * cols;
    if index == 0 || index > element_count {
        return Err(RuntimeError::InvalidIndex(format!(
            "linear index {index} is out of bounds for {}x{} {kind}",
            rows, cols
        )));
    }

    let zero_based = index - 1;
    let row = zero_based % rows;
    let col = zero_based / rows;
    Ok(row * cols + col)
}

fn linear_position_for_dimensions(
    index: usize,
    rows: usize,
    cols: usize,
    dims: &[usize],
    kind: &str,
) -> Result<usize, RuntimeError> {
    if dims.len() <= 2 || dims.iter().product::<usize>() != rows * cols {
        return linear_column_major_position(index, rows, cols, kind);
    }

    let element_count = dims.iter().product::<usize>();
    if index == 0 || index > element_count {
        return Err(RuntimeError::InvalidIndex(format!(
            "linear index {index} is out of bounds for {:?} {kind}",
            dims
        )));
    }

    let index = column_major_multi_index(index - 1, dims);
    Ok(row_major_linear_index(&index, dims))
}

fn row_col_position(
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    kind: &str,
) -> Result<usize, RuntimeError> {
    if row == 0 || row > rows || col == 0 || col > cols {
        return Err(RuntimeError::InvalidIndex(format!(
            "index ({row}, {col}) is out of bounds for {}x{} {kind}",
            rows, cols
        )));
    }
    Ok((row - 1) * cols + (col - 1))
}

fn evaluated_index_argument(value: Value) -> Result<EvaluatedIndexArgument, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(EvaluatedIndexArgument::Numeric {
            values: vec![number],
            rows: 1,
            cols: 1,
            dims: vec![1, 1],
            logical: false,
        }),
        Value::Logical(flag) => Ok(EvaluatedIndexArgument::Numeric {
            values: vec![truth_number(flag)],
            rows: 1,
            cols: 1,
            dims: vec![1, 1],
            logical: true,
        }),
        Value::Matrix(matrix) => Ok(EvaluatedIndexArgument::Numeric {
            values: matrix.scalar_elements()?,
            rows: matrix.rows,
            cols: matrix.cols,
            dims: matrix.dims().to_vec(),
            logical: matrix.storage_class() == ArrayStorageClass::Logical,
        }),
        other => Err(RuntimeError::TypeError(format!(
            "indexing expects numeric scalar or matrix selectors, found {}",
            other.kind_name()
        ))),
    }
}

fn numeric_selector_indices(values: &[f64]) -> Result<Vec<usize>, RuntimeError> {
    values
        .iter()
        .copied()
        .map(scalar_numeric_index)
        .collect::<Result<Vec<_>, _>>()
}

fn scalar_numeric_index(value: f64) -> Result<usize, RuntimeError> {
    if value < 1.0 || value.fract() != 0.0 {
        return Err(RuntimeError::InvalidIndex(format!(
            "expected positive integer index, found {value}"
        )));
    }
    Ok(value as usize)
}

fn is_logical_selector(values: &[f64]) -> bool {
    values.iter().all(|value| *value == 0.0 || *value == 1.0)
}

fn is_scalar_logical_selector(values: &[f64], logical: bool) -> bool {
    logical && values.len() == 1
}

fn bounded_logical_selector_indices(
    values: &[f64],
    rows: usize,
    cols: usize,
    dims: &[usize],
    extent: usize,
    dimension: &str,
    kind: &str,
) -> Result<Vec<usize>, RuntimeError> {
    let indices = logical_selector_indices(values, rows, cols, dims);
    if let Some(index) = indices.iter().copied().find(|index| *index > extent) {
        return Err(RuntimeError::InvalidIndex(format!(
            "{dimension} logical index contains a true value outside of {kind} extent {extent} at position {index}"
        )));
    }
    Ok(indices)
}

fn logical_selector_indices(
    values: &[f64],
    rows: usize,
    cols: usize,
    dims: &[usize],
) -> Vec<usize> {
    let mut normalized_dims = if dims.len() > 2 && dims.iter().product::<usize>() == values.len() {
        dims.to_vec()
    } else if rows * cols == values.len() {
        vec![rows, cols]
    } else {
        vec![values.len().max(1), 1]
    };
    while normalized_dims.len() < 2 {
        normalized_dims.push(1);
    }

    (0..values.len())
        .filter_map(|linear| {
            let index = column_major_multi_index(linear, &normalized_dims);
            let offset = row_major_linear_index(&index, &normalized_dims);
            (values[offset] != 0.0).then_some(linear + 1)
        })
        .collect()
}

fn logical_linear_result_shape(
    target_rows: usize,
    target_cols: usize,
    count: usize,
) -> (usize, usize) {
    if target_rows == 1 && target_cols != 1 {
        (1, count)
    } else {
        (count, 1)
    }
}

fn scalar_logical_selection_positions(value: f64) -> Vec<usize> {
    if value != 0.0 {
        vec![1]
    } else {
        Vec::new()
    }
}

fn column_major_multi_index(mut linear: usize, dims: &[usize]) -> Vec<usize> {
    let mut index = vec![0usize; dims.len()];
    for axis in 0..dims.len() {
        let dim = dims[axis].max(1);
        index[axis] = linear % dim;
        linear /= dim;
    }
    index
}

fn row_major_multi_index(mut linear: usize, dims: &[usize]) -> Vec<usize> {
    let mut index = vec![0usize; dims.len()];
    for axis in (0..dims.len()).rev() {
        let dim = dims[axis].max(1);
        index[axis] = linear % dim;
        linear /= dim;
    }
    index
}

fn row_major_linear_index(index: &[usize], dims: &[usize]) -> usize {
    let mut linear = 0usize;
    for (axis, &value) in index.iter().enumerate() {
        linear = (linear * dims[axis].max(1)) + value;
    }
    linear
}

fn indexing_dimensions_from_dims(dims: &[usize], total_arguments: usize) -> Vec<usize> {
    if total_arguments == 0 {
        return Vec::new();
    }

    if total_arguments == 1 {
        return vec![dims.iter().product::<usize>()];
    }

    let mut normalized = dims.to_vec();
    while normalized.len() < total_arguments {
        normalized.push(1);
    }

    if total_arguments >= normalized.len() {
        return normalized;
    }

    let mut effective = Vec::with_capacity(total_arguments);
    for axis in 0..total_arguments {
        if axis + 1 < total_arguments {
            effective.push(normalized[axis]);
        } else {
            effective.push(normalized[axis..].iter().product());
        }
    }
    effective
}

fn storage_shape_from_dimensions(dims: &[usize]) -> (usize, usize) {
    match dims {
        [] => (1, 1),
        [only] => (*only, 1),
        [rows, cols] => (*rows, *cols),
        [rows, rest @ ..] => (*rows, rest.iter().product()),
    }
}

fn equivalent_dimensions(lhs: &[usize], rhs: &[usize]) -> bool {
    fn canonical(dims: &[usize]) -> Vec<usize> {
        let mut out = if dims.is_empty() {
            vec![1, 1]
        } else {
            dims.to_vec()
        };
        while out.len() > 2 && out.last() == Some(&1) {
            out.pop();
        }
        if out.len() == 1 {
            out.push(1);
        }
        out
    }

    canonical(lhs) == canonical(rhs)
}

fn read_field_value(target: &Value, field: &str) -> Result<Value, RuntimeError> {
    match target {
        Value::Struct(struct_value) => struct_value.fields.get(field).cloned().ok_or_else(|| {
            RuntimeError::MissingVariable(format!("struct field `{field}` is not defined"))
        }),
        Value::Object(object) => {
            if let Some(value) = object.property_value(field) {
                Ok(value)
            } else if object_has_method(object, field) {
                Ok(bound_method_value(object, field))
            } else {
                Err(RuntimeError::MissingVariable(format!(
                    "object property or method `{field}` is not defined for class `{}`",
                    object.class.class_name
                )))
            }
        }
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => {
            if matrix.rows == 1 && matrix.cols == 1 {
                let Value::Struct(struct_value) = &matrix.elements[0] else {
                    unreachable!("matrix_is_struct_array checked every element");
                };
                struct_value.fields.get(field).cloned().ok_or_else(|| {
                    RuntimeError::MissingVariable(format!("struct field `{field}` is not defined"))
                })
            } else {
                Err(single_output_dot_or_brace_result_error(matrix.elements.len()))
            }
        }
        _ => Err(RuntimeError::TypeError(format!(
            "field access is only defined for struct values or struct arrays in the current interpreter, found {}",
            target.kind_name()
        ))),
    }
}

fn read_field_outputs(target: &Value, field: &str) -> Result<Vec<Value>, RuntimeError> {
    match target {
        Value::Struct(struct_value) => {
            let value = struct_value.fields.get(field).cloned().ok_or_else(|| {
                RuntimeError::MissingVariable(format!("struct field `{field}` is not defined"))
            })?;
            Ok(vec![value])
        }
        Value::Object(object) => Ok(vec![read_field_value(&Value::Object(object.clone()), field)?]),
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => {
            let mut values = Vec::with_capacity(matrix.elements.len());
            for index in 1..=matrix.rows * matrix.cols {
                let position = linear_column_major_position(index, matrix.rows, matrix.cols, "struct array")?;
                let Value::Struct(struct_value) = &matrix.elements[position] else {
                    unreachable!("matrix_is_struct_array checked every element");
                };
                let value = struct_value.fields.get(field).cloned().ok_or_else(|| {
                    RuntimeError::MissingVariable(format!("struct field `{field}` is not defined"))
                })?;
                values.push(value);
            }
            Ok(values)
        }
        _ => Err(RuntimeError::TypeError(format!(
            "field access is only defined for struct values or struct arrays in the current interpreter, found {}",
            target.kind_name()
        ))),
    }
}

fn read_field_outputs_from_values(
    targets: &[Value],
    field: &str,
) -> Result<Vec<Value>, RuntimeError> {
    let mut values = Vec::new();
    for target in targets {
        values.extend(read_field_outputs(target, field)?);
    }
    Ok(values)
}

fn read_field_lvalue_value(target: &Value, field: &str) -> Result<Value, RuntimeError> {
    match target {
        Value::Object(_) => read_field_value(target, field),
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => {
            let mut elements = Vec::with_capacity(matrix.elements.len());
            for element in &matrix.elements {
                let value = match read_field_value(element, field) {
                    Ok(value) => value,
                    Err(RuntimeError::MissingVariable(_)) => default_struct_value_like(element),
                    Err(error) => return Err(error),
                };
                elements.push(value);
            }
            if elements.iter().all(|value| matches!(value, Value::Cell(_))) {
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
                    Err(RuntimeError::MissingVariable(_)) => default_struct_value_like(element),
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
            Err(RuntimeError::MissingVariable(_)) => Ok(default_struct_value_like(target)),
            Err(error) => Err(error),
        },
    }
}

fn read_field_lvalue_value_for_assignment(
    target: &Value,
    field: &str,
    rest: &[LValueProjection],
    leaf: &LValueLeaf,
) -> Result<Value, RuntimeError> {
    let fallback = || {
        default_field_projection_value(target, rest, leaf).ok_or_else(|| {
            RuntimeError::MissingVariable(format!("struct field `{field}` is not defined"))
        })
    };

    match target {
        Value::Object(_) => read_field_value(target, field),
        Value::Struct(_) => match read_field_value(target, field) {
            Ok(value) => Ok(value),
            Err(RuntimeError::MissingVariable(_)) => fallback(),
            Err(error) => Err(error),
        },
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => {
            if matrix.element_count() > 1 && field_subindexing_requires_single_struct(rest) {
                return Err(unsupported_multi_struct_field_subindexing_error(field));
            }
            let mut elements = Vec::with_capacity(matrix.elements.len());
            let mut all_cells = true;
            for element in &matrix.elements {
                let value = match read_field_value(element, field) {
                    Ok(value) => value,
                    Err(RuntimeError::MissingVariable(_)) => {
                        default_nested_lvalue_value(rest, leaf).ok_or_else(|| {
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
                        default_nested_lvalue_value(rest, leaf).ok_or_else(|| {
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

fn assign_struct_path(target: Value, path: &[String], value: Value) -> Result<Value, RuntimeError> {
    if path.is_empty() {
        return Ok(value);
    }

    match target {
        Value::Object(mut object) => {
            let field = &path[0];
            if !object.class.property_order.iter().any(|name| name == field) {
                return Err(RuntimeError::MissingVariable(format!(
                    "object property `{field}` is not defined for class `{}`",
                    object.class.class_name
                )));
            }
            let assigned = if path.len() == 1 {
                value
            } else {
                let next = object.property_value(field).unwrap_or_else(empty_matrix_value);
                assign_struct_path(next, &path[1..], value)?
            };
            object.set_property_value(field, assigned)?;
            Ok(Value::Object(object))
        }
        Value::Struct(mut struct_value) => {
            let field = &path[0];
            let assigned = if path.len() == 1 {
                value
            } else {
                let next = struct_value
                    .fields
                    .remove(field)
                    .unwrap_or_else(|| Value::Struct(StructValue::default()));
                assign_struct_path(next, &path[1..], value)?
            };
            struct_value.insert_field(field.clone(), assigned);
            Ok(Value::Struct(struct_value))
        }
        Value::Matrix(matrix) if matrix_is_struct_array(&matrix) => {
            let assigned_values =
                struct_assignment_values(&value, &matrix.dims, matrix.rows, matrix.cols)?;
            let mut elements = Vec::with_capacity(matrix.elements.len());
            for (element, assigned_value) in matrix.elements.into_iter().zip(assigned_values) {
                elements.push(assign_struct_path(element, path, assigned_value)?);
            }
            Ok(Value::Matrix(MatrixValue::with_dimensions(
                matrix.rows,
                matrix.cols,
                matrix.dims.clone(),
                elements,
            )?))
        }
        Value::Cell(cell) if cell.elements.iter().all(value_is_struct_assignment_target) => {
            let total_targets = nested_struct_assignment_target_count(&Value::Cell(cell.clone()))
                .expect("guard ensured nested struct assignment targets");
            let mut flat_values =
                distributed_struct_assignment_values(&value, total_targets)?.into_iter();
            let mut elements = Vec::with_capacity(cell.elements.len());
            for element in cell.elements {
                let count = nested_struct_assignment_target_count(&element)
                    .expect("guard ensured nested struct assignment targets");
                let chunk = flat_values.by_ref().take(count).collect::<Vec<_>>();
                let assigned_value = pack_struct_assignment_chunk(&element, chunk)?;
                elements.push(assign_struct_path(element, path, assigned_value)?);
            }
            Ok(Value::Cell(CellValue::with_dimensions(
                cell.rows,
                cell.cols,
                cell.dims.clone(),
                elements,
            )?))
        }
        other => Err(RuntimeError::TypeError(format!(
            "field assignment requires struct storage, found {}",
            other.kind_name()
        ))),
    }
}

fn matrix_is_struct_array(matrix: &MatrixValue) -> bool {
    matrix
        .iter()
        .all(|element| matches!(element, Value::Struct(_)))
}

fn value_is_struct_assignment_target(value: &Value) -> bool {
    nested_struct_assignment_target_count(value).is_some()
}

fn nested_struct_assignment_target_count(value: &Value) -> Option<usize> {
    match value {
        Value::Struct(_) | Value::Object(_) => Some(1),
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => Some(matrix.elements.len()),
        Value::Cell(cell) => {
            let mut total = 0;
            for element in &cell.elements {
                total += nested_struct_assignment_target_count(element)?;
            }
            Some(total)
        }
        _ => None,
    }
}

fn is_single_output_dot_or_brace_result_error(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Unsupported(message)
            if message.starts_with(
                "Expected one output from a curly brace or dot indexing expression, but there were "
            )
    )
}

fn single_output_dot_or_brace_result_error(count: usize) -> RuntimeError {
    RuntimeError::Unsupported(format!(
        "Expected one output from a curly brace or dot indexing expression, but there were {} results.",
        count
    ))
}

fn distributed_struct_assignment_values(
    value: &Value,
    count: usize,
) -> Result<Vec<Value>, RuntimeError> {
    match value {
        Value::Matrix(matrix) if matrix.rows == 1 && matrix.cols == 1 => {
            Ok(vec![matrix.elements[0].clone(); count])
        }
        Value::Matrix(matrix) if matrix.elements.len() == count => linearized_matrix_elements(matrix),
        Value::Cell(cell) if cell.rows == 1 && cell.cols == 1 => {
            Ok(vec![cell.elements[0].clone(); count])
        }
        Value::Cell(cell) if cell.elements.len() == count => linearized_cell_elements(cell),
        other if count > 0 => Ok(vec![other.clone(); count]),
        _ => Ok(Vec::new()),
    }
}

fn pack_struct_assignment_chunk(target: &Value, values: Vec<Value>) -> Result<Value, RuntimeError> {
    match target {
        Value::Struct(_) | Value::Object(_) => values.into_iter().next().ok_or_else(|| {
            RuntimeError::ShapeError("struct assignment expected one rhs element".to_string())
        }),
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => Ok(Value::Matrix(
            MatrixValue::with_dimensions(matrix.rows, matrix.cols, matrix.dims.clone(), values)?,
        )),
        Value::Cell(cell) if cell.elements.iter().all(value_is_struct_assignment_target) => {
            Ok(Value::Cell(CellValue::with_dimensions(
                cell.rows,
                cell.cols,
                cell.dims.clone(),
                values,
            )?))
        }
        other => Err(RuntimeError::TypeError(format!(
            "field assignment requires struct storage, found {}",
            other.kind_name()
        ))),
    }
}

fn default_struct_value_like(target: &Value) -> Value {
    match target {
        Value::Object(object) => Value::Object(object.clone()),
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => Value::Matrix(
            MatrixValue::with_dimensions(
                matrix.rows,
                matrix.cols,
                matrix.dims.clone(),
                vec![Value::Struct(StructValue::default()); matrix.elements.len()],
            )
            .expect("struct default matrix should preserve dimensions"),
        ),
        Value::Cell(cell) if cell.elements.iter().all(value_is_struct_assignment_target) => {
            Value::Cell(
                CellValue::with_dimensions(
                    cell.rows,
                    cell.cols,
                    cell.dims.clone(),
                    cell.elements
                        .iter()
                        .map(default_struct_value_like)
                        .collect::<Vec<_>>(),
                )
                .expect("struct default cell should preserve dimensions"),
            )
        }
        _ => Value::Struct(StructValue::default()),
    }
}

fn default_cell_contents_value_like(target: &Value) -> Option<Value> {
    match target {
        Value::Struct(_) => Some(empty_cell_value()),
        Value::Matrix(matrix) if matrix_is_struct_array(matrix) => Some(Value::Cell(
            CellValue::with_dimensions(
                matrix.rows,
                matrix.cols,
                matrix.dims.clone(),
                vec![empty_cell_value(); matrix.elements.len()],
            )
            .expect("cell default should preserve struct-array dimensions"),
        )),
        Value::Cell(cell) if cell.elements.iter().all(value_is_struct_assignment_target) => {
            Some(Value::Cell(
                CellValue::with_dimensions(
                    cell.rows,
                    cell.cols,
                    cell.dims.clone(),
                    vec![empty_cell_value(); cell.elements.len()],
                )
                .expect("cell default should preserve container dimensions"),
            ))
        }
        _ => None,
    }
}

fn default_field_projection_value(
    target: &Value,
    rest: &[LValueProjection],
    leaf: &LValueLeaf,
) -> Option<Value> {
    let element_default = default_nested_lvalue_value(rest, leaf)?;
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

fn default_struct_selection_value_for_index_update(
    current: &Value,
    indices: &[EvaluatedIndexArgument],
) -> Result<Option<Value>, RuntimeError> {
    match current {
        Value::Matrix(matrix) => {
            let plan = matrix_assignment_plan(matrix, indices)?;
            let count = plan.selection.positions.len();
            if count == 0 {
                return Ok(Some(empty_matrix_value()));
            }
            if count == 1 {
                return Ok(Some(Value::Struct(StructValue::default())));
            }
            Ok(Some(Value::Matrix(MatrixValue::with_dimensions(
                plan.selection.rows,
                plan.selection.cols,
                plan.selection.dims,
                vec![Value::Struct(StructValue::default()); count],
            )?)))
        }
        Value::Struct(_) => {
            let matrix = MatrixValue::new(1, 1, vec![Value::Struct(StructValue::default())])?;
            default_struct_selection_value_for_index_update(&Value::Matrix(matrix), indices)
        }
        _ => Ok(None),
    }
}

fn struct_assignment_values(
    value: &Value,
    dims: &[usize],
    rows: usize,
    cols: usize,
) -> Result<Vec<Value>, RuntimeError> {
    match value {
        Value::Matrix(matrix) if matrix.rows == 1 && matrix.cols == 1 => {
            Ok(vec![matrix.elements[0].clone(); rows * cols])
        }
        Value::Matrix(matrix) if matrix.elements.len() == rows * cols => Ok(matrix.elements.clone()),
        Value::Matrix(matrix)
            if matrix.rows == rows
                && matrix.cols == cols
                && equivalent_dimensions(&matrix.dims, dims) =>
        {
            Ok(matrix.elements.clone())
        }
        Value::Cell(cell) if cell.rows == 1 && cell.cols == 1 => {
            Ok(vec![cell.elements[0].clone(); rows * cols])
        }
        Value::Cell(cell) if cell.elements.len() == rows * cols => Ok(cell.elements.clone()),
        Value::Cell(cell)
            if cell.rows == rows
                && cell.cols == cols
                && equivalent_dimensions(&cell.dims, dims) =>
        {
            Ok(cell.elements.clone())
        }
        _ => Ok(vec![value.clone(); rows * cols]),
    }
}

fn first_output_or_unit(outputs: Vec<Value>) -> Value {
    outputs.into_iter().next().unwrap_or(Value::Scalar(0.0))
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

fn field_target_requires_list_assignment(expression: &HirExpression) -> bool {
    match expression {
        HirExpression::CellIndex { .. } | HirExpression::Call { .. } => true,
        HirExpression::FieldAccess { target, .. } => field_target_requires_list_assignment(target),
        _ => false,
    }
}

fn field_subindexing_requires_single_struct(rest: &[LValueProjection]) -> bool {
    rest.iter()
        .any(|projection| matches!(projection, LValueProjection::Paren(_)))
}

fn unsupported_simple_csl_assignment_error(count: usize) -> RuntimeError {
    RuntimeError::Unsupported(format!(
        "assigning to {count} elements using a simple assignment statement is not supported; use comma-separated list assignment"
    ))
}

fn unsupported_multi_struct_field_subindexing_error(field: &str) -> RuntimeError {
    RuntimeError::Unsupported(format!(
        "indexing into part of field `{field}` for multiple struct-array elements is not supported; index a single struct element first"
    ))
}

fn direct_multi_value_count(value: &Value) -> Option<usize> {
    match value {
        Value::Matrix(matrix) if matrix.element_count() > 1 => Some(matrix.element_count()),
        Value::Cell(cell) if cell.element_count() > 1 => Some(cell.element_count()),
        _ => None,
    }
}

fn direct_multi_value_values_ordered(value: &Value) -> Result<Option<Vec<Value>>, RuntimeError> {
    match value {
        Value::Matrix(matrix) if matrix.element_count() > 1 => {
            Ok(Some(linearized_matrix_elements(matrix)?))
        }
        Value::Cell(cell) if cell.element_count() > 1 => Ok(Some(linearized_cell_elements(cell)?)),
        _ => Ok(None),
    }
}

fn index_argument_contains_end_keyword(argument: &HirIndexArgument) -> bool {
    match argument {
        HirIndexArgument::Expression(expression) => expression_contains_end_keyword(expression),
        HirIndexArgument::FullSlice => false,
        HirIndexArgument::End => true,
    }
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
        HirExpression::MatrixLiteral(rows) | HirExpression::CellLiteral(rows) => {
            rows.iter().flatten().any(expression_contains_end_keyword)
        }
        HirExpression::Call { target, args } => {
            matches!(
                target,
                HirCallTarget::Expression(target_expression)
                    if expression_contains_end_keyword(target_expression)
            ) || args.iter().any(index_argument_contains_end_keyword)
        }
        HirExpression::CellIndex { target, indices } => {
            expression_contains_end_keyword(target)
                || indices.iter().any(index_argument_contains_end_keyword)
        }
        HirExpression::FieldAccess { target, .. } => expression_contains_end_keyword(target),
        _ => false,
    }
}

fn mexception_method_builtin_name<'a>(value: &Value, field: &'a str) -> Option<&'a str> {
    if !is_mexception_like_value(value) {
        return None;
    }

    match field {
        "getReport" | "addCause" | "throw" | "rethrow" | "throwAsCaller" => Some(field),
        _ => None,
    }
}

fn is_mexception_like_value(value: &Value) -> bool {
    let Value::Struct(struct_value) = value else {
        return false;
    };
    struct_value.fields.contains_key("identifier")
        && struct_value.fields.contains_key("message")
        && struct_value.fields.contains_key("stack")
        && struct_value.fields.contains_key("cause")
}

fn text_value(value: &Value) -> Result<&str, RuntimeError> {
    match value {
        Value::CharArray(text) | Value::String(text) => Ok(text),
        other => Err(RuntimeError::TypeError(format!(
            "expected char or string value, found {}",
            other.kind_name()
        ))),
    }
}

fn looks_like_warning_identifier(text: &str) -> bool {
    text.contains(':') && !text.chars().any(char::is_whitespace)
}

fn format_text_with_values_for_builtin(
    format: &str,
    values: &[Value],
    builtin_name: &str,
) -> Result<String, RuntimeError> {
    if values.is_empty() {
        return Ok(format.to_string());
    }

    let mut compose_args = Vec::with_capacity(values.len() + 1);
    compose_args.push(Value::CharArray(format.to_string()));
    compose_args.extend(values.iter().cloned());
    let rendered = invoke_stdlib_builtin_outputs("compose", &compose_args, 1)?;
    let Some(value) = rendered.first() else {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} formatting did not produce an output value"
        )));
    };
    match value {
        Value::Cell(cell) if cell.elements.len() == 1 => Ok(text_value(&cell.elements[0])?.to_string()),
        Value::Cell(_) => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} formatting currently expects a single rendered text result"
        ))),
        _ => Ok(text_value(value)?.to_string()),
    }
}

fn decode_text_literal(lexeme: &str, delimiter: char) -> Result<String, RuntimeError> {
    let Some(inner) = lexeme
        .strip_prefix(delimiter)
        .and_then(|text| text.strip_suffix(delimiter))
    else {
        return Err(RuntimeError::TypeError(format!(
            "invalid quoted literal `{lexeme}`"
        )));
    };

    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == delimiter && chars.peek() == Some(&delimiter) {
            out.push(delimiter);
            chars.next();
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

pub(crate) fn parse_numeric_literal(text: &str) -> Result<Value, RuntimeError> {
    if let Some(number) = text.strip_suffix(['i', 'j']) {
        let imag = number.parse::<f64>().map_err(|error| {
            RuntimeError::TypeError(format!("failed to parse numeric literal `{text}`: {error}"))
        })?;
        return Ok(value_from_numeric_complex_parts(NumericComplexParts {
            real: 0.0,
            imag,
        }));
    }

    text.parse::<f64>().map(Value::Scalar).map_err(|error| {
        RuntimeError::TypeError(format!("failed to parse numeric literal `{text}`: {error}"))
    })
}

fn truth_number(value: bool) -> f64 {
    if value {
        1.0
    } else {
        0.0
    }
}

fn logical_value(value: bool) -> Value {
    Value::Logical(value)
}

fn default_lvalue_root_value(projections: &[LValueProjection], leaf: &LValueLeaf) -> Option<Value> {
    match projections.first() {
        Some(LValueProjection::Field(_)) => Some(Value::Struct(StructValue::default())),
        Some(LValueProjection::Paren(_)) => Some(empty_matrix_value()),
        Some(LValueProjection::Brace(_)) => Some(empty_cell_value()),
        None => match leaf {
            LValueLeaf::Field { .. } => Some(Value::Struct(StructValue::default())),
            LValueLeaf::Index {
                kind: IndexAssignmentKind::Paren,
                ..
            } => Some(empty_matrix_value()),
            LValueLeaf::Index {
                kind: IndexAssignmentKind::Brace,
                ..
            } => Some(empty_cell_value()),
        },
    }
}

fn default_nested_lvalue_value(
    projections: &[LValueProjection],
    leaf: &LValueLeaf,
) -> Option<Value> {
    match projections.first() {
        Some(LValueProjection::Field(_)) => Some(Value::Struct(StructValue::default())),
        Some(LValueProjection::Paren(_)) => Some(empty_matrix_value()),
        Some(LValueProjection::Brace(_)) => Some(empty_cell_value()),
        None => match leaf {
            LValueLeaf::Field { .. } => Some(Value::Struct(StructValue::default())),
            LValueLeaf::Index {
                kind: IndexAssignmentKind::Paren,
                ..
            } => Some(empty_matrix_value()),
            LValueLeaf::Index {
                kind: IndexAssignmentKind::Brace,
                ..
            } => Some(empty_cell_value()),
        },
    }
}

fn brace_args_contain_full_slice(args: &[HirIndexArgument]) -> bool {
    args.iter()
        .any(|argument| matches!(argument, HirIndexArgument::FullSlice))
}

fn infer_missing_root_receiver_count_from_value(
    receiver_indices: &[HirIndexArgument],
    value: &Value,
) -> Option<usize> {
    let full_slice_axes = receiver_indices
        .iter()
        .enumerate()
        .filter_map(|(axis, argument)| matches!(argument, HirIndexArgument::FullSlice).then_some(axis))
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

fn undefined_brace_assignment_with_colon(
    projections: &[LValueProjection],
    leaf: &LValueLeaf,
) -> bool {
    projections.iter().any(|projection| match projection {
        LValueProjection::Brace(args) => brace_args_contain_full_slice(args),
        _ => false,
    }) || matches!(
        leaf,
        LValueLeaf::Index {
            kind: IndexAssignmentKind::Brace,
            indices,
            ..
        } if brace_args_contain_full_slice(indices)
    )
}

fn apply_index_update(
    current: Value,
    indices: &[EvaluatedIndexArgument],
    value: Value,
    kind: IndexAssignmentKind,
) -> Result<Value, RuntimeError> {
    match (kind, current) {
        (IndexAssignmentKind::Paren, Value::Matrix(matrix)) => {
            Ok(Value::Matrix(assign_matrix_index(matrix, indices, value)?))
        }
        (IndexAssignmentKind::Paren, Value::CharArray(text)) => {
            Ok(Value::CharArray(assign_char_array_index(text, indices, value)?))
        }
        (IndexAssignmentKind::Brace, Value::Cell(cell_value)) => {
            Ok(Value::Cell(assign_cell_content_index(cell_value, indices, value)?))
        }
        (IndexAssignmentKind::Paren, Value::Cell(cell_value)) => {
            Ok(Value::Cell(assign_cell_index(cell_value, indices, value)?))
        }
        (IndexAssignmentKind::Paren, Value::Struct(struct_value)) => {
            let matrix = MatrixValue::new(1, 1, vec![Value::Struct(struct_value)])
                .expect("single struct matrix is valid");
            let updated = assign_matrix_index(matrix, indices, value)?;
            if updated.rows == 1
                && updated.cols == 1
                && matches!(updated.elements.first(), Some(Value::Struct(_)))
            {
                Ok(updated.elements.into_iter().next().expect("single struct element"))
            } else {
                Ok(Value::Matrix(updated))
            }
        }
        (IndexAssignmentKind::Paren, other) => Err(RuntimeError::TypeError(format!(
            "indexed assignment with `()` is only defined for matrix, char, cell, or current struct-array values in the current interpreter, found {}",
            other.kind_name()
        ))),
        (IndexAssignmentKind::Brace, other) => Err(RuntimeError::TypeError(format!(
            "indexed assignment with `{{}}` is only defined for cell values in the current interpreter, found {}",
            other.kind_name()
        ))),
    }
}

fn builtin_value_from_name(name: &str) -> Result<Value, RuntimeError> {
    match name {
        "true" => Ok(logical_value(true)),
        "false" => Ok(logical_value(false)),
        "i" | "j" => Ok(Value::Complex(ComplexValue {
            real: 0.0,
            imag: 1.0,
        })),
        "pi" => Ok(Value::Scalar(std::f64::consts::PI)),
        "eps" => Ok(Value::Scalar(f64::EPSILON)),
        "realmin" => Ok(Value::Scalar(f64::MIN_POSITIVE)),
        "realmax" => Ok(Value::Scalar(f64::MAX)),
        "flintmax" => Ok(Value::Scalar(9_007_199_254_740_992.0)),
        "inf" | "Inf" => Ok(Value::Scalar(f64::INFINITY)),
        "nan" | "NaN" => Ok(Value::Scalar(f64::NAN)),
        other => Err(RuntimeError::Unsupported(format!(
            "builtin value `{other}` is not implemented in the current interpreter"
        ))),
    }
}

fn control_name(control: ControlFlow) -> &'static str {
    match control {
        ControlFlow::Continue => "continue",
        ControlFlow::Break => "break",
        ControlFlow::Return => "return",
    }
}

fn format_frontend_diagnostics(diagnostics: &[matlab_frontend::diagnostics::Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| format!("{} {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn format_semantic_diagnostics(
    diagnostics: &[matlab_semantics::diagnostics::SemanticDiagnostic],
) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| format!("{} {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::{
        consume_figure_backend_events, distributed_cell_assignment_values,
        distributed_struct_assignment_values, execute_function_file,
        execute_function_file_bytecode_module, execute_script, execute_script_bytecode,
        execute_script_bytecode_module,
        invoke_graphics_builtin_outputs, linearized_cell_elements, linearized_matrix_elements,
        logical_value, plan_dimension_selector, plan_linear_selector, render_execution_result,
        render_figure_backend_index, render_matlab_execution_result,
        render_native_figure_host_index, session_manifest_json, EvaluatedIndexArgument,
        FigureBackendState, Frame, HirItem, Interpreter, RenderedFigure, SelectorPlanMode,
        resize_matrix_to_dims,
    };
    use matlab_frontend::{
        ast::CompilationUnitKind,
        parser::{parse_source, ParseMode},
        source::SourceFileId,
    };
    use matlab_interop::{read_mat_file, write_mat_file};
    use matlab_ir::lower_to_hir;
    use matlab_codegen::emit_bytecode;
    use matlab_optimizer::optimize_module;
    use matlab_resolver::ResolverContext;
    use matlab_runtime::{
        CellValue, FunctionHandleTarget, MatrixValue, RuntimeError, Value, Workspace,
    };
    use matlab_semantics::analyze_compilation_unit_with_context;
    use std::{
        collections::BTreeSet,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn execute_script_source_result(source: &str) -> Result<super::ExecutionResult, RuntimeError> {
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(PathBuf::from("inline_test.m")),
        );
        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let hir = lower_to_hir(&unit, &analysis);
        execute_script(&hir)
    }

    fn execute_script_source(source: &str) -> super::ExecutionResult {
        execute_script_source_result(source).expect("execute script")
    }

    fn execute_script_source_bytecode_result(
        source: &str,
    ) -> Result<super::ExecutionResult, RuntimeError> {
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(PathBuf::from("inline_test.m")),
        );
        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let hir = lower_to_hir(&unit, &analysis);
        execute_script_bytecode(&hir)
    }

    fn execute_script_source_bytecode(source: &str) -> super::ExecutionResult {
        execute_script_source_bytecode_result(source).expect("execute bytecode script")
    }

    fn execute_path_result(
        path: &std::path::Path,
        args: &[Value],
    ) -> Result<super::ExecutionResult, RuntimeError> {
        let source = fs::read_to_string(path).expect("read source");
        let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(path.to_path_buf()),
        );
        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let hir = lower_to_hir(&unit, &analysis);
        match unit.kind {
            CompilationUnitKind::Script => execute_script(&hir),
            CompilationUnitKind::FunctionFile => execute_function_file(&hir, args),
            CompilationUnitKind::ClassFile => Err(RuntimeError::Unsupported(
                "class-file tests must execute a script or function entrypoint".to_string(),
            )),
        }
    }

    fn execute_path(path: &std::path::Path, args: &[Value]) -> super::ExecutionResult {
        execute_path_result(path, args).expect("execute source path")
    }

    fn execute_path_bytecode_result(
        path: &std::path::Path,
        args: &[Value],
    ) -> Result<super::ExecutionResult, RuntimeError> {
        let source = fs::read_to_string(path).expect("read source");
        let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(path.to_path_buf()),
        );
        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let hir = lower_to_hir(&unit, &analysis);
        let optimized = optimize_module(&hir);
        let bytecode = emit_bytecode(&optimized.module);
        match unit.kind {
            CompilationUnitKind::Script => {
                execute_script_bytecode_module(&bytecode, path.display().to_string())
            }
            CompilationUnitKind::FunctionFile => {
                execute_function_file_bytecode_module(&bytecode, args, path.display().to_string())
            }
            CompilationUnitKind::ClassFile => Err(RuntimeError::Unsupported(
                "class-file tests must execute a script or function entrypoint".to_string(),
            )),
        }
    }

    fn execute_path_bytecode(path: &std::path::Path, args: &[Value]) -> super::ExecutionResult {
        execute_path_bytecode_result(path, args).expect("execute bytecode source path")
    }

    #[test]
    fn matlab_render_displays_assignments_and_expression_ans() {
        let result = execute_script_source("a = 1\n1 + 2\n");
        assert_eq!(
            render_matlab_execution_result(&result),
            "a = 1\n\nans = 3\n"
        );
    }

    #[test]
    fn workspace_render_hides_implicit_ans() {
        let result = execute_script_source("1 + 2\nx = ans;\n");
        assert_eq!(render_execution_result(&result), "workspace\n  x = 3\n");
    }

    #[test]
    fn matlab_render_includes_disp_output() {
        let result = execute_script_source("disp(\"hello\")\n1 + 2\n");
        assert_eq!(
            render_matlab_execution_result(&result),
            "hello\n\nans = 3\n"
        );
    }

    #[test]
    fn disp_does_not_silently_produce_assignment_output() {
        let error = execute_script_source_result("x = disp(\"hello\")\n")
            .expect_err("disp should not return a value");
        assert!(
            error
                .to_string()
                .contains("disp currently does not return outputs"),
            "{error}"
        );
    }

    #[test]
    fn matlab_render_includes_fprintf_output() {
        let result = execute_script_source("fprintf(\"value=%d\", 5)\n");
        assert_eq!(render_matlab_execution_result(&result), "value=5");
    }

    #[test]
    fn sprintf_returns_formatted_text() {
        let result = execute_script_source("s = sprintf(\"value=%d\", 5)\n");
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  s = 'value=5'\n"
        );
    }

    #[test]
    fn pause_zero_is_a_valid_no_op() {
        let result = execute_script_source("pause(0);\nx = 1;\n");
        assert_eq!(render_execution_result(&result), "workspace\n  x = 1\n");
    }

    #[test]
    fn clc_clears_previous_displayed_output() {
        let result = execute_script_source("disp(\"hello\")\nclc\n1 + 2\n");
        assert_eq!(render_matlab_execution_result(&result), "ans = 3\n");
    }

    #[test]
    fn who_lists_current_workspace_variables() {
        let result = execute_script_source("x = 1;\ny = 2;\nwho\n");
        let rendered = render_matlab_execution_result(&result);
        assert!(rendered.contains("Your variables are:"), "{rendered}");
        assert!(rendered.contains("x"), "{rendered}");
        assert!(rendered.contains("y"), "{rendered}");
    }

    #[test]
    fn whos_displays_workspace_summary_table() {
        let result = execute_script_source("x = 1;\ny = [1, 2, 3];\nwhos\n");
        let rendered = render_matlab_execution_result(&result);
        assert!(rendered.contains("Name"), "{rendered}");
        assert!(rendered.contains("Bytes"), "{rendered}");
        assert!(rendered.contains("Class"), "{rendered}");
        assert!(rendered.contains("x"), "{rendered}");
        assert!(rendered.contains("y"), "{rendered}");
    }

    #[test]
    fn command_form_who_and_whos_file_queries_work_in_interpreter_and_bytecode() {
        let path = unique_temp_mat_path("who-whos-command-form");
        let mut workspace = Workspace::new();
        workspace.insert("alpha".to_string(), Value::Scalar(1.0));
        workspace.insert("beta".to_string(), Value::CharArray("ok".to_string()));
        write_mat_file(&path, &workspace).expect("write mat");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!("who -file {matlab_path}\nwhos -file {matlab_path}\n");

        let interpreted = execute_script_source(&source);
        let interpreted_rendered = render_matlab_execution_result(&interpreted);
        assert!(
            interpreted_rendered.contains("alpha"),
            "{interpreted_rendered}"
        );
        assert!(
            interpreted_rendered.contains("beta"),
            "{interpreted_rendered}"
        );
        assert!(
            interpreted_rendered.contains("Class"),
            "{interpreted_rendered}"
        );

        let bytecode = execute_script_source_bytecode(&source);
        let bytecode_rendered = render_matlab_execution_result(&bytecode);
        assert!(bytecode_rendered.contains("alpha"), "{bytecode_rendered}");
        assert!(bytecode_rendered.contains("beta"), "{bytecode_rendered}");
        assert!(bytecode_rendered.contains("Class"), "{bytecode_rendered}");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn tic_and_toc_support_timer_tokens() {
        let result = execute_script_source("t = tic();\ne = toc(t);\n");
        let t = result.workspace.get("t").expect("tic token");
        let e = result.workspace.get("e").expect("toc elapsed");
        assert!(t.as_scalar().expect("tic scalar token") > 0.0);
        assert!(e.as_scalar().expect("toc scalar elapsed") >= 0.0);
    }

    #[test]
    fn save_writes_selected_workspace_variables() {
        let path = unique_temp_mat_path("save-selected");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!("x = 1;\ny = 2;\nsave('{matlab_path}', 'x');\n");
        let _ = execute_script_source(&source);
        let snapshot = read_mat_file(&path).expect("read saved MAT-file");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot.get("x"), Some(&Value::Scalar(1.0)));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_without_output_populates_workspace() {
        let path = unique_temp_mat_path("load-workspace");
        let mut workspace = Workspace::new();
        workspace.insert("x".to_string(), Value::Scalar(3.0));
        workspace.insert("y".to_string(), Value::CharArray("hello".to_string()));
        write_mat_file(&path, &workspace).expect("write mat");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!("load('{matlab_path}');\n");
        let result = execute_script_source(&source);
        assert_eq!(result.workspace.get("x"), Some(&Value::Scalar(3.0)));
        assert_eq!(
            result.workspace.get("y"),
            Some(&Value::CharArray("hello".to_string()))
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_with_output_returns_struct() {
        let path = unique_temp_mat_path("load-struct");
        let mut workspace = Workspace::new();
        workspace.insert("x".to_string(), Value::Scalar(9.0));
        write_mat_file(&path, &workspace).expect("write mat");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!("s = load('{matlab_path}');\n");
        let result = execute_script_source(&source);
        let Value::Struct(struct_value) = result.workspace.get("s").expect("loaded struct") else {
            panic!("expected struct output");
        };
        assert_eq!(struct_value.fields.get("x"), Some(&Value::Scalar(9.0)));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_struct_writes_struct_fields_as_top_level_variables() {
        let path = unique_temp_mat_path("save-struct");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source =
            format!("s = struct('alpha', 1, 'beta', 2);\nsave('{matlab_path}', '-struct', 's');\n");
        let _ = execute_script_source(&source);
        let workspace = read_mat_file(&path).expect("read saved mat");
        assert_eq!(workspace.get("alpha"), Some(&Value::Scalar(1.0)));
        assert_eq!(workspace.get("beta"), Some(&Value::Scalar(2.0)));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_struct_with_selected_fields_filters_output() {
        let path = unique_temp_mat_path("save-struct-selected");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!(
            "s = struct('alpha', 1, 'beta', 2);\nsave('{matlab_path}', '-struct', 's', 'beta');\n"
        );
        let _ = execute_script_source(&source);
        let workspace = read_mat_file(&path).expect("read saved mat");
        assert_eq!(workspace.len(), 1);
        assert_eq!(workspace.get("beta"), Some(&Value::Scalar(2.0)));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_regexp_filters_workspace_variables() {
        let path = unique_temp_mat_path("save-regexp");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!(
            "alpha = 1;\nalphabet = 2;\nbeta = 3;\nsave('{matlab_path}', '-regexp', '^alpha');\n"
        );
        let _ = execute_script_source(&source);
        let workspace = read_mat_file(&path).expect("read saved mat");
        assert_eq!(workspace.len(), 2);
        assert_eq!(workspace.get("alpha"), Some(&Value::Scalar(1.0)));
        assert_eq!(workspace.get("alphabet"), Some(&Value::Scalar(2.0)));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_append_merges_new_variables_into_existing_file() {
        let path = unique_temp_mat_path("save-append");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let first = format!("alpha = 1;\nsave('{matlab_path}', 'alpha');\n");
        let second = format!("beta = 2;\nsave('{matlab_path}', '-append', 'beta');\n");
        let _ = execute_script_source(&first);
        let _ = execute_script_source(&second);
        let workspace = read_mat_file(&path).expect("read appended mat");
        assert_eq!(workspace.get("alpha"), Some(&Value::Scalar(1.0)));
        assert_eq!(workspace.get("beta"), Some(&Value::Scalar(2.0)));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn command_form_save_append_and_load_work_in_interpreter_and_bytecode() {
        let path = unique_temp_mat_path("save-append-command-form");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!(
            "alpha = 1;\n\
             save {matlab_path} alpha\n\
             beta = 2;\n\
             save {matlab_path} -append beta\n\
             clear alpha beta\n\
             out = load('{matlab_path}');\n"
        );

        let interpreted = execute_script_source(&source);
        let Value::Struct(interpreted_struct) =
            interpreted.workspace.get("out").expect("load struct")
        else {
            panic!("expected struct output");
        };
        assert_eq!(
            interpreted_struct.fields.get("alpha"),
            Some(&Value::Scalar(1.0))
        );
        assert_eq!(
            interpreted_struct.fields.get("beta"),
            Some(&Value::Scalar(2.0))
        );

        let bytecode = execute_script_source_bytecode(&source);
        let Value::Struct(bytecode_struct) = bytecode.workspace.get("out").expect("load struct")
        else {
            panic!("expected struct output");
        };
        assert_eq!(
            bytecode_struct.fields.get("alpha"),
            Some(&Value::Scalar(1.0))
        );
        assert_eq!(
            bytecode_struct.fields.get("beta"),
            Some(&Value::Scalar(2.0))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_v7_flag_is_accepted() {
        let path = unique_temp_mat_path("save-v7");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!("alpha = 1;\nsave('{matlab_path}', 'alpha', '-v7');\n");
        let _ = execute_script_source(&source);
        let workspace = read_mat_file(&path).expect("read saved mat");
        assert_eq!(workspace.get("alpha"), Some(&Value::Scalar(1.0)));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn who_file_lists_variables_from_mat_file() {
        let path = unique_temp_mat_path("who-file");
        let mut workspace = Workspace::new();
        workspace.insert("alpha".to_string(), Value::Scalar(1.0));
        workspace.insert("beta".to_string(), Value::Scalar(2.0));
        write_mat_file(&path, &workspace).expect("write mat");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let result = execute_script_source(&format!("who('-file', '{matlab_path}');\n"));
        let rendered = render_matlab_execution_result(&result);
        assert!(rendered.contains("alpha"), "{rendered}");
        assert!(rendered.contains("beta"), "{rendered}");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn whos_file_summarizes_variables_from_mat_file() {
        let path = unique_temp_mat_path("whos-file");
        let mut workspace = Workspace::new();
        workspace.insert("alpha".to_string(), Value::Scalar(1.0));
        workspace.insert("beta".to_string(), Value::CharArray("ok".to_string()));
        write_mat_file(&path, &workspace).expect("write mat");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let result = execute_script_source(&format!("whos('-file', '{matlab_path}');\n"));
        let rendered = render_matlab_execution_result(&result);
        assert!(rendered.contains("Name"), "{rendered}");
        assert!(rendered.contains("Class"), "{rendered}");
        assert!(rendered.contains("alpha"), "{rendered}");
        assert!(rendered.contains("beta"), "{rendered}");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_wildcard_filters_variables() {
        let path = unique_temp_mat_path("load-wildcard");
        let mut workspace = Workspace::new();
        workspace.insert("alpha".to_string(), Value::Scalar(1.0));
        workspace.insert("alphabet".to_string(), Value::Scalar(2.0));
        workspace.insert("beta".to_string(), Value::Scalar(3.0));
        write_mat_file(&path, &workspace).expect("write mat");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let result = execute_script_source(&format!("s = load('{matlab_path}', 'alpha*');\n"));
        let Value::Struct(struct_value) = result.workspace.get("s").expect("loaded struct") else {
            panic!("expected struct output");
        };
        assert_eq!(struct_value.fields.get("alpha"), Some(&Value::Scalar(1.0)));
        assert_eq!(
            struct_value.fields.get("alphabet"),
            Some(&Value::Scalar(2.0))
        );
        assert!(!struct_value.fields.contains_key("beta"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_regexp_filters_variables() {
        let path = unique_temp_mat_path("load-regexp");
        let mut workspace = Workspace::new();
        workspace.insert("alpha".to_string(), Value::Scalar(1.0));
        workspace.insert("alphabet".to_string(), Value::Scalar(2.0));
        workspace.insert("beta".to_string(), Value::Scalar(3.0));
        write_mat_file(&path, &workspace).expect("write mat");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let result = execute_script_source(&format!(
            "s = load('{matlab_path}', '-regexp', '^alpha');\n"
        ));
        let Value::Struct(struct_value) = result.workspace.get("s").expect("loaded struct") else {
            panic!("expected struct output");
        };
        assert_eq!(struct_value.fields.get("alpha"), Some(&Value::Scalar(1.0)));
        assert_eq!(
            struct_value.fields.get("alphabet"),
            Some(&Value::Scalar(2.0))
        );
        assert!(!struct_value.fields.contains_key("beta"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn command_form_save_load_clear_and_clearvars_support_regex_subset_in_interpreter_and_bytecode()
    {
        let path = unique_temp_mat_path("command-form-regexp");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!(
            "alpha = 1;\n\
             alphabet = 2;\n\
             beta = 3;\n\
             save {matlab_path} -regexp ^alpha\n\
             clear alpha alphabet beta\n\
             load {matlab_path} -regexp ^alpha\n\
             clearvars -except alphabet\n"
        );

        let interpreted = execute_script_source(&source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  alphabet = 2\n"
        );

        let bytecode = execute_script_source_bytecode(&source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  alphabet = 2\n"
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_and_load_function_handle_roundtrip() {
        let path = unique_temp_mat_path("save-load-function-handle");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!(
            "f = @sin;\nsave('{matlab_path}', 'f');\nclear('f');\ns = load('{matlab_path}');\n"
        );
        let result = execute_script_source(&source);
        let Value::Struct(struct_value) = result.workspace.get("s").expect("loaded struct") else {
            panic!("expected struct output");
        };
        let Value::FunctionHandle(handle) =
            struct_value.fields.get("f").expect("function handle field")
        else {
            panic!("expected function handle value");
        };
        assert_eq!(handle.display_name, "sin");
        assert_eq!(
            handle.target,
            FunctionHandleTarget::Named("sin".to_string())
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_and_load_string_roundtrip() {
        let path = unique_temp_mat_path("save-load-string");
        let matlab_path = path.to_string_lossy().replace('\\', "/");
        let source = format!(
            "s0 = string('hello');\ns1 = [\"left\" \"right\"];\nsave('{matlab_path}', 's0', 's1');\nclear('s0', 's1');\nout = load('{matlab_path}');\n"
        );
        let result = execute_script_source(&source);
        let Value::Struct(struct_value) = result.workspace.get("out").expect("loaded struct")
        else {
            panic!("expected struct output");
        };
        assert_eq!(
            struct_value.fields.get("s0"),
            Some(&Value::String("hello".to_string()))
        );
        let Value::Matrix(matrix) = struct_value.fields.get("s1").expect("string matrix field")
        else {
            panic!("expected string matrix");
        };
        assert_eq!(matrix.elements.len(), 2);
        assert_eq!(matrix.elements[0], Value::String("left".to_string()));
        assert_eq!(matrix.elements[1], Value::String("right".to_string()));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn nd_matrix_assignment_accepts_flat_row_vector_rhs_in_matlab_linear_order() {
        let result = execute_script_source(
            "a = zeros(2, 2, 3);\n\
             a(1, :, 2:3) = [5 6 7 8];\n\
             out = a(1, :, 2:3);\n\
             clear a;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out(:,:,1) = [5, 6]\n  out(:,:,2) = [7, 8]\n"
        );
    }

    #[test]
    fn nd_matrix_assignment_accepts_flat_column_vector_rhs_in_matlab_linear_order() {
        let result = execute_script_source(
            "a = zeros(2, 2, 3);\n\
             a(1, :, 2:3) = [5; 6; 7; 8];\n\
             out = a(1, :, 2:3);\n\
             clear a;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out(:,:,1) = [5, 6]\n  out(:,:,2) = [7, 8]\n"
        );
    }

    #[test]
    fn nd_matrix_assignment_still_accepts_exact_shape_rhs() {
        let result = execute_script_source(
            "a = zeros(2, 2, 3);\n\
             a(1, :, 2:3) = cat(3, [5 6], [7 8]);\n\
             out = a(1, :, 2:3);\n\
             clear a;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out(:,:,1) = [5, 6]\n  out(:,:,2) = [7, 8]\n"
        );
    }

    #[test]
    fn nd_cell_assignment_accepts_flat_rhs_and_scalar_expansion() {
        let result = execute_script_source(
            "cells = cat(3, {0 0; 0 0}, {0 0; 0 0}, {0 0; 0 0});\n\
             cells(1, :, 2:3) = {50 60 70 80};\n\
             row_out = cells(1, :, 2:3);\n\
             cells2 = cat(3, {0 0; 0 0}, {0 0; 0 0}, {0 0; 0 0});\n\
             cells2(1, :, 2:3) = {9};\n\
             scalar_out = cells2(1, :, 2:3);\n\
             clear cells cells2;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  row_out(:,:,1) = {50, 60}\n  row_out(:,:,2) = {70, 80}\n  scalar_out(:,:,1) = {9, 9}\n  scalar_out(:,:,2) = {9, 9}\n"
        );
    }

    #[test]
    fn nd_linearized_helpers_follow_matlab_column_major_order() {
        let matrix = MatrixValue::with_dimensions(
            2,
            4,
            vec![2, 2, 2],
            (1..=8)
                .map(|value| Value::Scalar(value as f64))
                .collect::<Vec<_>>(),
        )
        .expect("matrix dimensions should be valid");
        let matrix_linearized = linearized_matrix_elements(&matrix)
            .expect("matrix linearization should succeed")
            .into_iter()
            .map(|value| value.as_scalar().expect("matrix helper should return scalars"))
            .collect::<Vec<_>>();
        assert_eq!(matrix_linearized, vec![1.0, 5.0, 3.0, 7.0, 2.0, 6.0, 4.0, 8.0]);

        let cell = CellValue::with_dimensions(
            2,
            4,
            vec![2, 2, 2],
            (1..=8)
                .map(|value| Value::Scalar(value as f64))
                .collect::<Vec<_>>(),
        )
        .expect("cell dimensions should be valid");
        let cell_linearized = linearized_cell_elements(&cell)
            .expect("cell linearization should succeed")
            .into_iter()
            .map(|value| value.as_scalar().expect("cell helper should return scalars"))
            .collect::<Vec<_>>();
        assert_eq!(cell_linearized, vec![1.0, 5.0, 3.0, 7.0, 2.0, 6.0, 4.0, 8.0]);

        let distributed_cell = distributed_cell_assignment_values(&Value::Cell(cell.clone()), 8)
            .expect("distributed cell assignment should accept matching nd rhs")
            .into_iter()
            .map(|value| value.as_scalar().expect("distributed cell helper should return scalars"))
            .collect::<Vec<_>>();
        assert_eq!(distributed_cell, vec![1.0, 5.0, 3.0, 7.0, 2.0, 6.0, 4.0, 8.0]);

        let distributed_struct =
            distributed_struct_assignment_values(&Value::Matrix(matrix.clone()), 8)
                .expect("distributed struct helper should accept matching nd rhs")
                .into_iter()
                .map(|value| {
                    value
                        .as_scalar()
                        .expect("distributed struct helper should return scalars")
                })
                .collect::<Vec<_>>();
        assert_eq!(
            distributed_struct,
            vec![1.0, 5.0, 3.0, 7.0, 2.0, 6.0, 4.0, 8.0]
        );
    }

    #[test]
    fn collapsed_nd_resize_uses_folded_view_storage_order() {
        let matrix = MatrixValue::with_dimensions(
            2,
            4,
            vec![2, 2, 2],
            vec![
                Value::Scalar(1.0),
                Value::Scalar(5.0),
                Value::Scalar(2.0),
                Value::Scalar(6.0),
                Value::Scalar(3.0),
                Value::Scalar(7.0),
                Value::Scalar(4.0),
                Value::Scalar(8.0),
            ],
        )
        .expect("matrix dimensions should be valid");
        let resized = resize_matrix_to_dims(matrix, &[2, 5], Value::Scalar(0.0));
        let resized_scalars = resized
            .elements()
            .iter()
            .map(|value| value.as_scalar().expect("resized matrix helper should return scalars"))
            .collect::<Vec<_>>();
        assert_eq!(
            resized_scalars,
            vec![1.0, 5.0, 2.0, 6.0, 0.0, 3.0, 7.0, 4.0, 8.0, 0.0]
        );
    }

    #[test]
    fn cat_nd_storage_order_matches_runtime_expectations() {
        let result =
            execute_script_source("a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n");
        let Value::Matrix(matrix) = result.workspace.get("a").expect("cat output matrix") else {
            panic!("expected matrix output from cat");
        };
        let scalars = matrix
            .elements()
            .iter()
            .map(|value| value.as_scalar().expect("cat storage scalar"))
            .collect::<Vec<_>>();
        assert_eq!(scalars, vec![1.0, 5.0, 2.0, 6.0, 3.0, 7.0, 4.0, 8.0]);
    }

    #[test]
    fn nd_linearized_assignment_and_nested_cell_distribution_follow_matlab_order() {
        let source = "source = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             matrix_out = zeros(1, 8);\n\
             matrix_out(:) = source;\n\
             cell_source = cat(3, {1 2; 3 4}, {5 6; 7 8});\n\
             cell_out = {0, 0, 0, 0, 0, 0, 0, 0};\n\
             cell_out{1:8} = cell_source;\n\
             nested_out = {{0, 0}, {0, 0}};\n\
             [nested_out{:}{:}] = {31, 32; 33, 34};\n\
             clear source cell_source;\n";
        let expected =
            "workspace\n  cell_out = {1, 3, 2, 4, 5, 7, 6, 8}\n  matrix_out = [1, 3, 2, 4, 5, 7, 6, 8]\n  nested_out = {{31, 33}, {32, 34}}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn plain_multi_target_brace_assignment_does_not_flatten_nested_cells() {
        let source = "outer = {{1, 2}, {3, 4}};\n\
             outer{:} = {10, 20, 30, 40};\n";
        let interpreted = execute_script_source_result(source)
            .expect_err("plain brace assignment should not flatten nested cell contents");
        assert!(
            interpreted
                .to_string()
                .contains("linear cell assignment expects 2 rhs element(s), found 4"),
            "{interpreted}"
        );

        let bytecode = execute_script_source_bytecode_result(source)
            .expect_err("bytecode plain brace assignment should not flatten nested cell contents");
        assert!(
            bytecode
                .to_string()
                .contains("linear cell assignment expects 2 rhs element(s), found 4"),
            "{bytecode}"
        );

        let nested = execute_script_source(
            "outer = {{0, 0}, {0, 0}};\n\
             [outer{:}{:}] = {10, 20, 30, 40};\n\
             out = outer;\n\
             clear outer;\n",
        );
        assert_eq!(
            render_execution_result(&nested),
            "workspace\n  out = {{10, 20}, {30, 40}}\n"
        );

        let nested_bytecode = execute_script_source_bytecode(
            "outer = {{0, 0}, {0, 0}};\n\
             [outer{:}{:}] = {10, 20, 30, 40};\n\
             out = outer;\n\
             clear outer;\n",
        );
        assert_eq!(
            render_execution_result(&nested_bytecode),
            "workspace\n  out = {{10, 20}, {30, 40}}\n"
        );
    }

    #[test]
    fn single_output_multi_cell_content_read_requires_csl_context() {
        let source = "cells = {1, 2};\n\
             out = cells{:};\n";
        let interpreted = execute_script_source_result(source)
            .expect_err("single-output brace read should require csl context");
        assert!(
            interpreted
                .to_string()
                .contains("Expected one output from a curly brace or dot indexing expression, but there were 2 results."),
            "{interpreted}"
        );

        let bytecode = execute_script_source_bytecode_result(source)
            .expect_err("bytecode single-output brace read should require csl context");
        assert!(
            bytecode
                .to_string()
                .contains("Expected one output from a curly brace or dot indexing expression, but there were 2 results."),
            "{bytecode}"
        );

        let wrapped = execute_script_source(
            "cells = {1, 2};\n\
             out = {cells{:}};\n",
        );
        assert_eq!(render_execution_result(&wrapped), "workspace\n  cells = {1, 2}\n  out = {1, 2}\n");

        let wrapped_bytecode = execute_script_source_bytecode(
            "cells = {1, 2};\n\
             out = {cells{:}};\n",
        );
        assert_eq!(
            render_execution_result(&wrapped_bytecode),
            "workspace\n  cells = {1, 2}\n  out = {1, 2}\n"
        );
    }

    #[test]
    fn nd_assignment_with_mismatched_flat_rhs_count_still_errors() {
        let error = execute_script_source_result(
            "a = zeros(2, 2, 3);\n\
             a(1, :, 2:3) = [5 6 7];\n",
        )
        .expect_err("expected nd assignment shape error");
        assert!(
            error
                .to_string()
                .contains("matrix assignment expects rhs dimensions [1, 2, 2], found [1, 3]"),
            "{error}"
        );

        let error = execute_script_source_result(
            "cells = cat(3, {0 0; 0 0}, {0 0; 0 0}, {0 0; 0 0});\n\
             cells(1, :, 2:3) = {5 6 7};\n",
        )
        .expect_err("expected nd cell assignment shape error");
        assert!(
            error
                .to_string()
                .contains("cell assignment expects rhs dimensions [1, 2, 2], found [1, 3]"),
            "{error}"
        );
    }

    #[test]
    fn nd_logical_linear_selection_uses_matlab_linear_order() {
        let result = execute_script_source(
            "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             mask = cat(3, [true false; false true], [false true; true false]);\n\
             out = a(mask);\n\
             clear a mask;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out = [1 ; 4 ; 7 ; 6]\n"
        );
    }

    #[test]
    fn nd_logical_linear_assignment_uses_matlab_linear_order_and_preserves_dims() {
        let result = execute_script_source(
            "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             mask = cat(3, [true false; false true], [false true; true false]);\n\
             a(mask) = [10 20 30 40];\n\
             out = a;\n\
             clear a mask;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out(:,:,1) = [10, 2 ; 3, 20]\n  out(:,:,2) = [5, 40 ; 30, 8]\n"
        );
    }

    #[test]
    fn end_in_mixed_index_expressions_supports_builtin_and_nested_index_forms() {
        let source = "x = [1 2 3; 4 5 6; 7 8 9];\n\
             idx = [1 3];\n\
             cells = {1, 2, 3};\n\
             a = x(max(1, end - 1), 2);\n\
             b = x(idx(end), 1);\n\
             c = x(cells{end}, end);\n\
             x(max(1, end - 1), end - 1:end) = [88 99];\n\
             out = x;\n\
             clear idx cells x;\n";
        let expected =
            "workspace\n  a = 5\n  b = 7\n  c = 9\n  out = [1, 2, 3 ; 4, 88, 99 ; 7, 8, 9]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn numeric_zero_one_index_vectors_are_not_treated_as_logical_masks() {
        let source = "x = [1 2 3; 4 5 6; 7 8 9];\nout = x([1 0 1], :);\n";
        let interpreted =
            execute_script_source_result(source).expect_err("numeric 0/1 row selector should fail");
        assert!(
            interpreted
                .to_string()
                .contains("expected positive integer index, found 0"),
            "{interpreted}"
        );

        let bytecode =
            execute_script_source_bytecode_result(source).expect_err("bytecode numeric 0/1 row selector should fail");
        assert!(
            bytecode
                .to_string()
                .contains("expected positive integer index, found 0"),
            "{bytecode}"
        );

        let assign_source =
            "x = [1 2 3; 4 5 6; 7 8 9];\nx([1 0 1], [0 1 1]) = [10 20; 30 40];\n";
        let interpreted = execute_script_source_result(assign_source)
            .expect_err("numeric 0/1 assignment selectors should fail");
        assert!(
            interpreted
                .to_string()
                .contains("expected positive integer index, found 0"),
            "{interpreted}"
        );

        let bytecode = execute_script_source_bytecode_result(assign_source)
            .expect_err("bytecode numeric 0/1 assignment selectors should fail");
        assert!(
            bytecode
                .to_string()
                .contains("expected positive integer index, found 0"),
            "{bytecode}"
        );
    }

    #[test]
    fn selector_planner_accepts_longer_logical_masks_when_extra_entries_are_false() {
        let logical_linear = EvaluatedIndexArgument::Numeric {
            values: vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            rows: 1,
            cols: 10,
            dims: vec![1, 10],
            logical: true,
        };
        let linear_plan = plan_linear_selector(
            &logical_linear,
            2,
            4,
            &[2, 2, 2],
            "matrix",
            SelectorPlanMode::Selection,
        )
        .expect("linear logical selector plan");
        assert_eq!(linear_plan.indices, vec![1, 3, 5, 7]);
        assert_eq!(linear_plan.output_dims, vec![4, 1]);

        let logical_axis = EvaluatedIndexArgument::Numeric {
            values: vec![1.0, 0.0, 1.0, 0.0, 0.0],
            rows: 1,
            cols: 5,
            dims: vec![1, 5],
            logical: true,
        };
        let axis_plan = plan_dimension_selector(
            &logical_axis,
            3,
            "column",
            "matrix",
            SelectorPlanMode::Selection,
        )
        .expect("dimension logical selector plan");
        assert_eq!(axis_plan.indices, vec![1, 3]);
        assert_eq!(axis_plan.target_extent, 3);
    }

    #[test]
    fn selector_planner_only_grows_numeric_assignment_extents() {
        let numeric = EvaluatedIndexArgument::Numeric {
            values: vec![2.0, 5.0],
            rows: 1,
            cols: 2,
            dims: vec![1, 2],
            logical: false,
        };
        let numeric_plan = plan_dimension_selector(
            &numeric,
            3,
            "column",
            "matrix",
            SelectorPlanMode::Assignment,
        )
        .expect("numeric assignment selector plan");
        assert_eq!(numeric_plan.indices, vec![2, 5]);
        assert_eq!(numeric_plan.target_extent, 5);

        let logical = EvaluatedIndexArgument::Numeric {
            values: vec![1.0, 0.0, 1.0, 0.0, 0.0],
            rows: 1,
            cols: 5,
            dims: vec![1, 5],
            logical: true,
        };
        let logical_plan = plan_dimension_selector(
            &logical,
            3,
            "column",
            "matrix",
            SelectorPlanMode::Assignment,
        )
        .expect("logical assignment selector plan");
        assert_eq!(logical_plan.indices, vec![1, 3]);
        assert_eq!(logical_plan.target_extent, 3);
    }

    #[test]
    fn arrayfun_supports_uniform_scalar_outputs_in_interpreter_and_bytecode() {
        let source = "out = arrayfun(@(x) x + 1, [1 2 3]);\n";
        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), "workspace\n  out = [2, 3, 4]\n");

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), "workspace\n  out = [2, 3, 4]\n");
    }

    #[test]
    fn arrayfun_supports_nonuniform_cell_outputs_in_interpreter_and_bytecode() {
        let source = "out = arrayfun(@(x) [x x + 1], [1 2], 'UniformOutput', false);\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = {[1, 2], [2, 3]}\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = {[1, 2], [2, 3]}\n"
        );
    }

    #[test]
    fn cellfun_supports_uniform_scalar_outputs_in_interpreter_and_bytecode() {
        let source = "out = cellfun(@(x) x + 1, {1, 2, 3});\n";
        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), "workspace\n  out = [2, 3, 4]\n");

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), "workspace\n  out = [2, 3, 4]\n");
    }

    #[test]
    fn cellfun_supports_nonuniform_cell_outputs_in_interpreter_and_bytecode() {
        let source = "out = cellfun(@(x) [x x + 1], {1, 2}, 'UniformOutput', false);\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = {[1, 2], [2, 3]}\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = {[1, 2], [2, 3]}\n"
        );
    }

    #[test]
    fn structfun_supports_uniform_and_nonuniform_outputs_in_interpreter_and_bytecode() {
        let uniform_source = "s = struct('a', 1, 'b', 2);\nout = structfun(@(x) x + 1, s);\n";
        let interpreted = execute_script_source(uniform_source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = [2 ; 3]\n  s = struct{a=1, b=2}\n"
        );

        let bytecode = execute_script_source_bytecode(uniform_source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = [2 ; 3]\n  s = struct{a=1, b=2}\n"
        );

        let nonuniform_source =
            "s = struct('a', 1, 'b', 2);\nout = structfun(@(x) [x x + 1], s, 'UniformOutput', false);\n";
        let interpreted = execute_script_source(nonuniform_source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = {[1, 2] ; [2, 3]}\n  s = struct{a=1, b=2}\n"
        );

        let bytecode = execute_script_source_bytecode(nonuniform_source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = {[1, 2] ; [2, 3]}\n  s = struct{a=1, b=2}\n"
        );
    }

    #[test]
    fn arrayfun_supports_error_handler_outputs_in_interpreter_and_bytecode() {
        let source = "out = arrayfun(@arrayfun_error_demo, [1 2 3], 'ErrorHandler', @arrayfun_error_fallback);\n\
                      [a, b] = arrayfun(@arrayfun_pair_demo, [1 2 3], 'ErrorHandler', @arrayfun_pair_fallback);\n\
                      function y = arrayfun_error_demo(x)\n\
                      if x == 2\n\
                          error('MATC:Arrayfun', 'boom %d', x);\n\
                      end\n\
                      y = x + 10;\n\
                      end\n\
                      function y = arrayfun_error_fallback(err, x)\n\
                      y = -x - err.index;\n\
                      end\n\
                      function [first, second] = arrayfun_pair_demo(x)\n\
                      if x == 2\n\
                          error('MATC:ArrayfunPair', 'pair %d', x);\n\
                      end\n\
                      first = x;\n\
                      second = x + 100;\n\
                      end\n\
                      function [first, second] = arrayfun_pair_fallback(err, x)\n\
                      first = -x;\n\
                      second = err.index;\n\
                      end\n";
        let expected = "workspace\n  a = [1, -2, 3]\n  b = [101, 2, 103]\n  out = [11, -4, 13]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn cellfun_supports_error_handler_outputs_in_interpreter_and_bytecode() {
        let source = "out = cellfun(@cellfun_error_demo, {1, 2, 3}, 'ErrorHandler', @cellfun_error_fallback);\n\
                      [a, b] = cellfun(@cellfun_pair_demo, {1, 2, 3}, 'ErrorHandler', @cellfun_pair_fallback);\n\
                      function y = cellfun_error_demo(x)\n\
                      if x == 2\n\
                          error('MATC:Cellfun', 'boom %d', x);\n\
                      end\n\
                      y = x + 20;\n\
                      end\n\
                      function y = cellfun_error_fallback(err, x)\n\
                      y = -x - err.index;\n\
                      end\n\
                      function [first, second] = cellfun_pair_demo(x)\n\
                      if x == 2\n\
                          error('MATC:CellfunPair', 'pair %d', x);\n\
                      end\n\
                      first = x;\n\
                      second = x + 200;\n\
                      end\n\
                      function [first, second] = cellfun_pair_fallback(err, x)\n\
                      first = -x;\n\
                      second = err.index;\n\
                      end\n";
        let expected = "workspace\n  a = [1, -2, 3]\n  b = [201, 2, 203]\n  out = [21, -4, 23]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn structfun_supports_error_handler_outputs_in_interpreter_and_bytecode() {
        let source = "s = struct('a', 1, 'b', 2, 'c', 3);\n\
                      out = structfun(@structfun_error_demo, s, 'ErrorHandler', @structfun_error_fallback);\n\
                      [a, b] = structfun(@structfun_pair_demo, s, 'ErrorHandler', @structfun_pair_fallback);\n\
                      function y = structfun_error_demo(x)\n\
                      if x == 2\n\
                          error('MATC:Structfun', 'boom %d', x);\n\
                      end\n\
                      y = x + 30;\n\
                      end\n\
                      function y = structfun_error_fallback(err, x)\n\
                      y = -x - err.index;\n\
                      end\n\
                      function [first, second] = structfun_pair_demo(x)\n\
                      if x == 2\n\
                          error('MATC:StructfunPair', 'pair %d', x);\n\
                      end\n\
                      first = x;\n\
                      second = x + 300;\n\
                      end\n\
                      function [first, second] = structfun_pair_fallback(err, x)\n\
                      first = -x;\n\
                      second = err.index;\n\
                      end\n";
        let expected = "workspace\n  a = [1 ; -2 ; 3]\n  b = [301 ; 2 ; 303]\n  out = [31 ; -4 ; 33]\n  s = struct{a=1, b=2, c=3}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn linear_growth_on_nd_arrays_collapses_to_folded_column_vector() {
        let result = execute_script_source(
            "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             a(13) = 9;\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             cells{13} = 90;\n\
             out_cells = cells;\n\
             clear a cells;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out = [1 ; 3 ; 2 ; 4 ; 5 ; 7 ; 6 ; 8 ; 0 ; 0 ; 0 ; 0 ; 9]\n  out_cells = {1 ; 3 ; 2 ; 4 ; 5 ; 7 ; 6 ; 8 ; [] ; [] ; [] ; [] ; 90}\n"
        );

        let bytecode = execute_script_source_bytecode(
            "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             a(13) = 9;\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             cells{13} = 90;\n\
             out_cells = cells;\n\
             clear a cells;\n",
        );
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = [1 ; 3 ; 2 ; 4 ; 5 ; 7 ; 6 ; 8 ; 0 ; 0 ; 0 ; 0 ; 9]\n  out_cells = {1 ; 3 ; 2 ; 4 ; 5 ; 7 ; 6 ; 8 ; [] ; [] ; [] ; [] ; 90}\n"
        );
    }

    #[test]
    fn folded_view_row_deletion_on_nd_arrays_remains_2d() {
        let result = execute_script_source(
            "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             a(2,:) = [];\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             cells(2,:) = [];\n\
             out_cells = cells;\n\
             clear a cells;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out = [1, 5, 2, 6]\n  out_cells = {1, 5, 2, 6}\n"
        );

        let bytecode = execute_script_source_bytecode(
            "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             a(2,:) = [];\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             cells(2,:) = [];\n\
             out_cells = cells;\n\
             clear a cells;\n",
        );
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = [1, 5, 2, 6]\n  out_cells = {1, 5, 2, 6}\n"
        );
    }

    #[test]
    fn folded_view_column_growth_on_nd_arrays_remains_2d() {
        let source = "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             a(:, 5) = [9; 10];\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             cells(:, 5) = {90; 100};\n\
             out_cells = cells;\n\
             clear a cells;\n";
        let expected =
            "workspace\n  out = [1, 5, 2, 6, 9 ; 3, 7, 4, 8, 10]\n  out_cells = {1, 5, 2, 6, 90 ; 3, 7, 4, 8, 100}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn folded_view_row_growth_on_nd_arrays_remains_2d() {
        let source = "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             a(3, :) = [9 10 11 12];\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             cells(3, :) = {90 100 110 120};\n\
             out_cells = cells;\n\
             clear a cells;\n";
        let expected =
            "workspace\n  out = [1, 5, 2, 6 ; 3, 7, 4, 8 ; 9, 10, 11, 12]\n  out_cells = {1, 5, 2, 6 ; 3, 7, 4, 8 ; 90, 100, 110, 120}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn folded_view_corner_growth_on_nd_arrays_remains_2d() {
        let source = "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             a(3, 5) = 99;\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             cells(3, 5) = {990};\n\
             out_cells = cells;\n\
             clear a cells;\n";
        let expected =
            "workspace\n  out = [1, 5, 2, 6, 0 ; 3, 7, 4, 8, 0 ; 0, 0, 0, 0, 99]\n  out_cells = {1, 5, 2, 6, [] ; 3, 7, 4, 8, [] ; [], [], [], [], 990}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn folded_view_rectangular_growth_on_nd_arrays_remains_2d() {
        let source = "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             a(3:4, 5:6) = [9 10; 11 12];\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             cells(3:4, 5:6) = {90 100; 110 120};\n\
             out_cells = cells;\n\
             clear a cells;\n";
        let expected =
            "workspace\n  out = [1, 5, 2, 6, 0, 0 ; 3, 7, 4, 8, 0, 0 ; 0, 0, 0, 0, 9, 10 ; 0, 0, 0, 0, 11, 12]\n  out_cells = {1, 5, 2, 6, [], [] ; 3, 7, 4, 8, [], [] ; [], [], [], [], 90, 100 ; [], [], [], [], 110, 120}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn folded_view_logical_axis_masks_accept_longer_false_trailers() {
        let source = "a = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             cols = a(:, [true false true false false]);\n\
             a(:, [true false true false false]) = [10 20; 30 40];\n\
             out = a;\n\
             cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             ccols = cells(:, [true false true false false]);\n\
             cells(:, [true false true false false]) = {100 200; 300 400};\n\
             out_cells = cells;\n\
             clear a cells;\n";
        let expected =
            "workspace\n  ccols = {1, 2 ; 3, 4}\n  cols = [1, 2 ; 3, 4]\n  out(:,:,1) = [10, 20 ; 30, 40]\n  out(:,:,2) = [5, 6 ; 7, 8]\n  out_cells(:,:,1) = {100, 200 ; 300, 400}\n  out_cells(:,:,2) = {5, 6 ; 7, 8}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn nested_brace_assignment_accepts_matrix_rhs_in_matlab_linear_order() {
        let source = "outer = {{0, 0}, {0, 0}};\n\
             [outer{1:2}{:}] = [1 2; 3 4];\n\
             out = [outer{1:2}{:}];\n";
        let expected =
            "workspace\n  out = [1, 3, 2, 4]\n  outer = {{1, 3}, {2, 4}}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_colon_brace_assignment_materializes_cell_array() {
        let source = "[y{:}] = deal([10 20], [14 12]);\nout = y;\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = {[10, 20], [14, 12]}\n  y = {[10, 20], [14, 12]}\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = {[10, 20], [14, 12]}\n  y = {[10, 20], [14, 12]}\n"
        );

        let explicit = execute_script_source(
            "[z{1:2}] = deal([10 20], [14 12]);\n\
             out = z;\n",
        );
        assert_eq!(
            render_execution_result(&explicit),
            "workspace\n  out = {[10, 20], [14, 12]}\n  z = {[10, 20], [14, 12]}\n"
        );

        let explicit_bytecode = execute_script_source_bytecode(
            "[z{1:2}] = deal([10 20], [14 12]);\n\
             out = z;\n",
        );
        assert_eq!(
            render_execution_result(&explicit_bytecode),
            "workspace\n  out = {[10, 20], [14, 12]}\n  z = {[10, 20], [14, 12]}\n"
        );

        let column = execute_script_source("[c{:,1}] = deal(10, 20);\nout = c;\n");
        assert_eq!(
            render_execution_result(&column),
            "workspace\n  c = {10 ; 20}\n  out = {10 ; 20}\n"
        );

        let column_bytecode =
            execute_script_source_bytecode("[c{:,1}] = deal(10, 20);\nout = c;\n");
        assert_eq!(
            render_execution_result(&column_bytecode),
            "workspace\n  c = {10 ; 20}\n  out = {10 ; 20}\n"
        );

        let matrix_missing_root = execute_script_source(
            "[m{:}] = [1 2; 3 4];\n\
             out = [m{:}];\n",
        );
        assert_eq!(
            render_execution_result(&matrix_missing_root),
            "workspace\n  m = {1, 3, 2, 4}\n  out = [1, 3, 2, 4]\n"
        );

        let matrix_missing_root_bytecode = execute_script_source_bytecode(
            "[m{:}] = [1 2; 3 4];\n\
             out = [m{:}];\n",
        );
        assert_eq!(
            render_execution_result(&matrix_missing_root_bytecode),
            "workspace\n  m = {1, 3, 2, 4}\n  out = [1, 3, 2, 4]\n"
        );

        let explicit_rhs = execute_script_source(
            "[q{:}] = {[10 20], [14 12]};\n\
             out = q;\n",
        );
        assert_eq!(
            render_execution_result(&explicit_rhs),
            "workspace\n  out = {[10, 20], [14, 12]}\n  q = {[10, 20], [14, 12]}\n"
        );

        let explicit_rhs_bytecode = execute_script_source_bytecode(
            "[q{:}] = {[10 20], [14 12]};\n\
             out = q;\n",
        );
        assert_eq!(
            render_execution_result(&explicit_rhs_bytecode),
            "workspace\n  out = {[10, 20], [14, 12]}\n  q = {[10, 20], [14, 12]}\n"
        );

        let matrix_rhs = execute_script_source(
            "c = {0, 0; 0, 0};\n\
             [c{:}] = [1 2; 3 4];\n\
             out = [c{:}];\n",
        );
        assert_eq!(
            render_execution_result(&matrix_rhs),
            "workspace\n  c = {1, 2 ; 3, 4}\n  out = [1, 3, 2, 4]\n"
        );

        let matrix_rhs_bytecode = execute_script_source_bytecode(
            "c = {0, 0; 0, 0};\n\
             [c{:}] = [1 2; 3 4];\n\
             out = [c{:}];\n",
        );
        assert_eq!(
            render_execution_result(&matrix_rhs_bytecode),
            "workspace\n  c = {1, 2 ; 3, 4}\n  out = [1, 3, 2, 4]\n"
        );
    }

    #[test]
    fn undefined_cell_receiver_struct_field_brace_assignment_materializes_missing_root() {
        let source = "[root_cell_structs{1:2}.items{:}] = deal(91, 92, 93, 94);\n\
             out = [root_cell_structs{:}.items{:}];\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = [91, 92, 93, 94]\n  root_cell_structs = {struct{items={91, 92}}, struct{items={93, 94}}}\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = [91, 92, 93, 94]\n  root_cell_structs = {struct{items={91, 92}}, struct{items={93, 94}}}\n"
        );
    }

    #[test]
    fn undefined_nested_cell_receiver_struct_field_brace_assignment_materializes_missing_root() {
        let source = "[root_nested_cell_structs{1:2}.inner.items{:}] = deal(101, 102, 103, 104);\n\
             out = [root_nested_cell_structs{:}.inner.items{:}];\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = [101, 102, 103, 104]\n  root_nested_cell_structs = {struct{inner=struct{items={101, 102}}}, struct{inner=struct{items={103, 104}}}}\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = [101, 102, 103, 104]\n  root_nested_cell_structs = {struct{inner=struct{items={101, 102}}}, struct{inner=struct{items={103, 104}}}}\n"
        );
    }

    #[test]
    fn struct_cell_brace_assignment_accepts_matrix_rhs_in_matlab_linear_order() {
        let source = "struct_cells = struct('items', {{0, 0}, {0, 0}});\n\
             [struct_cells.items{:}] = [1 2; 3 4];\n\
             out = [struct_cells.items{:}];\n";
        let expected =
            "workspace\n  out = [1, 3, 2, 4]\n  struct_cells = [struct{items={1, 3}}, struct{items={2, 4}}]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_cell_receiver_struct_field_direct_matrix_assignment_materializes_missing_root() {
        let source = "[direct_root_cells{1:2}.score] = [171 173];\n\
             out = [direct_root_cells{:}.score];\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  direct_root_cells = {struct{score=171}, struct{score=173}}\n  out = [171, 173]\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  direct_root_cells = {struct{score=171}, struct{score=173}}\n  out = [171, 173]\n"
        );
    }

    #[test]
    fn undefined_prefixed_cell_receiver_struct_field_direct_matrix_assignment_materializes_missing_root(
    ) {
        let source = "[deep_direct.groups{1:2}.score] = [181 183];\n\
             out = [deep_direct.groups{:}.score];\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  deep_direct = struct{groups={struct{score=181}, struct{score=183}}}\n  out = [181, 183]\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  deep_direct = struct{groups={struct{score=181}, struct{score=183}}}\n  out = [181, 183]\n"
        );
    }

    #[test]
    fn undefined_cell_receiver_struct_field_matrix_rhs_uses_matlab_linear_order() {
        let source = "[root_matrix_cell_structs{1:2}.items{:}] = [191 192; 193 194];\n\
             out = [root_matrix_cell_structs{:}.items{:}];\n";
        let expected =
            "workspace\n  out = [191, 193, 192, 194]\n  root_matrix_cell_structs = {struct{items={191, 193}}, struct{items={192, 194}}}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_prefixed_cell_receiver_struct_field_matrix_rhs_uses_matlab_linear_order() {
        let source = "[deep_matrix.groups{1:2}.items{:}] = [301 302; 303 304];\n\
             out = [deep_matrix.groups{:}.items{:}];\n";
        let expected =
            "workspace\n  deep_matrix = struct{groups={struct{items={301, 303}}, struct{items={302, 304}}}}\n  out = [301, 303, 302, 304]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_colon_cell_receiver_struct_field_matrix_rhs_uses_matlab_linear_order() {
        let source = "[root_matrix_cell_structs{:}.items{:}] = [141 142; 143 144];\n\
             out = [root_matrix_cell_structs{:}.items{:}];\n";
        let expected =
            "workspace\n  out = [141, 143, 142, 144]\n  root_matrix_cell_structs = {struct{items={141, 143}}, struct{items={142, 144}}}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_struct_field_explicit_indices_accept_distinct_deal_outputs() {
        let source = "[s(1:2).field1] = deal([10 20], [14 12]);\nout = s;\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n  s = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n  s = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n"
        );
    }

    #[test]
    fn undefined_struct_field_explicit_indices_accept_direct_matrix_rhs() {
        let source = "[s(1:2).field1] = [10 12];\nout = s;\n";
        let expected =
            "workspace\n  out = [struct{field1=10}, struct{field1=12}]\n  s = [struct{field1=10}, struct{field1=12}]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_nested_struct_field_explicit_indices_accept_direct_matrix_rhs() {
        let source = "[s.inner(1:2).field1] = [21 23];\nout = s;\n";
        let expected =
            "workspace\n  out = struct{inner=[struct{field1=21}, struct{field1=23}]}\n  s = struct{inner=[struct{field1=21}, struct{field1=23}]}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_struct_field_colon_indices_accept_distinct_deal_outputs() {
        let source = "[s(:).field1] = deal(10, 12);\nout = s;\n";
        let expected =
            "workspace\n  out = [struct{field1=10}, struct{field1=12}]\n  s = [struct{field1=10}, struct{field1=12}]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_nested_struct_field_colon_indices_accept_distinct_deal_outputs() {
        let source = "[s.inner(:).field1] = deal(21, 23);\nout = s;\n";
        let expected =
            "workspace\n  out = struct{inner=[struct{field1=21}, struct{field1=23}]}\n  s = struct{inner=[struct{field1=21}, struct{field1=23}]}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_indexed_struct_brace_colon_receivers_materialize_missing_root() {
        let source = "[s(:).items{1:2}] = deal(181, 182, 183, 184);\nout = [s.items{:}];\n";
        let expected =
            "workspace\n  out = [181, 182, 183, 184]\n  s = [struct{items={181, 182}}, struct{items={183, 184}}]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_nested_indexed_struct_brace_colon_receivers_materialize_missing_root() {
        let source =
            "[s.inner(:).items{1:2}] = deal(191, 192, 193, 194);\nout = [s.inner.items{:}];\n";
        let expected =
            "workspace\n  out = [191, 192, 193, 194]\n  s = struct{inner=[struct{items={191, 192}}, struct{items={193, 194}}]}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_indexed_struct_brace_colon_receivers_accept_direct_matrix_rhs() {
        let source = "[s(:).items{1:2}] = [201 202 203 204];\nout = [s.items{:}];\n";
        let expected =
            "workspace\n  out = [201, 202, 203, 204]\n  s = [struct{items={201, 202}}, struct{items={203, 204}}]\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn undefined_struct_field_scalar_csl_materializes_struct_array() {
        let source = "[s.field1] = deal([10 20], [14 12]);\n";
        let interpreted = execute_script_source(format!("{source}out = s;\n").as_str());
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n  s = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n"
        );

        let bytecode = execute_script_source_bytecode(format!("{source}out = s;\n").as_str());
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n  s = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n"
        );

        let explicit_source = "[q.field1] = {[10 20], [14 12]};\nout = q;\n";
        let explicit_interpreted = execute_script_source(explicit_source);
        assert_eq!(
            render_execution_result(&explicit_interpreted),
            "workspace\n  out = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n  q = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n"
        );

        let explicit_bytecode = execute_script_source_bytecode(explicit_source);
        assert_eq!(
            render_execution_result(&explicit_bytecode),
            "workspace\n  out = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n  q = [struct{field1=[10, 20]}, struct{field1=[14, 12]}]\n"
        );

    }

    #[test]
    fn undefined_nested_struct_field_scalar_csl_materializes_nested_struct_array() {
        let source = "[s.inner.field1] = deal([10 20], [14 12]);\n";
        let interpreted = execute_script_source(format!("{source}out = s;\n").as_str());
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = struct{inner=[struct{field1=[10, 20]}, struct{field1=[14, 12]}]}\n  s = struct{inner=[struct{field1=[10, 20]}, struct{field1=[14, 12]}]}\n"
        );

        let bytecode = execute_script_source_bytecode(format!("{source}out = s;\n").as_str());
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = struct{inner=[struct{field1=[10, 20]}, struct{field1=[14, 12]}]}\n  s = struct{inner=[struct{field1=[10, 20]}, struct{field1=[14, 12]}]}\n"
        );
    }

    #[test]
    fn undefined_struct_field_single_output_deal_materializes_scalar_struct() {
        let source = "s.field1 = deal([10 20]);\nout = s;\n";
        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  out = struct{field1=[10, 20]}\n  s = struct{field1=[10, 20]}\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  out = struct{field1=[10, 20]}\n  s = struct{field1=[10, 20]}\n"
        );
    }

    #[test]
    fn plain_scalar_struct_field_assignment_preserves_matrix_rhs_as_scalar_field_value() {
        let source = "s.field1 = [10 20];\nout = s;\n";
        let expected = "workspace\n  out = struct{field1=[10, 20]}\n  s = struct{field1=[10, 20]}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn plain_nested_scalar_struct_field_assignment_preserves_matrix_rhs_as_scalar_field_value() {
        let source = "s.inner.field1 = [21 23];\nout = s;\n";
        let expected =
            "workspace\n  out = struct{inner=struct{field1=[21, 23]}}\n  s = struct{inner=struct{field1=[21, 23]}}\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_execution_result(&bytecode), expected);
    }

    #[test]
    fn plain_multi_element_struct_field_assignment_errors_in_interpreter_and_bytecode() {
        let source = "s = struct('field1', {0, 0});\ns(1:2).field1 = [31 32];\n";
        let interpreted = execute_script_source_result(source)
            .expect_err("plain multi-element struct field assignment should fail");
        assert!(
            interpreted
                .to_string()
                .contains("assigning to 2 elements using a simple assignment statement is not supported"),
            "{interpreted}"
        );

        let bytecode = execute_script_source_bytecode_result(source)
            .expect_err("bytecode plain multi-element struct field assignment should fail");
        assert!(
            bytecode
                .to_string()
                .contains("assigning to 2 elements using a simple assignment statement is not supported"),
            "{bytecode}"
        );
    }

    #[test]
    fn plain_missing_root_multi_element_struct_field_assignment_errors_in_interpreter_and_bytecode()
    {
        let source = "s(1:2).field1 = [31 32];\n";
        let interpreted = execute_script_source_result(source)
            .expect_err("plain missing-root multi-element struct field assignment should fail");
        assert!(
            interpreted
                .to_string()
                .contains("assigning to 2 elements using a simple assignment statement is not supported"),
            "{interpreted}"
        );

        let bytecode = execute_script_source_bytecode_result(source)
            .expect_err(
                "bytecode plain missing-root multi-element struct field assignment should fail",
            );
        assert!(
            bytecode
                .to_string()
                .contains("assigning to 2 elements using a simple assignment statement is not supported"),
            "{bytecode}"
        );
    }

    #[test]
    fn multi_struct_field_paren_subindex_assignment_errors_in_interpreter_and_bytecode() {
        let source = "s(1).test = [1 2; 3 4];\n\
             s(2).test = [5 6; 7 8];\n\
             s(1:2).test(1,1) = [9 10];\n";
        let interpreted = execute_script_source_result(source)
            .expect_err("multi-struct field paren subindex assignment should fail");
        assert!(
            interpreted
                .to_string()
                .contains("indexing into part of field `test` for multiple struct-array elements is not supported"),
            "{interpreted}"
        );

        let bytecode = execute_script_source_bytecode_result(source)
            .expect_err("bytecode multi-struct field paren subindex assignment should fail");
        assert!(
            bytecode
                .to_string()
                .contains("indexing into part of field `test` for multiple struct-array elements is not supported"),
            "{bytecode}"
        );
    }

    #[test]
    fn single_output_struct_array_field_access_requires_concatenation() {
        let source = "s = struct('name', {'alpha', 'beta'});\nout = s.name;\n";
        let interpreted =
            execute_script_source_result(source).expect_err("single-output struct array field read should fail");
        assert!(
            interpreted
                .to_string()
                .contains("Expected one output from a curly brace or dot indexing expression, but there were 2 results."),
            "{interpreted}"
        );

        let bytecode = execute_script_source_bytecode_result(source)
            .expect_err("bytecode single-output struct array field read should fail");
        assert!(
            bytecode
                .to_string()
                .contains("Expected one output from a curly brace or dot indexing expression, but there were 2 results."),
            "{bytecode}"
        );
    }


    #[test]
    fn linear_matrix_deletion_flattens_nonvector_inputs_in_matlab_order() {
        let result = execute_script_source(
            "a = [1 2; 3 4];\n\
             a([1 3]) = [];\n\
             out = a;\n\
             b = cat(3, [1 2; 3 4], [5 6; 7 8]);\n\
             b([1 3 7]) = [];\n\
             out_nd = b;\n\
             clear a b;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out = [3 ; 4]\n  out_nd = [3 ; 4 ; 5 ; 7 ; 8]\n"
        );
    }

    #[test]
    fn linear_cell_deletion_flattens_nonvector_inputs_in_matlab_order() {
        let result = execute_script_source(
            "c = {1, 2; 3, 4};\n\
             c([1 3]) = [];\n\
             out = c;\n\
             d = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});\n\
             d([1 3 7]) = [];\n\
             out_nd = d;\n\
             clear c d;\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  out = {3 ; 4}\n  out_nd = {3 ; 4 ; 5 ; 7 ; 8}\n"
        );
    }

    #[test]
    fn clear_removes_named_variables_from_workspace() {
        let result = execute_script_source("x = 1;\ny = 2;\nclear('x');\n");
        assert_eq!(render_execution_result(&result), "workspace\n  y = 2\n");
    }

    #[test]
    fn clear_without_arguments_removes_all_workspace_variables() {
        let result = execute_script_source("x = 1;\ny = 2;\nclear();\n");
        assert_eq!(render_execution_result(&result), "workspace\n");
    }

    #[test]
    fn clearvars_except_preserves_requested_variables() {
        let result = execute_script_source("x = 1;\ny = 2;\nz = 3;\nclearvars('-except', 'y');\n");
        assert_eq!(render_execution_result(&result), "workspace\n  y = 2\n");
    }

    #[test]
    fn clear_regexp_removes_matching_variables() {
        let result = execute_script_source(
            "alpha = 1;\nalphabet = 2;\nbeta = 3;\nclear('-regexp', '^alpha');\n",
        );
        assert_eq!(render_execution_result(&result), "workspace\n  beta = 3\n");
    }

    #[test]
    fn clearvars_regexp_except_preserves_matching_variables() {
        let result = execute_script_source(
            "alpha = 1;\nalphabet = 2;\nbeta = 3;\nclearvars('-regexp', 'a$', '-except', '-regexp', '^be');\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  alphabet = 2\n  beta = 3\n"
        );
    }

    #[test]
    fn clear_global_clears_shared_global_state() {
        let result = execute_script_source(
            "global g;\ng = 1;\nclear global;\nglobal g;\nisempty_after = isempty(g);\n",
        );
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  g = []\n  isempty_after = true\n"
        );
    }

    #[test]
    fn clear_all_removes_workspace_variables() {
        let result = execute_script_source("global g;\ng = 1;\nx = 2;\nclear all;\n");
        assert_eq!(render_execution_result(&result), "workspace\n");
    }

    #[test]
    fn format_bank_command_form_affects_workspace_rendering_in_interpreter_and_bytecode() {
        let source = "format bank\nx = pi;\ny = [1 / 3, 2];\n";

        let interpreted = execute_script_source(source);
        assert_eq!(
            render_execution_result(&interpreted),
            "workspace\n  x = 3.14\n  y = [0.33, 2.00]\n"
        );

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(
            render_execution_result(&bytecode),
            "workspace\n  x = 3.14\n  y = [0.33, 2.00]\n"
        );
    }

    #[test]
    fn format_function_call_affects_disp_rendering_in_interpreter_and_bytecode() {
        let source = "format('short g');\ndisp(pi)\n";

        let interpreted = execute_script_source(source);
        assert_eq!(render_matlab_execution_result(&interpreted), "3.14159\n");

        let bytecode = execute_script_source_bytecode(source);
        assert_eq!(render_matlab_execution_result(&bytecode), "3.14159\n");
    }

    #[test]
    fn format_compact_and_loose_toggle_named_output_spacing() {
        let compact = execute_script_source("format compact\na = 1\n1 + 2\n");
        assert_eq!(render_matlab_execution_result(&compact), "a = 1\nans = 3\n");

        let loose = execute_script_source("format compact\na = 1\nformat loose\n1 + 2\n");
        assert_eq!(render_matlab_execution_result(&loose), "a = 1\n\nans = 3\n");
    }

    #[test]
    fn format_default_restores_default_numeric_and_spacing() {
        let interpreted_bank = execute_script_source("format bank\nx = pi;\n");
        assert_eq!(
            render_execution_result(&interpreted_bank),
            "workspace\n  x = 3.14\n"
        );
        let bytecode_bank = execute_script_source_bytecode("format bank\nx = pi;\n");
        assert_eq!(
            render_execution_result(&bytecode_bank),
            "workspace\n  x = 3.14\n"
        );

        let interpreted_default = execute_script_source("format bank\nformat default\nx = pi;\n");
        assert_eq!(
            render_execution_result(&interpreted_default),
            "workspace\n  x = 3.141592653589793\n"
        );
        let bytecode_default =
            execute_script_source_bytecode("format bank\nformat default\nx = pi;\n");
        assert_eq!(
            render_execution_result(&bytecode_default),
            "workspace\n  x = 3.141592653589793\n"
        );
    }

    #[test]
    fn format_shortg_and_longg_aliases_work_in_interpreter_and_bytecode() {
        let short_source = "format shortG\nx = pi;\n";
        let long_source = "format longG\nx = pi;\n";

        let interpreted_short = execute_script_source(short_source);
        assert_eq!(
            render_execution_result(&interpreted_short),
            "workspace\n  x = 3.14159\n"
        );
        let bytecode_short = execute_script_source_bytecode(short_source);
        assert_eq!(
            render_execution_result(&bytecode_short),
            "workspace\n  x = 3.14159\n"
        );

        let interpreted_long = execute_script_source(long_source);
        assert_eq!(
            render_execution_result(&interpreted_long),
            "workspace\n  x = 3.141592653589793\n"
        );
        let bytecode_long = execute_script_source_bytecode(long_source);
        assert_eq!(
            render_execution_result(&bytecode_long),
            "workspace\n  x = 3.141592653589793\n"
        );
    }

    #[test]
    fn format_shorte_and_longe_aliases_work_in_interpreter_and_bytecode() {
        let short_source = "format shortE\nx = pi;\n";
        let long_source = "format long e\nx = pi;\n";

        let interpreted_short = execute_script_source(short_source);
        assert_eq!(
            render_execution_result(&interpreted_short),
            "workspace\n  x = 3.1416e+00\n"
        );
        let bytecode_short = execute_script_source_bytecode(short_source);
        assert_eq!(
            render_execution_result(&bytecode_short),
            "workspace\n  x = 3.1416e+00\n"
        );

        let interpreted_long = execute_script_source(long_source);
        assert_eq!(
            render_execution_result(&interpreted_long),
            "workspace\n  x = 3.141592653589793e+00\n"
        );
        let bytecode_long = execute_script_source_bytecode(long_source);
        assert_eq!(
            render_execution_result(&bytecode_long),
            "workspace\n  x = 3.141592653589793e+00\n"
        );
    }

    #[test]
    fn clear_functions_resets_persistent_state() {
        let temp_dir = unique_temp_script_dir("clear-functions");
        let main_path = temp_dir.join("main.m");
        let helper_path = temp_dir.join("counter.m");
        fs::write(
            &main_path,
            "a = counter();\n\
             b = counter();\n\
             clear functions;\n\
             c = counter();\n",
        )
        .expect("write main script");
        fs::write(
            &helper_path,
            "function y = counter()\n\
             persistent n;\n\
             if isempty(n)\n\
             n = 0;\n\
             end\n\
             n = n + 1;\n\
             y = n;\n\
             end\n",
        )
        .expect("write counter helper");

        let source = fs::read_to_string(&main_path).expect("read main");
        let parsed = parse_source(&source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(&main_path),
        );
        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let hir = lower_to_hir(&unit, &analysis);
        let result = execute_script(&hir).expect("execute script");
        assert_eq!(
            render_execution_result(&result),
            "workspace\n  a = 1\n  b = 2\n  c = 1\n"
        );
        let _ = fs::remove_file(main_path);
        let _ = fs::remove_file(helper_path);
        let _ = fs::remove_dir(temp_dir);
    }

    #[test]
    fn matlab_render_omits_ans_for_graphics_side_effect_calls() {
        let result = execute_script_source("figure(91)\nplot([0, 1], [0, 1])\n1 + 2\n");
        assert_eq!(render_matlab_execution_result(&result), "ans = 3\n");
    }

    #[test]
    fn matlab_render_still_shows_graphics_query_outputs() {
        let result = execute_script_source("figure(92);\nplot([0, 2], [0, 1]);\nxlim\n");
        assert_eq!(render_matlab_execution_result(&result), "ans = [0, 2]\n");
    }

    #[test]
    fn figure_backend_viewer_is_standalone_and_interactive() {
        let html = render_figure_backend_index(
            "MATC Figure Viewer",
            &[RenderedFigure {
                handle: 7,
                title: "Figure 7: Session Figure".to_string(),
                visible: true,
                position: [80.0, 80.0, 1360.0, 960.0],
                window_style: "normal".to_string(),
                svg: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><text x=\"1\" y=\"5\">plot</text></svg>".to_string(),
            }],
            42,
        );
        assert!(html.contains("Figure 7: Session Figure"), "{html}");
        assert!(html.contains("Pan"), "{html}");
        assert!(html.contains("Rotate"), "{html}");
        assert!(html.contains("Brush"), "{html}");
        assert!(html.contains("Clear Brush"), "{html}");
        assert!(html.contains("Data Tips"), "{html}");
        assert!(html.contains("Clear Tips"), "{html}");
        assert!(html.contains("Zoom In"), "{html}");
        assert!(html.contains("Reset View"), "{html}");
        assert!(html.contains("Save SVG"), "{html}");
        assert!(html.contains("matc-figure-canvas"), "{html}");
        assert!(html.contains("matc-status-readout"), "{html}");
        assert!(html.contains("<svg"), "{html}");
        assert!(!html.contains("class=\"matc-native-host\""), "{html}");
        assert!(html.contains("<div class=\"matc-toolbar\">"), "{html}");
        assert!(html.contains("matcToggleActiveFigureBrushMode"), "{html}");
        assert!(html.contains("matcClearActiveFigureBrush"), "{html}");
        assert!(html.contains("matcToggleActiveFigureDataTips"), "{html}");
        assert!(html.contains("matcClearActiveFigureDataTips"), "{html}");
        assert!(html.contains("matcToggleActiveFigureRotateMode"), "{html}");
        assert!(html.contains("matcAxes2DGroups"), "{html}");
        assert!(html.contains("matcReproject2DGroup"), "{html}");
        assert!(html.contains("panelHas2DAxes"), "{html}");
        assert!(html.contains("matcSet2DLimitReadout"), "{html}");
        assert!(html.contains("data-window-style=\"normal\""), "{html}");
        assert!(
            !html.contains("session.json"),
            "standalone viewer should not depend on a local HTTP poller: {html}"
        );
    }

    #[test]
    fn native_host_viewer_preserves_svg_markup() {
        let html = render_native_figure_host_index(
            "MATC Figure Viewer",
            &[RenderedFigure {
                handle: 3,
                title: "Docked Plot".to_string(),
                visible: true,
                position: [100.0, 120.0, 900.0, 650.0],
                window_style: "docked".to_string(),
                svg: "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"900\" height=\"650\" viewBox=\"0 0 900 650\"><polyline points=\"0,0 5,5\"/></svg>".to_string(),
            }],
            7,
        );
        assert!(html.contains("X-UA-Compatible"), "{html}");
        assert!(html.contains("Docked Plot"), "{html}");
        assert!(html.contains("<svg"), "{html}");
        assert!(
            html.contains("class=\"matc-native-host\" data-matc-surface=\"native-host\""),
            "{html}"
        );
        assert!(html.contains("main{display:block;padding:18px;}"), "{html}");
        assert!(
            html.contains(".matc-figure{display:block;width:100%;box-sizing:border-box;"),
            "{html}"
        );
        assert!(
            html.contains(".matc-figure-canvas{display:block;width:100%;box-sizing:border-box;"),
            "{html}"
        );
        assert!(
            html.contains(
                ".matc-figure-canvas svg{display:block;width:100%;height:100%;max-width:none;}"
            ),
            "{html}"
        );
        assert!(html.contains("Rotate"), "{html}");
        assert!(html.contains("Brush"), "{html}");
        assert!(html.contains("Data Tips"), "{html}");
        assert!(
            html.contains(".matc-native-host .matc-toolbar{display:flex;}"),
            "{html}"
        );
        assert!(html.contains("matcZoomInActiveFigure"), "{html}");
        assert!(html.contains("matcSaveActiveFigure"), "{html}");
        assert!(html.contains("matcToggleActiveFigurePanMode"), "{html}");
        assert!(html.contains("matcToggleActiveFigureRotateMode"), "{html}");
        assert!(html.contains("matcToggleActiveFigureBrushMode"), "{html}");
        assert!(html.contains("matcClearActiveFigureBrush"), "{html}");
        assert!(html.contains("matcToggleActiveFigureDataTips"), "{html}");
        assert!(html.contains("matcClearActiveFigureDataTips"), "{html}");
        assert!(html.contains("matcAxes2DGroups"), "{html}");
        assert!(html.contains("matcReproject2DGroup"), "{html}");
        assert!(html.contains("panelHas2DAxes"), "{html}");
        assert!(html.contains("matcSet2DLimitReadout"), "{html}");
        assert!(
            html.contains("Pan mode | drag to move axis limits"),
            "{html}"
        );
        assert!(html.contains("matc-host-state"), "{html}");
        assert!(html.contains("window.ipc.postMessage"), "{html}");
    }

    #[test]
    fn session_manifest_includes_window_metadata_and_hidden_figures() {
        let json = session_manifest_json(
            "MATC Figure Viewer",
            &[
                RenderedFigure {
                    handle: 5,
                    title: "Figure 5: Visible".to_string(),
                    visible: true,
                    position: [80.0, 90.0, 640.0, 480.0],
                    window_style: "normal".to_string(),
                    svg: "<svg></svg>".to_string(),
                },
                RenderedFigure {
                    handle: 6,
                    title: "Hidden Dock".to_string(),
                    visible: false,
                    position: [100.0, 120.0, 800.0, 600.0],
                    window_style: "docked".to_string(),
                    svg: "<svg></svg>".to_string(),
                },
            ],
            99,
        );
        assert!(json.contains("\"visible_figure_count\":1"), "{json}");
        assert!(json.contains("\"handle\":5"), "{json}");
        assert!(json.contains("\"handle\":6"), "{json}");
        assert!(json.contains("\"window_style\":\"normal\""), "{json}");
        assert!(json.contains("\"window_style\":\"docked\""), "{json}");
        assert!(json.contains("\"visible\":false"), "{json}");
        assert!(json.contains("\"page\":\"figure-5.html\""), "{json}");
        assert!(
            json.contains("\"browser_page\":\"figure-5.html\""),
            "{json}"
        );
        assert!(
            json.contains("\"host_page\":\"host-figure-6.html\""),
            "{json}"
        );
        assert!(json.contains("\"svg\":\"figure-6.svg\""), "{json}");
    }

    #[test]
    fn host_position_event_runs_resize_callback_during_live_execution() {
        let source = "f = figure(1);\nset(f, 'ResizeFcn', @mark_resize);\nfunction mark_resize()\nset(gcf(), 'Name', 'Host Resize Ran');\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(PathBuf::from("host_resize_live_test.m")),
        );
        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let hir = lower_to_hir(&unit, &analysis);
        let mut interpreter = Interpreter::new(&hir);
        let mut frame = Frame::new(interpreter.module_functions.clone());
        if let Some(ans) = &interpreter.module.implicit_ans {
            frame.declare_binding(ans).expect("declare ans");
        }
        let statements = interpreter
            .module
            .items
            .iter()
            .filter_map(|item| match item {
                HirItem::Statement(statement) => Some(statement),
                HirItem::Function(_) => None,
            })
            .collect::<Vec<_>>();
        interpreter
            .execute_statement(&mut frame, statements[0])
            .expect("execute figure");
        interpreter
            .execute_statement(&mut frame, statements[1])
            .expect("execute set");

        let temp_dir = unique_temp_script_dir("host-resize-live");
        let backend = FigureBackendState {
            session_dir: temp_dir.clone(),
            title: "MATC Figure Viewer".to_string(),
            known_handles: BTreeSet::new(),
        };
        fs::write(temp_dir.join("event-position-1.txt"), "11,22,333,444")
            .expect("write position event");
        {
            let mut state = interpreter.shared_state.borrow_mut();
            consume_figure_backend_events(&mut state, &backend);
        }
        interpreter
            .drain_pending_host_figure_events(&frame)
            .expect("drain host resize events");
        let name_value = {
            let mut state = interpreter.shared_state.borrow_mut();
            invoke_graphics_builtin_outputs(
                &mut state.graphics,
                "get",
                &[Value::Scalar(1.0), Value::CharArray("Name".to_string())],
                1,
            )
            .expect("graphics get available")
            .expect("graphics get result")
            .into_iter()
            .next()
            .expect("name output")
        };
        assert_eq!(name_value, Value::CharArray("Host Resize Ran".to_string()));
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn host_close_event_runs_close_request_callback_during_live_execution() {
        let source = "f = figure(1);\nset(f, 'CloseRequestFcn', @make_callback_figure);\nfunction make_callback_figure()\nfigure(2, 'Name', 'Host Close Ran');\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let analysis = analyze_compilation_unit_with_context(
            &unit,
            &ResolverContext::from_source_file(PathBuf::from("host_close_live_test.m")),
        );
        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let hir = lower_to_hir(&unit, &analysis);
        let mut interpreter = Interpreter::new(&hir);
        let mut frame = Frame::new(interpreter.module_functions.clone());
        if let Some(ans) = &interpreter.module.implicit_ans {
            frame.declare_binding(ans).expect("declare ans");
        }
        let statements = interpreter
            .module
            .items
            .iter()
            .filter_map(|item| match item {
                HirItem::Statement(statement) => Some(statement),
                HirItem::Function(_) => None,
            })
            .collect::<Vec<_>>();
        interpreter
            .execute_statement(&mut frame, statements[0])
            .expect("execute figure");
        interpreter
            .execute_statement(&mut frame, statements[1])
            .expect("execute set");

        let temp_dir = unique_temp_script_dir("host-close-live");
        let backend = FigureBackendState {
            session_dir: temp_dir.clone(),
            title: "MATC Figure Viewer".to_string(),
            known_handles: BTreeSet::new(),
        };
        fs::write(temp_dir.join("event-close-1.txt"), "1").expect("write close event");
        {
            let mut state = interpreter.shared_state.borrow_mut();
            consume_figure_backend_events(&mut state, &backend);
        }
        interpreter
            .drain_pending_host_figure_events(&frame)
            .expect("drain host close events");

        let (figure_one_alive, figure_two_name) = {
            let mut state = interpreter.shared_state.borrow_mut();
            let alive_one = invoke_graphics_builtin_outputs(
                &mut state.graphics,
                "isgraphics",
                &[Value::Scalar(1.0), Value::CharArray("figure".to_string())],
                1,
            )
            .expect("isgraphics available")
            .expect("isgraphics result")
            .into_iter()
            .next()
            .expect("alive output");
            let name_two = invoke_graphics_builtin_outputs(
                &mut state.graphics,
                "get",
                &[Value::Scalar(2.0), Value::CharArray("Name".to_string())],
                1,
            )
            .expect("graphics get available")
            .expect("graphics get result")
            .into_iter()
            .next()
            .expect("name output");
            (alive_one, name_two)
        };
        assert_eq!(figure_one_alive, logical_value(true));
        assert_eq!(
            figure_two_name,
            Value::CharArray("Host Close Ran".to_string())
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn classdef_value_objects_support_defaults_methods_and_copy_semantics() {
        let temp_dir = unique_temp_script_dir("classdef-value");
        let class_path = temp_dir.join("Point.m");
        let main_path = temp_dir.join("main.m");
        fs::write(
            &class_path,
            "classdef Point\n\
             properties\n\
             x = 1;\n\
             y = 2;\n\
             end\n\
             methods\n\
             function obj = Point(x, y)\n\
             obj.x = x;\n\
             obj.y = y;\n\
             end\n\
             function total = total(obj)\n\
             total = obj.x + obj.y;\n\
             end\n\
             function obj = setX(obj, value)\n\
             obj.x = value;\n\
             end\n\
             end\n\
             end\n",
        )
        .expect("write value class");
        fs::write(
            &main_path,
            "p = Point(3, 4);\n\
             q = p;\n\
             p = p.setX(10);\n\
             sum_value = p.total();\n\
             out = [p.x q.x sum_value];\n\
             kind = class(p);\n\
             ok = isa(p, 'Point');\n",
        )
        .expect("write value script");

        let expected =
            "workspace\n  kind = 'Point'\n  ok = true\n  out = [10, 3, 14]\n  p = Point with properties {x=10, y=4}\n  q = Point with properties {x=3, y=4}\n  sum_value = 14\n";
        let interpreted = execute_path(&main_path, &[]);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_path_bytecode(&main_path, &[]);
        assert_eq!(render_execution_result(&bytecode), expected);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn classdef_handle_objects_support_aliasing_and_external_dot_methods() {
        let temp_dir = unique_temp_script_dir("classdef-handle");
        let class_dir = temp_dir.join("@Counter");
        fs::create_dir_all(&class_dir).expect("create class dir");
        let class_path = class_dir.join("Counter.m");
        let method_path = class_dir.join("increment.m");
        let main_path = temp_dir.join("main.m");
        fs::write(
            &class_path,
            "classdef Counter < handle\n\
             properties\n\
             value = 0;\n\
             end\n\
             methods\n\
             function obj = Counter(value)\n\
             obj.value = value;\n\
             end\n\
             end\n\
             end\n",
        )
        .expect("write handle class");
        fs::write(
            &method_path,
            "function obj = increment(obj, delta)\n\
             obj.value = obj.value + delta;\n\
             end\n",
        )
        .expect("write method file");
        fs::write(
            &main_path,
            "c = Counter(5);\n\
             d = c;\n\
             c.increment(2);\n\
             out = [c.value d.value];\n\
             ok = isa(c, 'Counter');\n\
             is_handle = isa(c, 'handle');\n",
        )
        .expect("write handle script");

        let expected =
            "workspace\n  c = Counter with properties {value=7}\n  d = Counter with properties {value=7}\n  is_handle = true\n  ok = true\n  out = [7, 7]\n";
        let interpreted = execute_path(&main_path, &[]);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_path_bytecode(&main_path, &[]);
        assert_eq!(render_execution_result(&bytecode), expected);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn classdef_objects_roundtrip_through_save_and_load() {
        let temp_dir = unique_temp_script_dir("classdef-save-load");
        let class_path = temp_dir.join("Pair.m");
        let main_path = temp_dir.join("main.m");
        let mat_path = unique_temp_mat_path("classdef-pair");
        let matlab_path = mat_path.to_string_lossy().replace('\\', "/");
        fs::write(
            &class_path,
            "classdef Pair\n\
             properties\n\
             left = 1;\n\
             right = 2;\n\
             end\n\
             end\n",
        )
        .expect("write pair class");
        fs::write(
            &main_path,
            format!(
                "p = Pair();\n\
                 save('{matlab_path}', 'p');\n\
                 clear('p');\n\
                 s = load('{matlab_path}');\n\
                 kind = class(s.p);\n\
                 ok = isa(s.p, 'Pair');\n\
                 out = [s.p.left s.p.right];\n"
            ),
        )
        .expect("write pair script");

        let expected =
            "workspace\n  kind = 'Pair'\n  ok = true\n  out = [1, 2]\n  s = struct{p=Pair with properties {left=1, right=2}}\n";
        let interpreted = execute_path(&main_path, &[]);
        assert_eq!(render_execution_result(&interpreted), expected);

        let bytecode = execute_path_bytecode(&main_path, &[]);
        assert_eq!(render_execution_result(&bytecode), expected);

        let _ = fs::remove_file(mat_path);
        let _ = fs::remove_dir_all(temp_dir);
    }

    fn unique_temp_mat_path(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("matlab-{label}-{}-{stamp}.mat", std::process::id()))
    }

    fn unique_temp_script_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("matlab-{label}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp script dir");
        path
    }
}

pub fn summary() -> &'static str {
    "Owns interpreter, bytecode execution, and future JIT/AOT execution orchestration."
}

