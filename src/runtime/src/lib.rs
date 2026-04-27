//! Runtime crate for executable MATLAB semantics.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

pub const CRATE_NAME: &str = "matlab-runtime";

static NEXT_HANDLE_OBJECT_ID: AtomicU64 = AtomicU64::new(1);

pub type Workspace = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq)]
pub struct ComplexValue {
    pub real: f64,
    pub imag: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Scalar(f64),
    Int64(i64),
    UInt64(u64),
    Complex(ComplexValue),
    Logical(bool),
    CharArray(String),
    String(String),
    Matrix(MatrixValue),
    Cell(CellValue),
    Struct(StructValue),
    Object(ObjectValue),
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

#[derive(Debug, Clone, Default)]
pub struct StructValue {
    pub fields: BTreeMap<String, Value>,
    pub field_order: Vec<String>,
}

impl PartialEq for StructValue {
    fn eq(&self, other: &Self) -> bool {
        self.fields == other.fields
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectStorageKind {
    Value,
    Handle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectClassMetadata {
    pub class_name: String,
    pub package: Option<String>,
    pub superclass_name: Option<String>,
    pub ancestor_class_names: BTreeSet<String>,
    pub storage_kind: ObjectStorageKind,
    pub source_path: Option<PathBuf>,
    pub module_target: Option<ObjectMethodTarget>,
    pub property_order: Vec<String>,
    pub private_properties: BTreeSet<String>,
    pub private_property_owners: BTreeMap<String, String>,
    pub inline_methods: BTreeSet<String>,
    pub private_inline_methods: BTreeSet<String>,
    pub private_instance_method_owners: BTreeMap<String, String>,
    pub private_static_inline_methods: BTreeSet<String>,
    pub external_methods: BTreeMap<String, ObjectMethodTarget>,
    pub constructor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectMethodTarget {
    Path(PathBuf),
    BundleModule(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectInstance {
    pub properties: StructValue,
}

#[derive(Debug, Clone)]
pub enum ObjectStorage {
    Value(ObjectInstance),
    Handle {
        id: u64,
        shared: Rc<RefCell<ObjectInstance>>,
    },
}

#[derive(Debug, Clone)]
pub struct ObjectValue {
    pub class: ObjectClassMetadata,
    pub storage: ObjectStorage,
}

pub fn qualified_object_class_name(class_name: &str, package: Option<&str>) -> String {
    match package {
        Some(package) if !package.is_empty() => format!("{package}.{class_name}"),
        _ => class_name.to_string(),
    }
}

pub fn observe_handle_object_id(id: u64) {
    NEXT_HANDLE_OBJECT_ID.fetch_max(id.saturating_add(1), Ordering::Relaxed);
}

fn validate_matrix_object_elements(elements: &[Value]) -> Result<(), RuntimeError> {
    let mut first_object_class = None::<String>;
    let mut saw_non_object = false;
    for element in elements {
        match element {
            Value::Object(object) => {
                let class_name = object.class.qualified_name();
                if saw_non_object {
                    return Err(RuntimeError::TypeError(format!(
                        "matrix values currently do not support mixing object elements of class `{class_name}` with non-object elements; use cell arrays for heterogeneous containers"
                    )));
                }
                if let Some(first) = &first_object_class {
                    if !class_name.eq_ignore_ascii_case(first) {
                        return Err(RuntimeError::TypeError(format!(
                            "matrix values currently require object elements to have the same class; found `{first}` and `{class_name}`; use cell arrays for heterogeneous objects"
                        )));
                    }
                } else {
                    first_object_class = Some(class_name);
                }
            }
            _ => {
                if let Some(first) = &first_object_class {
                    return Err(RuntimeError::TypeError(format!(
                        "matrix values currently do not support mixing object elements of class `{first}` with non-object elements; use cell arrays for heterogeneous containers"
                    )));
                }
                saw_non_object = true;
            }
        }
    }
    Ok(())
}

impl ObjectClassMetadata {
    pub fn qualified_name(&self) -> String {
        qualified_object_class_name(&self.class_name, self.package.as_deref())
    }

    pub fn private_property_owner(&self, name: &str) -> Option<String> {
        self.private_property_owners.get(name).cloned().or_else(|| {
            self.private_properties
                .contains(name)
                .then(|| self.qualified_name())
        })
    }

    pub fn private_instance_method_owner(&self, name: &str) -> Option<String> {
        self.private_instance_method_owners
            .get(name)
            .cloned()
            .or_else(|| {
                self.private_inline_methods
                    .contains(name)
                    .then(|| self.qualified_name())
            })
    }
}

impl PartialEq for ObjectValue {
    fn eq(&self, other: &Self) -> bool {
        self.class == other.class && self.properties() == other.properties()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionHandleValue {
    pub display_name: String,
    pub target: FunctionHandleTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FunctionHandleTarget {
    Named(String),
    ResolvedPath(PathBuf),
    BundleModule(String),
    BoundMethod {
        class_name: String,
        package: Option<String>,
        method_name: String,
        receiver: Box<Value>,
    },
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
            Self::Int64(value) => Ok(*value as f64),
            Self::UInt64(value) => Ok(*value as f64),
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
            Self::Int64(_) => "int64",
            Self::UInt64(_) => "uint64",
            Self::Complex(_) => "complex",
            Self::Logical(_) => "logical",
            Self::CharArray(_) => "char",
            Self::String(_) => "string",
            Self::Matrix(_) => "matrix",
            Self::Cell(_) => "cell",
            Self::Struct(_) => "struct",
            Self::Object(_) => "object",
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

        validate_matrix_object_elements(&elements)?;
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
                Value::Scalar(_) | Value::Int64(_) | Value::UInt64(_) => {}
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

impl StructValue {
    pub fn from_fields(fields: BTreeMap<String, Value>) -> Self {
        let field_order = fields.keys().cloned().collect();
        Self {
            fields,
            field_order,
        }
    }

    pub fn with_field_order(fields: BTreeMap<String, Value>, field_order: Vec<String>) -> Self {
        let mut normalized_order = Vec::with_capacity(fields.len());
        let mut seen = BTreeSet::new();
        for name in field_order {
            if fields.contains_key(&name) && seen.insert(name.clone()) {
                normalized_order.push(name);
            }
        }
        for name in fields.keys() {
            if seen.insert(name.clone()) {
                normalized_order.push(name.clone());
            }
        }
        Self {
            fields,
            field_order: normalized_order,
        }
    }

    pub fn field_names(&self) -> &[String] {
        &self.field_order
    }

    pub fn ordered_entries(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.field_order.iter().filter_map(|name| {
            self.fields
                .get_key_value(name)
                .map(|(field_name, value)| (field_name.as_str(), value))
        })
    }

    pub fn ordered_values(&self) -> impl Iterator<Item = &Value> {
        self.field_order
            .iter()
            .filter_map(|name| self.fields.get(name))
    }

    pub fn insert_field(&mut self, name: String, value: Value) {
        if !self.fields.contains_key(&name) {
            self.field_order.push(name.clone());
        }
        self.fields.insert(name, value);
    }

    pub fn remove_field(&mut self, name: &str) -> Option<Value> {
        let removed = self.fields.remove(name);
        if removed.is_some() {
            self.field_order.retain(|existing| existing != name);
        }
        removed
    }
}

impl ObjectValue {
    pub fn new(class: ObjectClassMetadata, properties: StructValue) -> Self {
        let storage = match class.storage_kind {
            ObjectStorageKind::Value => ObjectStorage::Value(ObjectInstance { properties }),
            ObjectStorageKind::Handle => ObjectStorage::Handle {
                id: NEXT_HANDLE_OBJECT_ID.fetch_add(1, Ordering::Relaxed),
                shared: Rc::new(RefCell::new(ObjectInstance { properties })),
            },
        };
        Self { class, storage }
    }

    pub fn from_storage(class: ObjectClassMetadata, storage: ObjectStorage) -> Self {
        Self { class, storage }
    }

    pub fn storage_kind(&self) -> ObjectStorageKind {
        self.class.storage_kind
    }

    pub fn handle_id(&self) -> Option<u64> {
        match &self.storage {
            ObjectStorage::Handle { id, .. } => Some(*id),
            ObjectStorage::Value(_) => None,
        }
    }

    pub fn properties(&self) -> StructValue {
        match &self.storage {
            ObjectStorage::Value(instance) => instance.properties.clone(),
            ObjectStorage::Handle { shared, .. } => shared.borrow().properties.clone(),
        }
    }

    pub fn property_value(&self, name: &str) -> Option<Value> {
        match &self.storage {
            ObjectStorage::Value(instance) => instance.properties.fields.get(name).cloned(),
            ObjectStorage::Handle { shared, .. } => {
                shared.borrow().properties.fields.get(name).cloned()
            }
        }
    }

    pub fn set_property_value(&mut self, name: &str, value: Value) -> Result<(), RuntimeError> {
        match &mut self.storage {
            ObjectStorage::Value(instance) => {
                instance.properties.insert_field(name.to_string(), value);
            }
            ObjectStorage::Handle { shared, .. } => {
                shared
                    .borrow_mut()
                    .properties
                    .insert_field(name.to_string(), value);
            }
        }
        Ok(())
    }

    pub fn remove_property_value(&mut self, name: &str) -> Option<Value> {
        match &mut self.storage {
            ObjectStorage::Value(instance) => instance.properties.remove_field(name),
            ObjectStorage::Handle { shared, .. } => {
                shared.borrow_mut().properties.remove_field(name)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArrayStorageClass, CellValue, ComplexValue, MatrixValue, ObjectClassMetadata,
        ObjectStorageKind, ObjectValue, StructValue, Value,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn test_object(class_name: &str) -> Value {
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
    }

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
            vec![
                Value::Logical(true),
                Value::Logical(false),
                Value::Logical(true),
            ],
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
            vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
            ],
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
    fn matrix_rejects_mixed_object_classes() {
        let error = MatrixValue::new(1, 2, vec![test_object("Point"), test_object("Other")])
            .expect_err("mixed object classes should be rejected");
        assert!(
            error
                .to_string()
                .contains("matrix values currently require object elements to have the same class"),
            "{error}"
        );
    }

    #[test]
    fn matrix_rejects_mixed_object_and_non_object_elements() {
        let error = MatrixValue::new(1, 2, vec![test_object("Point"), Value::Scalar(1.0)])
            .expect_err("mixed object and scalar elements should be rejected");
        assert!(
            error
                .to_string()
                .contains("matrix values currently do not support mixing object elements"),
            "{error}"
        );
    }

    #[test]
    fn matrix_and_cell_accessors_expose_shape_and_storage_views() {
        let mut matrix =
            MatrixValue::new(1, 2, vec![Value::Scalar(1.0), Value::Scalar(2.0)]).expect("matrix");
        assert_eq!(matrix.dims(), &[1, 2]);
        assert_eq!(matrix.element_count(), 2);
        matrix.elements_mut()[1] = Value::Scalar(4.0);
        assert_eq!(matrix.elements()[1], Value::Scalar(4.0));

        let mut cell =
            CellValue::new(1, 2, vec![Value::Scalar(1.0), Value::Scalar(2.0)]).expect("cell");
        assert_eq!(cell.dims(), &[1, 2]);
        assert_eq!(cell.element_count(), 2);
        cell.elements_mut()[0] = Value::String("x".to_string());
        assert_eq!(cell.elements()[0], Value::String("x".to_string()));
    }

    #[test]
    fn struct_value_preserves_explicit_field_order_and_mutation_order() {
        let fields = BTreeMap::from([
            ("a".to_string(), Value::Scalar(1.0)),
            ("b".to_string(), Value::Scalar(2.0)),
        ]);
        let mut struct_value =
            StructValue::with_field_order(fields, vec!["b".to_string(), "a".to_string()]);
        assert_eq!(
            struct_value.field_names(),
            &["b".to_string(), "a".to_string()]
        );
        assert_eq!(
            struct_value
                .ordered_entries()
                .map(|(name, _)| name.to_string())
                .collect::<Vec<_>>(),
            vec!["b".to_string(), "a".to_string()]
        );

        struct_value.insert_field("c".to_string(), Value::Scalar(3.0));
        assert_eq!(
            struct_value.field_names(),
            &["b".to_string(), "a".to_string(), "c".to_string()]
        );

        struct_value.remove_field("a");
        assert_eq!(
            struct_value.field_names(),
            &["b".to_string(), "c".to_string()]
        );
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

pub fn render_special_display_value(value: &Value) -> Option<String> {
    special_object_display_lines(value).map(|lines| lines.join("\n"))
}

pub fn render_value(value: &Value) -> String {
    match value {
        Value::Scalar(number) => render_number(*number),
        Value::Int64(number) => number.to_string(),
        Value::UInt64(number) => number.to_string(),
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
                .ordered_entries()
                .map(|(name, value)| format!("{name}={}", render_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("struct{{{fields}}}")
        }
        Value::Object(object) => format!(
            "{} with properties {{{}}}",
            object.class.qualified_name(),
            object
                .properties()
                .ordered_entries()
                .map(|(name, value)| format!("{name}={}", render_value(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::FunctionHandle(handle) => format!("@{}", handle.display_name),
    }
}

pub fn render_named_value(out: &mut String, indent: &str, name: &str, value: &Value) {
    if let Some(lines) = special_object_display_lines(value) {
        out.push_str(&format!("{indent}{name} = {}", lines[0]));
        out.push('\n');
        for line in lines.iter().skip(1) {
            if line.is_empty() {
                out.push('\n');
            } else {
                out.push_str(indent);
                out.push_str(line);
                out.push('\n');
            }
        }
        return;
    }
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

fn special_object_display_lines(value: &Value) -> Option<Vec<String>> {
    let Value::Object(object) = value else {
        return None;
    };
    if !object
        .class
        .qualified_name()
        .eq_ignore_ascii_case("digitalFilter")
    {
        return None;
    }
    let properties = object.properties();
    let stage_names = properties
        .field_order
        .iter()
        .filter(|name| name.starts_with("Stage"))
        .cloned()
        .collect::<Vec<_>>();
    if stage_names.is_empty() {
        let mut lines = vec![
            "digitalFilter with properties:".to_string(),
            String::new(),
            "  Coefficients:".to_string(),
        ];
        for name in ["Numerator", "Denominator", "ScaleValues"] {
            if let Some(value) = properties.fields.get(name) {
                lines.push(format!("    {name}: {}", render_value(value)));
            }
        }
        lines.push(String::new());
        lines.push("  Specifications:".to_string());
        if let Some(value) = properties.fields.get("NormalizedFrequency") {
            lines.push(format!(
                "    NormalizedFrequency: {}",
                render_object_property_value(value)
            ));
        }
        if let Some(value) = properties.fields.get("SampleRate") {
            lines.push(format!(
                "    SampleRate: {}",
                render_object_property_value(value)
            ));
        }
        return Some(lines);
    }

    let mut lines = vec![
        "digitalFilter cascade with properties:".to_string(),
        String::new(),
        "  Stages:".to_string(),
    ];
    for stage_name in stage_names {
        lines.push(format!("    {stage_name}: [1x1 digitalFilter]"));
    }
    lines.push(String::new());
    lines.push("  Specifications:".to_string());
    if let Some(value) = properties.fields.get("NormalizedFrequency") {
        lines.push(format!(
            "    NormalizedFrequency: {}",
            render_object_property_value(value)
        ));
    }
    if let Some(value) = properties.fields.get("SampleRate") {
        lines.push(format!(
            "    SampleRate: {}",
            render_object_property_value(value)
        ));
    }
    Some(lines)
}

fn render_object_property_value(value: &Value) -> String {
    match value {
        Value::Logical(flag) => {
            if *flag {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        _ => render_value(value),
    }
}

fn render_matrix_inline(matrix: &MatrixValue, tail_index: Option<&[usize]>) -> String {
    let rows = matrix.dims.first().copied().unwrap_or(matrix.rows);
    let cols = matrix.dims.get(1).copied().unwrap_or(matrix.cols);
    if rows == 0 || cols == 0 {
        return "[]".to_string();
    }
    if let Some(rendered) = render_char_matrix_inline(matrix, tail_index) {
        return rendered;
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

fn render_char_matrix_inline(matrix: &MatrixValue, tail_index: Option<&[usize]>) -> Option<String> {
    if !matrix_is_char_matrix(matrix) {
        return None;
    }

    let rows = matrix.dims.first().copied().unwrap_or(matrix.rows);
    let cols = matrix.dims.get(1).copied().unwrap_or(matrix.cols);
    let row_texts = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let mut index = vec![row, col];
                    if let Some(tail_index) = tail_index {
                        index.extend_from_slice(tail_index);
                    }
                    let linear = row_major_linear_index(&index, &matrix.dims);
                    single_char_text_value(&matrix.elements[linear])
                        .expect("char matrix render guard ensures single-character elements")
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    Some(if row_texts.len() == 1 {
        render_quoted_text(&row_texts[0], '\'')
    } else {
        format!(
            "[{}]",
            row_texts
                .into_iter()
                .map(|row| render_quoted_text(&row, '\''))
                .collect::<Vec<_>>()
                .join(" ; ")
        )
    })
}

fn matrix_is_char_matrix(matrix: &MatrixValue) -> bool {
    !matrix.elements().is_empty()
        && matrix
            .iter()
            .all(|value| single_char_text_value(value).is_some())
}

fn single_char_text_value(value: &Value) -> Option<char> {
    let Value::CharArray(text) = value else {
        return None;
    };
    let mut chars = text.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
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
