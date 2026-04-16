use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use matlab_execution::execute_script;
use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_interop::{
    decode_mat_file, decode_workspace_snapshot, encode_mat_file, encode_workspace_snapshot,
    read_mat_file, read_workspace_snapshot, write_mat_file, write_workspace_snapshot,
};
use matlab_ir::lower_to_hir;
use matlab_resolver::ResolverContext;
use matlab_runtime::{
    render_workspace, CellValue, ComplexValue, FunctionHandleTarget, FunctionHandleValue,
    MatrixValue, StructValue, Value, Workspace,
};
use matlab_semantics::analyze_compilation_unit_with_context;

fn sample_workspace() -> Workspace {
    let mut workspace = Workspace::new();
    workspace.insert("a".to_string(), Value::Scalar(1.5));
    workspace.insert(
        "cx".to_string(),
        Value::Complex(ComplexValue {
            real: 3.0,
            imag: -4.0,
        }),
    );
    workspace.insert("flag".to_string(), Value::Logical(true));
    workspace.insert("ch".to_string(), Value::CharArray("alpha".to_string()));
    workspace.insert("str".to_string(), Value::String("beta".to_string()));
    workspace.insert(
        "m".to_string(),
        Value::Matrix(
            MatrixValue::new(
                2,
                2,
                vec![
                    Value::Scalar(1.0),
                    Value::Scalar(2.0),
                    Value::Scalar(3.0),
                    Value::Scalar(4.0),
                ],
            )
            .expect("matrix"),
        ),
    );
    workspace.insert(
        "lm".to_string(),
        Value::Matrix(
            MatrixValue::new(
                1,
                3,
                vec![
                    Value::Logical(false),
                    Value::Logical(true),
                    Value::Logical(false),
                ],
            )
            .expect("logical matrix"),
        ),
    );
    workspace.insert(
        "sm".to_string(),
        Value::Matrix(
            MatrixValue::new(
                1,
                2,
                vec![
                    Value::String("left".to_string()),
                    Value::String("right".to_string()),
                ],
            )
            .expect("string matrix"),
        ),
    );
    workspace.insert(
        "c".to_string(),
        Value::Cell(
            CellValue::new(1, 2, vec![Value::Scalar(9.0), Value::Scalar(10.0)]).expect("cell"),
        ),
    );
    workspace.insert("str".to_string(), Value::String("beta".to_string()));
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("name".to_string(), Value::CharArray("named".to_string()));
    fields.insert(
        "fn".to_string(),
        Value::FunctionHandle(FunctionHandleValue {
            display_name: "helper".to_string(),
            target: FunctionHandleTarget::BundleModule("dep7".to_string()),
        }),
    );
    workspace.insert(
        "s".to_string(),
        Value::Struct(StructValue::with_field_order(
            fields,
            vec!["name".to_string(), "fn".to_string()],
        )),
    );
    workspace
}

#[test]
fn workspace_snapshot_roundtrips_values() {
    let workspace = sample_workspace();
    let encoded = encode_workspace_snapshot(&workspace).expect("encode");
    let decoded = decode_workspace_snapshot(&encoded).expect("decode");
    assert_eq!(decoded, workspace);
    let Value::Struct(decoded_struct) = decoded.get("s").expect("decoded struct") else {
        panic!("expected struct value");
    };
    assert_eq!(
        decoded_struct.field_names(),
        &["name".to_string(), "fn".to_string()]
    );
}

fn sample_mat_workspace() -> Workspace {
    let mut workspace = Workspace::new();
    workspace.insert("a".to_string(), Value::Scalar(1.5));
    workspace.insert(
        "cx".to_string(),
        Value::Complex(ComplexValue {
            real: 3.0,
            imag: -4.0,
        }),
    );
    workspace.insert("flag".to_string(), Value::Logical(true));
    workspace.insert("ch".to_string(), Value::CharArray("alpha".to_string()));
    workspace.insert(
        "m".to_string(),
        Value::Matrix(
            MatrixValue::new(
                2,
                2,
                vec![
                    Value::Scalar(1.0),
                    Value::Scalar(2.0),
                    Value::Scalar(3.0),
                    Value::Scalar(4.0),
                ],
            )
            .expect("matrix"),
        ),
    );
    workspace.insert(
        "lm".to_string(),
        Value::Matrix(
            MatrixValue::new(
                1,
                3,
                vec![
                    Value::Logical(false),
                    Value::Logical(true),
                    Value::Logical(false),
                ],
            )
            .expect("logical matrix"),
        ),
    );
    workspace.insert(
        "c".to_string(),
        Value::Cell(
            CellValue::new(1, 2, vec![Value::Scalar(9.0), Value::Scalar(10.0)]).expect("cell"),
        ),
    );
    workspace.insert(
        "fh".to_string(),
        Value::FunctionHandle(FunctionHandleValue {
            display_name: "sin".to_string(),
            target: FunctionHandleTarget::Named("sin".to_string()),
        }),
    );
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("name".to_string(), Value::CharArray("named".to_string()));
    fields.insert("value".to_string(), Value::Scalar(7.0));
    workspace.insert(
        "s".to_string(),
        Value::Struct(StructValue::with_field_order(
            fields,
            vec!["value".to_string(), "name".to_string()],
        )),
    );
    let mut first = std::collections::BTreeMap::new();
    first.insert("name".to_string(), Value::CharArray("first".to_string()));
    first.insert("value".to_string(), Value::Scalar(1.0));
    let mut second = std::collections::BTreeMap::new();
    second.insert("name".to_string(), Value::CharArray("second".to_string()));
    second.insert("value".to_string(), Value::Scalar(2.0));
    workspace.insert(
        "sa".to_string(),
        Value::Matrix(
            MatrixValue::new(
                1,
                2,
                vec![
                    Value::Struct(StructValue::with_field_order(
                        first,
                        vec!["value".to_string(), "name".to_string()],
                    )),
                    Value::Struct(StructValue::with_field_order(
                        second,
                        vec!["value".to_string(), "name".to_string()],
                    )),
                ],
            )
            .expect("struct array"),
        ),
    );
    workspace
}

#[test]
fn mat_file_roundtrips_supported_values() {
    let workspace = sample_mat_workspace();
    let encoded = encode_mat_file(&workspace).expect("encode mat");
    let decoded = decode_mat_file(&encoded).expect("decode mat");
    assert_eq!(decoded, workspace);
    let Value::Struct(decoded_struct) = decoded.get("s").expect("decoded scalar struct") else {
        panic!("expected scalar struct");
    };
    assert_eq!(
        decoded_struct.field_names(),
        &["value".to_string(), "name".to_string()]
    );
    let Value::Matrix(decoded_array) = decoded.get("sa").expect("decoded struct array") else {
        panic!("expected struct array");
    };
    let Value::Struct(first) = decoded_array.elements().first().expect("first struct element") else {
        panic!("expected struct array element");
    };
    assert_eq!(
        first.field_names(),
        &["value".to_string(), "name".to_string()]
    );
}

#[test]
fn workspace_snapshot_file_roundtrips_values() {
    let workspace = sample_workspace();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("matc-workspace-{suffix}.matws"));

    write_workspace_snapshot(&path, &workspace).expect("write");
    let decoded = read_workspace_snapshot(&path).expect("read");
    assert_eq!(decoded, workspace);

    let _ = fs::remove_file(path);
}

#[test]
fn mat_file_roundtrips_from_disk() {
    let workspace = sample_mat_workspace();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("matc-workspace-{suffix}.mat"));

    write_mat_file(&path, &workspace).expect("write mat");
    let decoded = read_mat_file(&path).expect("read mat");
    assert_eq!(decoded, workspace);

    let _ = fs::remove_file(path);
}

#[test]
fn decode_rejects_invalid_header() {
    let error = decode_workspace_snapshot("not-a-snapshot\n").expect_err("invalid header");
    assert!(error.to_string().contains("expected snapshot header"));
}

#[test]
fn execution_result_roundtrips_through_snapshot() {
    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/execution/interpreter/builtin_switch.m");
    let source = fs::read_to_string(&source_path).expect("fixture source");
    let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
    assert!(
        !parsed.has_errors(),
        "unexpected parse diagnostics: {:?}",
        parsed.diagnostics
    );
    let unit = parsed.unit.expect("compilation unit");
    let analysis = analyze_compilation_unit_with_context(
        &unit,
        &ResolverContext::from_source_file(source_path),
    );
    assert!(
        !analysis.has_errors(),
        "unexpected semantic diagnostics: {:?}",
        analysis.diagnostics
    );
    let hir = lower_to_hir(&unit, &analysis);
    let result = execute_script(&hir).expect("execute");

    let encoded = encode_workspace_snapshot(&result.workspace).expect("encode");
    let decoded = decode_workspace_snapshot(&encoded).expect("decode");
    assert_eq!(
        render_workspace(&decoded),
        render_workspace(&result.workspace)
    );
}

