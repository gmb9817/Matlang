use std::{fs, path::PathBuf};

use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_semantics::{analyze_compilation_unit, testing::render_analysis};

fn fixture_path(name: &str, extension: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/semantics/binder")
        .join(format!("{name}.{extension}"))
}

fn assert_fixture(name: &str, mode: ParseMode) {
    let source = fs::read_to_string(fixture_path(name, "m")).expect("fixture source");
    let expected = fs::read_to_string(fixture_path(name, "golden")).expect("fixture golden");

    let parsed = parse_source(&source, SourceFileId(1), mode);
    assert!(
        !parsed.has_errors(),
        "unexpected parse diagnostics: {:?}",
        parsed.diagnostics
    );

    let unit = parsed.unit.expect("compilation unit");
    let analysis = analyze_compilation_unit(&unit);
    let rendered = render_analysis(&analysis);
    assert_eq!(rendered, expected);
}

#[test]
fn script_binding_fixture_matches_golden() {
    assert_fixture("script_binding", ParseMode::Script);
}

#[test]
fn function_binding_fixture_matches_golden() {
    assert_fixture("function_binding", ParseMode::AutoDetect);
}

#[test]
fn closure_capture_fixture_matches_golden() {
    assert_fixture("closure_capture", ParseMode::AutoDetect);
}

#[test]
fn resolution_precedence_fixture_matches_golden() {
    assert_fixture("resolution_precedence", ParseMode::AutoDetect);
}

#[test]
fn capture_modes_fixture_matches_golden() {
    assert_fixture("capture_modes", ParseMode::AutoDetect);
}

#[test]
fn switch_binding_fixture_matches_golden() {
    assert_fixture("switch_binding", ParseMode::Script);
}

#[test]
fn global_persistent_fixture_matches_golden() {
    assert_fixture("global_persistent", ParseMode::AutoDetect);
}

#[test]
fn qualified_package_refs_fixture_matches_golden() {
    assert_fixture("qualified_package_refs", ParseMode::AutoDetect);
}

#[test]
fn matrix_whitespace_binding_fixture_matches_golden() {
    assert_fixture("matrix_whitespace_binding", ParseMode::Script);
}

#[test]
fn builtin_logical_values_fixture_matches_golden() {
    assert_fixture("builtin_logical_values", ParseMode::AutoDetect);
}

#[test]
fn implicit_assignment_roots_fixture_matches_golden() {
    assert_fixture("implicit_assignment_roots", ParseMode::Script);
}

#[test]
fn try_catch_fixture_matches_golden() {
    assert_fixture("try_catch", ParseMode::Script);
}

#[test]
fn error_and_reflection_fixture_matches_golden() {
    assert_fixture("error_and_reflection", ParseMode::Script);
}

#[test]
fn mexception_helpers_fixture_matches_golden() {
    assert_fixture("mexception_helpers", ParseMode::Script);
}

#[test]
fn warning_and_lastwarn_fixture_matches_golden() {
    assert_fixture("warning_and_lastwarn", ParseMode::Script);
}

#[test]
fn classdef_basic_fixture_matches_golden() {
    assert_fixture("classdef_basic", ParseMode::AutoDetect);
}

#[test]
fn classdef_static_fixture_matches_golden() {
    assert_fixture("classdef_static", ParseMode::AutoDetect);
}

#[test]
fn classdef_private_fixture_matches_golden() {
    assert_fixture("classdef_private", ParseMode::AutoDetect);
}

#[test]
fn indexed_receiver_handle_fixture_matches_golden() {
    assert_fixture("indexed_receiver_handle", ParseMode::AutoDetect);
}

#[test]
fn receiver_expression_handle_fixture_matches_golden() {
    assert_fixture("receiver_expression_handle", ParseMode::AutoDetect);
}
