use std::{fs, path::PathBuf};

use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_ir::{lower_to_hir, testing::render_hir};
use matlab_optimizer::{optimize_module, render_optimization_summary};
use matlab_resolver::ResolverContext;
use matlab_semantics::analyze_compilation_unit_with_context;

fn fixture_path(name: &str, extension: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/optimizer/hir")
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
    let mut rendered = render_optimization_summary(&optimized.summary);
    rendered.push('\n');
    rendered.push_str(&render_hir(&optimized.module));
    assert_eq!(rendered, expected);
}

#[test]
fn constant_control_flow_fixture_matches_golden() {
    assert_fixture("constant_control_flow");
}

#[test]
fn range_and_identity_fixture_matches_golden() {
    assert_fixture("range_and_identity");
}
