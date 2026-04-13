f = figure('Name', 'Primary Figure', 'Position', [10, 20, 700, 500]);
name_initial = get(f, 'Name');
number_title_initial = get(f, 'NumberTitle');
visible_initial = get(f, 'Visible');
position_initial = get(f, 'Position');
window_style_initial = get(f, 'WindowStyle');

props = struct('Name', "Docked Figure", 'NumberTitle', 'off', 'Visible', 'off', 'Position', [11, 22, 333, 444], 'WindowStyle', 'docked');
set(f, props);

fig_props = get(f);
name_struct = fig_props.Name;
number_title_struct = fig_props.NumberTitle;
visible_struct = fig_props.Visible;
position_struct = fig_props.Position;
window_style_struct = fig_props.WindowStyle;

figure(f, 'Name', 'Shown Again');
name_after_figure = get(f, 'Name');
visible_after_figure = get(f, 'Visible');

plot([0, 1], [1, 0]);
clf(f);

name_after_clf = get(f, 'Name');
number_title_after_clf = get(f, 'NumberTitle');
visible_after_clf = get(f, 'Visible');
position_after_clf = get(f, 'Position');
window_style_after_clf = get(f, 'WindowStyle');

set(f, 'Name', 'Reset Me', 'NumberTitle', 'off', 'Visible', 'off');
set(f, 'PaperUnits', 'centimeters');
set(f, 'PaperPositionMode', 'manual');
set(f, 'PaperPosition', [1, 2, 3, 4]);
set(f, 'PaperType', 'a4');
set(f, 'PaperOrientation', 'landscape');
clf(f, 'reset');

name_after_clf_reset = get(f, 'Name');
number_title_after_clf_reset = get(f, 'NumberTitle');
visible_after_clf_reset = get(f, 'Visible');
position_after_clf_reset = get(f, 'Position');
window_style_after_clf_reset = get(f, 'WindowStyle');
paper_units_after_clf_reset = get(f, 'PaperUnits');
paper_position_after_clf_reset = get(f, 'PaperPosition');
paper_mode_after_clf_reset = get(f, 'PaperPositionMode');
paper_type_after_clf_reset = get(f, 'PaperType');
paper_size_after_clf_reset = get(f, 'PaperSize');
paper_orientation_after_clf_reset = get(f, 'PaperOrientation');

g = figure(72, 'Name', 'Secondary Figure', 'WindowStyle', 'normal');
g_name = get(g, 'Name');
g_visible = get(g, 'Visible');
g_window_style = get(g, 'WindowStyle');
