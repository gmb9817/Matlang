use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use matlab_execution::{execute_script_bytecode_with_identity, execute_script_with_identity};
use matlab_frontend::{
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_ir::lower_to_hir;
use matlab_resolver::ResolverContext;
use matlab_runtime::RuntimeError;
use matlab_semantics::analyze_compilation_unit_with_context;

#[derive(Debug, Clone, Copy)]
enum ExecutionKind {
    Interpreter,
    Bytecode,
}

#[test]
fn saveas_exports_svg_in_interpreter_mode() {
    assert_svg_export(ExecutionKind::Interpreter, "saveas");
}

#[test]
fn exportgraphics_exports_svg_in_bytecode_mode() {
    assert_svg_export(ExecutionKind::Bytecode, "exportgraphics");
}

#[test]
fn saveas_exports_png_in_interpreter_mode() {
    assert_png_export(ExecutionKind::Interpreter, "saveas");
}

#[test]
fn exportgraphics_exports_png_in_bytecode_mode() {
    assert_png_export(ExecutionKind::Bytecode, "exportgraphics");
}

#[test]
fn saveas_exports_pdf_in_interpreter_mode() {
    assert_pdf_export(ExecutionKind::Interpreter, "saveas");
}

#[test]
fn exportgraphics_exports_pdf_in_bytecode_mode() {
    assert_pdf_export(ExecutionKind::Bytecode, "exportgraphics");
}

#[test]
fn print_function_form_exports_png_with_handle_auto_extension_and_resolution() {
    let temp_dir = unique_temp_dir("print-function-png");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_stem = temp_dir.join("printed_figure");
    let output_path = output_stem.with_extension("png");
    let source_path = temp_dir.join("graphics_print_function_export.m");
    let matlab_output_stem = output_stem.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(240);\n\
         plot([0, 1, 2, 3], [0, 1, 4, 9]);\n\
         title(\"Print PNG\");\n\
         print(f, \"{matlab_output_stem}\", \"-dpng\", \"-r192\");\n"
    );

    let png = execute_and_read_png(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert_png_signature_and_dimensions(&png, 1800, 1300);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_command_form_exports_current_figure_png_in_bytecode_mode() {
    let temp_dir = unique_temp_dir("print-command-png");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("command_form_print.png");
    let source_path = temp_dir.join("graphics_print_command_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "figure(241);\n\
         plot([0, 1, 2], [1, 0, 1]);\n\
         title(\"Command Print PNG\");\n\
         print -r192 -dpng {matlab_output_path}\n"
    );

    let png = execute_and_read_png(ExecutionKind::Bytecode, &source_path, &source, &output_path);
    assert_png_signature_and_dimensions(&png, 1800, 1300);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_command_form_exports_svg_with_auto_extension() {
    let temp_dir = unique_temp_dir("print-command-svg");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_stem = temp_dir.join("command_form_print");
    let output_path = output_stem.with_extension("svg");
    let source_path = temp_dir.join("graphics_print_command_svg_export.m");
    let matlab_output_stem = output_stem.to_string_lossy().replace('\\', "/");
    let source = format!(
        "figure(242);\n\
         plot([0, 1, 2], [2, 1, 2]);\n\
         title(\"Command Print SVG\");\n\
         print -dsvg {matlab_output_stem}\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Command Print SVG"),
        "missing command-form print title in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_rejects_missing_device_flags() {
    let temp_dir = unique_temp_dir("print-missing-device");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let source_path = temp_dir.join("graphics_print_missing_device.m");
    let source = "figure(243);\nplot([0, 1], [0, 1]);\nprint(\"missing_device\")\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &source_path,
        source,
        "requires a file-output device",
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_rejects_printer_and_clipboard_targets() {
    let temp_dir = unique_temp_dir("print-target-errors");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let clipboard_source_path = temp_dir.join("graphics_print_clipboard_error.m");
    let clipboard_source = "figure(244);\nplot([0, 1], [0, 1]);\nprint(\"-clipboard\", \"plot\")\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &clipboard_source_path,
        clipboard_source,
        "clipboard targets are not supported",
    );

    let printer_source_path = temp_dir.join("graphics_print_printer_error.m");
    let printer_source =
        "figure(245);\nplot([0, 1], [0, 1]);\nprint(\"-Poffice\", \"plot\", \"-dpng\")\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &printer_source_path,
        printer_source,
        "physical printer targets are not supported",
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_rejects_duplicate_and_unsupported_flags() {
    let temp_dir = unique_temp_dir("print-flag-errors");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let unsupported_device_path = temp_dir.join("graphics_print_bad_device.m");
    let unsupported_device_source =
        "figure(246);\nplot([0, 1], [0, 1]);\nprint(\"plot\", \"-djpeg\")\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &unsupported_device_path,
        unsupported_device_source,
        "supports only SVG, PNG, or PDF output",
    );

    let duplicate_device_path = temp_dir.join("graphics_print_duplicate_device.m");
    let duplicate_device_source =
        "figure(247);\nplot([0, 1], [0, 1]);\nprint(\"plot\", \"-dpng\", \"-dsvg\")\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &duplicate_device_path,
        duplicate_device_source,
        "supports only one `-d...` device flag",
    );

    let duplicate_resolution_path = temp_dir.join("graphics_print_duplicate_resolution.m");
    let duplicate_resolution_source =
        "figure(248);\nplot([0, 1], [0, 1]);\nprint(\"plot\", \"-dpng\", \"-r150\", \"-r300\")\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &duplicate_resolution_path,
        duplicate_resolution_source,
        "supports only one `-r...` resolution flag",
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_function_form_exports_pdf_with_manual_paper_layout() {
    let temp_dir = unique_temp_dir("print-function-pdf");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_stem = temp_dir.join("printed_figure_pdf");
    let output_path = output_stem.with_extension("pdf");
    let source_path = temp_dir.join("graphics_print_function_pdf_export.m");
    let matlab_output_stem = output_stem.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(249);\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         set(f, 'PaperOrientation', 'landscape');\n\
         set(f, 'PaperPositionMode', 'manual');\n\
         set(f, 'PaperPosition', [1, 2, 3, 4]);\n\
         print(f, \"{matlab_output_stem}\", \"-dpdf\");\n"
    );

    let pdf = execute_and_read_pdf(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert_pdf_signature(&pdf);
    assert_pdf_media_box(&pdf, 792.0, 612.0);
    assert_pdf_image_transform(&pdf, [216.0, 0.0, 0.0, 288.0, 72.0, 144.0]);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_command_form_pdf_supports_f_handle_bestfit_and_r0() {
    let temp_dir = unique_temp_dir("print-command-pdf");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("command_form_print.pdf");
    let source_path = temp_dir.join("graphics_print_command_pdf_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "figure(1);\n\
         plot([0, 1], [0, 1]);\n\
         f2 = figure(2);\n\
         plot([0, 1], [1, 0]);\n\
         set(f2, 'PaperType', 'a4');\n\
         print -f2 -dpdf {matlab_output_path} -bestfit -r0\n"
    );

    let pdf = execute_and_read_pdf(ExecutionKind::Bytecode, &source_path, &source, &output_path);
    assert_pdf_signature(&pdf);
    assert_pdf_media_box(&pdf, 595.2756, 841.8898);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_fillpage_pdf_uses_page_margins() {
    let temp_dir = unique_temp_dir("print-fillpage-pdf");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fillpage_print.pdf");
    let source_path = temp_dir.join("graphics_print_fillpage_pdf_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "figure(250);\n\
         plot([0, 1], [1, 0]);\n\
         print -dpdf {matlab_output_path} -fillpage\n"
    );

    let pdf = execute_and_read_pdf(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert_pdf_image_transform(&pdf, [576.0, 0.0, 0.0, 756.0, 18.0, 18.0]);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn paper_size_rejects_normalized_units() {
    let temp_dir = unique_temp_dir("paper-size-normalized-error");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let source_path = temp_dir.join("graphics_paper_size_normalized_error.m");
    let source =
        "f = figure(251);\nset(f, 'PaperUnits', 'normalized');\nset(f, 'PaperSize', [1, 1]);\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &source_path,
        source,
        "does not support figure `PaperSize` while `PaperUnits` is `normalized`",
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn unsupported_paper_type_is_rejected() {
    let temp_dir = unique_temp_dir("paper-type-error");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let source_path = temp_dir.join("graphics_paper_type_error.m");
    let source = "f = figure(252);\nset(f, 'PaperType', 'ledger');\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &source_path,
        source,
        "supports figure `PaperType` values",
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn print_rejects_bestfit_and_fillpage_for_png() {
    let temp_dir = unique_temp_dir("print-bestfit-png-error");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let source_path = temp_dir.join("graphics_print_bestfit_png_error.m");
    let source = "figure(253);\nplot([0, 1], [0, 1]);\nprint('plot', '-dpng', '-bestfit')\n";
    assert_runtime_error(
        ExecutionKind::Interpreter,
        &source_path,
        source,
        "supports `-bestfit` and `-fillpage` only for PDF output",
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn subplot_exports_multiple_axes_svg() {
    let temp_dir = unique_temp_dir("subplot");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("subplot.svg");
    let source_path = temp_dir.join("graphics_subplot_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(9);\n\
         subplot(211);\n\
         plot([0, 1, 2], [0, 1, 4]);\n\
         title(\"Top Plot\");\n\
         subplot(212);\n\
         bar([1, 2, 3], [3, 1, 2]);\n\
         title(\"Bottom Bars\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Top Plot"),
        "missing top subplot title in SVG: {svg}"
    );
    assert!(
        svg.contains("Bottom Bars"),
        "missing bottom subplot title in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#cccccc\"").count() >= 2,
        "expected multiple subplot frames in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn tiledlayout_exports_multiple_axes_svg() {
    let temp_dir = unique_temp_dir("tiledlayout");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("tiledlayout.svg");
    let source_path = temp_dir.join("graphics_tiledlayout_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(90);\n\
         tiledlayout(2, 1);\n\
         nexttile;\n\
         plot([0, 1, 2], [0, 1, 4]);\n\
         title(\"Top Tile\");\n\
         nexttile;\n\
         bar([1, 2, 3], [3, 1, 2]);\n\
         title(\"Bottom Tile\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Top Tile"),
        "missing top tile title in SVG: {svg}"
    );
    assert!(
        svg.contains("Bottom Tile"),
        "missing bottom tile title in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#cccccc\"").count() >= 2,
        "expected multiple tiled axes frames in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn fplot_exports_svg_curve() {
    let temp_dir = unique_temp_dir("fplot");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fplot.svg");
    let source_path = temp_dir.join("graphics_fplot_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(91);\n\
         fplot(@sin, [0, 2*pi], 'r--');\n\
         title(\"FPlot Curve\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("FPlot Curve"),
        "missing fplot title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polyline"),
        "expected fplot polyline in SVG: {svg}"
    );
    assert!(
        svg.contains("stroke=\"#d62728\"") || svg.contains("stroke=\"#ff0000\""),
        "expected styled fplot stroke color in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn fplot3_exports_svg_curve() {
    let temp_dir = unique_temp_dir("fplot3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fplot3.svg");
    let source_path = temp_dir.join("graphics_fplot3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(191);\n\
         fplot3(@cos, @sin, @(t) t ./ pi, [0, 2*pi], 'm--', 'MeshDensity', 57, 'LineWidth', 1.5);\n\
         view(52, 19);\n\
         title(\"FPlot3 Curve\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("FPlot3 Curve"),
        "missing fplot3 title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polyline"),
        "expected fplot3 polyline in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-data="),
        "expected fplot3 metadata in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-screen="),
        "expected fplot3 screen metadata in SVG: {svg}"
    );
    assert!(
        svg.contains("stroke=\"#cc00cc\"")
            || svg.contains("stroke=\"#e377c2\"")
            || svg.contains("stroke=\"#ff00ff\""),
        "expected styled fplot3 stroke color in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn xline_yline_export_labels_and_metadata() {
    let temp_dir = unique_temp_dir("xline-yline");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("xline_yline.svg");
    let source_path = temp_dir.join("graphics_xline_yline_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(194);\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         hold on\n\
         xline(1, 'r--', 'Center', 'LineWidth', 1.5);\n\
         yline(0.5, 'k:', 'Threshold');\n\
         title(\"Reference Lines\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Reference Lines"),
        "missing reference-line title in SVG: {svg}"
    );
    assert!(svg.contains("Center"), "missing xline label in SVG: {svg}");
    assert!(
        svg.contains("Threshold"),
        "missing yline label in SVG: {svg}"
    );
    assert!(
        svg.matches("data-matc-data=").count() >= 3,
        "expected plotted metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn sgtitle_and_subtitle_render_in_svg() {
    let temp_dir = unique_temp_dir("sgtitle-subtitle");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("sgtitle_subtitle.svg");
    let source_path = temp_dir.join("graphics_sgtitle_subtitle_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(195);\n\
         tiledlayout(2, 1);\n\
         nexttile;\n\
         plot([0, 1, 2], [0, 1, 4]);\n\
         title(\"Top Tile\");\n\
         subtitle(\"Top Detail\");\n\
         nexttile;\n\
         bar([1, 2, 3], [3, 1, 2]);\n\
         title(\"Bottom Tile\");\n\
         figure(196);\n\
         plot([0, 1], [1, 0]);\n\
         sgtitle(f, \"Overview\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(svg.contains("Overview"), "missing sgtitle in SVG: {svg}");
    assert!(svg.contains("Top Detail"), "missing subtitle in SVG: {svg}");
    assert!(
        svg.contains("Top Tile"),
        "missing per-axes title in SVG: {svg}"
    );
    assert!(
        svg.contains("Bottom Tile"),
        "missing second axes title in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn errorbar_exports_error_caps_and_stems() {
    let temp_dir = unique_temp_dir("errorbar");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("errorbar.svg");
    let source_path = temp_dir.join("graphics_errorbar_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(196);\n\
         errorbar([0, 1, 2], [1, 2, 1], [0.2, 0.3, 0.1], 'o-', 'LineWidth', 1.5);\n\
         title(\"Error Bars\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Error Bars"),
        "missing errorbar title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polyline"),
        "missing errorbar line path in SVG: {svg}"
    );
    assert!(
        svg.matches("<line ").count() >= 9,
        "expected error stems and caps in SVG: {svg}"
    );
    assert!(
        svg.matches("<circle").count() >= 3,
        "expected errorbar markers in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn errorbar_horizontal_and_both_direction_export() {
    let temp_dir = unique_temp_dir("errorbar-directions");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("errorbar_directions.svg");
    let source_path = temp_dir.join("graphics_errorbar_directions_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(197);\n\
         errorbar([1, 2, 3], [2, 1, 2], [0.2, 0.1, 0.3], 'horizontal', 'LineStyle', 'none');\n\
         hold on\n\
         errorbar([1, 2, 3], [2, 1, 2], [0.2, 0.1, 0.3], [0.4, 0.2, 0.5], [0.1, 0.2, 0.1], [0.2, 0.3, 0.2], 's-');\n\
         title(\"Error Directions\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Error Directions"),
        "missing errorbar directions title in SVG: {svg}"
    );
    assert!(
        svg.matches("<line ").count() >= 18,
        "expected horizontal/vertical error segments in SVG: {svg}"
    );
    assert!(
        svg.matches("<circle").count() >= 3 || svg.matches("<rect ").count() >= 3,
        "expected markers in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn loglog_exports_logarithmic_spacing() {
    let temp_dir = unique_temp_dir("loglog");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("loglog.svg");
    let source_path = temp_dir.join("graphics_loglog_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(198);\n\
         loglog([1, 10, 100], [0.1, 1, 10], 'o-');\n\
         title(\"Log Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Log Plot"),
        "missing loglog title in SVG: {svg}"
    );
    let polyline = svg
        .lines()
        .find(|line| line.contains("<polyline"))
        .expect("expected loglog polyline");
    let points = svg_polygon_points(polyline).expect("loglog polyline points");
    assert!(
        points.len() >= 3,
        "expected three plotted points in loglog SVG: {svg}"
    );
    let dx1 = (points[1].0 - points[0].0).abs();
    let dx2 = (points[2].0 - points[1].0).abs();
    let dy1 = (points[1].1 - points[0].1).abs();
    let dy2 = (points[2].1 - points[1].1).abs();
    assert!(
        (dx1 - dx2).abs() <= 1.0,
        "expected log-spaced x coordinates in SVG: {polyline}"
    );
    assert!(
        (dy1 - dy2).abs() <= 1.0,
        "expected log-spaced y coordinates in SVG: {polyline}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn xscale_yscale_drive_logarithmic_spacing() {
    let temp_dir = unique_temp_dir("axis-scale");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("axis_scale.svg");
    let source_path = temp_dir.join("graphics_axis_scale_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(199);\n\
         plot([1, 10, 100], [0.1, 1, 10], 'o-');\n\
         xscale('log');\n\
         yscale('log');\n\
         title(\"Axis Scale Log\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Axis Scale Log"),
        "missing xscale/yscale title in SVG: {svg}"
    );
    let polyline = svg
        .lines()
        .find(|line| line.contains("<polyline"))
        .expect("expected axis-scale polyline");
    let points = svg_polygon_points(polyline).expect("axis-scale polyline points");
    assert!(
        points.len() >= 3,
        "expected three plotted points in axis-scale SVG: {svg}"
    );
    let dx1 = (points[1].0 - points[0].0).abs();
    let dx2 = (points[2].0 - points[1].0).abs();
    let dy1 = (points[1].1 - points[0].1).abs();
    let dy2 = (points[2].1 - points[1].1).abs();
    assert!(
        (dx1 - dx2).abs() <= 1.0,
        "expected log-spaced x coordinates in SVG: {polyline}"
    );
    assert!(
        (dy1 - dy2).abs() <= 1.0,
        "expected log-spaced y coordinates in SVG: {polyline}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn legend_location_and_orientation_render_in_svg() {
    let temp_dir = unique_temp_dir("legend-options");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("legend_options.svg");
    let source_path = temp_dir.join("graphics_legend_options_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(200);\n\
         plot([1, 2, 3], [1, 4, 2]);\n\
         hold on\n\
         plot([1, 2, 3], [2, 1, 3]);\n\
         legend(\"First\", \"Second\", \"Location\", \"southwest\", \"Orientation\", \"horizontal\");\n\
         title(\"Legend Options\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Legend Options"),
        "missing legend-options title in SVG: {svg}"
    );
    assert!(
        svg.contains(">First<"),
        "missing first legend label in SVG: {svg}"
    );
    assert!(
        svg.contains(">Second<"),
        "missing second legend label in SVG: {svg}"
    );
    let legend_rect = svg
        .lines()
        .find(|line| line.contains("<rect") && line.contains("fill-opacity=\"0.94\""))
        .expect("expected legend box");
    assert!(
        legend_rect.contains("y=\""),
        "expected positioned legend box in SVG: {legend_rect}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn annotation_objects_render_in_svg() {
    let temp_dir = unique_temp_dir("annotation");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("annotation.svg");
    let source_path = temp_dir.join("graphics_annotation_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(201);\n\
         annotation('line', [0.1, 0.4], [0.2, 0.5], 'Color', 'r');\n\
         annotation('textarrow', [0.25, 0.45], [0.8, 0.6], 'Peak', 'LineStyle', '--');\n\
         annotation('textbox', [0.65, 0.2, 0.2, 0.1], 'Notes', 'FaceColor', 'y');\n\
         annotation('ellipse', [0.12, 0.12, 0.18, 0.12]);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Peak"),
        "missing textarrow label in SVG: {svg}"
    );
    assert!(svg.contains("Notes"), "missing textbox label in SVG: {svg}");
    assert!(
        svg.contains("<ellipse"),
        "missing ellipse annotation in SVG: {svg}"
    );
    assert!(
        svg.matches("<line ").count() >= 3,
        "expected annotation lines/arrows in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn barh_exports_horizontal_bars_svg() {
    let temp_dir = unique_temp_dir("barh");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("barh.svg");
    let source_path = temp_dir.join("graphics_barh_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(202);\n\
         barh([1, 2, 3], [3, 1, 2]);\n\
         title(\"Horizontal Bars\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Horizontal Bars"),
        "missing barh title in SVG: {svg}"
    );
    assert!(
        svg.matches("<rect ").count() >= 3,
        "expected horizontal bar rects in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn stem3_exports_projected_stems_svg() {
    let temp_dir = unique_temp_dir("stem3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("stem3.svg");
    let source_path = temp_dir.join("graphics_stem3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(203);\n\
         stem3([0, 1, 2], [0, 1, 0], [1, 2, 3]);\n\
         title(\"Stem3 Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Stem3 Plot"),
        "missing stem3 title in SVG: {svg}"
    );
    assert!(
        svg.matches("<line ").count() >= 3,
        "expected stem3 stems in SVG: {svg}"
    );
    assert!(
        svg.matches("<circle").count() >= 3,
        "expected stem3 tip markers in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn plotyy_exports_dual_axis_wrapper_svg() {
    let temp_dir = unique_temp_dir("plotyy");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("plotyy.svg");
    let source_path = temp_dir.join("graphics_plotyy_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(204);\n\
         plotyy([0, 1, 2], [1, 4, 2], [0, 1, 2], [10, 20, 30]);\n\
         ylabel(\"Left\");\n\
         yyaxis right;\n\
         ylabel(\"Right\");\n\
         title(\"PlotYY Wrapper\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("PlotYY Wrapper"),
        "missing plotyy title in SVG: {svg}"
    );
    assert!(
        svg.contains("Left"),
        "missing left ylabel in plotyy SVG: {svg}"
    );
    assert!(
        svg.contains("Right"),
        "missing right ylabel in plotyy SVG: {svg}"
    );
    assert!(
        svg.matches("<polyline").count() >= 2,
        "expected both plotyy series in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn bar3_exports_surface_patches_svg() {
    let temp_dir = unique_temp_dir("bar3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("bar3.svg");
    let source_path = temp_dir.join("graphics_bar3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(205);\n\
         bar3([1, 2, 3; 2, 1, 2]);\n\
         title(\"Bar3 Surface\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Bar3 Surface"),
        "missing bar3 title in SVG: {svg}"
    );
    assert!(
        svg.matches("<polygon").count() >= 6,
        "expected bar3 surface polygons in SVG: {svg}"
    );
    assert!(
        svg.contains("class=\"matc-3d-patch\""),
        "missing bar3 patch viewer metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn bar3h_exports_surface_patches_svg() {
    let temp_dir = unique_temp_dir("bar3h");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("bar3h.svg");
    let source_path = temp_dir.join("graphics_bar3h_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(206);\n\
         bar3h([1, 2, 3; 2, 1, 2]);\n\
         title(\"Bar3H Surface\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Bar3H Surface"),
        "missing bar3h title in SVG: {svg}"
    );
    assert!(
        svg.matches("<polygon").count() >= 6,
        "expected bar3h surface polygons in SVG: {svg}"
    );
    assert!(
        svg.contains("class=\"matc-3d-patch\""),
        "missing bar3h patch viewer metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn linkaxes_synchronizes_subplot_limits_in_svg() {
    let temp_dir = unique_temp_dir("linkaxes");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("linkaxes.svg");
    let source_path = temp_dir.join("graphics_linkaxes_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(206);\n\
         ax1 = subplot(211);\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         title(\"Top Linked\");\n\
         ax2 = subplot(212);\n\
         plot([10, 20, 30], [1, 2, 3]);\n\
         title(\"Bottom Linked\");\n\
         linkaxes([ax1, ax2], 'x');\n\
         subplot(211);\n\
         xlim([0, 30]);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Top Linked"),
        "missing top subplot title in SVG: {svg}"
    );
    assert!(
        svg.contains("Bottom Linked"),
        "missing bottom subplot title in SVG: {svg}"
    );
    assert!(
        svg.matches(">30</text>").count() >= 2,
        "expected synchronized x-axis limits across both subplots in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-link-group=") && svg.contains("data-matc-link-mode=\"x\""),
        "expected linkaxes viewer metadata in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-xlim=\"0,30\"") && svg.contains("data-matc-base-xlim="),
        "expected live and base x-limit metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn yyaxis_exports_dual_axis_labels_svg() {
    let temp_dir = unique_temp_dir("yyaxis");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("yyaxis.svg");
    let source_path = temp_dir.join("graphics_yyaxis_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(92);\n\
         yyaxis left;\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         ylabel(\"Left Axis\");\n\
         yyaxis right;\n\
         plot([0, 1, 2], [0, 10, 20]);\n\
         ylabel(\"Right Axis\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Left Axis"),
        "missing left yyaxis label in SVG: {svg}"
    );
    assert!(
        svg.contains("Right Axis"),
        "missing right yyaxis label in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#333333\"").count() >= 3,
        "expected dual y-axis frame lines in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn imagesc_exports_svg_with_colorbar() {
    let temp_dir = unique_temp_dir("imagesc");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("imagesc.svg");
    let source_path = temp_dir.join("graphics_images_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(12);\n\
         imagesc([0, 1, 2; 3, 4, 5]);\n\
         colormap(\"hot\");\n\
         colorbar();\n\
         title(\"Heatmap\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(ExecutionKind::Bytecode, &source_path, &source, &output_path);
    assert!(
        svg.contains("Heatmap"),
        "missing imagesc title in SVG: {svg}"
    );
    assert!(
        svg.contains("rgb("),
        "missing colormap-colored cells in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#777777\"").count() >= 1,
        "missing colorbar outline in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn cool_colormap_exports_svg_colors() {
    let temp_dir = unique_temp_dir("cool-colormap");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("cool_colormap.svg");
    let source_path = temp_dir.join("graphics_cool_colormap_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(78);\n\
         imagesc([1, 2; 3, 4]);\n\
         colormap(\"cool\");\n\
         colorbar();\n\
         title(\"Cool Colormap\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Cool Colormap"),
        "missing cool-colormap title in SVG: {svg}"
    );
    assert!(
        svg.contains("rgb(0,255,255)") && svg.contains("rgb(255,0,255)"),
        "expected cool colormap endpoint colors in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn image_exports_direct_colormap_svg() {
    let temp_dir = unique_temp_dir("image");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("image.svg");
    let source_path = temp_dir.join("graphics_image_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(13);\n\
         image([1, 4; 8, 2]);\n\
         colormap(\"jet\");\n\
         caxis(\"auto\");\n\
         title(\"Indexed Image\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Indexed Image"),
        "missing image title in SVG: {svg}"
    );
    assert!(
        svg.contains("rgb("),
        "missing direct colormap cells in SVG: {svg}"
    );
    assert!(
        svg.contains("<rect"),
        "missing indexed image rectangles in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn axis_off_hides_axes_lines_in_svg() {
    let temp_dir = unique_temp_dir("axis-off");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("axis_off.svg");
    let source_path = temp_dir.join("graphics_axis_off_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(14);\n\
         plot([0, 1, 2], [1, 0, 1]);\n\
         axis off;\n\
         title(\"Hidden Axes\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Hidden Axes"),
        "missing axis-off title in SVG: {svg}"
    );
    assert!(
        !svg.contains("stroke=\"#333333\""),
        "expected axes lines to be hidden in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn grid_on_renders_grid_lines_in_svg() {
    let temp_dir = unique_temp_dir("grid-on");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("grid_on.svg");
    let source_path = temp_dir.join("graphics_grid_on_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(15);\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         grid on;\n\
         title(\"Grid On\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(svg.contains("Grid On"), "missing grid title in SVG: {svg}");
    assert!(
        svg.contains("stroke=\"#ececec\""),
        "expected grid lines in SVG after `grid on`: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn box_on_off_changes_svg_frame() {
    let temp_dir = unique_temp_dir("box-on-off");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("box_on_off.svg");
    let source_path = temp_dir.join("graphics_box_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(69);\n\
         subplot(211);\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         box on;\n\
         title(\"Box On\");\n\
         subplot(212);\n\
         plot([0, 1, 2], [1, 0, 1]);\n\
         box off;\n\
         title(\"Box Off\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Box On"),
        "missing `box on` title in SVG: {svg}"
    );
    assert!(
        svg.contains("Box Off"),
        "missing `box off` title in SVG: {svg}"
    );
    assert_eq!(
        svg.matches("stroke=\"#cccccc\"").count(),
        1,
        "expected exactly one boxed plot frame in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn axis_image_keeps_image_cells_square_in_svg() {
    let temp_dir = unique_temp_dir("axis-image");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("axis_image.svg");
    let source_path = temp_dir.join("graphics_axis_image_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(16);\n\
         image([1, 2, 3, 4; 5, 6, 7, 8]);\n\
         axis image;\n\
         title(\"Axis Image\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Axis Image"),
        "missing axis-image title in SVG: {svg}"
    );

    let rgb_rect = svg
        .lines()
        .find(|line| line.contains("<rect") && line.contains("fill=\"rgb("))
        .expect("expected image cell rectangle in SVG");
    let width = svg_rect_attribute(rgb_rect, "width").expect("image cell width");
    let height = svg_rect_attribute(rgb_rect, "height").expect("image cell height");
    assert!(
        (width - height).abs() <= 0.6,
        "expected near-square image cells for `axis image`, found width={width} height={height} in {rgb_rect}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn axis_square_keeps_plot_frame_square_in_svg() {
    let temp_dir = unique_temp_dir("axis-square");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("axis_square.svg");
    let source_path = temp_dir.join("graphics_axis_square_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(77);\n\
         plot([0, 2], [0, 1]);\n\
         axis([0, 2, 0, 1]);\n\
         axis square;\n\
         title(\"Axis Square\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Axis Square"),
        "missing axis-square title in SVG: {svg}"
    );

    let frame_rect = svg
        .lines()
        .find(|line| line.contains("<rect") && line.contains("stroke=\"#cccccc\""))
        .expect("expected plot frame rectangle in SVG");
    let width = svg_rect_attribute(frame_rect, "width").expect("plot frame width");
    let height = svg_rect_attribute(frame_rect, "height").expect("plot frame height");
    assert!(
        (width - height).abs() <= 0.6,
        "expected near-square plot frame for `axis square`, found width={width} height={height} in {frame_rect}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn imshow_rgb_exports_svg_true_color_cells() {
    let temp_dir = unique_temp_dir("imshow-rgb");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("imshow_rgb.svg");
    let source_path = temp_dir.join("graphics_imshow_rgb_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "rgb = cat(3, [255, 0; 0, 255], [0, 255; 0, 255], [0, 0; 255, 255]);\n\
         f = figure(71);\n\
         imshow(rgb);\n\
         title(\"RGB Image\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("RGB Image"),
        "missing RGB image title in SVG: {svg}"
    );
    for color in [
        "rgb(255,0,0)",
        "rgb(0,255,0)",
        "rgb(0,0,255)",
        "rgb(255,255,255)",
    ] {
        assert!(
            svg.contains(color),
            "missing RGB fill `{color}` in SVG: {svg}"
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn image_and_imagesc_rgb_export_svg_true_color_cells() {
    let temp_dir = unique_temp_dir("image-family-rgb");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("image_family_rgb.svg");
    let source_path = temp_dir.join("graphics_image_family_rgb_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(75);\n\
         subplot(121);\n\
         image(cat(3, [1, 0; 0, 1], [0, 1; 0, 1], [0, 0; 1, 1]));\n\
         subplot(122);\n\
         imagesc(cat(3, [255, 128; 0, 64], [0, 255; 128, 64], [255, 0; 64, 128]));\n\
         title(\"RGB Family\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("RGB Family"),
        "missing RGB family title in SVG: {svg}"
    );
    for color in [
        "rgb(255,0,0)",
        "rgb(0,255,0)",
        "rgb(0,0,255)",
        "rgb(255,255,255)",
        "rgb(128,255,0)",
        "rgb(64,64,128)",
    ] {
        assert!(
            svg.contains(color),
            "missing RGB family fill `{color}` in SVG: {svg}"
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn fsurf_exports_surface_svg() {
    let temp_dir = unique_temp_dir("fsurf");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fsurf.svg");
    let source_path = temp_dir.join("graphics_fsurf_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(226);\n\
         fsurf(@(x, y) sin(x) + cos(y), [-2, 2, -3, 3], 'MeshDensity', 17);\n\
         title(\"Fsurf Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Fsurf Plot"),
        "missing fsurf title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected fsurf polygons in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn fmesh_exports_wireframe_svg() {
    let temp_dir = unique_temp_dir("fmesh");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fmesh.svg");
    let source_path = temp_dir.join("graphics_fmesh_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(227);\n\
         fmesh(@(x, y) x.^2 - y.^2, [-1, 1, -1, 1], 'MeshDensity', 15);\n\
         title(\"Fmesh Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Fmesh Plot"),
        "missing fmesh title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected fmesh polygons in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn fcontour_exports_line_segments_svg() {
    let temp_dir = unique_temp_dir("fcontour");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fcontour.svg");
    let source_path = temp_dir.join("graphics_fcontour_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(228);\n\
         fcontour(@(x, y) sin(x) + cos(y), [-2, 2, -3, 3], 'MeshDensity', 17);\n\
         title(\"Fcontour Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Fcontour Plot"),
        "missing fcontour title in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#1f77b4\"").count() >= 3,
        "expected fcontour line segments in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn fcontour3_exports_projected_line_segments_svg() {
    let temp_dir = unique_temp_dir("fcontour3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fcontour3.svg");
    let source_path = temp_dir.join("graphics_fcontour3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(229);\n\
         fcontour3(@(x, y) x.^2 - y.^2, [-1, 1, -1, 1], 'MeshDensity', 15);\n\
         title(\"Fcontour3 Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Fcontour3 Plot"),
        "missing fcontour3 title in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-3d=\"true\""),
        "missing fcontour3 3-D axes metadata in SVG: {svg}"
    );
    assert!(
        svg.matches("data-matc-dim=\"3\"").count() >= 2,
        "expected fcontour3 projected segment metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn fimplicit_exports_zero_level_svg() {
    let temp_dir = unique_temp_dir("fimplicit");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fimplicit.svg");
    let source_path = temp_dir.join("graphics_fimplicit_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(230);\n\
         fimplicit(@(x, y) x.^2 + y.^2 - 1, [-2, 2, -2, 2], 'MeshDensity', 19);\n\
         title(\"Fimplicit Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Fimplicit Plot"),
        "missing fimplicit title in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#1f77b4\"").count() >= 2,
        "expected fimplicit zero-level contour segments in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn contour_exports_svg_line_segments() {
    let temp_dir = unique_temp_dir("contour");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("contour.svg");
    let source_path = temp_dir.join("graphics_contour_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(17);\n\
         contour([1, 2, 3], [1, 2, 3], [1, 2, 3; 2, 4, 2; 1, 2, 1], [1.5, 2.5, 3.5]);\n\
         title(\"Contour Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Contour Plot"),
        "missing contour title in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#1f77b4\"").count() >= 3,
        "expected contour line segments in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn surf_exports_svg_polygons_with_colorbar() {
    let temp_dir = unique_temp_dir("surf");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("surf.svg");
    let source_path = temp_dir.join("graphics_surf_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(18);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         colormap(\"hot\");\n\
         colorbar();\n\
         title(\"Surface Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Surface Plot"),
        "missing surf title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected surface polygons in SVG: {svg}"
    );
    assert!(
        svg.contains("rgb("),
        "expected colormapped surface fills in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#777777\"").count() >= 1,
        "expected colorbar outline in surf SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn surf_with_meshgrid_coordinates_exports_svg() {
    let temp_dir = unique_temp_dir("meshgrid-surf");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("meshgrid_surf.svg");
    let source_path = temp_dir.join("graphics_meshgrid_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "[xg, yg] = meshgrid([-1, 0, 1], [-1, 0, 1]);\n\
         z = xg + yg;\n\
         f = figure(37);\n\
         surf(xg, yg, z);\n\
         title(\"Meshgrid Surface\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Meshgrid Surface"),
        "missing meshgrid surf title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected meshgrid surf polygons in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn surfc_exports_surface_and_contour_svg() {
    let temp_dir = unique_temp_dir("surfc");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("surfc.svg");
    let source_path = temp_dir.join("graphics_surfc_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(31);\n\
         surfc([0, 1, 0; 1, 2, 1; 0, 1, 0], [0.5, 1.0, 1.5]);\n\
         colormap(\"hot\");\n\
         colorbar();\n\
         title(\"Surface Contours\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Surface Contours"),
        "missing surfc title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected surface polygons in surfc SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#1f77b4\"").count() >= 1,
        "expected contour line segments in surfc SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#777777\"").count() >= 1,
        "expected colorbar outline in surfc SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn mesh_exports_svg_wireframe_with_colorbar() {
    let temp_dir = unique_temp_dir("mesh");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("mesh.svg");
    let source_path = temp_dir.join("graphics_mesh_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(19);\n\
         mesh([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         colormap(\"jet\");\n\
         colorbar();\n\
         title(\"Mesh Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Mesh Plot"),
        "missing mesh title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected mesh polygons in SVG: {svg}"
    );
    assert!(
        svg.contains("fill=\"none\""),
        "expected wireframe mesh polygons in SVG: {svg}"
    );
    assert!(
        svg.contains("rgb("),
        "expected colormapped mesh strokes in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#777777\"").count() >= 1,
        "expected colorbar outline in mesh SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn meshc_exports_wireframe_and_contour_svg() {
    let temp_dir = unique_temp_dir("meshc");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("meshc.svg");
    let source_path = temp_dir.join("graphics_meshc_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(32);\n\
         meshc([0, 1, 0; 1, 2, 1; 0, 1, 0], [0.5, 1.0, 1.5]);\n\
         colormap(\"jet\");\n\
         colorbar();\n\
         title(\"Mesh Contours\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Mesh Contours"),
        "missing meshc title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected mesh polygons in meshc SVG: {svg}"
    );
    assert!(
        svg.contains("fill=\"none\""),
        "expected wireframe polygons in meshc SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#1f77b4\"").count() >= 1,
        "expected contour line segments in meshc SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#777777\"").count() >= 1,
        "expected colorbar outline in meshc SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn contourf_exports_svg_filled_polygons_with_colorbar() {
    let temp_dir = unique_temp_dir("contourf");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("contourf.svg");
    let source_path = temp_dir.join("graphics_contourf_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(20);\n\
         contourf([1, 2, 3; 2, 4, 2; 1, 2, 1], [1.5, 2.5, 3.5]);\n\
         colormap(\"hot\");\n\
         colorbar();\n\
         title(\"Filled Contours\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Filled Contours"),
        "missing contourf title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected filled contour polygons in SVG: {svg}"
    );
    assert!(
        svg.contains("fill-opacity=\"0.94\""),
        "expected filled contour shading in SVG: {svg}"
    );
    assert!(
        svg.contains("rgb("),
        "expected colormapped contourf fills in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#777777\"").count() >= 1,
        "expected colorbar outline in contourf SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn view_2_projects_surface_top_down_in_svg() {
    let temp_dir = unique_temp_dir("view-2");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("view2.svg");
    let source_path = temp_dir.join("graphics_view2_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(21);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         view(2);\n\
         title(\"Top View Surface\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Top View Surface"),
        "missing view title in SVG: {svg}"
    );

    let polygon = svg
        .lines()
        .find(|line| line.contains("<polygon") && line.contains("fill-opacity=\"0.96\""))
        .expect("expected surface polygon in SVG");
    let points = svg_polygon_points(polygon).expect("surface polygon points");
    let unique_x = unique_coordinate_count(points.iter().map(|point| point.0));
    let unique_y = unique_coordinate_count(points.iter().map(|point| point.1));
    assert!(
        unique_x <= 2 && unique_y <= 2,
        "expected top-down view polygons to be axis-aligned, found x-count={unique_x} y-count={unique_y} in {polygon}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn meshz_exports_surface_and_curtain_svg() {
    let temp_dir = unique_temp_dir("meshz");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("meshz.svg");
    let source_path = temp_dir.join("graphics_meshz_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(217);\n\
         meshz([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         title(\"Mesh Curtain\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Mesh Curtain"),
        "missing meshz title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected meshz polygons in SVG: {svg}"
    );
    assert!(
        svg.matches("class=\"matc-3d-patch\"").count() >= 8,
        "expected meshz surface and curtain patch metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn waterfall_exports_row_lines_and_curtain_svg() {
    let temp_dir = unique_temp_dir("waterfall");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("waterfall.svg");
    let source_path = temp_dir.join("graphics_waterfall_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(219);\n\
         waterfall([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         title(\"Waterfall Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Waterfall Plot"),
        "missing waterfall title in SVG: {svg}"
    );
    assert!(
        svg.matches("class=\"matc-series-path\"").count() >= 2,
        "expected row-strip waterfall line metadata in SVG: {svg}"
    );
    assert!(
        svg.matches("class=\"matc-3d-patch\"").count() >= 2,
        "expected waterfall curtain polygons in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn ribbon_exports_surface_strips_svg() {
    let temp_dir = unique_temp_dir("ribbon");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("ribbon.svg");
    let source_path = temp_dir.join("graphics_ribbon_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(221);\n\
         ribbon([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         title(\"Ribbon Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Ribbon Plot"),
        "missing ribbon title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected ribbon polygons in SVG: {svg}"
    );
    assert!(
        svg.matches("class=\"matc-3d-patch\"").count() >= 2,
        "expected ribbon patch metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn fill3_exports_projected_patch_svg() {
    let temp_dir = unique_temp_dir("fill3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("fill3.svg");
    let source_path = temp_dir.join("graphics_fill3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(223);\n\
         fill3([0, 1, 0], [0, 0, 1], [0, 1, 2], 'r');\n\
         title(\"Fill3 Patch\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Fill3 Patch"),
        "missing fill3 title in SVG: {svg}"
    );
    assert!(
        svg.contains("class=\"matc-3d-patch\""),
        "missing fill3 3-D patch metadata in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected fill3 polygon in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn plot3_and_scatter3_export_projected_svg() {
    let temp_dir = unique_temp_dir("plot3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("plot3.svg");
    let source_path = temp_dir.join("graphics_plot3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(35);\n\
         plot3([0, 1, 2, 3], [0, 1, 0, 1], [0, 1, 2, 3]);\n\
         hold on\n\
         scatter3([0, 1, 2], [1, 0, 1], [3, 2, 1]);\n\
         view(45, 20);\n\
         title(\"3D Series\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("3D Series"),
        "missing plot3 title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polyline"),
        "expected projected 3D line in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-3d=\"true\""),
        "missing 3D axes metadata in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-view=\"45,20\""),
        "missing stored 3D view angles in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-base-view=\"45,20\""),
        "missing base 3D view angles in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-plot-frame="),
        "missing 3D plot-frame metadata in SVG: {svg}"
    );
    assert!(
        svg.matches("<circle").count() >= 3,
        "expected projected 3D scatter markers in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn scatter_supports_sizes_colors_and_filled_markers_in_svg() {
    let temp_dir = unique_temp_dir("scatter-sized");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("scatter_sized.svg");
    let source_path = temp_dir.join("graphics_scatter_sized_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(79);\n\
         scatter([0, 1, 2], [0, 1, 0], [36, 64, 100], [1, 0, 0; 0, 1, 0; 0, 0, 1], 'filled');\n\
         title(\"Scatter Sized\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Scatter Sized"),
        "missing scatter-sized title in SVG: {svg}"
    );
    for color in [
        "fill=\"rgb(255,0,0)\"",
        "fill=\"rgb(0,255,0)\"",
        "fill=\"rgb(0,0,255)\"",
    ] {
        assert!(
            svg.contains(color),
            "missing scatter point color `{color}` in SVG: {svg}"
        );
    }
    assert!(
        svg.contains("r=\"3\"") && svg.contains("r=\"4\"") && svg.contains("r=\"5\""),
        "expected varied scatter marker radii in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn quiver_exports_svg_arrows() {
    let temp_dir = unique_temp_dir("quiver");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("quiver.svg");
    let source_path = temp_dir.join("graphics_quiver_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(82);\n\
         quiver([0, 1], [0, 1], [1, 0], [0, 1], 0);\n\
         xlim([0, 1.5]);\n\
         ylim([0, 2]);\n\
         title(\"Vector Field\");\n\
         legend(\"Field\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Vector Field"),
        "missing quiver title in SVG: {svg}"
    );
    assert!(
        svg.contains("Field"),
        "missing quiver legend label in SVG: {svg}"
    );
    assert!(
        svg.contains("<line class=\"matc-arrow-shaft\" x1=\"92\" y1=\"570\" x2=\"604\" y2=\"570\" stroke=\"#1f77b4\" stroke-width=\"2.2\" stroke-linecap=\"round\"/>"),
        "missing quiver shaft line in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke-linecap=\"round\"").count() >= 6,
        "expected arrow shafts and heads in quiver SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn quiver3_exports_projected_svg_arrows() {
    let temp_dir = unique_temp_dir("quiver3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("quiver3.svg");
    let source_path = temp_dir.join("graphics_quiver3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(86);\n\
         quiver3([0, 1], [1, 0], [0.5, 1.5], [1, -0.5], [0.25, 0.75], [0.5, -0.25], 0);\n\
         view(45, 20);\n\
         title(\"3D Vector Field\");\n\
         legend(\"Spatial Field\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("3D Vector Field"),
        "missing quiver3 title in SVG: {svg}"
    );
    assert!(
        svg.contains("Spatial Field"),
        "missing quiver3 legend label in SVG: {svg}"
    );
    assert!(
        svg.contains("class=\"matc-3d-arrow\""),
        "missing quiver3 viewer metadata in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke-linecap=\"round\"").count() >= 9,
        "expected projected quiver3 shafts, heads, and legend arrow in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn zlabel_and_zlim_render_for_3d_surface_svg() {
    let temp_dir = unique_temp_dir("zaxis");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let default_output_path = temp_dir.join("zaxis_default.svg");
    let default_source_path = temp_dir.join("graphics_zaxis_default_export.m");
    let default_matlab_output_path = default_output_path.to_string_lossy().replace('\\', "/");
    let default_source = format!(
        "f = figure(39);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         view(45, 20);\n\
         saveas(f, \"{default_matlab_output_path}\", \"svg\");\n"
    );

    let explicit_output_path = temp_dir.join("zaxis_explicit.svg");
    let explicit_source_path = temp_dir.join("graphics_zaxis_explicit_export.m");
    let explicit_matlab_output_path = explicit_output_path.to_string_lossy().replace('\\', "/");
    let explicit_source = format!(
        "f = figure(40);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         view(45, 20);\n\
         zlim([-10, 10]);\n\
         zlabel(\"Height\");\n\
         title(\"Z Axis Surface\");\n\
         saveas(f, \"{explicit_matlab_output_path}\", \"svg\");\n"
    );

    let default_svg = execute_and_read_svg(
        ExecutionKind::Bytecode,
        &default_source_path,
        &default_source,
        &default_output_path,
    );
    let explicit_svg = execute_and_read_svg(
        ExecutionKind::Bytecode,
        &explicit_source_path,
        &explicit_source,
        &explicit_output_path,
    );

    assert!(
        explicit_svg.contains("Height"),
        "missing zlabel text in SVG: {explicit_svg}"
    );
    assert!(
        explicit_svg.contains("Z Axis Surface"),
        "missing z-axis title in SVG: {explicit_svg}"
    );

    let default_slope = surface_polygon_mean_edge_dy(&default_svg);
    let explicit_slope = surface_polygon_mean_edge_dy(&explicit_svg);
    assert!(
        explicit_slope < default_slope * 0.8,
        "expected explicit zlim to flatten projected surface patches, found default slope={default_slope} explicit slope={explicit_slope}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn shading_modes_change_surface_and_mesh_svg() {
    let temp_dir = unique_temp_dir("shading");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let faceted_output_path = temp_dir.join("surf_faceted.svg");
    let faceted_source_path = temp_dir.join("graphics_shading_faceted_export.m");
    let faceted_matlab_output_path = faceted_output_path.to_string_lossy().replace('\\', "/");
    let faceted_source = format!(
        "f = figure(45);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         title(\"Faceted Surface\");\n\
         saveas(f, \"{faceted_matlab_output_path}\", \"svg\");\n"
    );

    let flat_output_path = temp_dir.join("surf_flat.svg");
    let flat_source_path = temp_dir.join("graphics_shading_flat_export.m");
    let flat_matlab_output_path = flat_output_path.to_string_lossy().replace('\\', "/");
    let flat_source = format!(
        "f = figure(46);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         shading(\"flat\");\n\
         title(\"Flat Surface\");\n\
         saveas(f, \"{flat_matlab_output_path}\", \"svg\");\n"
    );

    let interp_output_path = temp_dir.join("surf_interp.svg");
    let interp_source_path = temp_dir.join("graphics_shading_interp_export.m");
    let interp_matlab_output_path = interp_output_path.to_string_lossy().replace('\\', "/");
    let interp_source = format!(
        "f = figure(47);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         shading(\"interp\");\n\
         title(\"Interp Surface\");\n\
         saveas(f, \"{interp_matlab_output_path}\", \"svg\");\n"
    );

    let mesh_flat_output_path = temp_dir.join("mesh_flat.svg");
    let mesh_flat_source_path = temp_dir.join("graphics_shading_mesh_flat_export.m");
    let mesh_flat_matlab_output_path = mesh_flat_output_path.to_string_lossy().replace('\\', "/");
    let mesh_flat_source = format!(
        "f = figure(48);\n\
         mesh([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         shading(\"flat\");\n\
         title(\"Flat Mesh\");\n\
         saveas(f, \"{mesh_flat_matlab_output_path}\", \"svg\");\n"
    );

    let faceted_svg = execute_and_read_svg(
        ExecutionKind::Bytecode,
        &faceted_source_path,
        &faceted_source,
        &faceted_output_path,
    );
    let flat_svg = execute_and_read_svg(
        ExecutionKind::Bytecode,
        &flat_source_path,
        &flat_source,
        &flat_output_path,
    );
    let interp_svg = execute_and_read_svg(
        ExecutionKind::Bytecode,
        &interp_source_path,
        &interp_source,
        &interp_output_path,
    );
    let mesh_flat_svg = execute_and_read_svg(
        ExecutionKind::Bytecode,
        &mesh_flat_source_path,
        &mesh_flat_source,
        &mesh_flat_output_path,
    );

    assert!(
        faceted_svg.contains("stroke=\"#444444\""),
        "expected faceted surf edges in SVG: {faceted_svg}"
    );
    assert!(
        !flat_svg.contains("stroke=\"#444444\""),
        "expected `shading flat` to remove surf facet edges: {flat_svg}"
    );
    assert!(
        interp_svg.contains("<linearGradient"),
        "expected `shading interp` to emit gradients in SVG: {interp_svg}"
    );
    assert!(
        interp_svg.contains("fill=\"url(#patch-grad-"),
        "expected gradient-backed fills for `shading interp`: {interp_svg}"
    );
    assert!(
        !mesh_flat_svg.contains("fill=\"none\""),
        "expected `shading flat` to fill mesh patches instead of wireframe only: {mesh_flat_svg}"
    );
    assert!(
        mesh_flat_svg.contains("fill=\"rgb("),
        "expected filled mesh patches after `shading flat`: {mesh_flat_svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn image_alpha_data_exports_svg_opacity() {
    let temp_dir = unique_temp_dir("image-alpha");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("image_alpha.svg");
    let source_path = temp_dir.join("graphics_image_alpha_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "rgb = cat(3, [1, 1; 1, 1], [0, 0; 0, 0], [0, 0; 0, 0]);\n\
         f = figure(78);\n\
         img = image(rgb);\n\
         set(img, 'AlphaData', [1, 0.25; 0.5, 0]);\n\
         title(\"Alpha Image\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Alpha Image"),
        "missing alpha image title in SVG: {svg}"
    );
    assert!(
        svg.contains("rgb(255,0,0)"),
        "missing red image fill in SVG: {svg}"
    );
    for opacity in [
        "fill-opacity=\"0.25\"",
        "fill-opacity=\"0.5\"",
        "fill-opacity=\"0\"",
    ] {
        assert!(svg.contains(opacity), "missing `{opacity}` in SVG: {svg}");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn custom_ticks_render_in_2d_and_3d_svg() {
    let temp_dir = unique_temp_dir("ticks");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("ticks.svg");
    let source_path = temp_dir.join("graphics_ticks_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(50);\n\
         subplot(211);\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         xticks([0.37, 1.63]);\n\
         yticks([0.12, 0.88]);\n\
         title(\"Custom 2D Ticks\");\n\
         subplot(212);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         view(45, 20);\n\
         zlim([-2, 3]);\n\
         zticks([-1.7, 0.6, 2.4]);\n\
         zlabel(\"Height\");\n\
         title(\"Custom Z Ticks\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    for label in ["0.37", "1.63", "0.12", "0.88", "-1.7", "0.6", "2.4"] {
        assert!(
            svg.contains(&format!(">{label}<")),
            "missing custom tick label `{label}` in SVG: {svg}"
        );
    }
    assert!(svg.contains("Height"), "missing z-axis label in SVG: {svg}");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn custom_ticklabels_render_in_2d_and_3d_svg() {
    let temp_dir = unique_temp_dir("ticklabels");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("ticklabels.svg");
    let source_path = temp_dir.join("graphics_ticklabels_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(52);\n\
         subplot(211);\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         xticks([0.37, 1.63]);\n\
         xticklabels([\"left edge\" \"right edge\"]);\n\
         yticks([0.12, 0.88]);\n\
         yticklabels({{\"low mark\", \"high mark\"}});\n\
         title(\"Custom 2D Labels\");\n\
         subplot(212);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         view(45, 20);\n\
         zlim([-2, 3]);\n\
         zticks([-1.7, 2.4]);\n\
         zticklabels([\"floor level\" \"peak level\"]);\n\
         zlabel(\"Height\");\n\
         title(\"Custom Z Labels\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    for label in [
        "left edge",
        "right edge",
        "low mark",
        "high mark",
        "floor level",
        "peak level",
        "Height",
    ] {
        assert!(
            svg.contains(&format!(">{label}<")),
            "missing custom tick label `{label}` in SVG: {svg}"
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn tickangles_rotate_tick_labels_in_svg() {
    let temp_dir = unique_temp_dir("tickangles");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("tickangles.svg");
    let source_path = temp_dir.join("graphics_tickangles_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(53);\n\
         subplot(211);\n\
         plot([0, 1, 2], [0, 1, 0]);\n\
         xticks([0, 1, 2]);\n\
         xticklabels([\"left\" \"mid\" \"right\"]);\n\
         xtickangle(35);\n\
         yticks([0, 0.5, 1]);\n\
         yticklabels({{\"low\", \"mid\", \"high\"}});\n\
         ytickangle(-25);\n\
         subplot(212);\n\
         surf([0, 1, 0; 1, 2, 1; 0, 1, 0]);\n\
         view(45, 20);\n\
         zlim([-2, 2]);\n\
         zticks([-2, 0, 2]);\n\
         zticklabels([\"floor\" \"center\" \"peak\"]);\n\
         ztickangle(40);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    for label in ["left", "mid", "right", "low", "high", "floor", "peak"] {
        assert!(
            svg.contains(&format!(">{label}<")),
            "missing rotated tick label `{label}` in SVG: {svg}"
        );
    }
    for rotation in ["rotate(-35", "rotate(25", "rotate(-40"] {
        assert!(
            svg.contains(rotation),
            "missing tick-angle rotation `{rotation}` in SVG: {svg}"
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn stairs_exports_stepwise_polyline_svg() {
    let temp_dir = unique_temp_dir("stairs");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("stairs.svg");
    let source_path = temp_dir.join("graphics_stairs_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(58);\n\
         stairs([0, 1, 2], [1, 3, 2]);\n\
         xlim([0, 2]);\n\
         ylim([0, 3]);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("92,398.6667 476,398.6667 476,56 860,56 860,227.3333"),
        "missing stairs step polyline in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn area_exports_filled_polygon_svg() {
    let temp_dir = unique_temp_dir("area");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("area.svg");
    let source_path = temp_dir.join("graphics_area_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(61);\n\
         area([0, 1, 2], [1, 3, 2]);\n\
         xlim([0, 2]);\n\
         ylim([0, 3]);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("polygon points=\"92,570 92,398.6667 476,56 860,227.3333 860,570\""),
        "missing filled area polygon in SVG: {svg}"
    );
    assert!(
        svg.contains("fill-opacity=\"0.3\""),
        "missing filled area opacity in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn animatedline_exports_svg_after_addpoints() {
    let temp_dir = unique_temp_dir("animatedline");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("animatedline.svg");
    let source_path = temp_dir.join("graphics_animatedline_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(80);\n\
         h = animatedline('Color', 'r', 'Marker', 'o');\n\
         addpoints(h, [0, 1, 2], [0, 1, 0]);\n\
         drawnow;\n\
         title(\"Animated Line\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Animated Line"),
        "missing animatedline title in SVG: {svg}"
    );
    assert!(
        svg.contains("<polyline"),
        "expected animatedline polyline in SVG: {svg}"
    );
    assert!(
        svg.matches("<circle").count() >= 3,
        "expected animatedline markers in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn histogram_exports_binned_rectangles_svg() {
    let temp_dir = unique_temp_dir("histogram");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("histogram.svg");
    let source_path = temp_dir.join("graphics_histogram_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(64);\n\
         histogram([0.2, 0.4, 0.8, 1.2, 1.9], [0, 0.5, 1.0, 1.5, 2.0]);\n\
         xlim([0, 2]);\n\
         ylim([0, 2]);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("<rect x=\"92\" y=\"56\" width=\"192\" height=\"514\" fill=\"#1f77b4\" fill-opacity=\"0.75\" stroke=\"#1f77b4\" stroke-width=\"1\"/>"),
        "missing first histogram bin rect in SVG: {svg}"
    );
    assert!(
        svg.contains("<rect x=\"284\" y=\"313\" width=\"192\" height=\"257\" fill=\"#1f77b4\" fill-opacity=\"0.75\" stroke=\"#1f77b4\" stroke-width=\"1\"/>"),
        "missing second histogram bin rect in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn histogram_probability_normalization_exports_scaled_rectangles_svg() {
    let temp_dir = unique_temp_dir("histogram-probability");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("histogram_probability.svg");
    let source_path = temp_dir.join("graphics_histogram_probability_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(164);\n\
         histogram([0.2, 0.4, 0.8, 1.2, 1.9], [0, 0.5, 1.0, 1.5, 2.0], \"Normalization\", \"probability\");\n\
         xlim([0, 2]);\n\
         ylim([0, 1]);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("<rect x=\"92\" y=\"364.4\" width=\"192\" height=\"205.6\" fill=\"#1f77b4\" fill-opacity=\"0.75\" stroke=\"#1f77b4\" stroke-width=\"1\"/>"),
        "missing probability-normalized first histogram bin rect in SVG: {svg}"
    );
    assert!(
        svg.contains("<rect x=\"284\" y=\"467.2\" width=\"192\" height=\"102.8\" fill=\"#1f77b4\" fill-opacity=\"0.75\" stroke=\"#1f77b4\" stroke-width=\"1\"/>"),
        "missing probability-normalized second histogram bin rect in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn histogram_binwidth_exports_expected_bin_rectangles_svg() {
    let temp_dir = unique_temp_dir("histogram-binwidth");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("histogram_binwidth.svg");
    let source_path = temp_dir.join("graphics_histogram_binwidth_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(167);\n\
         histogram([0, 0.5, 1, 1.5, 2, 2.5, 3], \"BinWidth\", 1);\n\
         xlim([0, 3]);\n\
         ylim([0, 3]);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("<rect x=\"92\" y=\"227.3333\" width=\"256\" height=\"342.6667\" fill=\"#1f77b4\" fill-opacity=\"0.75\" stroke=\"#1f77b4\" stroke-width=\"1\"/>"),
        "missing first BinWidth histogram rect in SVG: {svg}"
    );
    assert!(
        svg.contains("<rect x=\"604\" y=\"56\" width=\"256\" height=\"514\" fill=\"#1f77b4\" fill-opacity=\"0.75\" stroke=\"#1f77b4\" stroke-width=\"1\"/>"),
        "missing last BinWidth histogram rect in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn histogram_numbins_exports_expected_bin_rectangles_svg() {
    let temp_dir = unique_temp_dir("histogram-numbins");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("histogram_numbins.svg");
    let source_path = temp_dir.join("graphics_histogram_numbins_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(168);\n\
         histogram([0, 0.5, 1, 1.5, 2, 2.5, 3], \"NumBins\", 4);\n\
         xlim([0, 3]);\n\
         ylim([0, 2]);\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("<rect x=\"92\" y=\"56\" width=\"192\" height=\"514\" fill=\"#1f77b4\" fill-opacity=\"0.75\" stroke=\"#1f77b4\" stroke-width=\"1\"/>"),
        "missing first NumBins histogram rect in SVG: {svg}"
    );
    assert!(
        svg.contains("<rect x=\"284\" y=\"313\" width=\"192\" height=\"257\" fill=\"#1f77b4\" fill-opacity=\"0.75\" stroke=\"#1f77b4\" stroke-width=\"1\"/>"),
        "missing second NumBins histogram rect in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn histogram2_exports_colormapped_tiles_with_colorbar() {
    let temp_dir = unique_temp_dir("histogram2");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("histogram2.svg");
    let source_path = temp_dir.join("graphics_histogram2_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(84);\n\
         histogram2([0.2, 0.7, 1.4, 1.8], [0.1, 0.9, 1.1, 1.9], [0, 1, 2], [0, 1, 2]);\n\
         colorbar();\n\
         title(\"Bivariate Histogram\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Bivariate Histogram"),
        "missing histogram2 title in SVG: {svg}"
    );
    assert!(
        svg.matches("fill=\"rgb(").count() >= 4,
        "expected colormapped histogram2 tiles in SVG: {svg}"
    );
    assert!(
        svg.matches("stroke=\"#777777\"").count() >= 1,
        "expected histogram2 colorbar outline in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn histogram2_pdf_normalization_exports_normalized_tile_colors() {
    let temp_dir = unique_temp_dir("histogram2-pdf");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("histogram2_pdf.svg");
    let source_path = temp_dir.join("graphics_histogram2_pdf_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(185);\n\
         histogram2([0, 0.5, 1, 1.5, 2, 3], [10, 10, 20, 20, 30, 30], [0, 1, 3], [10, 15, 30], \"Normalization\", \"pdf\");\n\
         title(\"PDF Histogram2\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("PDF Histogram2"),
        "missing histogram2 PDF title in SVG: {svg}"
    );
    assert!(
        svg.contains("fill=\"rgb(253,231,37)\" fill-opacity=\"0.82\" stroke=\"#666666\" stroke-width=\"0.8\""),
        "missing maximum-density histogram2 tile color in SVG: {svg}"
    );
    assert!(
        svg.contains("fill=\"rgb(8,133,187)\" fill-opacity=\"0.82\" stroke=\"#666666\" stroke-width=\"0.8\""),
        "missing intermediate-density histogram2 tile color in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn contour3_exports_projected_svg_line_segments() {
    let temp_dir = unique_temp_dir("contour3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("contour3.svg");
    let source_path = temp_dir.join("graphics_contour3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(215);\n\
         contour3([1, 2, 3; 2, 4, 2; 1, 2, 1], [1.5, 2.5, 3.5]);\n\
         title(\"Contour3 Plot\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Contour3 Plot"),
        "missing contour3 title in SVG: {svg}"
    );
    assert!(
        svg.contains("data-matc-3d=\"true\""),
        "missing contour3 3-D axes metadata in SVG: {svg}"
    );
    assert!(
        svg.matches("data-matc-dim=\"3\"").count() >= 2,
        "expected projected contour3 segment metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn pie_exports_slice_polygons_and_labels_svg() {
    let temp_dir = unique_temp_dir("pie");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("pie.svg");
    let source_path = temp_dir.join("graphics_pie_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(67);\n\
         pie([2, 1, 3], [0, 1, 0], {{\"North\", \"East\", \"West\"}});\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    for label in ["North", "East", "West"] {
        assert!(
            svg.contains(&format!(">{label}<")),
            "missing pie label `{label}` in SVG: {svg}"
        );
    }
    assert!(
        svg.contains("fill-opacity=\"0.88\""),
        "missing pie slice fill opacity in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "missing pie slice polygon in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

fn assert_svg_export(kind: ExecutionKind, export_builtin: &str) {
    let temp_dir = unique_temp_dir(export_builtin);
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join(format!("{export_builtin}.svg"));
    let source_path = temp_dir.join("graphics_export_runtime.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "x = [0, 1, 2, 3];\n\
         y = [0, 1, 4, 9];\n\
         f = figure(4);\n\
         plot(x, y);\n\
         hold on\n\
         scatter(x, y);\n\
         bar([0.5, 1.5, 2.5], [1, 2, 1]);\n\
         stem(x, [0, 2, 1, 3]);\n\
         title(\"Headless Export\");\n\
         xlabel('Input');\n\
         ylabel(\"Output\");\n\
         legend(\"Line\", \"Dots\", \"Bars\", \"Stem\");\n\
         {export_builtin}(f, \"{matlab_output_path}\", \"svg\");\n"
    );
    let svg = execute_and_read_svg(kind, &source_path, &source, &output_path);
    assert!(svg.contains("<svg"), "expected SVG output, found: {svg}");
    assert!(
        svg.contains("Headless Export"),
        "missing title in SVG: {svg}"
    );
    assert!(svg.contains("Input"), "missing xlabel in SVG: {svg}");
    assert!(svg.contains("Output"), "missing ylabel in SVG: {svg}");
    assert!(svg.contains("Line"), "missing legend label in SVG: {svg}");
    assert!(svg.contains("Dots"), "missing legend label in SVG: {svg}");
    assert!(svg.contains("Bars"), "missing legend label in SVG: {svg}");
    assert!(svg.contains("Stem"), "missing legend label in SVG: {svg}");
    assert!(
        svg.contains("<clipPath"),
        "missing plot clip path in SVG: {svg}"
    );
    assert!(
        svg.contains("clip-path=\"url(#matc-axes-clip-"),
        "missing clipped plot series group in SVG: {svg}"
    );
    assert!(
        svg.contains("polyline"),
        "missing plotted series in SVG: {svg}"
    );
    assert!(svg.contains("<rect"), "missing bar series in SVG: {svg}");
    assert!(
        svg.contains("circle"),
        "missing point markers in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn extended_linespec_markers_render_in_svg() {
    let temp_dir = unique_temp_dir("extended-linespec");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("extended_linespec.svg");
    let source_path = temp_dir.join("graphics_extended_linespec_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(75);\n\
         plot([0, 1, 2], [0, 1, 0], 'k-.x');\n\
         hold on;\n\
         plot([0, 1, 2], [1, 0, 1], 'r^');\n\
         title(\"Extended LineSpec\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("stroke-dasharray=\"8 5 2.5 5\""),
        "expected dash-dot polyline styling in SVG: {svg}"
    );
    assert!(
        svg.contains("<polygon"),
        "expected polygon marker output for triangle markers in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn pie3_exports_extruded_polygons_and_labels_svg() {
    let temp_dir = unique_temp_dir("pie3");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("pie3.svg");
    let source_path = temp_dir.join("graphics_pie3_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(210);\n\
         pie3([2, 3, 1], [0, 1, 0], [\"A\", \"B\", \"C\"]);\n\
         title(\"Pie3 Chart\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("Pie3 Chart"),
        "missing pie3 title in SVG: {svg}"
    );
    assert!(
        svg.contains(">A</text>") && svg.contains(">B</text>") && svg.contains(">C</text>"),
        "missing pie3 labels in SVG: {svg}"
    );
    assert!(
        svg.matches("class=\"matc-3d-patch\"").count() >= 6,
        "expected extruded pie3 patch metadata in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn marker_face_and_edge_colors_render_in_svg() {
    let temp_dir = unique_temp_dir("marker-colors");
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join("marker_colors.svg");
    let source_path = temp_dir.join("graphics_marker_colors_export.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "f = figure(76);\n\
         plot([0, 1, 2], [0, 1, 0], 'o');\n\
         hold on;\n\
         plot([0, 1, 2], [1, 0, 1], 's', 'MarkerFaceColor', 'r', 'MarkerEdgeColor', 'k');\n\
         title(\"Marker Colors\");\n\
         saveas(f, \"{matlab_output_path}\", \"svg\");\n"
    );

    let svg = execute_and_read_svg(
        ExecutionKind::Interpreter,
        &source_path,
        &source,
        &output_path,
    );
    assert!(
        svg.contains("fill=\"none\""),
        "expected default hollow marker rendering in SVG: {svg}"
    );
    assert!(
        svg.contains("fill=\"#ff0000\"") && svg.contains("stroke=\"#000000\""),
        "expected explicit marker face/edge colors in SVG: {svg}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

fn execute_and_read_svg(
    kind: ExecutionKind,
    source_path: &PathBuf,
    source: &str,
    output_path: &PathBuf,
) -> String {
    execute_source(kind, source_path, source).expect("execute graphics export source");
    fs::read_to_string(output_path).expect("read svg output")
}

fn execute_and_read_png(
    kind: ExecutionKind,
    source_path: &PathBuf,
    source: &str,
    output_path: &PathBuf,
) -> Vec<u8> {
    execute_source(kind, source_path, source).expect("execute graphics export source");
    fs::read(output_path).expect("read png output")
}

fn execute_and_read_pdf(
    kind: ExecutionKind,
    source_path: &PathBuf,
    source: &str,
    output_path: &PathBuf,
) -> Vec<u8> {
    execute_source(kind, source_path, source).expect("execute graphics export source");
    fs::read(output_path).expect("read pdf output")
}

fn execute_source(
    kind: ExecutionKind,
    source_path: &PathBuf,
    source: &str,
) -> Result<(), RuntimeError> {
    fs::write(source_path, source).expect("write source");

    let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
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
    match kind {
        ExecutionKind::Interpreter => {
            execute_script_with_identity(&hir, source_path.display().to_string(), None)
        }
        ExecutionKind::Bytecode => {
            execute_script_bytecode_with_identity(&hir, source_path.display().to_string(), None)
        }
    }
    .map(|_| ())
}

fn assert_runtime_error(
    kind: ExecutionKind,
    source_path: &PathBuf,
    source: &str,
    expected_substring: &str,
) {
    let error = execute_source(kind, source_path, source).expect_err("expected runtime error");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(expected_substring),
        "expected error containing `{expected_substring}`, found `{rendered}`"
    );
}

fn assert_png_export(kind: ExecutionKind, export_builtin: &str) {
    let temp_dir = unique_temp_dir(&format!("{export_builtin}-png"));
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join(format!("{export_builtin}.png"));
    let source_path = temp_dir.join("graphics_export_runtime_png.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "x = [0, 1, 2, 3];\n\
         y = [0, 1, 4, 9];\n\
         f = figure(14);\n\
         plot(x, y);\n\
         hold on\n\
         scatter(x, y);\n\
         title(\"Headless PNG Export\");\n\
         {export_builtin}(f, \"{matlab_output_path}\", \"png\");\n"
    );
    let png = execute_and_read_png(kind, &source_path, &source, &output_path);
    assert_png_signature_and_dimensions(&png, 900, 650);

    let _ = fs::remove_dir_all(&temp_dir);
}

fn assert_pdf_export(kind: ExecutionKind, export_builtin: &str) {
    let temp_dir = unique_temp_dir(&format!("{export_builtin}-pdf"));
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let output_path = temp_dir.join(format!("{export_builtin}.pdf"));
    let source_path = temp_dir.join("graphics_export_runtime_pdf.m");
    let matlab_output_path = output_path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "x = [0, 1, 2, 3];\n\
         y = [0, 1, 4, 9];\n\
         f = figure(15);\n\
         plot(x, y);\n\
         hold on\n\
         scatter(x, y);\n\
         title(\"Headless PDF Export\");\n\
         {export_builtin}(f, \"{matlab_output_path}\", \"pdf\");\n"
    );
    let pdf = execute_and_read_pdf(kind, &source_path, &source, &output_path);
    assert_pdf_signature(&pdf);
    assert_pdf_media_box(&pdf, 612.0, 792.0);

    let _ = fs::remove_dir_all(&temp_dir);
}

fn assert_png_signature_and_dimensions(png: &[u8], expected_width: u32, expected_height: u32) {
    assert!(
        png.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "expected PNG signature, found: {png:?}"
    );
    let (width, height) = png_dimensions(png).expect("png dimensions");
    assert_eq!(width, expected_width, "unexpected PNG width");
    assert_eq!(height, expected_height, "unexpected PNG height");
}

fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    Some((width, height))
}

fn assert_pdf_signature(pdf: &[u8]) {
    assert!(
        pdf.starts_with(b"%PDF-1.7"),
        "expected PDF signature, found: {pdf:?}"
    );
}

fn assert_pdf_media_box(pdf: &[u8], expected_width: f64, expected_height: f64) {
    let values = pdf_array_after(pdf, "/MediaBox [").expect("pdf media box");
    assert_eq!(values.len(), 4, "unexpected MediaBox entry count");
    assert!(values[0].abs() <= 0.01, "unexpected MediaBox x0");
    assert!(values[1].abs() <= 0.01, "unexpected MediaBox y0");
    assert!(
        (values[2] - expected_width).abs() <= 0.1,
        "unexpected MediaBox width: {:?}",
        values
    );
    assert!(
        (values[3] - expected_height).abs() <= 0.1,
        "unexpected MediaBox height: {:?}",
        values
    );
}

fn assert_pdf_image_transform(pdf: &[u8], expected: [f64; 6]) {
    let values = pdf_image_transform(pdf).expect("pdf image transform");
    for (actual, expected) in values.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= 0.1,
            "unexpected PDF image transform component: actual={actual} expected={expected}"
        );
    }
}

fn pdf_array_after(pdf: &[u8], marker: &str) -> Option<Vec<f64>> {
    let text = String::from_utf8_lossy(pdf);
    let start = text.find(marker)? + marker.len();
    let end = text[start..].find(']')? + start;
    text[start..end]
        .split_whitespace()
        .map(|value| value.parse::<f64>().ok())
        .collect()
}

fn pdf_image_transform(pdf: &[u8]) -> Option<[f64; 6]> {
    let text = String::from_utf8_lossy(pdf);
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    for index in 0..tokens.len() {
        if tokens[index] == "/Im1" && index >= 7 && tokens[index - 1] == "cm" {
            let values = tokens[index - 7..index - 1]
                .iter()
                .map(|value| value.parse::<f64>().ok())
                .collect::<Option<Vec<_>>>()?;
            return Some([
                values[0], values[1], values[2], values[3], values[4], values[5],
            ]);
        }
    }
    None
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("matlab-{label}-{}-{stamp}", std::process::id()))
}

fn svg_rect_attribute(line: &str, attribute: &str) -> Option<f64> {
    let needle = format!("{attribute}=\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    rest[..end].parse::<f64>().ok()
}

fn svg_polygon_points(line: &str) -> Option<Vec<(f64, f64)>> {
    let needle = "points=\"";
    let start = line.find(needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    let points = rest[..end]
        .split_whitespace()
        .map(|pair| {
            let (x, y) = pair.split_once(',')?;
            Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(points)
}

fn unique_coordinate_count(values: impl IntoIterator<Item = f64>) -> usize {
    let mut uniques = Vec::new();
    for value in values {
        let rounded = (value * 10.0).round() / 10.0;
        if !uniques
            .iter()
            .any(|existing: &f64| (existing - rounded).abs() <= 0.05)
        {
            uniques.push(rounded);
        }
    }
    uniques.len()
}

fn surface_polygon_mean_edge_dy(svg: &str) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;

    for line in svg
        .lines()
        .filter(|line| line.contains("<polygon") && line.contains("fill-opacity=\"0.96\""))
    {
        let points = svg_polygon_points(line).expect("surface polygon points");
        for index in 0..points.len() {
            let current = points[index];
            let next = points[(index + 1) % points.len()];
            total += (current.1 - next.1).abs();
            count += 1;
        }
    }

    assert!(
        count > 0,
        "expected surface polygon coordinates in SVG: {svg}"
    );
    total / count as f64
}
