f = figure(47);
ax = axes('Position', [0.2, 0.2, 0.4, 0.3]);
plot([0, 1], [1, 0]);

ax_props = get(ax);
ax_props.Box = 'off';
ax_props.XLim = [0, 2];
ax_props.XTick = [0, 2];
ax_props.XTickLabel = {'lo', 'hi'};
set(ax, ax_props);

ax_box = get(ax, 'Box');
ax_xlim = get(ax, 'XLim');
ax_labels = get(ax, 'XTickLabel');

line_handle = line([0, 1, 2], [0, 1, 0]);
line_props = get(line_handle);
line_props.Color = [1, 0, 0];
line_props.LineStyle = '--';
line_props.Marker = 'o';
set(line_handle, line_props);

line_color = get(line_handle, 'Color');
line_style = get(line_handle, 'LineStyle');
line_marker = get(line_handle, 'Marker');

copy = copyobj(line_handle, ax);
shared = get(copy);
shared.Visible = 'off';
shared.LineWidth = 3;
set([line_handle, copy], shared);
shared_visible = get([line_handle, copy], 'Visible');
shared_width = get([line_handle, copy], 'LineWidth');

fig_props = struct("PaperUnits", "centimeters", "PaperType", "a4", "PaperOrientation", "landscape", "PaperPositionMode", "manual", "PaperPosition", [1, 2, 3, 4]);
set(f, fig_props);
fig_units = get(f, 'PaperUnits');
fig_type = get(f, 'PaperType');
fig_orientation = get(f, 'PaperOrientation');
fig_mode = get(f, 'PaperPositionMode');
fig_position = get(f, 'PaperPosition');
