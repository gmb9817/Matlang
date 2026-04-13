use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use matlab_codegen::emit_bytecode;
use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_ir::lower_to_hir;
use matlab_optimizer::optimize_module;
use matlab_platform::{
    attach_bundle_module_id, collect_bytecode_dependency_paths, decode_bytecode_bundle,
    decode_bytecode_module, encode_bytecode_bundle, encode_bytecode_module, read_bytecode_artifact,
    rewrite_bytecode_bundle_targets, write_bytecode_artifact, write_bytecode_bundle,
    BytecodeBundle, PackagedBytecodeModule,
};
use matlab_resolver::ResolverContext;
use matlab_semantics::analyze_compilation_unit_with_context;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/codegen/bytecode")
        .join(format!("{name}.m"))
}

fn compile_fixture(name: &str) -> matlab_codegen::BytecodeModule {
    let source_path = fixture_path(name);
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
    let optimized = optimize_module(&hir);
    emit_bytecode(&optimized.module)
}

#[test]
fn encode_decode_roundtrip_matches_bytecode_module() {
    let module = compile_fixture("control_flow_codegen");
    let encoded = encode_bytecode_module(&module);
    let decoded = decode_bytecode_module(&encoded).expect("decode");
    assert_eq!(decoded, module);
}

#[test]
fn write_and_read_artifact_roundtrip_matches_bytecode_module() {
    let module = compile_fixture("handles_and_nested_codegen");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let artifact_path = std::env::temp_dir().join(format!("matc-bytecode-{suffix}.matbc"));

    write_bytecode_artifact(&artifact_path, &module).expect("write artifact");
    let decoded = read_bytecode_artifact(&artifact_path).expect("read artifact");
    assert_eq!(decoded, module);

    let _ = fs::remove_file(artifact_path);
}

#[test]
fn decode_rejects_invalid_header() {
    let error = decode_bytecode_module("not-a-real-artifact\n").expect_err("invalid header");
    assert!(error.to_string().contains("expected artifact header"));
}

#[test]
fn encode_decode_bundle_roundtrip_matches_bundle() {
    let root = compile_fixture("control_flow_codegen");
    let dependency = compile_fixture("handles_and_nested_codegen");
    let bundle = BytecodeBundle {
        root_source_path: fixture_path("control_flow_codegen").display().to_string(),
        root_module: root,
        dependency_modules: vec![PackagedBytecodeModule {
            module_id: "dep0".to_string(),
            source_path: fixture_path("handles_and_nested_codegen")
                .display()
                .to_string(),
            module: dependency,
        }],
    };

    let encoded = encode_bytecode_bundle(&bundle);
    let decoded = decode_bytecode_bundle(&encoded).expect("decode bundle");
    assert_eq!(decoded, bundle);
}

#[test]
fn write_and_read_bundle_roundtrip_matches_bundle() {
    let root = compile_fixture("control_flow_codegen");
    let bundle = BytecodeBundle {
        root_source_path: fixture_path("control_flow_codegen").display().to_string(),
        root_module: root,
        dependency_modules: Vec::new(),
    };
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let bundle_path = std::env::temp_dir().join(format!("matc-bytecode-bundle-{suffix}.matpkg"));

    write_bytecode_bundle(&bundle_path, &bundle).expect("write bundle");
    let decoded = matlab_platform::read_bytecode_bundle(&bundle_path).expect("read bundle");
    assert_eq!(decoded, bundle);

    let _ = fs::remove_file(bundle_path);
}

#[test]
fn collects_resolved_dependency_paths_from_external_fixture() {
    let module = compile_fixture("index_and_field_codegen");
    assert!(collect_bytecode_dependency_paths(&module).is_empty());

    let external_source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/execution/interpreter/external_resolution.m");
    let external_source = fs::read_to_string(&external_source_path).expect("fixture source");
    let parsed = parse_source(&external_source, SourceFileId(1), ParseMode::AutoDetect);
    assert!(
        !parsed.has_errors(),
        "unexpected parse diagnostics: {:?}",
        parsed.diagnostics
    );
    let unit = parsed.unit.expect("compilation unit");
    let analysis = analyze_compilation_unit_with_context(
        &unit,
        &ResolverContext::from_source_file(external_source_path.clone()),
    );
    assert!(
        !analysis.has_errors(),
        "unexpected semantic diagnostics: {:?}",
        analysis.diagnostics
    );
    let hir = lower_to_hir(&unit, &analysis);
    let optimized = optimize_module(&hir);
    let bytecode = emit_bytecode(&optimized.module);
    let paths = collect_bytecode_dependency_paths(&bytecode);

    assert_eq!(paths.len(), 2);
    assert!(paths.iter().any(|path| path.ends_with("helper.m")));
    assert!(paths.iter().any(|path| path.ends_with("+pkg\\helper.m")));
}

#[test]
fn rewrite_bundle_targets_adds_bundle_ids_for_packaged_paths() {
    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/execution/interpreter/external_resolution.m");
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
        &ResolverContext::from_source_file(source_path.clone()),
    );
    assert!(
        !analysis.has_errors(),
        "unexpected semantic diagnostics: {:?}",
        analysis.diagnostics
    );
    let hir = lower_to_hir(&unit, &analysis);
    let optimized = optimize_module(&hir);
    let bytecode = emit_bytecode(&optimized.module);
    let path_map = collect_bytecode_dependency_paths(&bytecode)
        .into_iter()
        .enumerate()
        .map(|(index, path)| (path, format!("dep{index}")))
        .collect::<std::collections::HashMap<_, _>>();

    let rewritten = rewrite_bytecode_bundle_targets(&bytecode, &path_map);
    let rendered = matlab_codegen::render_bytecode(&rewritten);
    assert!(rendered.contains("bundle_id=dep0") || rendered.contains("bundle_id=dep1"));
}

#[test]
fn attach_bundle_module_id_is_stable_when_reapplied() {
    let target = r#"helper [semantic=ExternalFunctionCandidate final=ResolvedPath { kind: CurrentDirectory, path: "helper.m", package: None, shadowed_builtin: false }]"#;
    let once = attach_bundle_module_id(target, "dep0");
    let twice = attach_bundle_module_id(&once, "dep0");
    assert_eq!(once, twice);
}
