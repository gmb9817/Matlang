use std::{fs, path::PathBuf};

use matlab_execution::{execute_function_file, execute_script, render_execution_result};
use matlab_frontend::{
    ast::CompilationUnitKind,
    parser::{parse_source, ParseMode},
    source::SourceFileId,
};
use matlab_ir::lower_to_hir;
use matlab_resolver::ResolverContext;
use matlab_runtime::Value;
use matlab_semantics::analyze_compilation_unit_with_context;

fn fixture_path(name: &str, extension: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/execution/interpreter")
        .join(format!("{name}.{extension}"))
}

fn assert_fixture(name: &str, mode: ParseMode, args: &[Value]) {
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
    let result = match unit.kind {
        CompilationUnitKind::Script => execute_script(&hir),
        CompilationUnitKind::FunctionFile => execute_function_file(&hir, args),
    }
    .expect("execute module");
    let rendered = render_execution_result(&result);
    assert_eq!(rendered, expected);
}

#[test]
fn builtin_switch_fixture_matches_golden() {
    assert_fixture("builtin_switch", ParseMode::Script, &[]);
}

#[test]
fn builtin_disp_runtime_fixture_matches_golden() {
    assert_fixture("builtin_disp_runtime", ParseMode::Script, &[]);
}

#[test]
fn builtin_fprintf_runtime_fixture_matches_golden() {
    assert_fixture("builtin_fprintf_runtime", ParseMode::Script, &[]);
}

#[test]
fn builtin_sprintf_runtime_fixture_matches_golden() {
    assert_fixture("builtin_sprintf_runtime", ParseMode::Script, &[]);
}

#[test]
fn loops_and_conditionals_fixture_matches_golden() {
    assert_fixture("loops_and_conditionals", ParseMode::Script, &[]);
}

#[test]
fn matrix_indexing_fixture_matches_golden() {
    assert_fixture("matrix_indexing", ParseMode::Script, &[]);
}

#[test]
fn function_file_nested_capture_fixture_matches_golden() {
    assert_fixture(
        "function_file_nested_capture",
        ParseMode::AutoDetect,
        &[Value::Scalar(4.0)],
    );
}

#[test]
fn function_file_transitive_nested_capture_fixture_matches_golden() {
    assert_fixture(
        "function_file_transitive_nested_capture",
        ParseMode::AutoDetect,
        &[],
    );
}

#[test]
fn function_file_persistent_nested_fixture_matches_golden() {
    assert_fixture(
        "function_file_persistent_nested",
        ParseMode::AutoDetect,
        &[Value::Scalar(4.0)],
    );
}

#[test]
fn function_stack_trace_fixture_matches_golden() {
    assert_fixture("function_stack_trace", ParseMode::AutoDetect, &[]);
}

#[test]
fn function_handles_fixture_matches_golden() {
    assert_fixture("function_handles", ParseMode::Script, &[]);
}

#[test]
fn external_resolution_fixture_matches_golden() {
    assert_fixture("external_resolution", ParseMode::Script, &[]);
}

#[test]
fn cell_indexing_and_handles_fixture_matches_golden() {
    assert_fixture("cell_indexing_and_handles", ParseMode::Script, &[]);
}

#[test]
fn comma_separated_cells_fixture_matches_golden() {
    assert_fixture("comma_separated_cells", ParseMode::Script, &[]);
}

#[test]
fn comma_separated_struct_fields_fixture_matches_golden() {
    assert_fixture("comma_separated_struct_fields", ParseMode::Script, &[]);
}

#[test]
fn comma_separated_forwarded_fields_fixture_matches_golden() {
    assert_fixture("comma_separated_forwarded_fields", ParseMode::Script, &[]);
}

#[test]
fn comma_separated_forwarded_cells_fixture_matches_golden() {
    assert_fixture("comma_separated_forwarded_cells", ParseMode::Script, &[]);
}

#[test]
fn comma_separated_forwarded_paren_fixture_matches_golden() {
    assert_fixture("comma_separated_forwarded_paren", ParseMode::Script, &[]);
}

#[test]
fn end_and_indexed_assignment_fixture_matches_golden() {
    assert_fixture("end_and_indexed_assignment", ParseMode::Script, &[]);
}

#[test]
fn nd_end_slice_parity_fixture_matches_golden() {
    assert_fixture("nd_end_slice_parity", ParseMode::Script, &[]);
}

#[test]
fn nd_linearization_parity_fixture_matches_golden() {
    assert_fixture("nd_linearization_parity", ParseMode::Script, &[]);
}

#[test]
fn nd_rectangular_assignment_reshape_fixture_matches_golden() {
    assert_fixture("nd_rectangular_assignment_reshape", ParseMode::Script, &[]);
}

#[test]
fn callable_index_parity_fixture_matches_golden() {
    assert_fixture("callable_index_parity", ParseMode::Script, &[]);
}

#[test]
fn external_function_handles_fixture_matches_golden() {
    assert_fixture("external_function_handles", ParseMode::Script, &[]);
}

#[test]
fn cell_subarray_assignment_fixture_matches_golden() {
    assert_fixture("cell_subarray_assignment", ParseMode::Script, &[]);
}

#[test]
fn global_persistent_runtime_fixture_matches_golden() {
    assert_fixture("global_persistent_runtime", ParseMode::Script, &[]);
}

#[test]
fn external_global_persistent_runtime_fixture_matches_golden() {
    assert_fixture("external_global_persistent_runtime", ParseMode::Script, &[]);
}

#[test]
fn multi_output_local_calls_fixture_matches_golden() {
    assert_fixture("multi_output_local_calls", ParseMode::Script, &[]);
}

#[test]
fn multi_output_external_calls_fixture_matches_golden() {
    assert_fixture("multi_output_external_calls", ParseMode::Script, &[]);
}

#[test]
fn builtin_outputs_fixture_matches_golden() {
    assert_fixture("builtin_outputs", ParseMode::Script, &[]);
}

#[test]
fn implicit_ans_runtime_fixture_matches_golden() {
    assert_fixture("implicit_ans_runtime", ParseMode::Script, &[]);
}

#[test]
fn struct_fields_and_handles_fixture_matches_golden() {
    assert_fixture("struct_fields_and_handles", ParseMode::Script, &[]);
}

#[test]
fn slice_indexing_and_assignment_fixture_matches_golden() {
    assert_fixture("slice_indexing_and_assignment", ParseMode::Script, &[]);
}

#[test]
fn growth_indexing_and_assignment_fixture_matches_golden() {
    assert_fixture("growth_indexing_and_assignment", ParseMode::Script, &[]);
}

#[test]
fn vector_indexing_and_assignment_fixture_matches_golden() {
    assert_fixture("vector_indexing_and_assignment", ParseMode::Script, &[]);
}

#[test]
fn deletion_assignment_fixture_matches_golden() {
    assert_fixture("deletion_assignment", ParseMode::Script, &[]);
}

#[test]
fn logical_indexing_fixture_matches_golden() {
    assert_fixture("logical_indexing", ParseMode::Script, &[]);
}

#[test]
fn logical_nd_indexing_fixture_matches_golden() {
    assert_fixture("logical_nd_indexing", ParseMode::Script, &[]);
}

#[test]
fn logical_value_model_fixture_matches_golden() {
    assert_fixture("logical_value_model", ParseMode::Script, &[]);
}

#[test]
fn logical_builtin_forms_fixture_matches_golden() {
    assert_fixture("logical_builtin_forms", ParseMode::Script, &[]);
}

#[test]
fn builtin_array_helpers_fixture_matches_golden() {
    assert_fixture("builtin_array_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_container_helpers_fixture_matches_golden() {
    assert_fixture("builtin_container_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_meshgrid_fixture_matches_golden() {
    assert_fixture("builtin_meshgrid", ParseMode::Script, &[]);
}

#[test]
fn builtin_ndgrid_fixture_matches_golden() {
    assert_fixture("builtin_ndgrid", ParseMode::Script, &[]);
}

#[test]
fn builtin_histcounts_fixture_matches_golden() {
    assert_fixture("builtin_histcounts", ParseMode::Script, &[]);
}

#[test]
fn builtin_histcounts2_fixture_matches_golden() {
    assert_fixture("builtin_histcounts2", ParseMode::Script, &[]);
}

#[test]
fn builtin_interp1_fixture_matches_golden() {
    assert_fixture("builtin_interp1", ParseMode::Script, &[]);
}

#[test]
fn builtin_interp2_fixture_matches_golden() {
    assert_fixture("builtin_interp2", ParseMode::Script, &[]);
}

#[test]
fn builtin_accumarray_fixture_matches_golden() {
    assert_fixture("builtin_accumarray", ParseMode::Script, &[]);
}

#[test]
fn builtin_diff_fixture_matches_golden() {
    assert_fixture("builtin_diff", ParseMode::Script, &[]);
}

#[test]
fn builtin_convolution_fixture_matches_golden() {
    assert_fixture("builtin_convolution", ParseMode::Script, &[]);
}

#[test]
fn builtin_deconv_fixture_matches_golden() {
    assert_fixture("builtin_deconv", ParseMode::Script, &[]);
}

#[test]
fn builtin_filter_fixture_matches_golden() {
    assert_fixture("builtin_filter", ParseMode::Script, &[]);
}

#[test]
fn builtin_poly_fixture_matches_golden() {
    assert_fixture("builtin_poly", ParseMode::Script, &[]);
}

#[test]
fn builtin_polyfit_fixture_matches_golden() {
    assert_fixture("builtin_polyfit", ParseMode::Script, &[]);
}

#[test]
fn builtin_polyval_fixture_matches_golden() {
    assert_fixture("builtin_polyval", ParseMode::Script, &[]);
}

#[test]
fn builtin_polyvalm_fixture_matches_golden() {
    assert_fixture("builtin_polyvalm", ParseMode::Script, &[]);
}

#[test]
fn builtin_linsolve_fixture_matches_golden() {
    assert_fixture("builtin_linsolve", ParseMode::Script, &[]);
}

#[test]
fn builtin_svd_fixture_matches_golden() {
    assert_fixture("builtin_svd", ParseMode::Script, &[]);
}

#[test]
fn builtin_eig_fixture_matches_golden() {
    assert_fixture("builtin_eig", ParseMode::Script, &[]);
}

#[test]
fn builtin_expm_fixture_matches_golden() {
    assert_fixture("builtin_expm", ParseMode::Script, &[]);
}

#[test]
fn builtin_sqrtm_fixture_matches_golden() {
    assert_fixture("builtin_sqrtm", ParseMode::Script, &[]);
}

#[test]
fn builtin_logm_fixture_matches_golden() {
    assert_fixture("builtin_logm", ParseMode::Script, &[]);
}

#[test]
fn builtin_funm_fixture_matches_golden() {
    assert_fixture("builtin_funm", ParseMode::Script, &[]);
}

#[test]
fn builtin_matrix_trig_fixture_matches_golden() {
    assert_fixture("builtin_matrix_trig", ParseMode::Script, &[]);
}

#[test]
fn builtin_matrix_invtrig_fixture_matches_golden() {
    assert_fixture("builtin_matrix_invtrig", ParseMode::Script, &[]);
}

#[test]
fn builtin_matrix_reciprocal_invtrig_fixture_matches_golden() {
    assert_fixture("builtin_matrix_reciprocal_invtrig", ParseMode::Script, &[]);
}

#[test]
fn builtin_transcendentals_fixture_matches_golden() {
    assert_fixture("builtin_transcendentals", ParseMode::Script, &[]);
}

#[test]
fn builtin_angle_helpers_fixture_matches_golden() {
    assert_fixture("builtin_angle_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_rounding_helpers_fixture_matches_golden() {
    assert_fixture("builtin_rounding_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_array_rearrangement_fixture_matches_golden() {
    assert_fixture("builtin_array_rearrangement", ParseMode::Script, &[]);
}

#[test]
fn builtin_nd_constructor_helpers_fixture_matches_golden() {
    assert_fixture("builtin_nd_constructor_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_nd_indexing_helpers_fixture_matches_golden() {
    assert_fixture("builtin_nd_indexing_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_nd_deletion_helpers_fixture_matches_golden() {
    assert_fixture("builtin_nd_deletion_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_nd_shape_helpers_fixture_matches_golden() {
    assert_fixture("builtin_nd_shape_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_repetition_helpers_fixture_matches_golden() {
    assert_fixture("builtin_repetition_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_dimension_shift_helpers_fixture_matches_golden() {
    assert_fixture("builtin_dimension_shift_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_floating_point_helpers_fixture_matches_golden() {
    assert_fixture("builtin_floating_point_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_numeric_constants_fixture_matches_golden() {
    assert_fixture("builtin_numeric_constants", ParseMode::Script, &[]);
}

#[test]
fn builtin_dimension_helpers_fixture_matches_golden() {
    assert_fixture("builtin_dimension_helpers", ParseMode::Script, &[]);
}

#[test]
fn builtin_complex_numbers_fixture_matches_golden() {
    assert_fixture("builtin_complex_numbers", ParseMode::Script, &[]);
}

#[test]
fn builtin_covariance_fixture_matches_golden() {
    assert_fixture("builtin_covariance", ParseMode::Script, &[]);
}

#[test]
fn builtin_quantile_fixture_matches_golden() {
    assert_fixture("builtin_quantile", ParseMode::Script, &[]);
}

#[test]
fn builtin_spread_fixture_matches_golden() {
    assert_fixture("builtin_spread", ParseMode::Script, &[]);
}

#[test]
fn builtin_zscore_fixture_matches_golden() {
    assert_fixture("builtin_zscore", ParseMode::Script, &[]);
}

#[test]
fn builtin_polycalculus_fixture_matches_golden() {
    assert_fixture("builtin_polycalculus", ParseMode::Script, &[]);
}

#[test]
fn builtin_del2_fixture_matches_golden() {
    assert_fixture("builtin_del2", ParseMode::Script, &[]);
}

#[test]
fn builtin_divergence_fixture_matches_golden() {
    assert_fixture("builtin_divergence", ParseMode::Script, &[]);
}

#[test]
fn builtin_curl_fixture_matches_golden() {
    assert_fixture("builtin_curl", ParseMode::Script, &[]);
}

#[test]
fn builtin_gradient_fixture_matches_golden() {
    assert_fixture("builtin_gradient", ParseMode::Script, &[]);
}

#[test]
fn builtin_trapezoid_fixture_matches_golden() {
    assert_fixture("builtin_trapezoid", ParseMode::Script, &[]);
}

#[test]
fn text_literals_and_builtins_fixture_matches_golden() {
    assert_fixture("text_literals_and_builtins", ParseMode::Script, &[]);
}

#[test]
fn command_form_text_fixture_matches_golden() {
    assert_fixture("command_form_text", ParseMode::Script, &[]);
}

#[test]
fn graphics_plotting_fixture_matches_golden() {
    assert_fixture("graphics_plotting", ParseMode::Script, &[]);
}

#[test]
fn graphics_fplot_fixture_matches_golden() {
    assert_fixture("graphics_fplot", ParseMode::Script, &[]);
}

#[test]
fn graphics_fsurf_fixture_matches_golden() {
    assert_fixture("graphics_fsurf", ParseMode::Script, &[]);
}

#[test]
fn graphics_fmesh_fixture_matches_golden() {
    assert_fixture("graphics_fmesh", ParseMode::Script, &[]);
}

#[test]
fn graphics_fimplicit_fixture_matches_golden() {
    assert_fixture("graphics_fimplicit", ParseMode::Script, &[]);
}

#[test]
fn graphics_fcontour_fixture_matches_golden() {
    assert_fixture("graphics_fcontour", ParseMode::Script, &[]);
}

#[test]
fn graphics_fcontour3_fixture_matches_golden() {
    assert_fixture("graphics_fcontour3", ParseMode::Script, &[]);
}

#[test]
fn graphics_fplot3_fixture_matches_golden() {
    assert_fixture("graphics_fplot3", ParseMode::Script, &[]);
}

#[test]
fn graphics_xline_yline_fixture_matches_golden() {
    assert_fixture("graphics_xline_yline", ParseMode::Script, &[]);
}

#[test]
fn graphics_sgtitle_subtitle_fixture_matches_golden() {
    assert_fixture("graphics_sgtitle_subtitle", ParseMode::Script, &[]);
}

#[test]
fn graphics_errorbar_fixture_matches_golden() {
    assert_fixture("graphics_errorbar", ParseMode::Script, &[]);
}

#[test]
fn graphics_errorbar_directions_fixture_matches_golden() {
    assert_fixture("graphics_errorbar_directions", ParseMode::Script, &[]);
}

#[test]
fn graphics_log_scales_fixture_matches_golden() {
    assert_fixture("graphics_log_scales", ParseMode::Script, &[]);
}

#[test]
fn graphics_axis_scale_properties_fixture_matches_golden() {
    assert_fixture("graphics_axis_scale_properties", ParseMode::Script, &[]);
}

#[test]
fn graphics_legend_options_fixture_matches_golden() {
    assert_fixture("graphics_legend_options", ParseMode::Script, &[]);
}

#[test]
fn graphics_annotation_fixture_matches_golden() {
    assert_fixture("graphics_annotation", ParseMode::Script, &[]);
}

#[test]
fn graphics_barh_fixture_matches_golden() {
    assert_fixture("graphics_barh", ParseMode::Script, &[]);
}

#[test]
fn graphics_stem3_fixture_matches_golden() {
    assert_fixture("graphics_stem3", ParseMode::Script, &[]);
}

#[test]
fn graphics_plotyy_fixture_matches_golden() {
    assert_fixture("graphics_plotyy", ParseMode::Script, &[]);
}

#[test]
fn graphics_bar3_fixture_matches_golden() {
    assert_fixture("graphics_bar3", ParseMode::Script, &[]);
}

#[test]
fn graphics_bar3h_fixture_matches_golden() {
    assert_fixture("graphics_bar3h", ParseMode::Script, &[]);
}

#[test]
fn graphics_meshz_fixture_matches_golden() {
    assert_fixture("graphics_meshz", ParseMode::Script, &[]);
}

#[test]
fn graphics_waterfall_fixture_matches_golden() {
    assert_fixture("graphics_waterfall", ParseMode::Script, &[]);
}

#[test]
fn graphics_ribbon_fixture_matches_golden() {
    assert_fixture("graphics_ribbon", ParseMode::Script, &[]);
}

#[test]
fn graphics_fill3_fixture_matches_golden() {
    assert_fixture("graphics_fill3", ParseMode::Script, &[]);
}

#[test]
fn graphics_contour3_fixture_matches_golden() {
    assert_fixture("graphics_contour3", ParseMode::Script, &[]);
}

#[test]
fn graphics_rotate3d_fixture_matches_golden() {
    assert_fixture("graphics_rotate3d", ParseMode::Script, &[]);
}

#[test]
fn graphics_linkaxes_fixture_matches_golden() {
    assert_fixture("graphics_linkaxes", ParseMode::Script, &[]);
}

#[test]
fn graphics_pie3_fixture_matches_golden() {
    assert_fixture("graphics_pie3", ParseMode::Script, &[]);
}

#[test]
fn graphics_yyaxis_fixture_matches_golden() {
    assert_fixture("graphics_yyaxis", ParseMode::Script, &[]);
}

#[test]
fn graphics_plot_multiple_groups_fixture_matches_golden() {
    assert_fixture("graphics_plot_multiple_groups", ParseMode::Script, &[]);
}

#[test]
fn graphics_plot3_fixture_matches_golden() {
    assert_fixture("graphics_plot3", ParseMode::Script, &[]);
}

#[test]
fn graphics_quiver_fixture_matches_golden() {
    assert_fixture("graphics_quiver", ParseMode::Script, &[]);
}

#[test]
fn graphics_quiver3_fixture_matches_golden() {
    assert_fixture("graphics_quiver3", ParseMode::Script, &[]);
}

#[test]
fn graphics_subplot_fixture_matches_golden() {
    assert_fixture("graphics_subplot", ParseMode::Script, &[]);
}

#[test]
fn graphics_tiledlayout_fixture_matches_golden() {
    assert_fixture("graphics_tiledlayout", ParseMode::Script, &[]);
}

#[test]
fn graphics_images_fixture_matches_golden() {
    assert_fixture("graphics_images", ParseMode::Script, &[]);
}

#[test]
fn graphics_image_properties_fixture_matches_golden() {
    assert_fixture("graphics_image_properties", ParseMode::Script, &[]);
}

#[test]
fn graphics_image_coordinates_fixture_matches_golden() {
    assert_fixture("graphics_image_coordinates", ParseMode::Script, &[]);
}

#[test]
fn graphics_image_structs_fixture_matches_golden() {
    assert_fixture("graphics_image_structs", ParseMode::Script, &[]);
}

#[test]
fn graphics_plot_property_pairs_fixture_matches_golden() {
    assert_fixture("graphics_plot_property_pairs", ParseMode::Script, &[]);
}

#[test]
fn graphics_plot_matrix_inputs_fixture_matches_golden() {
    assert_fixture("graphics_plot_matrix_inputs", ParseMode::Script, &[]);
}

#[test]
fn graphics_marker_colors_fixture_matches_golden() {
    assert_fixture("graphics_marker_colors", ParseMode::Script, &[]);
}

#[test]
fn graphics_rgb_imshow_fixture_matches_golden() {
    assert_fixture("graphics_rgb_imshow", ParseMode::Script, &[]);
}

#[test]
fn graphics_rgb_image_family_fixture_matches_golden() {
    assert_fixture("graphics_rgb_image_family", ParseMode::Script, &[]);
}

#[test]
fn graphics_colormap_extended_fixture_matches_golden() {
    assert_fixture("graphics_colormap_extended", ParseMode::Script, &[]);
}

#[test]
fn graphics_meshgrid_fixture_matches_golden() {
    assert_fixture("graphics_meshgrid", ParseMode::Script, &[]);
}

#[test]
fn graphics_axes_fixture_matches_golden() {
    assert_fixture("graphics_axes", ParseMode::Script, &[]);
}

#[test]
fn graphics_axis_square_fixture_matches_golden() {
    assert_fixture("graphics_axis_square", ParseMode::Script, &[]);
}

#[test]
fn graphics_scatter_semantics_fixture_matches_golden() {
    assert_fixture("graphics_scatter_semantics", ParseMode::Script, &[]);
}

#[test]
fn graphics_scatter_matrix_inputs_fixture_matches_golden() {
    assert_fixture("graphics_scatter_matrix_inputs", ParseMode::Script, &[]);
}

#[test]
fn graphics_animatedline_fixture_matches_golden() {
    assert_fixture("graphics_animatedline", ParseMode::Script, &[]);
}

#[test]
fn graphics_animatedline_limit_fixture_matches_golden() {
    assert_fixture("graphics_animatedline_limit", ParseMode::Script, &[]);
}

#[test]
fn graphics_handle_properties_fixture_matches_golden() {
    assert_fixture("graphics_handle_properties", ParseMode::Script, &[]);
}

#[test]
fn graphics_series_properties_fixture_matches_golden() {
    assert_fixture("graphics_series_properties", ParseMode::Script, &[]);
}

#[test]
fn graphics_hierarchy_properties_fixture_matches_golden() {
    assert_fixture("graphics_hierarchy_properties", ParseMode::Script, &[]);
}

#[test]
fn graphics_hierarchy_helpers_fixture_matches_golden() {
    assert_fixture("graphics_hierarchy_helpers", ParseMode::Script, &[]);
}

#[test]
fn graphics_handle_management_fixture_matches_golden() {
    assert_fixture("graphics_handle_management", ParseMode::Script, &[]);
}

#[test]
fn graphics_search_helpers_fixture_matches_golden() {
    assert_fixture("graphics_search_helpers", ParseMode::Script, &[]);
}

#[test]
fn graphics_vectorized_properties_fixture_matches_golden() {
    assert_fixture("graphics_vectorized_properties", ParseMode::Script, &[]);
}

#[test]
fn graphics_text_fixture_matches_golden() {
    assert_fixture("graphics_text", ParseMode::Script, &[]);
}

#[test]
fn graphics_line_properties_fixture_matches_golden() {
    assert_fixture("graphics_line_properties", ParseMode::Script, &[]);
}

#[test]
fn graphics_line_styles_fixture_matches_golden() {
    assert_fixture("graphics_line_styles", ParseMode::Script, &[]);
}

#[test]
fn graphics_plot_stylespec_fixture_matches_golden() {
    assert_fixture("graphics_plot_stylespec", ParseMode::Script, &[]);
}

#[test]
fn graphics_plot_stylespec_extended_fixture_matches_golden() {
    assert_fixture("graphics_plot_stylespec_extended", ParseMode::Script, &[]);
}

#[test]
fn graphics_rectangle_fixture_matches_golden() {
    assert_fixture("graphics_rectangle", ParseMode::Script, &[]);
}

#[test]
fn graphics_patch_fixture_matches_golden() {
    assert_fixture("graphics_patch", ParseMode::Script, &[]);
}

#[test]
fn graphics_axes_position_fixture_matches_golden() {
    assert_fixture("graphics_axes_position", ParseMode::Script, &[]);
}

#[test]
fn graphics_get_structs_fixture_matches_golden() {
    assert_fixture("graphics_get_structs", ParseMode::Script, &[]);
}

#[test]
fn graphics_figure_windows_fixture_matches_golden() {
    assert_fixture("graphics_figure_windows", ParseMode::Script, &[]);
}

#[test]
fn graphics_current_object_fixture_matches_golden() {
    assert_fixture("graphics_current_object", ParseMode::Script, &[]);
}

#[test]
fn graphics_set_structs_fixture_matches_golden() {
    assert_fixture("graphics_set_structs", ParseMode::Script, &[]);
}

#[test]
fn graphics_reset_fixture_matches_golden() {
    assert_fixture("graphics_reset", ParseMode::Script, &[]);
}

#[test]
fn graphics_copyobj_fixture_matches_golden() {
    assert_fixture("graphics_copyobj", ParseMode::Script, &[]);
}

#[test]
fn graphics_close_fixture_matches_golden() {
    assert_fixture("graphics_close", ParseMode::Script, &[]);
}

#[test]
fn graphics_close_callback_fixture_matches_golden() {
    assert_fixture("graphics_close_callback", ParseMode::Script, &[]);
}

#[test]
fn graphics_resize_callback_fixture_matches_golden() {
    assert_fixture("graphics_resize_callback", ParseMode::Script, &[]);
}

#[test]
fn graphics_box_fixture_matches_golden() {
    assert_fixture("graphics_box", ParseMode::Script, &[]);
}

#[test]
fn graphics_contour_fixture_matches_golden() {
    assert_fixture("graphics_contour", ParseMode::Script, &[]);
}

#[test]
fn graphics_surface_fixture_matches_golden() {
    assert_fixture("graphics_surface", ParseMode::Script, &[]);
}

#[test]
fn graphics_surface_combo_fixture_matches_golden() {
    assert_fixture("graphics_surface_combo", ParseMode::Script, &[]);
}

#[test]
fn graphics_mesh_fixture_matches_golden() {
    assert_fixture("graphics_mesh", ParseMode::Script, &[]);
}

#[test]
fn graphics_contourf_fixture_matches_golden() {
    assert_fixture("graphics_contourf", ParseMode::Script, &[]);
}

#[test]
fn graphics_view_fixture_matches_golden() {
    assert_fixture("graphics_view", ParseMode::Script, &[]);
}

#[test]
fn graphics_zaxis_fixture_matches_golden() {
    assert_fixture("graphics_zaxis", ParseMode::Script, &[]);
}

#[test]
fn graphics_shading_fixture_matches_golden() {
    assert_fixture("graphics_shading", ParseMode::Script, &[]);
}

#[test]
fn graphics_ticks_fixture_matches_golden() {
    assert_fixture("graphics_ticks", ParseMode::Script, &[]);
}

#[test]
fn graphics_ticklabels_fixture_matches_golden() {
    assert_fixture("graphics_ticklabels", ParseMode::Script, &[]);
}

#[test]
fn graphics_tickangles_fixture_matches_golden() {
    assert_fixture("graphics_tickangles", ParseMode::Script, &[]);
}

#[test]
fn graphics_stairs_fixture_matches_golden() {
    assert_fixture("graphics_stairs", ParseMode::Script, &[]);
}

#[test]
fn graphics_area_fixture_matches_golden() {
    assert_fixture("graphics_area", ParseMode::Script, &[]);
}

#[test]
fn graphics_histogram_fixture_matches_golden() {
    assert_fixture("graphics_histogram", ParseMode::Script, &[]);
}

#[test]
fn graphics_histogram2_fixture_matches_golden() {
    assert_fixture("graphics_histogram2", ParseMode::Script, &[]);
}

#[test]
fn graphics_pie_fixture_matches_golden() {
    assert_fixture("graphics_pie", ParseMode::Script, &[]);
}

#[test]
fn struct_field_helpers_fixture_matches_golden() {
    assert_fixture("struct_field_helpers", ParseMode::Script, &[]);
}

#[test]
fn struct_array_helpers_fixture_matches_golden() {
    assert_fixture("struct_array_helpers", ParseMode::Script, &[]);
}

#[test]
fn struct_nd_field_helpers_fixture_matches_golden() {
    assert_fixture("struct_nd_field_helpers", ParseMode::Script, &[]);
}

#[test]
fn struct_constructor_helpers_fixture_matches_golden() {
    assert_fixture("struct_constructor_helpers", ParseMode::Script, &[]);
}

#[test]
fn indexed_struct_assignment_fixture_matches_golden() {
    assert_fixture("indexed_struct_assignment", ParseMode::Script, &[]);
}

#[test]
fn undefined_struct_root_assignment_fixture_matches_golden() {
    assert_fixture(
        "undefined_struct_root_assignment",
        ParseMode::AutoDetect,
        &[],
    );
}

#[test]
fn implicit_root_growth_fixture_matches_golden() {
    assert_fixture("implicit_root_growth", ParseMode::Script, &[]);
}

#[test]
fn text_indexing_and_search_fixture_matches_golden() {
    assert_fixture("text_indexing_and_search", ParseMode::Script, &[]);
}

#[test]
fn linear_assignment_preserves_extent_fixture_matches_golden() {
    assert_fixture("linear_assignment_preserves_extent", ParseMode::Script, &[]);
}

#[test]
fn reduction_and_search_helpers_fixture_matches_golden() {
    assert_fixture("reduction_and_search_helpers", ParseMode::Script, &[]);
}

#[test]
fn ordering_and_shape_helpers_fixture_matches_golden() {
    assert_fixture("ordering_and_shape_helpers", ParseMode::Script, &[]);
}

#[test]
fn text_case_and_concat_helpers_fixture_matches_golden() {
    assert_fixture("text_case_and_concat_helpers", ParseMode::Script, &[]);
}

#[test]
fn sequence_and_accumulation_helpers_fixture_matches_golden() {
    assert_fixture("sequence_and_accumulation_helpers", ParseMode::Script, &[]);
}

#[test]
fn text_transform_helpers_fixture_matches_golden() {
    assert_fixture("text_transform_helpers", ParseMode::Script, &[]);
}

#[test]
fn text_join_and_between_helpers_fixture_matches_golden() {
    assert_fixture("text_join_and_between_helpers", ParseMode::Script, &[]);
}

#[test]
fn text_before_after_join_helpers_fixture_matches_golden() {
    assert_fixture("text_before_after_join_helpers", ParseMode::Script, &[]);
}

#[test]
fn text_replace_and_pad_helpers_fixture_matches_golden() {
    assert_fixture("text_replace_and_pad_helpers", ParseMode::Script, &[]);
}

#[test]
fn text_format_helpers_fixture_matches_golden() {
    assert_fixture("text_format_helpers", ParseMode::Script, &[]);
}

#[test]
fn text_array_helpers_fixture_matches_golden() {
    assert_fixture("text_array_helpers", ParseMode::Script, &[]);
}

#[test]
fn try_catch_runtime_fixture_matches_golden() {
    assert_fixture("try_catch_runtime", ParseMode::Script, &[]);
}

#[test]
fn error_and_reflection_fixture_matches_golden() {
    assert_fixture("error_and_reflection", ParseMode::Script, &[]);
}

#[test]
fn error_formatting_runtime_fixture_matches_golden() {
    assert_fixture("error_formatting_runtime", ParseMode::Script, &[]);
}

#[test]
fn get_report_runtime_fixture_matches_golden() {
    assert_fixture("get_report_runtime", ParseMode::Script, &[]);
}

#[test]
fn mexception_formatting_runtime_fixture_matches_golden() {
    assert_fixture("mexception_formatting_runtime", ParseMode::Script, &[]);
}

#[test]
fn mexception_helpers_fixture_matches_golden() {
    assert_fixture("mexception_helpers", ParseMode::Script, &[]);
}

#[test]
fn mexception_method_calls_fixture_matches_golden() {
    assert_fixture("mexception_method_calls", ParseMode::Script, &[]);
}

#[test]
fn rethrow_runtime_fixture_matches_golden() {
    assert_fixture("rethrow_runtime", ParseMode::Script, &[]);
}

#[test]
fn throw_as_caller_runtime_fixture_matches_golden() {
    assert_fixture("throw_as_caller_runtime", ParseMode::AutoDetect, &[]);
}

#[test]
fn warning_and_lastwarn_fixture_matches_golden() {
    assert_fixture("warning_and_lastwarn", ParseMode::Script, &[]);
}

#[test]
fn warning_control_query_fixture_matches_golden() {
    assert_fixture("warning_control_query", ParseMode::Script, &[]);
}

#[test]
fn pause_control_query_fixture_matches_golden() {
    assert_fixture("pause_control_query", ParseMode::Script, &[]);
}
