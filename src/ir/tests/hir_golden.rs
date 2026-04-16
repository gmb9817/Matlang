use std::{fs, path::PathBuf};

use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_ir::{lower_to_hir, testing::render_hir};
use matlab_resolver::ResolverContext;
use matlab_semantics::analyze_compilation_unit_with_context;

fn fixture_path(name: &str, extension: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ir/hir")
        .join(format!("{name}.{extension}"))
}

fn assert_fixture(name: &str, mode: ParseMode) {
    let source_path = fixture_path(name, "m");
    let source = fs::read_to_string(&source_path).expect("fixture source");
    let expected = fs::read_to_string(fixture_path(name, "golden")).expect("fixture golden");

    let parsed = parse_source(&source, SourceFileId(1), mode);
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
    let rendered = render_hir(&hir);
    assert_eq!(rendered, expected);
}

#[test]
fn package_and_closure_fixture_matches_golden() {
    assert_fixture("package_and_closure", ParseMode::AutoDetect);
}

#[test]
fn script_control_flow_fixture_matches_golden() {
    assert_fixture("script_control_flow", ParseMode::Script);
}

#[test]
fn try_catch_flow_fixture_matches_golden() {
    assert_fixture("try_catch_flow", ParseMode::Script);
}

#[test]
fn classdef_basic_fixture_matches_golden() {
    assert_fixture("classdef_basic", ParseMode::AutoDetect);
}
