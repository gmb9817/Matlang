use std::{fs, path::PathBuf};

use matlab_codegen::{
    emit_bytecode, render_bytecode, render_codegen_summary, render_verification_summary,
    summarize_bytecode, verify_bytecode, BackendKind, BytecodeFunction, BytecodeInstruction,
    BytecodeModule,
};
use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_ir::lower_to_hir;
use matlab_optimizer::optimize_module;
use matlab_resolver::ResolverContext;
use matlab_semantics::analyze_compilation_unit_with_context;

fn fixture_path(name: &str, extension: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/codegen/bytecode")
        .join(format!("{name}.{extension}"))
}

fn assert_fixture(name: &str) {
    let source_path = fixture_path(name, "m");
    let source = fs::read_to_string(&source_path).expect("fixture source");
    let expected = fs::read_to_string(fixture_path(name, "golden")).expect("fixture golden");

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
    let bytecode = emit_bytecode(&optimized.module);
    let summary = summarize_bytecode(&bytecode);
    let verification = verify_bytecode(&bytecode);
    assert!(
        verification.ok(),
        "unexpected bytecode verification issues: {:?}",
        verification.issues
    );
    let mut rendered = render_codegen_summary(&summary);
    rendered.push('\n');
    rendered.push_str(&render_verification_summary(&verification));
    rendered.push('\n');
    rendered.push_str(&render_bytecode(&bytecode));
    assert_eq!(rendered, expected);
}

#[test]
fn control_flow_fixture_matches_golden() {
    assert_fixture("control_flow_codegen");
}

#[test]
fn handles_and_nested_fixture_matches_golden() {
    assert_fixture("handles_and_nested_codegen");
}

#[test]
fn index_and_field_fixture_matches_golden() {
    assert_fixture("index_and_field_codegen");
}

#[test]
fn comma_separated_fixture_matches_golden() {
    assert_fixture("comma_separated_codegen");
}

#[test]
fn comma_separated_forwarded_cells_fixture_matches_golden() {
    assert_fixture("comma_separated_forwarded_cells_codegen");
}

#[test]
fn comma_separated_forwarded_paren_fixture_matches_golden() {
    assert_fixture("comma_separated_forwarded_paren_codegen");
}

#[test]
fn try_catch_fixture_matches_golden() {
    assert_fixture("try_catch_codegen");
}

#[test]
fn handle_receivers_fixture_matches_golden() {
    assert_fixture("handle_receivers_codegen");
}

#[test]
fn verifier_reports_invalid_jump_target() {
    let module = BytecodeModule {
        backend: BackendKind::Bytecode,
        unit_kind: "Script".to_string(),
        entry: "<script>".to_string(),
        classes: Vec::new(),
        functions: vec![BytecodeFunction {
            name: "<script>".to_string(),
            role: "script_entry".to_string(),
            owner_class_name: None,
            params: Vec::new(),
            outputs: Vec::new(),
            captures: Vec::new(),
            temp_count: 1,
            label_count: 0,
            instructions: vec![
                BytecodeInstruction::Jump { target: 99 },
                BytecodeInstruction::Return { values: Vec::new() },
            ],
        }],
    };

    let verification = verify_bytecode(&module);
    assert!(!verification.ok());
    assert!(verification
        .issues
        .iter()
        .any(|issue| issue.message.contains("jump target L99 is missing")));
}
