use std::{fs, path::PathBuf};

use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
    testing::render_compilation_unit,
};

fn fixture_path(name: &str, extension: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/frontend/parser")
        .join(format!("{name}.{extension}"))
}

fn assert_fixture(name: &str, mode: ParseMode) {
    let source = fs::read_to_string(fixture_path(name, "m")).expect("fixture source");
    let expected = fs::read_to_string(fixture_path(name, "golden")).expect("fixture golden");

    let parsed = parse_source(&source, SourceFileId(1), mode);
    assert!(
        !parsed.has_errors(),
        "unexpected diagnostics: {:?}",
        parsed.diagnostics
    );

    let unit = parsed.unit.expect("compilation unit");
    let rendered = render_compilation_unit(&unit);
    assert_eq!(rendered, expected);
}

#[test]
fn control_flow_fixture_matches_golden() {
    assert_fixture("control_flow", ParseMode::AutoDetect);
}

#[test]
fn loops_fixture_matches_golden() {
    assert_fixture("loops", ParseMode::Script);
}

#[test]
fn switch_case_fixture_matches_golden() {
    assert_fixture("switch_case", ParseMode::Script);
}

#[test]
fn qualified_package_refs_fixture_matches_golden() {
    assert_fixture("qualified_package_refs", ParseMode::AutoDetect);
}

#[test]
fn matrix_whitespace_separators_fixture_matches_golden() {
    assert_fixture("matrix_whitespace_separators", ParseMode::Script);
}

#[test]
fn command_form_fixture_matches_golden() {
    assert_fixture("command_form", ParseMode::Script);
}

#[test]
fn command_form_utilities_fixture_matches_golden() {
    assert_fixture("command_form_utilities", ParseMode::Script);
}

#[test]
fn complex_literals_fixture_matches_golden() {
    assert_fixture("complex_literals", ParseMode::Script);
}

#[test]
fn try_catch_fixture_matches_golden() {
    assert_fixture("try_catch", ParseMode::Script);
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
