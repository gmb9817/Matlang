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
    decode_mat_file, decode_workspace_snapshot, decode_workspace_snapshot_with_modules,
    encode_mat_file, encode_workspace_snapshot, encode_workspace_snapshot_with_modules,
    read_mat_file, read_workspace_snapshot, write_mat_file, write_workspace_snapshot,
    WorkspaceSnapshotBundleModule,
};
use matlab_ir::lower_to_hir;
use matlab_resolver::ResolverContext;
use matlab_runtime::{
    render_workspace, CellValue, ComplexValue, FunctionHandleTarget, FunctionHandleValue,
    MatrixValue, ObjectClassMetadata, ObjectMethodTarget, ObjectStorage, ObjectStorageKind,
    ObjectValue, StructValue, Value, Workspace,
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
    workspace.insert(
        "obj".to_string(),
        Value::Object(ObjectValue::new(
            ObjectClassMetadata {
                class_name: "Vault".to_string(),
                package: Some("pkg".to_string()),
                superclass_name: Some("pkg.Base".to_string()),
                ancestor_class_names: std::collections::BTreeSet::from([
                    "pkg.Base".to_string(),
                ]),
                storage_kind: ObjectStorageKind::Value,
                source_path: Some("+pkg/Vault.m".into()),
                module_target: Some(ObjectMethodTarget::Path("+pkg/Vault.m".into())),
                property_order: vec!["secret".to_string()],
                private_properties: std::collections::BTreeSet::from([
                    "secret".to_string(),
                ]),
                private_property_owners: std::collections::BTreeMap::from([(
                    "secret".to_string(),
                    "pkg.Vault".to_string(),
                )]),
                inline_methods: std::collections::BTreeSet::from([
                    "reveal".to_string(),
                    "hidden".to_string(),
                ]),
                private_inline_methods: std::collections::BTreeSet::from([
                    "hidden".to_string(),
                ]),
                private_instance_method_owners: std::collections::BTreeMap::from([(
                    "hidden".to_string(),
                    "pkg.Vault".to_string(),
                )]),
                private_static_inline_methods: std::collections::BTreeSet::from([
                    "code".to_string(),
                ]),
                external_methods: std::collections::BTreeMap::new(),
                constructor: Some("Vault".to_string()),
            },
            StructValue::with_field_order(
                std::collections::BTreeMap::from([(
                    "secret".to_string(),
                    Value::Scalar(41.0),
                )]),
                vec!["secret".to_string()],
            ),
        )),
    );
    let obj = workspace.get("obj").expect("object").clone();
    workspace.insert(
        "bf".to_string(),
        Value::FunctionHandle(FunctionHandleValue {
            display_name: "@pkg.Vault.reveal".to_string(),
            target: FunctionHandleTarget::BoundMethod {
                class_name: "Vault".to_string(),
                package: Some("pkg".to_string()),
                method_name: "reveal".to_string(),
                receiver: Box::new(obj),
            },
        }),
    );
    workspace
}

fn handle_alias_workspace() -> Workspace {
    let mut workspace = Workspace::new();
    let counter = Value::Object(ObjectValue::new(
        ObjectClassMetadata {
            class_name: "Counter".to_string(),
            package: None,
            superclass_name: None,
            ancestor_class_names: std::collections::BTreeSet::new(),
            storage_kind: ObjectStorageKind::Handle,
            source_path: Some("Counter.m".into()),
            module_target: Some(ObjectMethodTarget::Path("Counter.m".into())),
            property_order: vec!["value".to_string()],
            private_properties: std::collections::BTreeSet::new(),
            private_property_owners: std::collections::BTreeMap::new(),
            inline_methods: std::collections::BTreeSet::from(["increment".to_string()]),
            private_inline_methods: std::collections::BTreeSet::new(),
            private_instance_method_owners: std::collections::BTreeMap::new(),
            private_static_inline_methods: std::collections::BTreeSet::new(),
            external_methods: std::collections::BTreeMap::new(),
            constructor: Some("Counter".to_string()),
        },
        StructValue::with_field_order(
            std::collections::BTreeMap::from([(
                "value".to_string(),
                Value::Scalar(5.0),
            )]),
            vec!["value".to_string()],
        ),
    ));
    workspace.insert("c".to_string(), counter.clone());
    workspace.insert("d".to_string(), counter);
    workspace
}

fn nested_handle_counter_class() -> ObjectClassMetadata {
    ObjectClassMetadata {
        class_name: "Counter".to_string(),
        package: None,
        superclass_name: None,
        ancestor_class_names: std::collections::BTreeSet::new(),
        storage_kind: ObjectStorageKind::Handle,
        source_path: Some("Counter.m".into()),
        module_target: Some(ObjectMethodTarget::Path("Counter.m".into())),
        property_order: vec!["value".to_string(), "child".to_string()],
        private_properties: std::collections::BTreeSet::new(),
        private_property_owners: std::collections::BTreeMap::new(),
        inline_methods: std::collections::BTreeSet::from(["total".to_string()]),
        private_inline_methods: std::collections::BTreeSet::new(),
        private_instance_method_owners: std::collections::BTreeMap::new(),
        private_static_inline_methods: std::collections::BTreeSet::new(),
        external_methods: std::collections::BTreeMap::new(),
        constructor: Some("Counter".to_string()),
    }
}

fn nested_handle_counter(value: f64, child: Value) -> Value {
    Value::Object(ObjectValue::new(
        nested_handle_counter_class(),
        StructValue::with_field_order(
            std::collections::BTreeMap::from([
                ("value".to_string(), Value::Scalar(value)),
                ("child".to_string(), child),
            ]),
            vec!["value".to_string(), "child".to_string()],
        ),
    ))
}

fn property_produced_handle_bound_alias_workspace() -> Workspace {
    let mut workspace = Workspace::new();
    let empty = Value::Matrix(MatrixValue::new(0, 0, Vec::new()).expect("empty matrix"));
    let child_left = nested_handle_counter(10.0, empty.clone());
    let child_right = nested_handle_counter(20.0, empty);
    let receiver = Value::Matrix(
        MatrixValue::new(1, 2, vec![child_left.clone(), child_right.clone()])
            .expect("receiver matrix"),
    );
    let parents = Value::Matrix(
        MatrixValue::new(
            1,
            2,
            vec![
                nested_handle_counter(1.0, child_left),
                nested_handle_counter(2.0, child_right),
            ],
        )
        .expect("parent matrix"),
    );
    workspace.insert("objs".to_string(), parents);
    workspace.insert(
        "f".to_string(),
        Value::FunctionHandle(FunctionHandleValue {
            display_name: "@Counter.total".to_string(),
            target: FunctionHandleTarget::BoundMethod {
                class_name: "Counter".to_string(),
                package: None,
                method_name: "total".to_string(),
                receiver: Box::new(receiver),
            },
        }),
    );
    workspace
}

fn assert_property_produced_handle_bound_aliasing(decoded: &Workspace) {
    let Value::Matrix(objs) = decoded.get("objs").expect("objs value") else {
        panic!("expected object matrix");
    };
    let Value::FunctionHandle(FunctionHandleValue {
        target: FunctionHandleTarget::BoundMethod { receiver, .. },
        ..
    }) = decoded.get("f").expect("f value")
    else {
        panic!("expected bound method handle");
    };
    let Value::Matrix(receiver_matrix) = receiver.as_ref() else {
        panic!("expected matrix receiver");
    };

    for ((parent, receiver_value), expected) in objs
        .elements()
        .iter()
        .zip(receiver_matrix.elements().iter())
        .zip([100.0, 200.0].into_iter())
    {
        let Value::Object(parent_object) = parent else {
            panic!("expected parent object");
        };
        let child = parent_object.property_value("child").expect("child property");
        let (
            Value::Object(child_object),
            Value::Object(receiver_object),
        ) = (&child, receiver_value)
        else {
            panic!("expected handle child objects");
        };
        let (
            ObjectStorage::Handle {
                id: child_id,
                shared: child_shared,
            },
            ObjectStorage::Handle {
                id: receiver_id,
                shared: receiver_shared,
            },
        ) = (&child_object.storage, &receiver_object.storage)
        else {
            panic!("expected handle-backed child objects");
        };
        assert_eq!(child_id, receiver_id);
        assert!(std::rc::Rc::ptr_eq(child_shared, receiver_shared));

        let mut child_mut = child_object.clone();
        child_mut
            .set_property_value("value", Value::Scalar(expected))
            .expect("mutate child handle");
        assert_eq!(
            receiver_object.property_value("value"),
            Some(Value::Scalar(expected))
        );
    }
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

#[test]
fn workspace_snapshot_roundtrips_embedded_bundle_module_records() {
    let workspace = sample_workspace();
    let bundle_modules = vec![WorkspaceSnapshotBundleModule {
        module_id: "dep0".to_string(),
        source_path: "C:/tmp/Point.m".to_string(),
        encoded_module: "MATC-BYTECODE\t1\nMODULE\tbytecode\tClassFile\tPoint\n".to_string(),
    }];
    let encoded = encode_workspace_snapshot_with_modules(&workspace, &bundle_modules)
        .expect("encode with modules");
    let decoded =
        decode_workspace_snapshot_with_modules(&encoded).expect("decode with modules");
    assert_eq!(decoded.workspace, workspace);
    assert_eq!(decoded.bundle_modules, bundle_modules);
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
    let bound_receiver = Value::Object(ObjectValue::new(
        ObjectClassMetadata {
            class_name: "Point".to_string(),
            package: Some("pkg".to_string()),
            superclass_name: Some("pkg.Base".to_string()),
            ancestor_class_names: std::collections::BTreeSet::from([
                "pkg.Base".to_string(),
            ]),
            storage_kind: ObjectStorageKind::Value,
            source_path: Some("+pkg/Point.m".into()),
            module_target: Some(ObjectMethodTarget::Path("+pkg/Point.m".into())),
            property_order: vec!["x".to_string()],
            private_properties: std::collections::BTreeSet::from([
                "x".to_string(),
            ]),
            private_property_owners: std::collections::BTreeMap::from([(
                "x".to_string(),
                "pkg.Point".to_string(),
            )]),
            inline_methods: std::collections::BTreeSet::from([
                "value".to_string(),
                "hidden".to_string(),
            ]),
            private_inline_methods: std::collections::BTreeSet::from([
                "hidden".to_string(),
            ]),
            private_instance_method_owners: std::collections::BTreeMap::from([(
                "hidden".to_string(),
                "pkg.Point".to_string(),
            )]),
            private_static_inline_methods: std::collections::BTreeSet::from([
                "code".to_string(),
            ]),
            external_methods: std::collections::BTreeMap::new(),
            constructor: Some("Point".to_string()),
        },
        StructValue::with_field_order(
            std::collections::BTreeMap::from([(
                "x".to_string(),
                Value::Scalar(3.0),
            )]),
            vec!["x".to_string()],
        ),
    ));
    workspace.insert(
        "bf".to_string(),
        Value::FunctionHandle(FunctionHandleValue {
            display_name: "@pkg.Point.value".to_string(),
            target: FunctionHandleTarget::BoundMethod {
                class_name: "Point".to_string(),
                package: Some("pkg".to_string()),
                method_name: "value".to_string(),
                receiver: Box::new(bound_receiver),
            },
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
    workspace.insert(
        "oa".to_string(),
        Value::Matrix(
            MatrixValue::new(
                1,
                2,
                vec![
                    Value::Object(ObjectValue::new(
                        ObjectClassMetadata {
                            class_name: "Point".to_string(),
                            package: Some("pkg".to_string()),
                            superclass_name: Some("pkg.Base".to_string()),
                            ancestor_class_names: std::collections::BTreeSet::from([
                                "pkg.Base".to_string(),
                            ]),
                            storage_kind: ObjectStorageKind::Value,
                            source_path: Some("+pkg/Point.m".into()),
                            module_target: Some(ObjectMethodTarget::Path("+pkg/Point.m".into())),
                            property_order: vec!["x".to_string()],
                            private_properties: std::collections::BTreeSet::from([
                                "x".to_string(),
                            ]),
                            private_property_owners: std::collections::BTreeMap::from([(
                                "x".to_string(),
                                "pkg.Point".to_string(),
                            )]),
                            inline_methods: std::collections::BTreeSet::from([
                                "value".to_string(),
                                "hidden".to_string(),
                            ]),
                            private_inline_methods: std::collections::BTreeSet::from([
                                "hidden".to_string(),
                            ]),
                            private_instance_method_owners: std::collections::BTreeMap::from([(
                                "hidden".to_string(),
                                "pkg.Point".to_string(),
                            )]),
                            private_static_inline_methods: std::collections::BTreeSet::from([
                                "code".to_string(),
                            ]),
                            external_methods: std::collections::BTreeMap::new(),
                            constructor: Some("Point".to_string()),
                        },
                        StructValue::with_field_order(
                            std::collections::BTreeMap::from([(
                                "x".to_string(),
                                Value::Scalar(1.0),
                            )]),
                            vec!["x".to_string()],
                        ),
                    )),
                    Value::Object(ObjectValue::new(
                        ObjectClassMetadata {
                            class_name: "Point".to_string(),
                            package: Some("pkg".to_string()),
                            superclass_name: Some("pkg.Base".to_string()),
                            ancestor_class_names: std::collections::BTreeSet::from([
                                "pkg.Base".to_string(),
                            ]),
                            storage_kind: ObjectStorageKind::Value,
                            source_path: Some("+pkg/Point.m".into()),
                            module_target: Some(ObjectMethodTarget::Path("+pkg/Point.m".into())),
                            property_order: vec!["x".to_string()],
                            private_properties: std::collections::BTreeSet::from([
                                "x".to_string(),
                            ]),
                            private_property_owners: std::collections::BTreeMap::from([(
                                "x".to_string(),
                                "pkg.Point".to_string(),
                            )]),
                            inline_methods: std::collections::BTreeSet::from([
                                "value".to_string(),
                                "hidden".to_string(),
                            ]),
                            private_inline_methods: std::collections::BTreeSet::from([
                                "hidden".to_string(),
                            ]),
                            private_instance_method_owners: std::collections::BTreeMap::from([(
                                "hidden".to_string(),
                                "pkg.Point".to_string(),
                            )]),
                            private_static_inline_methods: std::collections::BTreeSet::from([
                                "code".to_string(),
                            ]),
                            external_methods: std::collections::BTreeMap::new(),
                            constructor: Some("Point".to_string()),
                        },
                        StructValue::with_field_order(
                            std::collections::BTreeMap::from([(
                                "x".to_string(),
                                Value::Scalar(2.0),
                            )]),
                            vec!["x".to_string()],
                        ),
                    )),
                ],
            )
            .expect("object array"),
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
fn workspace_snapshot_preserves_handle_aliasing() {
    let workspace = handle_alias_workspace();
    let encoded = encode_workspace_snapshot(&workspace).expect("encode");
    let decoded = decode_workspace_snapshot(&encoded).expect("decode");
    let Value::Object(c) = decoded.get("c").expect("c value") else {
        panic!("expected object");
    };
    let Value::Object(d) = decoded.get("d").expect("d value") else {
        panic!("expected object");
    };
    let (
        ObjectStorage::Handle {
            id: c_id,
            shared: c_handle,
        },
        ObjectStorage::Handle {
            id: d_id,
            shared: d_handle,
        },
    ) = (&c.storage, &d.storage)
    else {
        panic!("expected handle-backed objects");
    };
    assert_eq!(c_id, d_id);
    assert!(std::rc::Rc::ptr_eq(c_handle, d_handle));
}

#[test]
fn workspace_snapshot_preserves_property_produced_handle_bound_aliasing() {
    let workspace = property_produced_handle_bound_alias_workspace();
    let encoded = encode_workspace_snapshot(&workspace).expect("encode");
    let decoded = decode_workspace_snapshot(&encoded).expect("decode");
    assert_property_produced_handle_bound_aliasing(&decoded);
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
fn mat_file_preserves_handle_aliasing() {
    let workspace = handle_alias_workspace();
    let encoded = encode_mat_file(&workspace).expect("encode");
    let decoded = decode_mat_file(&encoded).expect("decode");
    let Value::Object(c) = decoded.get("c").expect("c value") else {
        panic!("expected object");
    };
    let Value::Object(d) = decoded.get("d").expect("d value") else {
        panic!("expected object");
    };
    let (
        ObjectStorage::Handle {
            id: c_id,
            shared: c_handle,
        },
        ObjectStorage::Handle {
            id: d_id,
            shared: d_handle,
        },
    ) = (&c.storage, &d.storage)
    else {
        panic!("expected handle-backed objects");
    };
    assert_eq!(c_id, d_id);
    assert!(std::rc::Rc::ptr_eq(c_handle, d_handle));
}

#[test]
fn mat_file_preserves_property_produced_handle_bound_aliasing() {
    let workspace = property_produced_handle_bound_alias_workspace();
    let encoded = encode_mat_file(&workspace).expect("encode");
    let decoded = decode_mat_file(&encoded).expect("decode");
    assert_property_produced_handle_bound_aliasing(&decoded);
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

