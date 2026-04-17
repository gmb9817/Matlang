//! Interop crate for workspace snapshots, MAT-file, FFI, and MEX-compat boundaries.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    error::Error,
    fmt,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    str::Chars,
};

use matlab_runtime::{
    CellValue, ComplexValue, FunctionHandleTarget, FunctionHandleValue, MatrixValue,
    ObjectClassMetadata, ObjectInstance, ObjectMethodTarget, ObjectStorage, ObjectStorageKind,
    ObjectValue, RuntimeError, StructValue, Value, Workspace, observe_handle_object_id,
};

pub const CRATE_NAME: &str = "matlab-interop";
pub const WORKSPACE_SNAPSHOT_MAGIC: &str = "MATC-WORKSPACE";
pub const WORKSPACE_SNAPSHOT_VERSION: &str = "2";

#[derive(Default)]
struct HandleAliasEncodeContext {
}

impl HandleAliasEncodeContext {
    fn handle_id_for_object(&mut self, object: &ObjectValue) -> Option<String> {
        object.handle_id().map(|id| format!("h{id}"))
    }
}

#[derive(Default)]
struct HandleAliasDecodeContext {
    handles: HashMap<String, Rc<RefCell<ObjectInstance>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteropError {
    Io(String),
    Parse(String),
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshotBundleModule {
    pub module_id: String,
    pub source_path: String,
    pub encoded_module: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceSnapshotData {
    pub workspace: Workspace,
    pub bundle_modules: Vec<WorkspaceSnapshotBundleModule>,
}

impl fmt::Display for InteropError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) | Self::Parse(message) | Self::Unsupported(message) => {
                f.write_str(message)
            }
        }
    }
}

impl Error for InteropError {}

pub fn summary() -> &'static str {
    "Owns workspace snapshots, MAT-file support, FFI boundaries, and future MEX-compat infrastructure."
}

pub fn encode_workspace_snapshot(workspace: &Workspace) -> Result<String, InteropError> {
    encode_workspace_snapshot_with_modules(workspace, &[])
}

pub fn encode_workspace_snapshot_with_modules(
    workspace: &Workspace,
    bundle_modules: &[WorkspaceSnapshotBundleModule],
) -> Result<String, InteropError> {
    let mut context = HandleAliasEncodeContext::default();
    let mut out = String::new();
    out.push_str(WORKSPACE_SNAPSHOT_MAGIC);
    out.push('\t');
    out.push_str(WORKSPACE_SNAPSHOT_VERSION);
    out.push('\n');

    for module in bundle_modules {
        out.push_str("BUNDLE\t");
        out.push_str(&encode_string(&module.module_id));
        out.push('\t');
        out.push_str(&encode_string(&module.source_path));
        out.push('\t');
        out.push_str(&encode_string(&module.encoded_module));
        out.push('\n');
    }

    for (name, value) in workspace {
        out.push_str("VAR\t");
        out.push_str(&encode_string(name));
        out.push('\t');
        out.push_str(&encode_value_with_context(value, &mut context)?);
        out.push('\n');
    }

    Ok(out)
}

pub fn decode_workspace_snapshot(source: &str) -> Result<Workspace, InteropError> {
    Ok(decode_workspace_snapshot_with_modules(source)?.workspace)
}

pub fn decode_workspace_snapshot_with_modules(
    source: &str,
) -> Result<WorkspaceSnapshotData, InteropError> {
    let mut context = HandleAliasDecodeContext::default();
    let mut lines = source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());

    let Some((header_line, header)) = lines.next() else {
        return Err(InteropError::Parse(
            "workspace snapshot is empty".to_string(),
        ));
    };
    let header_fields = header.split('\t').collect::<Vec<_>>();
    if header_fields.len() != 2
        || header_fields[0] != WORKSPACE_SNAPSHOT_MAGIC
        || header_fields[1] != WORKSPACE_SNAPSHOT_VERSION
    {
        return Err(InteropError::Parse(format!(
            "line {}: expected snapshot header `{}` version `{}`",
            header_line + 1,
            WORKSPACE_SNAPSHOT_MAGIC,
            WORKSPACE_SNAPSHOT_VERSION
        )));
    }

    let mut workspace = Workspace::new();
    let mut bundle_modules = Vec::new();
    for (line_index, line) in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.first().copied() {
            Some("VAR") => {
                if fields.len() != 3 {
                    return Err(InteropError::Parse(format!(
                        "line {}: expected `VAR <name> <value>` record",
                        line_index + 1
                    )));
                }
                let name = decode_string(fields[1], line_index + 1)?;
                let value = decode_value_with_context(fields[2], line_index + 1, &mut context)?;
                workspace.insert(name, value);
            }
            Some("BUNDLE") => {
                if fields.len() != 4 {
                    return Err(InteropError::Parse(format!(
                        "line {}: expected `BUNDLE <id> <path> <module>` record",
                        line_index + 1
                    )));
                }
                let module_id = decode_string(fields[1], line_index + 1)?;
                let source_path = decode_string(fields[2], line_index + 1)?;
                let encoded_module = decode_string(fields[3], line_index + 1)?;
                bundle_modules.push(WorkspaceSnapshotBundleModule {
                    module_id,
                    source_path,
                    encoded_module,
                });
            }
            _ => {
                return Err(InteropError::Parse(format!(
                    "line {}: expected `VAR <name> <value>` or `BUNDLE <id> <path> <module>` record",
                    line_index + 1
                )))
            }
        }
    }

    Ok(WorkspaceSnapshotData {
        workspace,
        bundle_modules,
    })
}

pub fn write_workspace_snapshot(path: &Path, workspace: &Workspace) -> Result<(), InteropError> {
    let encoded = encode_workspace_snapshot(workspace)?;
    fs::write(path, encoded).map_err(|error| {
        InteropError::Io(format!(
            "failed to write workspace snapshot `{}`: {error}",
            path.display()
        ))
    })
}

pub fn write_workspace_snapshot_with_modules(
    path: &Path,
    workspace: &Workspace,
    bundle_modules: &[WorkspaceSnapshotBundleModule],
) -> Result<(), InteropError> {
    let encoded = encode_workspace_snapshot_with_modules(workspace, bundle_modules)?;
    fs::write(path, encoded).map_err(|error| {
        InteropError::Io(format!(
            "failed to write workspace snapshot `{}`: {error}",
            path.display()
        ))
    })
}

pub fn read_workspace_snapshot(path: &Path) -> Result<Workspace, InteropError> {
    Ok(read_workspace_snapshot_with_modules(path)?.workspace)
}

pub fn read_workspace_snapshot_with_modules(path: &Path) -> Result<WorkspaceSnapshotData, InteropError> {
    let source = fs::read_to_string(path).map_err(|error| {
        InteropError::Io(format!(
            "failed to read workspace snapshot `{}`: {error}",
            path.display()
        ))
    })?;
    decode_workspace_snapshot_with_modules(&source)
}

const MAT5_HEADER_TEXT_BYTES: usize = 116;
const MAT5_HEADER_TOTAL_BYTES: usize = 128;

const MI_INT8: u32 = 1;
const MI_UINT8: u32 = 2;
const MI_UINT16: u32 = 4;
const MI_INT32: u32 = 5;
const MI_UINT32: u32 = 6;
const MI_DOUBLE: u32 = 9;
const MI_MATRIX: u32 = 14;

const MX_CELL_CLASS: u32 = 1;
const MX_STRUCT_CLASS: u32 = 2;
const MX_OBJECT_CLASS: u32 = 3;
const MX_CHAR_CLASS: u32 = 4;
const MX_DOUBLE_CLASS: u32 = 6;
const MX_UINT8_CLASS: u32 = 9;

const ARRAY_FLAG_LOGICAL: u32 = 1 << 9;
const ARRAY_FLAG_COMPLEX: u32 = 1 << 11;

pub fn encode_mat_file(workspace: &Workspace) -> Result<Vec<u8>, InteropError> {
    let mut context = HandleAliasEncodeContext::default();
    let mut out = Vec::new();
    write_mat5_header(&mut out);
    for (name, value) in workspace {
        write_mat_matrix_element_with_context(&mut out, name, value, &mut context)?;
    }
    Ok(out)
}

pub fn decode_mat_file(bytes: &[u8]) -> Result<Workspace, InteropError> {
    let mut context = HandleAliasDecodeContext::default();
    let mut reader = ByteReader::new(bytes);
    reader.expect_len(MAT5_HEADER_TOTAL_BYTES)?;
    let header = reader.read_bytes(MAT5_HEADER_TOTAL_BYTES)?;
    if !header[..MAT5_HEADER_TEXT_BYTES].starts_with(b"MATLAB 5.0 MAT-file") {
        return Err(InteropError::Parse(
            "MAT-file header is not a supported MATLAB 5.0 MAT-file".to_string(),
        ));
    }
    if &header[126..128] != b"IM" {
        return Err(InteropError::Unsupported(
            "only little-endian MATLAB 5 MAT-files are currently supported".to_string(),
        ));
    }

    let mut workspace = Workspace::new();
    while !reader.is_empty() {
        reader.skip_zero_padding();
        if reader.is_empty() {
            break;
        }
        let (data_type, payload) = reader.read_tagged_element()?;
        if data_type != MI_MATRIX {
            return Err(InteropError::Unsupported(format!(
                "unsupported MAT-file top-level element type `{data_type}`"
            )));
        }
        let (name, value) = decode_mat_matrix_payload_with_context(payload, &mut context)?;
        if name.is_empty() {
            return Err(InteropError::Parse(
                "top-level MAT-file variable is missing a name".to_string(),
            ));
        }
        workspace.insert(name, value);
    }

    Ok(workspace)
}

pub fn write_mat_file(path: &Path, workspace: &Workspace) -> Result<(), InteropError> {
    let bytes = encode_mat_file(workspace)?;
    fs::write(path, bytes).map_err(|error| {
        InteropError::Io(format!(
            "failed to write MAT-file `{}`: {error}",
            path.display()
        ))
    })
}

pub fn read_mat_file(path: &Path) -> Result<Workspace, InteropError> {
    let bytes = fs::read(path).map_err(|error| {
        InteropError::Io(format!(
            "failed to read MAT-file `{}`: {error}",
            path.display()
        ))
    })?;
    decode_mat_file(&bytes)
}

fn write_mat5_header(out: &mut Vec<u8>) {
    let mut header = vec![b' '; MAT5_HEADER_TOTAL_BYTES];
    let description = format!(
        "MATLAB 5.0 MAT-file, Platform: MATC, Created on: {}",
        "Codex"
    );
    let bytes = description.as_bytes();
    let copy_len = bytes.len().min(MAT5_HEADER_TEXT_BYTES);
    header[..copy_len].copy_from_slice(&bytes[..copy_len]);
    header[124] = 0;
    header[125] = 1;
    header[126] = b'I';
    header[127] = b'M';
    out.extend_from_slice(&header);
}

fn write_mat_matrix_element_with_context(
    out: &mut Vec<u8>,
    name: &str,
    value: &Value,
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    let payload = encode_mat_matrix_payload_with_context(name, value, context)?;
    write_tagged_element(out, MI_MATRIX, &payload);
    Ok(())
}

fn encode_mat_matrix_payload_with_context(
    name: &str,
    value: &Value,
    context: &mut HandleAliasEncodeContext,
) -> Result<Vec<u8>, InteropError> {
    let mut out = Vec::new();
    match value {
        Value::Scalar(number) => {
            write_array_flags(&mut out, MX_DOUBLE_CLASS, 0);
            write_dimensions(&mut out, &[1, 1]);
            write_name(&mut out, name);
            write_f64_values(&mut out, &[*number]);
        }
        Value::Complex(number) => {
            write_array_flags(&mut out, MX_DOUBLE_CLASS, ARRAY_FLAG_COMPLEX);
            write_dimensions(&mut out, &[1, 1]);
            write_name(&mut out, name);
            write_f64_values(&mut out, &[number.real]);
            write_f64_values(&mut out, &[number.imag]);
        }
        Value::Logical(flag) => {
            write_array_flags(&mut out, MX_UINT8_CLASS, ARRAY_FLAG_LOGICAL);
            write_dimensions(&mut out, &[1, 1]);
            write_name(&mut out, name);
            write_u8_values(&mut out, &[if *flag { 1 } else { 0 }]);
        }
        Value::CharArray(text) => {
            write_array_flags(&mut out, MX_CHAR_CLASS, 0);
            write_dimensions(&mut out, &[1, text.encode_utf16().count()]);
            write_name(&mut out, name);
            write_u16_values(&mut out, &text.encode_utf16().collect::<Vec<_>>());
        }
        Value::String(text) => encode_mat_string_object(
            &mut out,
            name,
            std::slice::from_ref(text),
            &[1, 1],
            context,
        )?,
        Value::Matrix(matrix) => encode_mat_matrix_value(&mut out, name, matrix, context)?,
        Value::Cell(cell) => encode_mat_cell_value(&mut out, name, cell, context)?,
        Value::Struct(struct_value) => {
            encode_mat_struct_scalar(&mut out, name, struct_value, context)?
        }
        Value::Object(object) => encode_mat_object_value(&mut out, name, object, context)?,
        Value::FunctionHandle(handle) => {
            encode_mat_function_handle_object(&mut out, name, handle, context)?
        }
    }
    Ok(out)
}

fn encode_mat_matrix_value(
    out: &mut Vec<u8>,
    name: &str,
    matrix: &MatrixValue,
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    if matrix
        .elements
        .iter()
        .all(|value| matches!(value, Value::Logical(_)))
    {
        write_array_flags(out, MX_UINT8_CLASS, ARRAY_FLAG_LOGICAL);
        write_dimensions(out, &matrix.dims);
        write_name(out, name);
        let values = matrix_column_major_order(&matrix.dims, &matrix.elements)
            .into_iter()
            .map(|value| match value {
                Value::Logical(flag) => Ok(if flag { 1 } else { 0 }),
                other => Err(InteropError::Unsupported(format!(
                    "logical matrix expected only logical elements, found {}",
                    other.kind_name()
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        write_u8_values(out, &values);
        return Ok(());
    }

    if matrix
        .elements
        .iter()
        .all(|value| matches!(value, Value::String(_)))
    {
        let strings = matrix_column_major_order(&matrix.dims, &matrix.elements)
            .into_iter()
            .map(|value| match value {
                Value::String(text) => Ok(text),
                other => Err(InteropError::Unsupported(format!(
                    "string matrix expected only string elements, found {}",
                    other.kind_name()
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        return encode_mat_string_object(out, name, &strings, &matrix.dims, context);
    }

    if matrix
        .elements
        .iter()
        .all(|value| matches!(value, Value::Struct(_)))
    {
        return encode_mat_struct_array(out, name, &matrix.dims, &matrix.elements, context);
    }

    if let Some(class_metadata) = homogeneous_object_metadata(&matrix.elements) {
        return encode_mat_object_array(
            out,
            name,
            &class_metadata,
            &matrix.dims,
            &matrix.elements,
            context,
        );
    }

    let column_major = matrix_column_major_order(&matrix.dims, &matrix.elements);
    let has_complex = column_major
        .iter()
        .any(|value| matches!(value, Value::Complex(_)));
    write_array_flags(
        out,
        MX_DOUBLE_CLASS,
        if has_complex { ARRAY_FLAG_COMPLEX } else { 0 },
    );
    write_dimensions(out, &matrix.dims);
    write_name(out, name);
    let mut real = Vec::with_capacity(column_major.len());
    let mut imag = Vec::with_capacity(column_major.len());
    for value in column_major {
        match value {
            Value::Scalar(number) => {
                real.push(number);
                imag.push(0.0);
            }
            Value::Complex(number) => {
                real.push(number.real);
                imag.push(number.imag);
            }
            other => {
                return Err(InteropError::Unsupported(format!(
                    "MAT-file encoding currently supports only numeric/logical/struct matrices, found matrix element {}",
                    other.kind_name()
                )))
            }
        }
    }
    write_f64_values(out, &real);
    if has_complex {
        write_f64_values(out, &imag);
    }
    Ok(())
}

fn homogeneous_object_metadata(elements: &[Value]) -> Option<ObjectClassMetadata> {
    let mut objects = elements.iter().filter_map(|value| match value {
        Value::Object(object) => Some(&object.class),
        _ => None,
    });
    let first = objects.next()?.clone();
    if objects.all(|class| *class == first) {
        Some(first)
    } else {
        None
    }
}

fn encode_mat_cell_value(
    out: &mut Vec<u8>,
    name: &str,
    cell: &CellValue,
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    write_array_flags(out, MX_CELL_CLASS, 0);
    write_dimensions(out, &cell.dims);
    write_name(out, name);
    for value in matrix_column_major_order(&cell.dims, &cell.elements) {
        write_mat_matrix_element_with_context(out, "", &value, context)?;
    }
    Ok(())
}

fn encode_mat_struct_scalar(
    out: &mut Vec<u8>,
    name: &str,
    struct_value: &StructValue,
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    encode_mat_struct_array(out, name, &[1, 1], &[Value::Struct(struct_value.clone())], context)
}

fn encode_mat_function_handle_object(
    out: &mut Vec<u8>,
    name: &str,
    handle: &FunctionHandleValue,
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    write_array_flags(out, MX_OBJECT_CLASS, 0);
    write_dimensions(out, &[1, 1]);
    write_name(out, name);
    write_tagged_element(out, MI_INT8, b"function_handle");

    let field_names = vec![
        "display_name".to_string(),
        "target_kind".to_string(),
        "target_value".to_string(),
        "class_name".to_string(),
        "package".to_string(),
        "method_name".to_string(),
        "receiver".to_string(),
    ];
    let field_name_length = field_names
        .iter()
        .map(|field| field.len())
        .max()
        .unwrap_or(0)
        .max(1)
        + 1;

    let mut field_len_payload = Vec::new();
    push_i32_le(&mut field_len_payload, field_name_length as i32);
    write_tagged_element(out, MI_INT32, &field_len_payload);

    let mut field_names_payload = Vec::new();
    for field_name in &field_names {
        let mut bytes = vec![0u8; field_name_length];
        let name_bytes = field_name.as_bytes();
        let copy_len = name_bytes.len().min(field_name_length.saturating_sub(1));
        bytes[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        field_names_payload.extend_from_slice(&bytes);
    }
    write_tagged_element(out, MI_INT8, &field_names_payload);

    let empty_receiver = Value::Matrix(
        MatrixValue::new(0, 0, Vec::new())
            .expect("empty matrix value should be constructible"),
    );
    let (target_kind, target_value, class_name, package, method_name, receiver) = match &handle.target {
        FunctionHandleTarget::Named(name) => (
            "named",
            name.clone(),
            String::new(),
            String::new(),
            String::new(),
            empty_receiver,
        ),
        FunctionHandleTarget::ResolvedPath(path) => (
            "path",
            path.display().to_string(),
            String::new(),
            String::new(),
            String::new(),
            empty_receiver,
        ),
        FunctionHandleTarget::BundleModule(module_id) => (
            "bundle",
            module_id.clone(),
            String::new(),
            String::new(),
            String::new(),
            empty_receiver,
        ),
        FunctionHandleTarget::BoundMethod {
            class_name,
            package,
            method_name,
            receiver,
        } => (
            "bound",
            String::new(),
            class_name.clone(),
            package.clone().unwrap_or_default(),
            method_name.clone(),
            receiver.as_ref().clone(),
        ),
    };
    write_mat_matrix_element_with_context(out, "", &Value::CharArray(handle.display_name.clone()), context)?;
    write_mat_matrix_element_with_context(out, "", &Value::CharArray(target_kind.to_string()), context)?;
    write_mat_matrix_element_with_context(out, "", &Value::CharArray(target_value), context)?;
    write_mat_matrix_element_with_context(out, "", &Value::CharArray(class_name), context)?;
    write_mat_matrix_element_with_context(out, "", &Value::CharArray(package), context)?;
    write_mat_matrix_element_with_context(out, "", &Value::CharArray(method_name), context)?;
    write_mat_matrix_element_with_context(out, "", &receiver, context)?;
    Ok(())
}

fn encode_mat_object_value(
    out: &mut Vec<u8>,
    name: &str,
    object: &ObjectValue,
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    encode_mat_object_array(
        out,
        name,
        &object.class,
        &[1, 1],
        &[Value::Object(object.clone())],
        context,
    )
}

fn encode_mat_object_array(
    out: &mut Vec<u8>,
    name: &str,
    class: &ObjectClassMetadata,
    dims: &[usize],
    elements: &[Value],
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    write_array_flags(out, MX_OBJECT_CLASS, 0);
    write_dimensions(out, dims);
    write_name(out, name);
    write_tagged_element(out, MI_INT8, class.class_name.as_bytes());

    let mut field_names = vec![
        "__matc_package".to_string(),
        "__matc_superclass".to_string(),
        "__matc_ancestors".to_string(),
        "__matc_storage".to_string(),
        "__matc_source".to_string(),
        "__matc_module_target".to_string(),
        "__matc_constructor".to_string(),
        "__matc_inline_methods".to_string(),
        "__matc_private_properties".to_string(),
        "__matc_private_property_owners".to_string(),
        "__matc_private_inline_methods".to_string(),
        "__matc_private_instance_method_owners".to_string(),
        "__matc_private_static_inline_methods".to_string(),
        "__matc_handle_id".to_string(),
        "__matc_external_methods".to_string(),
    ];
    field_names.extend(class.property_order.iter().cloned());
    let field_name_length = field_names
        .iter()
        .map(|field| field.len())
        .max()
        .unwrap_or(0)
        .max(1)
        + 1;

    let mut field_len_payload = Vec::new();
    push_i32_le(&mut field_len_payload, field_name_length as i32);
    write_tagged_element(out, MI_INT32, &field_len_payload);

    let mut field_names_payload = Vec::new();
    for field_name in &field_names {
        let mut bytes = vec![0u8; field_name_length];
        let name_bytes = field_name.as_bytes();
        let copy_len = name_bytes.len().min(field_name_length.saturating_sub(1));
        bytes[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        field_names_payload.extend_from_slice(&bytes);
    }
    write_tagged_element(out, MI_INT8, &field_names_payload);

    let package = class.package.clone().unwrap_or_default();
    let superclass = class.superclass_name.clone().unwrap_or_default();
    let ancestors = class
        .ancestor_class_names
        .iter()
        .cloned()
        .map(Value::String)
        .collect::<Vec<_>>();
    let ancestors = Value::Cell(
        CellValue::new(1, ancestors.len(), ancestors)
            .map_err(|error| InteropError::Unsupported(error.to_string()))?,
    );
    let storage = match class.storage_kind {
        ObjectStorageKind::Value => "value".to_string(),
        ObjectStorageKind::Handle => "handle".to_string(),
    };
    let source = class
        .source_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let module_target = class
        .module_target
        .as_ref()
        .map(encode_object_method_target)
        .unwrap_or_default();
    let constructor = class.constructor.clone().unwrap_or_default();
    let inline_methods = class
        .inline_methods
        .iter()
        .cloned()
        .map(Value::String)
        .collect::<Vec<_>>();
    let inline_methods = Value::Cell(
        CellValue::new(1, inline_methods.len(), inline_methods)
            .map_err(|error| InteropError::Unsupported(error.to_string()))?,
    );
    let private_properties = class
        .private_properties
        .iter()
        .cloned()
        .map(Value::String)
        .collect::<Vec<_>>();
    let private_properties = Value::Cell(
        CellValue::new(1, private_properties.len(), private_properties)
            .map_err(|error| InteropError::Unsupported(error.to_string()))?,
    );
    let private_property_owners = Value::Struct(StructValue::from_fields(
        class
            .private_property_owners
            .iter()
            .map(|(name, owner)| (name.clone(), Value::String(owner.clone())))
            .collect(),
    ));
    let private_inline_methods = class
        .private_inline_methods
        .iter()
        .cloned()
        .map(Value::String)
        .collect::<Vec<_>>();
    let private_inline_methods = Value::Cell(
        CellValue::new(1, private_inline_methods.len(), private_inline_methods)
            .map_err(|error| InteropError::Unsupported(error.to_string()))?,
    );
    let private_instance_method_owners = Value::Struct(StructValue::from_fields(
        class
            .private_instance_method_owners
            .iter()
            .map(|(name, owner)| (name.clone(), Value::String(owner.clone())))
            .collect(),
    ));
    let private_static_inline_methods = class
        .private_static_inline_methods
        .iter()
        .cloned()
        .map(Value::String)
        .collect::<Vec<_>>();
    let private_static_inline_methods = Value::Cell(
        CellValue::new(1, private_static_inline_methods.len(), private_static_inline_methods)
            .map_err(|error| InteropError::Unsupported(error.to_string()))?,
    );
    let mut external_fields = BTreeMap::new();
    for (method, target) in &class.external_methods {
        external_fields.insert(
            method.clone(),
            Value::String(encode_object_method_target(target)),
        );
    }
    let external_methods = Value::Struct(StructValue::from_fields(external_fields));

    for element in matrix_column_major_order(dims, elements) {
        let Value::Object(object) = element else {
            return Err(InteropError::Unsupported(
                "MAT-file object array encoding requires object elements".to_string(),
            ));
        };
        let properties = object.properties();
        let handle_id = context.handle_id_for_object(&object).unwrap_or_default();
        write_mat_matrix_element_with_context(out, "", &Value::CharArray(package.clone()), context)?;
        write_mat_matrix_element_with_context(out, "", &Value::CharArray(superclass.clone()), context)?;
        write_mat_matrix_element_with_context(out, "", &ancestors, context)?;
        write_mat_matrix_element_with_context(out, "", &Value::CharArray(storage.clone()), context)?;
        write_mat_matrix_element_with_context(out, "", &Value::CharArray(source.clone()), context)?;
        write_mat_matrix_element_with_context(out, "", &Value::CharArray(module_target.clone()), context)?;
        write_mat_matrix_element_with_context(out, "", &Value::CharArray(constructor.clone()), context)?;
        write_mat_matrix_element_with_context(out, "", &inline_methods, context)?;
        write_mat_matrix_element_with_context(out, "", &private_properties, context)?;
        write_mat_matrix_element_with_context(out, "", &private_property_owners, context)?;
        write_mat_matrix_element_with_context(out, "", &private_inline_methods, context)?;
        write_mat_matrix_element_with_context(out, "", &private_instance_method_owners, context)?;
        write_mat_matrix_element_with_context(out, "", &private_static_inline_methods, context)?;
        write_mat_matrix_element_with_context(out, "", &Value::CharArray(handle_id), context)?;
        write_mat_matrix_element_with_context(out, "", &external_methods, context)?;
        for property_name in &class.property_order {
            let value = properties
                .fields
                .get(property_name)
                .cloned()
                .unwrap_or_else(|| {
                    Value::Matrix(
                        MatrixValue::new(0, 0, Vec::new())
                            .expect("empty matrix value should be constructible"),
                    )
                });
            write_mat_matrix_element_with_context(out, "", &value, context)?;
        }
    }
    Ok(())
}

fn encode_mat_string_object(
    out: &mut Vec<u8>,
    name: &str,
    strings: &[String],
    dims: &[usize],
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    write_array_flags(out, MX_OBJECT_CLASS, 0);
    write_dimensions(out, dims);
    write_name(out, name);
    write_tagged_element(out, MI_INT8, b"string");

    let field_names = vec!["data".to_string()];
    let field_name_length = field_names
        .iter()
        .map(|field| field.len())
        .max()
        .unwrap_or(0)
        .max(1)
        + 1;

    let mut field_len_payload = Vec::new();
    push_i32_le(&mut field_len_payload, field_name_length as i32);
    write_tagged_element(out, MI_INT32, &field_len_payload);

    let mut field_names_payload = Vec::new();
    for field_name in &field_names {
        let mut bytes = vec![0u8; field_name_length];
        let name_bytes = field_name.as_bytes();
        let copy_len = name_bytes.len().min(field_name_length.saturating_sub(1));
        bytes[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        field_names_payload.extend_from_slice(&bytes);
    }
    write_tagged_element(out, MI_INT8, &field_names_payload);

    for text in strings {
        write_mat_matrix_element_with_context(out, "", &Value::CharArray(text.clone()), context)?;
    }
    Ok(())
}

fn encode_mat_struct_array(
    out: &mut Vec<u8>,
    name: &str,
    dims: &[usize],
    elements: &[Value],
    context: &mut HandleAliasEncodeContext,
) -> Result<(), InteropError> {
    write_array_flags(out, MX_STRUCT_CLASS, 0);
    write_dimensions(out, dims);
    write_name(out, name);

    let mut field_names = BTreeMap::new();
    let mut ordered_field_names = Vec::new();
    for element in elements {
        let Value::Struct(struct_value) = element else {
            return Err(InteropError::Unsupported(
                "MAT-file struct array encoding requires struct elements".to_string(),
            ));
        };
        for field_name in struct_value.field_names() {
            if field_names.insert(field_name.clone(), ()).is_none() {
                ordered_field_names.push(field_name.clone());
            }
        }
    }
    let field_names = ordered_field_names;
    let field_name_length = field_names
        .iter()
        .map(|field| field.len())
        .max()
        .unwrap_or(0)
        .max(1)
        + 1;

    let mut field_len_payload = Vec::new();
    push_i32_le(&mut field_len_payload, field_name_length as i32);
    write_tagged_element(out, MI_INT32, &field_len_payload);

    let mut field_names_payload = Vec::new();
    for field_name in &field_names {
        let mut bytes = vec![0u8; field_name_length];
        let name_bytes = field_name.as_bytes();
        let copy_len = name_bytes.len().min(field_name_length.saturating_sub(1));
        bytes[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        field_names_payload.extend_from_slice(&bytes);
    }
    write_tagged_element(out, MI_INT8, &field_names_payload);

    for element in matrix_column_major_order(dims, elements) {
        let Value::Struct(struct_value) = element else {
            unreachable!("validated struct elements");
        };
        for field_name in &field_names {
            let value = struct_value
                .fields
                .get(field_name)
                .cloned()
                .unwrap_or_else(|| {
                    Value::Matrix(MatrixValue::new(0, 0, Vec::new()).expect("empty matrix"))
                });
            write_mat_matrix_element_with_context(out, "", &value, context)?;
        }
    }
    Ok(())
}

fn decode_mat_matrix_payload_with_context(
    payload: &[u8],
    context: &mut HandleAliasDecodeContext,
) -> Result<(String, Value), InteropError> {
    let mut reader = ByteReader::new(payload);
    let (_, flags_payload) = reader.read_tagged_element()?;
    if flags_payload.len() < 8 {
        return Err(InteropError::Parse(
            "MAT-file array flags payload is too short".to_string(),
        ));
    }
    let flags = u32::from_le_bytes(flags_payload[0..4].try_into().unwrap());
    let class = flags & 0xFF;
    let logical = (flags & ARRAY_FLAG_LOGICAL) != 0;
    let complex = (flags & ARRAY_FLAG_COMPLEX) != 0;

    let (dims_type, dims_payload) = reader.read_tagged_element()?;
    if dims_type != MI_INT32 && dims_type != MI_UINT32 {
        return Err(InteropError::Unsupported(format!(
            "unsupported MAT-file dimensions type `{dims_type}`"
        )));
    }
    let dims = decode_dims(&dims_payload)?;
    let rows = dims.first().copied().unwrap_or(0);
    let cols = dims.get(1).copied().unwrap_or(1);

    let (_, name_payload) = reader.read_tagged_element()?;
    let name = String::from_utf8_lossy(&name_payload)
        .trim_end_matches('\0')
        .to_string();

    let value = match class {
        MX_DOUBLE_CLASS => decode_mat_double_array(&mut reader, &dims, rows, cols, complex)?,
        MX_CHAR_CLASS => decode_mat_char_array(&mut reader, &dims)?,
        MX_CELL_CLASS => decode_mat_cell_array(&mut reader, &dims, rows, cols, context)?,
        MX_STRUCT_CLASS => decode_mat_struct_array(&mut reader, &dims, context)?,
        MX_OBJECT_CLASS => decode_mat_object_array(&mut reader, &dims, context)?,
        MX_UINT8_CLASS if logical => decode_mat_logical_array(&mut reader, &dims, rows, cols)?,
        other => {
            return Err(InteropError::Unsupported(format!(
                "unsupported MAT-file array class `{other}`"
            )))
        }
    };

    Ok((name, value))
}

fn decode_mat_double_array(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
    rows: usize,
    cols: usize,
    complex: bool,
) -> Result<Value, InteropError> {
    let (_, real_payload) = reader.read_tagged_element()?;
    let real = decode_f64_payload(&real_payload)?;
    let imag = if complex {
        let (_, imag_payload) = reader.read_tagged_element()?;
        Some(decode_f64_payload(&imag_payload)?)
    } else {
        None
    };
    let count = dims.iter().product::<usize>();
    if real.len() != count {
        return Err(InteropError::Parse(format!(
            "MAT-file numeric payload length {} does not match dimensions {:?}",
            real.len(),
            dims
        )));
    }
    let mut values = vec![Value::Scalar(0.0); count];
    for linear in 0..count {
        let index = column_major_multi_index(linear, dims);
        let row_major = row_major_linear_index(&index, dims);
        values[row_major] = match &imag {
            Some(imag) if imag[linear] != 0.0 => Value::Complex(ComplexValue {
                real: real[linear],
                imag: imag[linear],
            }),
            _ => Value::Scalar(real[linear]),
        };
    }
    if count == 1 {
        return Ok(values.into_iter().next().unwrap_or(Value::Scalar(0.0)));
    }
    MatrixValue::with_dimensions(rows, cols, dims.to_vec(), values)
        .map(Value::Matrix)
        .map_err(|error| InteropError::Parse(error.to_string()))
}

fn decode_mat_logical_array(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
    rows: usize,
    cols: usize,
) -> Result<Value, InteropError> {
    let (_, payload) = reader.read_tagged_element()?;
    let count = dims.iter().product::<usize>();
    if payload.len() < count {
        return Err(InteropError::Parse(
            "MAT-file logical payload is shorter than expected".to_string(),
        ));
    }
    let mut values = vec![Value::Logical(false); count];
    for linear in 0..count {
        let index = column_major_multi_index(linear, dims);
        let row_major = row_major_linear_index(&index, dims);
        values[row_major] = Value::Logical(payload[linear] != 0);
    }
    if count == 1 {
        return Ok(values.into_iter().next().unwrap_or(Value::Logical(false)));
    }
    MatrixValue::with_dimensions(rows, cols, dims.to_vec(), values)
        .map(Value::Matrix)
        .map_err(|error| InteropError::Parse(error.to_string()))
}

fn decode_mat_char_array(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
) -> Result<Value, InteropError> {
    let (data_type, payload) = reader.read_tagged_element()?;
    let code_units = match data_type {
        MI_UINT16 => payload
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>(),
        MI_UINT8 | MI_INT8 => payload.iter().map(|byte| *byte as u16).collect::<Vec<_>>(),
        other => {
            return Err(InteropError::Unsupported(format!(
                "unsupported MAT-file char payload type `{other}`"
            )))
        }
    };
    if dims.first().copied().unwrap_or(0) > 1 {
        return Err(InteropError::Unsupported(
            "multi-row char arrays are not implemented yet in MAT-file decoding".to_string(),
        ));
    }
    Ok(Value::CharArray(String::from_utf16_lossy(&code_units)))
}

fn decode_mat_cell_array(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
    rows: usize,
    cols: usize,
    context: &mut HandleAliasDecodeContext,
) -> Result<Value, InteropError> {
    let count = dims.iter().product::<usize>();
    let mut values = vec![Value::Scalar(0.0); count];
    for linear in 0..count {
        let (data_type, payload) = reader.read_tagged_element()?;
        if data_type != MI_MATRIX {
            return Err(InteropError::Unsupported(format!(
                "unsupported MAT-file cell element type `{data_type}`"
            )));
        }
        let (_, value) = decode_mat_matrix_payload_with_context(payload, context)?;
        let index = column_major_multi_index(linear, dims);
        let row_major = row_major_linear_index(&index, dims);
        values[row_major] = value;
    }
    CellValue::with_dimensions(rows, cols, dims.to_vec(), values)
        .map(Value::Cell)
        .map_err(|error| InteropError::Parse(error.to_string()))
}

fn decode_mat_struct_array(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
    context: &mut HandleAliasDecodeContext,
) -> Result<Value, InteropError> {
    let (_, field_len_payload) = reader.read_tagged_element()?;
    if field_len_payload.len() < 4 {
        return Err(InteropError::Parse(
            "MAT-file struct field-length payload is too short".to_string(),
        ));
    }
    let field_name_length = i32::from_le_bytes(field_len_payload[0..4].try_into().unwrap());
    if field_name_length <= 0 {
        return Err(InteropError::Parse(
            "MAT-file struct field-name length must be positive".to_string(),
        ));
    }
    let (_, field_names_payload) = reader.read_tagged_element()?;
    if field_names_payload.is_empty() {
        return Ok(Value::Struct(StructValue::default()));
    }
    let field_name_length = field_name_length as usize;
    if field_names_payload.len() % field_name_length != 0 {
        return Err(InteropError::Parse(
            "MAT-file struct field-name table has invalid width".to_string(),
        ));
    }
    let field_names = field_names_payload
        .chunks(field_name_length)
        .map(|chunk| {
            String::from_utf8_lossy(chunk)
                .trim_end_matches('\0')
                .to_string()
        })
        .collect::<Vec<_>>();
    let count = dims.iter().product::<usize>();
    let mut elements = vec![Value::Struct(StructValue::default()); count];
    for linear in 0..count {
        let index = column_major_multi_index(linear, dims);
        let row_major = row_major_linear_index(&index, dims);
        let mut fields = BTreeMap::new();
        for field_name in &field_names {
            let (data_type, payload) = reader.read_tagged_element()?;
            if data_type != MI_MATRIX {
                return Err(InteropError::Unsupported(format!(
                    "unsupported MAT-file struct field type `{data_type}`"
                )));
            }
            let (_, value) = decode_mat_matrix_payload_with_context(payload, context)?;
            fields.insert(field_name.clone(), value);
        }
        elements[row_major] =
            Value::Struct(StructValue::with_field_order(fields, field_names.clone()));
    }
    if count == 1 {
        return Ok(elements
            .into_iter()
            .next()
            .unwrap_or(Value::Struct(StructValue::default())));
    }
    let rows = dims.first().copied().unwrap_or(0);
    let cols = dims.get(1).copied().unwrap_or(1);
    MatrixValue::with_dimensions(rows, cols, dims.to_vec(), elements)
        .map(Value::Matrix)
        .map_err(|error| InteropError::Parse(error.to_string()))
}

fn decode_mat_object_array(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
    context: &mut HandleAliasDecodeContext,
) -> Result<Value, InteropError> {
    let (_, class_name_payload) = reader.read_tagged_element()?;
    let class_name = String::from_utf8_lossy(&class_name_payload)
        .trim_end_matches('\0')
        .to_string();
    let (_, field_len_payload) = reader.read_tagged_element()?;
    if field_len_payload.len() < 4 {
        return Err(InteropError::Parse(
            "MAT-file object field-length payload is too short".to_string(),
        ));
    }
    let field_name_length = i32::from_le_bytes(field_len_payload[0..4].try_into().unwrap());
    if field_name_length <= 0 {
        return Err(InteropError::Parse(
            "MAT-file object field-name length must be positive".to_string(),
        ));
    }
    let (_, field_names_payload) = reader.read_tagged_element()?;
    let field_name_length = field_name_length as usize;
    if field_names_payload.len() % field_name_length != 0 {
        return Err(InteropError::Parse(
            "MAT-file object field-name table has invalid width".to_string(),
        ));
    }
    let field_names = field_names_payload
        .chunks(field_name_length)
        .map(|chunk| {
            String::from_utf8_lossy(chunk)
                .trim_end_matches('\0')
                .to_string()
        })
        .collect::<Vec<_>>();
    match class_name.as_str() {
        "string" => decode_mat_string_object(reader, dims, &field_names, context),
        "function_handle" => decode_mat_function_handle_object(reader, dims, &field_names, context),
        _ => decode_mat_generic_object(reader, dims, &class_name, &field_names, context),
    }
}

fn decode_mat_string_object(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
    field_names: &[String],
    context: &mut HandleAliasDecodeContext,
) -> Result<Value, InteropError> {
    let count = dims.iter().product::<usize>();
    let mut values = vec![Value::String(String::new()); count];
    for linear in 0..count {
        let index = column_major_multi_index(linear, dims);
        let row_major = row_major_linear_index(&index, dims);
        let mut data = String::new();
        for field_name in field_names {
            let (data_type, payload) = reader.read_tagged_element()?;
            if data_type != MI_MATRIX {
                return Err(InteropError::Unsupported(format!(
                    "unsupported MAT-file object field type `{data_type}`"
                )));
            }
            let (_, value) = decode_mat_matrix_payload_with_context(payload, context)?;
            match field_name.as_str() {
                "data" => {
                    let Value::CharArray(text) = value else {
                        return Err(InteropError::Unsupported(format!(
                            "string object field `data` expected char data, found {}",
                            value.kind_name()
                        )));
                    };
                    data = text;
                }
                _ => {}
            }
        }
        values[row_major] = Value::String(data);
    }
    if count == 1 {
        return Ok(values
            .into_iter()
            .next()
            .unwrap_or(Value::String(String::new())));
    }
    let rows = dims.first().copied().unwrap_or(0);
    let cols = dims.get(1).copied().unwrap_or(1);
    MatrixValue::with_dimensions(rows, cols, dims.to_vec(), values)
        .map(Value::Matrix)
        .map_err(|error| InteropError::Parse(error.to_string()))
}

fn decode_mat_function_handle_object(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
    field_names: &[String],
    context: &mut HandleAliasDecodeContext,
) -> Result<Value, InteropError> {
    let count = dims.iter().product::<usize>();
    let mut values = vec![
        Value::FunctionHandle(FunctionHandleValue {
            display_name: String::new(),
            target: FunctionHandleTarget::Named(String::new()),
        });
        count
    ];
    for linear in 0..count {
        let index = column_major_multi_index(linear, dims);
        let row_major = row_major_linear_index(&index, dims);
        let mut display_name = None;
        let mut target_kind = None;
        let mut target_value = None;
        let mut class_name = None;
        let mut package = None;
        let mut method_name = None;
        let mut receiver = None;
        for field_name in field_names {
            let (data_type, payload) = reader.read_tagged_element()?;
            if data_type != MI_MATRIX {
                return Err(InteropError::Unsupported(format!(
                    "unsupported MAT-file object field type `{data_type}`"
                )));
            }
            match field_name.as_str() {
                "receiver" => {
                    let (_, value) = decode_mat_matrix_payload_with_context(payload, context)?;
                    receiver = Some(value);
                }
                _ => {
                    let (_, value) = decode_mat_matrix_payload_with_context(payload, context)?;
                    let text = match value {
                        Value::CharArray(text) => text,
                        other => {
                            return Err(InteropError::Unsupported(format!(
                                "function_handle object field `{field_name}` expected char data, found {}",
                                other.kind_name()
                            )))
                        }
                    };
                    match field_name.as_str() {
                        "display_name" => display_name = Some(text),
                        "target_kind" => target_kind = Some(text),
                        "target_value" => target_value = Some(text),
                        "class_name" => class_name = Some(text),
                        "package" => package = Some(text),
                        "method_name" => method_name = Some(text),
                        _ => {}
                    }
                }
            }
        }
        let display_name = display_name.unwrap_or_default();
        let target_kind = target_kind.unwrap_or_else(|| "named".to_string());
        let target_value = target_value.unwrap_or_default();
        let target = match target_kind.as_str() {
            "named" => FunctionHandleTarget::Named(target_value),
            "path" => FunctionHandleTarget::ResolvedPath(target_value.into()),
            "bundle" => FunctionHandleTarget::BundleModule(target_value),
            "bound" => FunctionHandleTarget::BoundMethod {
                class_name: class_name.unwrap_or_default(),
                package: package.filter(|package| !package.is_empty()),
                method_name: method_name.unwrap_or_default(),
                receiver: Box::new(receiver.unwrap_or_else(|| {
                    Value::Matrix(
                        MatrixValue::new(0, 0, Vec::new())
                            .expect("empty matrix value should be constructible"),
                    )
                })),
            },
            other => {
                return Err(InteropError::Unsupported(format!(
                    "unsupported function_handle target kind `{other}`"
                )))
            }
        };
        values[row_major] = Value::FunctionHandle(FunctionHandleValue {
            display_name,
            target,
        });
    }
    if count == 1 {
        return Ok(values.into_iter().next().unwrap_or(Value::FunctionHandle(
            FunctionHandleValue {
                display_name: String::new(),
                target: FunctionHandleTarget::Named(String::new()),
            },
        )));
    }
    let rows = dims.first().copied().unwrap_or(0);
    let cols = dims.get(1).copied().unwrap_or(1);
    MatrixValue::with_dimensions(rows, cols, dims.to_vec(), values)
        .map(Value::Matrix)
        .map_err(|error| InteropError::Parse(error.to_string()))
}

fn decode_mat_generic_object(
    reader: &mut ByteReader<'_>,
    dims: &[usize],
    class_name: &str,
    field_names: &[String],
    context: &mut HandleAliasDecodeContext,
) -> Result<Value, InteropError> {
    let count = dims.iter().product::<usize>();
    let rows = dims.first().copied().unwrap_or(0);
    let cols = dims.get(1).copied().unwrap_or(1);
    let mut values = vec![Value::Struct(StructValue::default()); count];
    for linear in 0..count {
        let index = column_major_multi_index(linear, dims);
        let row_major = row_major_linear_index(&index, dims);

        let mut package = None;
        let mut superclass_name = None;
        let mut ancestor_class_names = BTreeSet::new();
        let mut storage_kind = ObjectStorageKind::Value;
        let mut source_path = None;
        let mut module_target = None;
        let mut constructor = None;
        let mut handle_id = None;
        let mut inline_methods = BTreeSet::new();
        let mut private_properties = BTreeSet::new();
        let mut private_property_owners = BTreeMap::new();
        let mut private_inline_methods = BTreeSet::new();
        let mut private_instance_method_owners = BTreeMap::new();
        let mut private_static_inline_methods = BTreeSet::new();
        let mut external_methods = BTreeMap::new();
        let mut property_fields = BTreeMap::new();
        let mut property_order = Vec::new();

        for field_name in field_names {
            let (data_type, payload) = reader.read_tagged_element()?;
            if data_type != MI_MATRIX {
                return Err(InteropError::Unsupported(format!(
                    "unsupported MAT-file object field type `{data_type}`"
                )));
            }
            let (_, value) = decode_mat_matrix_payload_with_context(payload, context)?;
            match field_name.as_str() {
                "__matc_package" => package = optional_text_value(&value)?,
                "__matc_superclass" => superclass_name = optional_text_value(&value)?,
                "__matc_ancestors" => {
                    ancestor_class_names = decode_text_set(&value)?;
                }
                "__matc_storage" => {
                    storage_kind = match optional_text_value(&value)?.as_deref() {
                        Some("handle") => ObjectStorageKind::Handle,
                        _ => ObjectStorageKind::Value,
                    };
                }
                "__matc_source" => {
                    source_path = optional_text_value(&value)?.map(Into::into);
                }
                "__matc_module_target" => {
                    module_target = optional_text_value(&value)?
                        .map(|text| decode_object_method_target(&text))
                        .transpose()?;
                }
                "__matc_constructor" => constructor = optional_text_value(&value)?,
                "__matc_inline_methods" => {
                    inline_methods = decode_text_set(&value)?;
                }
                "__matc_private_properties" => {
                    private_properties = decode_text_set(&value)?;
                }
                "__matc_private_property_owners" => {
                    private_property_owners = decode_string_struct_map(&value)?;
                }
                "__matc_private_inline_methods" => {
                    private_inline_methods = decode_text_set(&value)?;
                }
                "__matc_private_instance_method_owners" => {
                    private_instance_method_owners = decode_string_struct_map(&value)?;
                }
                "__matc_private_static_inline_methods" => {
                    private_static_inline_methods = decode_text_set(&value)?;
                }
                "__matc_handle_id" => handle_id = optional_text_value(&value)?,
                "__matc_external_methods" => {
                    external_methods = decode_method_target_struct_map(&value)?;
                }
                _ => {
                    property_order.push(field_name.clone());
                    property_fields.insert(field_name.clone(), value);
                }
            }
        }

        let class = ObjectClassMetadata {
            class_name: class_name.to_string(),
            package,
            superclass_name,
            ancestor_class_names,
            storage_kind,
            source_path,
            module_target,
            property_order: property_order.clone(),
            private_properties,
            private_property_owners,
            inline_methods,
            private_inline_methods,
            private_instance_method_owners,
            private_static_inline_methods,
            external_methods,
            constructor,
        };
        let properties = StructValue::with_field_order(property_fields, property_order);
        values[row_major] = Value::Object(decode_object_with_alias_context(
            class,
            properties,
            handle_id,
            context,
        ));
    }

    if count == 1 {
        return Ok(values.into_iter().next().unwrap_or_else(|| {
            Value::Object(ObjectValue::new(
                ObjectClassMetadata {
                    class_name: class_name.to_string(),
                    package: None,
                    superclass_name: None,
                    ancestor_class_names: BTreeSet::new(),
                    storage_kind: ObjectStorageKind::Value,
                source_path: None,
                module_target: None,
                property_order: Vec::new(),
                private_properties: BTreeSet::new(),
                private_property_owners: BTreeMap::new(),
                inline_methods: BTreeSet::new(),
                private_inline_methods: BTreeSet::new(),
                private_instance_method_owners: BTreeMap::new(),
                private_static_inline_methods: BTreeSet::new(),
                external_methods: BTreeMap::new(),
                constructor: None,
                },
                StructValue::default(),
            ))
        }));
    }

    MatrixValue::with_dimensions(rows, cols, dims.to_vec(), values)
        .map(Value::Matrix)
        .map_err(|error| InteropError::Parse(error.to_string()))
}

fn decode_dims(payload: &[u8]) -> Result<Vec<usize>, InteropError> {
    if payload.len() % 4 != 0 {
        return Err(InteropError::Parse(
            "MAT-file dimensions payload length is not a multiple of 4".to_string(),
        ));
    }
    let mut dims = payload
        .chunks_exact(4)
        .map(|chunk| i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .map(|value| {
            if value < 0 {
                Err(InteropError::Parse(
                    "MAT-file dimensions cannot be negative".to_string(),
                ))
            } else {
                Ok(value as usize)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if dims.is_empty() {
        dims = vec![0, 0];
    }
    if dims.len() == 1 {
        dims.push(1);
    }
    Ok(dims)
}

fn decode_f64_payload(payload: &[u8]) -> Result<Vec<f64>, InteropError> {
    if payload.len() % 8 != 0 {
        return Err(InteropError::Parse(
            "MAT-file floating-point payload length is not a multiple of 8".to_string(),
        ));
    }
    Ok(payload
        .chunks_exact(8)
        .map(|chunk| {
            f64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ])
        })
        .collect())
}

fn write_array_flags(out: &mut Vec<u8>, class: u32, flags: u32) {
    let mut payload = Vec::new();
    push_u32_le(&mut payload, class | flags);
    push_u32_le(&mut payload, 0);
    write_tagged_element(out, MI_UINT32, &payload);
}

fn write_dimensions(out: &mut Vec<u8>, dims: &[usize]) {
    let mut payload = Vec::new();
    for &dimension in dims {
        push_i32_le(&mut payload, dimension as i32);
    }
    write_tagged_element(out, MI_INT32, &payload);
}

fn write_name(out: &mut Vec<u8>, name: &str) {
    write_tagged_element(out, MI_INT8, name.as_bytes());
}

fn write_f64_values(out: &mut Vec<u8>, values: &[f64]) {
    let mut payload = Vec::new();
    for &value in values {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    write_tagged_element(out, MI_DOUBLE, &payload);
}

fn write_u8_values(out: &mut Vec<u8>, values: &[u8]) {
    write_tagged_element(out, MI_UINT8, values);
}

fn write_u16_values(out: &mut Vec<u8>, values: &[u16]) {
    let mut payload = Vec::new();
    for &value in values {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    write_tagged_element(out, MI_UINT16, &payload);
}

fn write_tagged_element(out: &mut Vec<u8>, data_type: u32, payload: &[u8]) {
    push_u32_le(out, data_type);
    push_u32_le(out, payload.len() as u32);
    out.extend_from_slice(payload);
    pad_to_8(out, payload.len());
}

fn push_u32_le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i32_le(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn pad_to_8(out: &mut Vec<u8>, payload_len: usize) {
    let remainder = payload_len % 8;
    if remainder != 0 {
        out.extend(std::iter::repeat_n(0u8, 8 - remainder));
    }
}

fn matrix_column_major_order(dims: &[usize], elements: &[Value]) -> Vec<Value> {
    let count = dims.iter().product::<usize>();
    (0..count)
        .map(|linear| {
            let index = column_major_multi_index(linear, dims);
            let row_major = row_major_linear_index(&index, dims);
            elements[row_major].clone()
        })
        .collect()
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

fn row_major_linear_index(index: &[usize], dims: &[usize]) -> usize {
    let mut linear = 0usize;
    for (axis, &value) in index.iter().enumerate() {
        linear = (linear * dims[axis].max(1)) + value;
    }
    linear
}

fn optional_text_value(value: &Value) -> Result<Option<String>, InteropError> {
    match value {
        Value::CharArray(text) | Value::String(text) if text.is_empty() => Ok(None),
        Value::CharArray(text) | Value::String(text) => Ok(Some(text.clone())),
        other => Err(InteropError::Unsupported(format!(
            "expected text metadata value, found {}",
            other.kind_name()
        ))),
    }
}

fn decode_text_set(value: &Value) -> Result<BTreeSet<String>, InteropError> {
    match value {
        Value::Cell(cell) => {
            let mut values = BTreeSet::new();
            for element in &cell.elements {
                let Some(text) = optional_text_value(element)? else {
                    continue;
                };
                values.insert(text);
            }
            Ok(values)
        }
        Value::CharArray(text) | Value::String(text) if text.is_empty() => Ok(BTreeSet::new()),
        other => Err(InteropError::Unsupported(format!(
            "expected text-cell metadata value, found {}",
            other.kind_name()
        ))),
    }
}

fn decode_method_target_struct_map(
    value: &Value,
) -> Result<BTreeMap<String, ObjectMethodTarget>, InteropError> {
    let Value::Struct(struct_value) = value else {
        return Err(InteropError::Unsupported(format!(
            "expected struct metadata value, found {}",
            value.kind_name()
        )));
    };
    let mut values = BTreeMap::new();
    for (name, value) in struct_value.ordered_entries() {
        if let Some(text) = optional_text_value(value)? {
            values.insert(name.to_string(), decode_object_method_target(&text)?);
        }
    }
    Ok(values)
}

fn decode_string_struct_map(value: &Value) -> Result<BTreeMap<String, String>, InteropError> {
    let Value::Struct(struct_value) = value else {
        return Err(InteropError::Unsupported(format!(
            "expected struct metadata value, found {}",
            value.kind_name()
        )));
    };
    let mut values = BTreeMap::new();
    for (name, value) in struct_value.ordered_entries() {
        if let Some(text) = optional_text_value(value)? {
            values.insert(name.to_string(), text);
        }
    }
    Ok(values)
}

fn decode_object_with_alias_context(
    class: ObjectClassMetadata,
    properties: StructValue,
    handle_id: Option<String>,
    context: &mut HandleAliasDecodeContext,
) -> ObjectValue {
    if class.storage_kind == ObjectStorageKind::Handle {
        if let Some(handle_id) = handle_id.filter(|id| !id.is_empty()) {
            let numeric_id = handle_id
                .strip_prefix('h')
                .and_then(|text| text.parse::<u64>().ok());
            if let Some(id) = numeric_id {
                observe_handle_object_id(id);
            }
            if let Some(shared) = context.handles.get(&handle_id).cloned() {
                return ObjectValue::from_storage(
                    class,
                    ObjectStorage::Handle {
                        id: numeric_id.unwrap_or_default(),
                        shared,
                    },
                );
            }
            let shared = Rc::new(RefCell::new(ObjectInstance { properties }));
            context.handles.insert(handle_id, shared.clone());
            return ObjectValue::from_storage(
                class,
                ObjectStorage::Handle {
                    id: numeric_id.unwrap_or_default(),
                    shared,
                },
            );
        }
    }
    ObjectValue::new(class, properties)
}

fn encode_object_method_target(target: &ObjectMethodTarget) -> String {
    match target {
        ObjectMethodTarget::Path(path) => format!("path:{}", path.display()),
        ObjectMethodTarget::BundleModule(module_id) => format!("bundle:{module_id}"),
    }
}

fn decode_object_method_target(source: &str) -> Result<ObjectMethodTarget, InteropError> {
    if let Some(rest) = source.strip_prefix("bundle:") {
        return Ok(ObjectMethodTarget::BundleModule(rest.to_string()));
    }
    if let Some(rest) = source.strip_prefix("path:") {
        return Ok(ObjectMethodTarget::Path(PathBuf::from(rest)));
    }
    Ok(ObjectMethodTarget::Path(PathBuf::from(source)))
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset >= self.bytes.len()
    }

    fn expect_len(&self, minimum: usize) -> Result<(), InteropError> {
        if self.bytes.len() < minimum {
            return Err(InteropError::Parse(format!(
                "input is shorter than the required MAT-file header ({minimum} bytes)"
            )));
        }
        Ok(())
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], InteropError> {
        let end = self.offset.saturating_add(len);
        if end > self.bytes.len() {
            return Err(InteropError::Parse(
                "unexpected end of MAT-file payload".to_string(),
            ));
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn read_u32(&mut self) -> Result<u32, InteropError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_tagged_element(&mut self) -> Result<(u32, &'a [u8]), InteropError> {
        let data_type = self.read_u32()?;
        let payload_len = self.read_u32()? as usize;
        let payload = self.read_bytes(payload_len)?;
        self.skip_payload_padding(payload_len);
        Ok((data_type, payload))
    }

    fn skip_payload_padding(&mut self, payload_len: usize) {
        let remainder = payload_len % 8;
        if remainder != 0 {
            self.offset = self
                .offset
                .saturating_add(8 - remainder)
                .min(self.bytes.len());
        }
    }

    fn skip_zero_padding(&mut self) {
        while self.offset < self.bytes.len() && self.bytes[self.offset] == 0 {
            self.offset += 1;
        }
    }
}

fn encode_value(value: &Value) -> Result<String, InteropError> {
    let mut context = HandleAliasEncodeContext::default();
    encode_value_with_context(value, &mut context)
}

fn encode_value_with_context(
    value: &Value,
    context: &mut HandleAliasEncodeContext,
) -> Result<String, InteropError> {
    match value {
        Value::Scalar(number) => Ok(format!("S({number})")),
        Value::Complex(number) => Ok(format!("X({},{})", number.real, number.imag)),
        Value::Logical(flag) => Ok(format!("L({})", if *flag { "true" } else { "false" })),
        Value::CharArray(text) => Ok(format!("Q(char,{})", encode_string(text))),
        Value::String(text) => Ok(format!("Q(string,{})", encode_string(text))),
        Value::Matrix(matrix) => {
            encode_sequence_with_context("M", matrix.rows, matrix.cols, &matrix.elements, context)
        }
        Value::Cell(cell) => {
            encode_sequence_with_context("C", cell.rows, cell.cols, &cell.elements, context)
        }
        Value::Struct(struct_value) => {
            let mut fields = Vec::new();
            for (name, value) in struct_value.ordered_entries() {
                fields.push(format!(
                    "{}={}",
                    encode_string(name),
                    encode_value_with_context(value, context)?
                ));
            }
            Ok(format!("T([{}])", fields.join(",")))
        }
        Value::Object(object) => {
            let ancestors = object
                .class
                .ancestor_class_names
                .iter()
                .cloned()
                .map(Value::String)
                .collect::<Vec<_>>();
            let ancestors = Value::Cell(
                CellValue::new(1, ancestors.len(), ancestors)
                    .map_err(|error| InteropError::Unsupported(error.to_string()))?,
            );
            let properties = encode_value_with_context(&Value::Struct(object.properties()), context)?;
            let inline_methods = object
                .class
                .inline_methods
                .iter()
                .map(|name| encode_string(name))
                .collect::<Vec<_>>()
                .join(",");
            let private_properties = object
                .class
                .private_properties
                .iter()
                .map(|name| encode_string(name))
                .collect::<Vec<_>>()
                .join(",");
            let private_property_owners = object
                .class
                .private_property_owners
                .iter()
                .map(|(name, owner)| format!("{}=>{}", encode_string(name), encode_string(owner)))
                .collect::<Vec<_>>()
                .join(",");
            let private_inline_methods = object
                .class
                .private_inline_methods
                .iter()
                .map(|name| encode_string(name))
                .collect::<Vec<_>>()
                .join(",");
            let private_instance_method_owners = object
                .class
                .private_instance_method_owners
                .iter()
                .map(|(name, owner)| format!("{}=>{}", encode_string(name), encode_string(owner)))
                .collect::<Vec<_>>()
                .join(",");
            let private_static_inline_methods = object
                .class
                .private_static_inline_methods
                .iter()
                .map(|name| encode_string(name))
                .collect::<Vec<_>>()
                .join(",");
        let handle_id = context.handle_id_for_object(&object).unwrap_or_default();
            let external_methods = object
                .class
                .external_methods
                .iter()
                .map(|(name, target)| format!("{}=>{}", encode_string(name), encode_string(&encode_object_method_target(target))))
                .collect::<Vec<_>>()
                .join(",");
            Ok(format!(
                "O({},{},{},{},{},{},{},{},[{}],[{}],{},[{}],[{}],[{}],[{}],[{}],{})",
                encode_string(&object.class.class_name),
                encode_string(object.class.package.as_deref().unwrap_or("")),
                encode_string(object.class.superclass_name.as_deref().unwrap_or("")),
                encode_string(match object.class.storage_kind {
                    ObjectStorageKind::Value => "value",
                    ObjectStorageKind::Handle => "handle",
                }),
                encode_string(
                    &object
                        .class
                        .source_path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                ),
                encode_string(
                    &object
                        .class
                        .module_target
                        .as_ref()
                        .map(encode_object_method_target)
                        .unwrap_or_default()
                ),
                encode_string(object.class.constructor.as_deref().unwrap_or("")),
                encode_value(&ancestors)?,
                inline_methods,
                external_methods,
                properties,
                private_properties,
                private_inline_methods,
                private_static_inline_methods,
                private_property_owners,
                private_instance_method_owners,
                encode_string(&handle_id)
            ))
        }
        Value::FunctionHandle(handle) => match &handle.target {
            FunctionHandleTarget::Named(name) => Ok(format!(
                "H(named,{},{})",
                encode_string(&handle.display_name),
                encode_string(name)
            )),
            FunctionHandleTarget::ResolvedPath(path) => Ok(format!(
                "H(path,{},{})",
                encode_string(&handle.display_name),
                encode_string(&path.display().to_string())
            )),
            FunctionHandleTarget::BundleModule(module_id) => Ok(format!(
                "H(bundle,{},{})",
                encode_string(&handle.display_name),
                encode_string(module_id)
            )),
            FunctionHandleTarget::BoundMethod {
                class_name,
                package,
                method_name,
                receiver,
            } => Ok(format!(
                "H(bound,{},{},{},{},{})",
                encode_string(&handle.display_name),
                encode_string(class_name),
                encode_string(package.as_deref().unwrap_or("")),
                encode_string(method_name),
                encode_value_with_context(receiver, context)?
            )),
        },
    }
}

fn encode_sequence_with_context(
    prefix: &str,
    rows: usize,
    cols: usize,
    elements: &[Value],
    context: &mut HandleAliasEncodeContext,
) -> Result<String, InteropError> {
    let mut values = Vec::with_capacity(elements.len());
    for element in elements {
        values.push(encode_value_with_context(element, context)?);
    }
    Ok(format!("{prefix}({rows},{cols},[{}])", values.join(",")))
}

fn decode_value_with_context(
    source: &str,
    line_number: usize,
    context: &mut HandleAliasDecodeContext,
) -> Result<Value, InteropError> {
    let mut parser = Parser::new(source, line_number, context);
    let value = parser.parse_value()?;
    parser.expect_end()?;
    Ok(value)
}

fn encode_string(value: &str) -> String {
    let mut out = String::new();
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn decode_string(source: &str, line_number: usize) -> Result<String, InteropError> {
    let mut context = HandleAliasDecodeContext::default();
    let mut parser = Parser::new(source, line_number, &mut context);
    let value = parser.parse_string()?;
    parser.expect_end()?;
    Ok(value)
}

fn runtime_shape_error(error: RuntimeError, line_number: usize) -> InteropError {
    InteropError::Parse(format!("line {line_number}: {error}"))
}

struct Parser<'a, 'ctx> {
    chars: std::iter::Peekable<Chars<'a>>,
    line_number: usize,
    offset: usize,
    handle_aliases: &'ctx mut HandleAliasDecodeContext,
}

impl<'a, 'ctx> Parser<'a, 'ctx> {
    fn new(source: &'a str, line_number: usize, handle_aliases: &'ctx mut HandleAliasDecodeContext) -> Self {
        Self {
            chars: source.chars().peekable(),
            line_number,
            offset: 0,
            handle_aliases,
        }
    }

    fn parse_value(&mut self) -> Result<Value, InteropError> {
        match self.peek_char() {
            Some('S') => self.parse_scalar(),
            Some('X') => self.parse_complex(),
            Some('L') => self.parse_logical(),
            Some('Q') => self.parse_text(),
            Some('M') => self.parse_matrix(),
            Some('C') => self.parse_cell(),
            Some('T') => self.parse_struct(),
            Some('O') => self.parse_object(),
            Some('H') => self.parse_handle(),
            Some(other) => Err(self.error(format!("unexpected value prefix `{other}`"))),
            None => Err(self.error("unexpected end of input".to_string())),
        }
    }

    fn parse_scalar(&mut self) -> Result<Value, InteropError> {
        self.expect_char('S')?;
        self.expect_char('(')?;
        let number = self.parse_number()?;
        self.expect_char(')')?;
        Ok(Value::Scalar(number))
    }

    fn parse_complex(&mut self) -> Result<Value, InteropError> {
        self.expect_char('X')?;
        self.expect_char('(')?;
        let real = self.parse_number()?;
        self.expect_char(',')?;
        let imag = self.parse_number()?;
        self.expect_char(')')?;
        Ok(Value::Complex(ComplexValue { real, imag }))
    }

    fn parse_logical(&mut self) -> Result<Value, InteropError> {
        self.expect_char('L')?;
        self.expect_char('(')?;
        let token = self.parse_token_until(&[')'])?;
        self.expect_char(')')?;
        match token.as_str() {
            "true" => Ok(Value::Logical(true)),
            "false" => Ok(Value::Logical(false)),
            other => Err(self.error(format!("invalid logical literal `{other}`"))),
        }
    }

    fn parse_matrix(&mut self) -> Result<Value, InteropError> {
        self.parse_sequence_value('M', true)
    }

    fn parse_cell(&mut self) -> Result<Value, InteropError> {
        self.parse_sequence_value('C', false)
    }

    fn parse_text(&mut self) -> Result<Value, InteropError> {
        self.expect_char('Q')?;
        self.expect_char('(')?;
        let kind = self.parse_identifier()?;
        self.expect_char(',')?;
        let text = self.parse_string()?;
        self.expect_char(')')?;
        match kind.as_str() {
            "char" => Ok(Value::CharArray(text)),
            "string" => Ok(Value::String(text)),
            other => Err(self.error(format!("unsupported text value kind `{other}`"))),
        }
    }

    fn parse_sequence_value(&mut self, prefix: char, matrix: bool) -> Result<Value, InteropError> {
        self.expect_char(prefix)?;
        self.expect_char('(')?;
        let rows = self.parse_usize()?;
        self.expect_char(',')?;
        let cols = self.parse_usize()?;
        self.expect_char(',')?;
        self.expect_char('[')?;
        let mut elements = Vec::new();
        if self.peek_char() != Some(']') {
            loop {
                elements.push(self.parse_value()?);
                if self.peek_char() == Some(',') {
                    self.expect_char(',')?;
                    continue;
                }
                break;
            }
        }
        self.expect_char(']')?;
        self.expect_char(')')?;

        if matrix {
            MatrixValue::new(rows, cols, elements)
                .map(Value::Matrix)
                .map_err(|error| runtime_shape_error(error, self.line_number))
        } else {
            CellValue::new(rows, cols, elements)
                .map(Value::Cell)
                .map_err(|error| runtime_shape_error(error, self.line_number))
        }
    }

    fn parse_struct(&mut self) -> Result<Value, InteropError> {
        self.expect_char('T')?;
        self.expect_char('(')?;
        self.expect_char('[')?;
        let mut fields = std::collections::BTreeMap::new();
        let mut field_order = Vec::new();
        if self.peek_char() != Some(']') {
            loop {
                let name = self.parse_string()?;
                self.expect_char('=')?;
                let value = self.parse_value()?;
                field_order.push(name.clone());
                fields.insert(name, value);
                if self.peek_char() == Some(',') {
                    self.expect_char(',')?;
                    continue;
                }
                break;
            }
        }
        self.expect_char(']')?;
        self.expect_char(')')?;
        Ok(Value::Struct(StructValue::with_field_order(fields, field_order)))
    }

    fn parse_handle(&mut self) -> Result<Value, InteropError> {
        self.expect_char('H')?;
        self.expect_char('(')?;
        let kind = self.parse_identifier()?;
        self.expect_char(',')?;
        let display_name = self.parse_string()?;
        let target = match kind.as_str() {
            "named" => {
                self.expect_char(',')?;
                let raw_target = self.parse_string()?;
                self.expect_char(')')?;
                FunctionHandleTarget::Named(raw_target)
            }
            "path" => {
                self.expect_char(',')?;
                let raw_target = self.parse_string()?;
                self.expect_char(')')?;
                FunctionHandleTarget::ResolvedPath(raw_target.into())
            }
            "bundle" => {
                self.expect_char(',')?;
                let raw_target = self.parse_string()?;
                self.expect_char(')')?;
                FunctionHandleTarget::BundleModule(raw_target)
            }
            "bound" => {
                self.expect_char(',')?;
                let class_name = self.parse_string()?;
                self.expect_char(',')?;
                let package = self.parse_string()?;
                self.expect_char(',')?;
                let method_name = self.parse_string()?;
                self.expect_char(',')?;
                let receiver = self.parse_value()?;
                self.expect_char(')')?;
                FunctionHandleTarget::BoundMethod {
                    class_name,
                    package: (!package.is_empty()).then_some(package),
                    method_name,
                    receiver: Box::new(receiver),
                }
            }
            other => {
                return Err(self.error(format!("unsupported function handle target kind `{other}`")))
            }
        };
        Ok(Value::FunctionHandle(FunctionHandleValue {
            display_name,
            target,
        }))
    }

    fn parse_object(&mut self) -> Result<Value, InteropError> {
        self.expect_char('O')?;
        self.expect_char('(')?;
        let class_name = self.parse_string()?;
        self.expect_char(',')?;
        let package = self.parse_string()?;
        self.expect_char(',')?;
        let superclass = self.parse_string()?;
        self.expect_char(',')?;
        let storage = self.parse_string()?;
        self.expect_char(',')?;
        let source_path = self.parse_string()?;
        self.expect_char(',')?;
        let module_target = self.parse_string()?;
        self.expect_char(',')?;
        let constructor = self.parse_string()?;
        self.expect_char(',')?;
        let ancestors = self.parse_value()?;
        self.expect_char(',')?;
        let inline_methods = self.parse_string_list()?;
        self.expect_char(',')?;
        let external_methods = self.parse_string_pair_list()?;
        self.expect_char(',')?;
        let properties = self.parse_value()?;
        let mut private_properties = BTreeSet::new();
        let mut private_property_owners = BTreeMap::new();
        let mut private_inline_methods = BTreeSet::new();
        let mut private_instance_method_owners = BTreeMap::new();
        let mut private_static_inline_methods = BTreeSet::new();
        let mut handle_id = String::new();
        if self.peek_char() == Some(',') {
            self.expect_char(',')?;
            private_properties = self.parse_string_list()?.into_iter().collect();
            self.expect_char(',')?;
            private_inline_methods = self.parse_string_list()?.into_iter().collect();
            self.expect_char(',')?;
            private_static_inline_methods = self.parse_string_list()?.into_iter().collect();
            if self.peek_char() == Some(',') {
                self.expect_char(',')?;
                private_property_owners = self.parse_string_pair_list()?.into_iter().collect();
                self.expect_char(',')?;
                private_instance_method_owners =
                    self.parse_string_pair_list()?.into_iter().collect();
            }
            if self.peek_char() == Some(',') {
                self.expect_char(',')?;
                handle_id = self.parse_string()?;
            }
        }
        self.expect_char(')')?;

        let Value::Struct(properties) = properties else {
            return Err(self.error("object payload expected struct properties".to_string()));
        };
        let ancestor_class_names = decode_text_set(&ancestors)
            .map_err(|error| self.error(error.to_string()))?;

        let storage_kind = match storage.as_str() {
            "handle" => ObjectStorageKind::Handle,
            _ => ObjectStorageKind::Value,
        };
        let class = ObjectClassMetadata {
            class_name,
            package: (!package.is_empty()).then_some(package),
            superclass_name: (!superclass.is_empty()).then_some(superclass),
            ancestor_class_names,
            storage_kind,
            source_path: (!source_path.is_empty()).then_some(source_path.into()),
            module_target: (!module_target.is_empty())
                .then(|| decode_object_method_target(&module_target))
                .transpose()?,
            property_order: properties.field_names().to_vec(),
            private_properties,
            private_property_owners,
            inline_methods: inline_methods.into_iter().collect(),
            private_inline_methods,
            private_instance_method_owners,
            private_static_inline_methods,
            external_methods: external_methods
                .into_iter()
                .map(|(name, target)| {
                    let target =
                        decode_object_method_target(&target).expect("encoded object method target");
                    (name, target)
                })
                .collect(),
            constructor: (!constructor.is_empty()).then_some(constructor),
        };
        Ok(Value::Object(decode_object_with_alias_context(
            class,
            properties,
            (!handle_id.is_empty()).then_some(handle_id),
            self.handle_aliases,
        )))
    }

    fn parse_string_list(&mut self) -> Result<Vec<String>, InteropError> {
        self.expect_char('[')?;
        let mut values = Vec::new();
        if self.peek_char() != Some(']') {
            loop {
                values.push(self.parse_string()?);
                if self.peek_char() == Some(',') {
                    self.expect_char(',')?;
                    continue;
                }
                break;
            }
        }
        self.expect_char(']')?;
        Ok(values)
    }

    fn parse_string_pair_list(&mut self) -> Result<Vec<(String, String)>, InteropError> {
        self.expect_char('[')?;
        let mut values = Vec::new();
        if self.peek_char() != Some(']') {
            loop {
                let name = self.parse_string()?;
                self.expect_char('=')?;
                self.expect_char('>')?;
                let value = self.parse_string()?;
                values.push((name, value));
                if self.peek_char() == Some(',') {
                    self.expect_char(',')?;
                    continue;
                }
                break;
            }
        }
        self.expect_char(']')?;
        Ok(values)
    }

    fn parse_identifier(&mut self) -> Result<String, InteropError> {
        let mut out = String::new();
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                out.push(ch);
                self.next_char();
            } else {
                break;
            }
        }
        if out.is_empty() {
            return Err(self.error("expected identifier".to_string()));
        }
        Ok(out)
    }

    fn parse_string(&mut self) -> Result<String, InteropError> {
        self.expect_char('"')?;
        let mut out = String::new();
        loop {
            let Some(ch) = self.next_char() else {
                return Err(self.error("unterminated string literal".to_string()));
            };
            match ch {
                '"' => break,
                '\\' => {
                    let Some(escaped) = self.next_char() else {
                        return Err(self.error("unterminated escape sequence".to_string()));
                    };
                    match escaped {
                        '\\' => out.push('\\'),
                        '"' => out.push('"'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        other => {
                            return Err(
                                self.error(format!("unsupported escape sequence `\\{other}`"))
                            )
                        }
                    }
                }
                other => out.push(other),
            }
        }
        Ok(out)
    }

    fn parse_number(&mut self) -> Result<f64, InteropError> {
        let token = self.parse_token_until(&[',', ')'])?;
        token
            .parse::<f64>()
            .map_err(|error| self.error(format!("invalid numeric literal `{token}`: {error}")))
    }

    fn parse_usize(&mut self) -> Result<usize, InteropError> {
        let token = self.parse_token_until(&[',', ')'])?;
        token
            .parse::<usize>()
            .map_err(|error| self.error(format!("invalid integer literal `{token}`: {error}")))
    }

    fn parse_token_until(&mut self, delimiters: &[char]) -> Result<String, InteropError> {
        let mut token = String::new();
        while let Some(ch) = self.peek_char() {
            if delimiters.contains(&ch) {
                break;
            }
            token.push(ch);
            self.next_char();
        }
        if token.is_empty() {
            return Err(self.error("expected token".to_string()));
        }
        Ok(token)
    }

    fn expect_char(&mut self, expected: char) -> Result<(), InteropError> {
        match self.next_char() {
            Some(ch) if ch == expected => Ok(()),
            Some(ch) => Err(self.error(format!("expected `{expected}`, found `{ch}`"))),
            None => Err(self.error(format!("expected `{expected}`, found end of input"))),
        }
    }

    fn expect_end(&mut self) -> Result<(), InteropError> {
        if self.peek_char().is_some() {
            return Err(self.error("unexpected trailing input".to_string()));
        }
        Ok(())
    }

    fn peek_char(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }

    fn error(&self, message: String) -> InteropError {
        InteropError::Parse(format!(
            "line {} column {}: {}",
            self.line_number,
            self.offset + 1,
            message
        ))
    }
}

