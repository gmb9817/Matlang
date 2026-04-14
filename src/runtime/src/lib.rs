//! Runtime crate for executable MATLAB semantics.

use std::{collections::BTreeMap, error::Error, fmt, path::PathBuf};

pub const CRATE_NAME: &str = "matlab-runtime";

pub type Workspace = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq)]
pub struct ComplexValue {
    pub real: f64,
    pub imag: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Scalar(f64),
    Complex(ComplexValue),
    Logical(bool),
    CharArray(String),
    String(String),
    Matrix(MatrixValue),
    Cell(CellValue),
    Struct(StructValue),
    FunctionHandle(FunctionHandleValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayStorageClass {
    Numeric,
    Logical,
    Complex,
    String,
    Generic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatrixValue {
    pub rows: usize,
    pub cols: usize,
    pub dims: Vec<usize>,
    pub elements: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CellValue {
    pub rows: usize,
    pub cols: usize,
    pub dims: Vec<usize>,
    pub elements: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StructValue {
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionHandleValue {
    pub display_name: String,
    pub target: FunctionHandleTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionHandleTarget {
    Named(String),
    ResolvedPath(PathBuf),
    BundleModule(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStackFrame {
    pub file: String,
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    Unsupported(String),
    TypeError(String),
    MissingVariable(String),
    InvalidIndex(String),
    ShapeError(String),
    Captured {
        source: Box<RuntimeError>,
        stack: Vec<RuntimeStackFrame>,
    },
    UserDefined {
        identifier: String,
        message: String,
        stack: Vec<RuntimeStackFrame>,
        cause: Vec<RuntimeError>,
    },
    ThrowAsCaller {
        source: Box<RuntimeError>,
    },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message)
            | Self::TypeError(message)
            | Self::MissingVariable(message)
            | Self::InvalidIndex(message)
            | Self::ShapeError(message) => f.write_str(message),
            Self::Captured { source, .. } => source.fmt(f),
            Self::UserDefined { message, .. } => f.write_str(message),
            Self::ThrowAsCaller { source } => source.fmt(f),
        }
    }
}

impl Error for RuntimeError {}

impl RuntimeError {
    pub fn identifier(&self) -> &str {
        match self {
            Self::Unsupported(_) => "MATC:Unsupported",
            Self::TypeError(_) => "MATC:TypeError",
            Self::MissingVariable(_) => "MATC:MissingVariable",
            Self::InvalidIndex(_) => "MATC:InvalidIndex",
            Self::ShapeError(_) => "MATC:ShapeError",
            Self::Captured { source, .. } => source.identifier(),
            Self::UserDefined { identifier, .. } => identifier,
            Self::ThrowAsCaller { source } => source.identifier(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Unsupported(message)
            | Self::TypeError(message)
            | Self::MissingVariable(message)
            | Self::InvalidIndex(message)
            | Self::ShapeError(message) => message,
            Self::Captured { source, .. } => source.message(),
            Self::UserDefined { message, .. } => message,
            Self::ThrowAsCaller { source } => source.message(),
        }
    }

    pub fn stack(&self) -> &[RuntimeStackFrame] {
        match self {
            Self::Captured { stack, .. } => stack,
            Self::UserDefined { stack, .. } => stack,
            Self::ThrowAsCaller { source } => source.stack(),
            _ => &[],
        }
    }

    pub fn causes(&self) -> &[RuntimeError] {
        match self {
            Self::Captured { source, .. } => source.causes(),
            Self::UserDefined { cause, .. } => cause,
            Self::ThrowAsCaller { source } => source.causes(),
            _ => &[],
        }
    }

    pub fn with_cause(self, cause: RuntimeError) -> Self {
        match self {
            Self::Captured { source, stack } => Self::Captured {
                source: Box::new(source.with_cause(cause)),
                stack,
            },
            Self::UserDefined {
                identifier,
                message,
                stack,
                cause: mut causes,
            } => {
                causes.push(cause);
                Self::UserDefined {
                    identifier,
                    message,
                    stack,
                    cause: causes,
                }
            }
            Self::ThrowAsCaller { source } => Self::ThrowAsCaller {
                source: Box::new(source.with_cause(cause)),
            },
            other => other,
        }
    }

    pub fn clear_stack(self) -> Self {
        match self {
            Self::Captured { source, .. } => source.clear_stack(),
            Self::UserDefined {
                identifier,
                message,
                cause,
                ..
            } => Self::UserDefined {
                identifier,
                message,
                stack: Vec::new(),
                cause,
            },
            Self::ThrowAsCaller { source } => Self::ThrowAsCaller {
                source: Box::new(source.clear_stack()),
            },
            other => other,
        }
    }

    pub fn throw_as_caller(self) -> Self {
        Self::ThrowAsCaller {
            source: Box::new(self),
        }
    }

    pub fn capture_stack(self, stack: Vec<RuntimeStackFrame>) -> Self {
        match self {
            Self::Captured { .. } => self,
            Self::UserDefined {
                stack: ref existing,
                ..
            } if !existing.is_empty() => self,
            Self::ThrowAsCaller { source } => {
                let mut adjusted = stack;
                adjusted.pop();
                source.clear_stack().capture_stack(adjusted)
            }
            other if stack.is_empty() => other,
            other => Self::Captured {
                source: Box::new(other),
                stack,
            },
        }
    }
}

impl Value {
    pub fn as_scalar(&self) -> Result<f64, RuntimeError> {
        match self {
            Self::Scalar(value) => Ok(*value),
            Self::Logical(value) => Ok(if *value { 1.0 } else { 0.0 }),
            Self::Matrix(matrix) if matrix.rows == 1 && matrix.cols == 1 => {
                matrix.elements[0].as_scalar()
            }
            _ => Err(RuntimeError::TypeError(format!(
                "expected scalar value, found {}",
                self.kind_name()
            ))),
        }
    }

    pub fn truthy(&self) -> Result<bool, RuntimeError> {
        Ok(self.as_scalar()? != 0.0)
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Scalar(_) => "scalar",
            Self::Complex(_) => "complex",
            Self::Logical(_) => "logical",
            Self::CharArray(_) => "char",
            Self::String(_) => "string",
            Self::Matrix(_) => "matrix",
            Self::Cell(_) => "cell",
            Self::Struct(_) => "struct",
            Self::FunctionHandle(_) => "function_handle",
        }
    }
}

impl MatrixValue {
    pub fn new(rows: usize, cols: usize, elements: Vec<Value>) -> Result<Self, RuntimeError> {
        Self::with_dimensions(rows, cols, vec![rows, cols], elements)
    }

    pub fn with_dimensions(
        rows: usize,
        cols: usize,
        dims: Vec<usize>,
        elements: Vec<Value>,
    ) -> Result<Self, RuntimeError> {
        if rows * cols != elements.len() {
            return Err(RuntimeError::ShapeError(format!(
                "matrix shape {}x{} does not match {} elements",
                rows,
                cols,
                elements.len()
            )));
        }

        let dims = normalize_dimensions(rows, cols, dims)?;

        Ok(Self {
            rows,
            cols,
            dims,
            elements,
        })
    }

    pub fn from_rows(rows: Vec<Vec<Value>>) -> Result<Self, RuntimeError> {
        let row_count = rows.len();
        let col_count = rows.first().map(|row| row.len()).unwrap_or(0);
        if rows.iter().any(|row| row.len() != col_count) {
            return Err(RuntimeError::ShapeError(
                "matrix rows must have consistent column counts".to_string(),
            ));
        }

        let elements = rows.into_iter().flatten().collect::<Vec<_>>();
        Self::new(row_count, col_count, elements)
    }

    pub fn filled(rows: usize, cols: usize, value: Value) -> Self {
        Self {
            rows,
            cols,
            dims: vec![rows, cols],
            elements: vec![value; rows * cols],
        }
    }

    pub fn dims(&self) -> &[usize] {
        &self.dims
    }

    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    pub fn elements(&self) -> &[Value] {
        &self.elements
    }

    pub fn elements_mut(&mut self) -> &mut [Value] {
        &mut self.elements
    }

    pub fn into_elements(self) -> Vec<Value> {
        self.elements
    }

    pub fn storage_class(&self) -> ArrayStorageClass {
        let mut saw_complex = false;
        let mut saw_other = false;

        for element in self.elements() {
            match element {
                Value::Scalar(_) => {}
                Value::Logical(_) => {
                    return if self
                        .elements()
                        .iter()
                        .all(|value| matches!(value, Value::Logical(_)))
                    {
                        ArrayStorageClass::Logical
                    } else {
                        ArrayStorageClass::Generic
                    };
                }
                Value::Complex(_) => saw_complex = true,
                Value::String(_) => {
                    return if self
                        .elements()
                        .iter()
                        .all(|value| matches!(value, Value::String(_)))
                    {
                        ArrayStorageClass::String
                    } else {
                        ArrayStorageClass::Generic
                    };
                }
                _ => saw_other = true,
            }
        }

        if saw_other {
            ArrayStorageClass::Generic
        } else if saw_complex {
            ArrayStorageClass::Complex
        } else {
            ArrayStorageClass::Numeric
        }
    }

    pub fn scalar_elements(&self) -> Result<Vec<f64>, RuntimeError> {
        self.elements()
            .iter()
            .map(Value::as_scalar)
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn get(&self, row: usize, col: usize) -> &Value {
        &self.elements[row * self.cols + col]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Value> {
        self.elements.iter()
    }
}

impl CellValue {
    pub fn new(rows: usize, cols: usize, elements: Vec<Value>) -> Result<Self, RuntimeError> {
        Self::with_dimensions(rows, cols, vec![rows, cols], elements)
    }

    pub fn with_dimensions(
        rows: usize,
        cols: usize,
        dims: Vec<usize>,
        elements: Vec<Value>,
    ) -> Result<Self, RuntimeError> {
        if rows * cols != elements.len() {
            return Err(RuntimeError::ShapeError(format!(
                "cell shape {}x{} does not match {} elements",
                rows,
                cols,
                elements.len()
            )));
        }

        let dims = normalize_dimensions(rows, cols, dims)?;

        Ok(Self {
            rows,
            cols,
            dims,
            elements,
        })
    }

    pub fn from_rows(rows: Vec<Vec<Value>>) -> Result<Self, RuntimeError> {
        let row_count = rows.len();
        let col_count = rows.first().map(|row| row.len()).unwrap_or(0);
        if rows.iter().any(|row| row.len() != col_count) {
            return Err(RuntimeError::ShapeError(
                "cell rows must have consistent column counts".to_string(),
            ));
        }

        let elements = rows.into_iter().flatten().collect::<Vec<_>>();
        Self::new(row_count, col_count, elements)
    }

    pub fn dims(&self) -> &[usize] {
        &self.dims
    }

    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    pub fn elements(&self) -> &[Value] {
        &self.elements
    }

    pub fn elements_mut(&mut self) -> &mut [Value] {
        &mut self.elements
    }

    pub fn into_elements(self) -> Vec<Value> {
        self.elements
    }

    pub fn get(&self, row: usize, col: usize) -> &Value {
        &self.elements[row * self.cols + col]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Value> {
        self.elements.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::{ArrayStorageClass, CellValue, ComplexValue, MatrixValue, Value};

    #[test]
    fn matrix_with_dimensions_preserves_explicit_metadata() {
        let matrix = MatrixValue::with_dimensions(
            2,
            2,
            vec![2, 2, 1],
            vec![
                Value::Scalar(1.0),
                Value::Scalar(2.0),
                Value::Scalar(3.0),
                Value::Scalar(4.0),
            ],
        )
        .expect("matrix");
        assert_eq!(matrix.dims, vec![2, 2, 1]);
    }

    #[test]
    fn cell_with_dimensions_preserves_explicit_metadata() {
        let cell = CellValue::with_dimensions(
            1,
            2,
            vec![1, 2, 1],
            vec![Value::Scalar(1.0), Value::Scalar(2.0)],
        )
        .expect("cell");
        assert_eq!(cell.dims, vec![1, 2, 1]);
    }

    #[test]
    fn matrix_storage_class_tracks_homogeneous_array_identity() {
        let logical = MatrixValue::new(
            1,
            3,
            vec![Value::Logical(true), Value::Logical(false), Value::Logical(true)],
        )
        .expect("logical matrix");
        assert_eq!(logical.storage_class(), ArrayStorageClass::Logical);

        let complex = MatrixValue::new(
            1,
            2,
            vec![
                Value::Scalar(1.0),
                Value::Complex(ComplexValue {
                    real: 2.0,
                    imag: 3.0,
                }),
            ],
        )
        .expect("complex matrix");
        assert_eq!(complex.storage_class(), ArrayStorageClass::Complex);

        let strings = MatrixValue::new(
            1,
            2,
            vec![Value::String("a".to_string()), Value::String("b".to_string())],
        )
        .expect("string matrix");
        assert_eq!(strings.storage_class(), ArrayStorageClass::String);

        let generic = MatrixValue::new(
            1,
            2,
            vec![Value::Scalar(1.0), Value::String("b".to_string())],
        )
        .expect("generic matrix");
        assert_eq!(generic.storage_class(), ArrayStorageClass::Generic);
    }

    #[test]
    fn matrix_and_cell_accessors_expose_shape_and_storage_views() {
        let mut matrix = MatrixValue::new(1, 2, vec![Value::Scalar(1.0), Value::Scalar(2.0)])
            .expect("matrix");
        assert_eq!(matrix.dims(), &[1, 2]);
        assert_eq!(matrix.element_count(), 2);
        matrix.elements_mut()[1] = Value::Scalar(4.0);
        assert_eq!(matrix.elements()[1], Value::Scalar(4.0));

        let mut cell = CellValue::new(1, 2, vec![Value::Scalar(1.0), Value::Scalar(2.0)])
            .expect("cell");
        assert_eq!(cell.dims(), &[1, 2]);
        assert_eq!(cell.element_count(), 2);
        cell.elements_mut()[0] = Value::String("x".to_string());
        assert_eq!(cell.elements()[0], Value::String("x".to_string()));
    }
}

fn normalize_dimensions(
    rows: usize,
    cols: usize,
    mut dims: Vec<usize>,
) -> Result<Vec<usize>, RuntimeError> {
    if dims.is_empty() {
        dims = vec![rows, cols];
    }
    if dims.len() == 1 {
        dims.push(1);
    }
    let expected_elements = dims.iter().product::<usize>();
    if rows * cols != expected_elements {
        return Err(RuntimeError::ShapeError(format!(
            "dimension metadata {:?} does not agree with storage shape {}x{}",
            dims, rows, cols
        )));
    }
    Ok(dims)
}

pub fn render_workspace(workspace: &Workspace) -> String {
    let mut out = String::new();
    out.push_str("workspace\n");
    for (name, value) in workspace {
        render_named_value(&mut out, "  ", name, value);
    }
    out
}

pub fn render_value(value: &Value) -> String {
    match value {
        Value::Scalar(number) => render_number(*number),
        Value::Complex(number) => render_complex(number),
        Value::Logical(flag) => {
            if *flag {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::CharArray(text) => render_quoted_text(text, '\''),
        Value::String(text) => render_quoted_text(text, '"'),
        Value::Matrix(matrix) => render_matrix_inline(matrix, None),
        Value::Cell(cell) => render_cell_inline(cell, None),
        Value::Struct(struct_value) => {
            let fields = struct_value
                .fields
                .iter()
                .map(|(name, value)| format!("{name}={}", render_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("struct{{{fields}}}")
        }
        Value::FunctionHandle(handle) => format!("@{}", handle.display_name),
    }
}

pub fn render_named_value(out: &mut String, indent: &str, name: &str, value: &Value) {
    match value {
        Value::Matrix(matrix) => {
            if let Some(page_dims) = paged_tail_dimensions(&matrix.dims) {
                if page_dims.iter().product::<usize>() == 0 {
                    out.push_str(&format!("{indent}{name} = []\n"));
                    return;
                }
                for page in 0..page_dims.iter().product::<usize>() {
                    let page_index = column_major_multi_index(page, &page_dims);
                    let label = page_index
                        .iter()
                        .map(|index| (index + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "{indent}{name}(:,:,{label}) = {}\n",
                        render_matrix_inline(matrix, Some(&page_index))
                    ));
                }
            } else {
                out.push_str(&format!(
                    "{indent}{name} = {}\n",
                    render_matrix_inline(matrix, None)
                ));
            }
        }
        Value::Cell(cell) => {
            if let Some(page_dims) = paged_tail_dimensions(&cell.dims) {
                if page_dims.iter().product::<usize>() == 0 {
                    out.push_str(&format!("{indent}{name} = {{}}\n"));
                    return;
                }
                for page in 0..page_dims.iter().product::<usize>() {
                    let page_index = column_major_multi_index(page, &page_dims);
                    let label = page_index
                        .iter()
                        .map(|index| (index + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "{indent}{name}(:,:,{label}) = {}\n",
                        render_cell_inline(cell, Some(&page_index))
                    ));
                }
            } else {
                out.push_str(&format!(
                    "{indent}{name} = {}\n",
                    render_cell_inline(cell, None)
                ));
            }
        }
        _ => out.push_str(&format!("{indent}{name} = {}\n", render_value(value))),
    }
}

fn render_matrix_inline(matrix: &MatrixValue, tail_index: Option<&[usize]>) -> String {
    let rows = matrix.dims.first().copied().unwrap_or(matrix.rows);
    let cols = matrix.dims.get(1).copied().unwrap_or(matrix.cols);
    if rows == 0 || cols == 0 {
        return "[]".to_string();
    }
    let rows = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let mut index = vec![row, col];
                    if let Some(tail_index) = tail_index {
                        index.extend_from_slice(tail_index);
                    }
                    let linear = row_major_linear_index(&index, &matrix.dims);
                    render_value(&matrix.elements[linear])
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect::<Vec<_>>()
        .join(" ; ");
    format!("[{rows}]")
}

fn render_cell_inline(cell: &CellValue, tail_index: Option<&[usize]>) -> String {
    let rows = cell.dims.first().copied().unwrap_or(cell.rows);
    let cols = cell.dims.get(1).copied().unwrap_or(cell.cols);
    if rows == 0 || cols == 0 {
        return "{}".to_string();
    }
    let rows = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let mut index = vec![row, col];
                    if let Some(tail_index) = tail_index {
                        index.extend_from_slice(tail_index);
                    }
                    let linear = row_major_linear_index(&index, &cell.dims);
                    render_value(&cell.elements[linear])
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect::<Vec<_>>()
        .join(" ; ");
    format!("{{{rows}}}")
}

fn paged_tail_dimensions(dims: &[usize]) -> Option<Vec<usize>> {
    let mut canonical = dims.to_vec();
    while canonical.len() > 2 && canonical.last() == Some(&1) {
        canonical.pop();
    }
    (canonical.len() > 2).then(|| canonical[2..].to_vec())
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

fn render_complex(number: &ComplexValue) -> String {
    if number.imag == 0.0 {
        return render_number(number.real);
    }
    if number.real == 0.0 {
        return render_imaginary(number.imag);
    }

    let sign = if number.imag.is_sign_negative() {
        "-"
    } else {
        "+"
    };
    let imag = render_number(number.imag.abs());
    format!("{} {} {}i", render_number(number.real), sign, imag)
}

fn render_imaginary(number: f64) -> String {
    format!("{}i", render_number(number))
}

fn render_number(number: f64) -> String {
    if number.fract() == 0.0 {
        format!("{number:.0}")
    } else {
        number.to_string()
    }
}

fn render_quoted_text(text: &str, delimiter: char) -> String {
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

pub fn summary() -> &'static str {
    "Owns value representation, arrays, memory, invocation, IO, and errors."
}
