f = figure(30);
ax1 = subplot(211);
plot([0, 1], [0, 1]);
before_axes = get(f, 'CurrentAxes');
before_hold = ishold(ax1);

set(ax1, 'Title', "Bridge", 'XLabel', 'time', 'YLabel', 'value');
set(ax1, 'XLim', [-1, 3], 'YLim', [0, 2], 'View', [10, 20]);
set(ax1, 'XTick', [0, 1, 2], 'XTickLabel', {'zero', 'one', 'two'}, 'XTickAngle', 30);
set(ax1, 'Box', 'off', 'Visible', 'off', 'Grid', 'on', 'Hold', 'on');
set(ax1, 'CLim', [2, 8], 'Colormap', 'hot', 'Colorbar', 'on');
after_hold = ishold(ax1);

title_text = get(ax1, 'Title');
xlabel_text = get(ax1, 'XLabel');
ylabel_text = get(ax1, 'YLabel');
xlim_value = get(ax1, 'XLim');
ylim_value = get(ax1, 'YLim');
xtick_value = get(ax1, 'XTick');
xticklabel_value = get(ax1, 'XTickLabel');
xtickangle_value = get(ax1, 'XTickAngle');
box_value = get(ax1, 'Box');
visible_value = get(ax1, 'Visible');
grid_value = get(ax1, 'Grid');
hold_value = get(ax1, 'Hold');
view_value = get(ax1, 'View');
clim_value = get(ax1, 'CLim');
colormap_value = get(ax1, 'Colormap');
colorbar_value = get(ax1, 'Colorbar');
figure_number = get(f, 'Number');
paper_units_default = get(f, 'PaperUnits');
paper_type_default = get(f, 'PaperType');
paper_size_default = get(f, 'PaperSize');
paper_orientation_default = get(f, 'PaperOrientation');
paper_mode_default = get(f, 'PaperPositionMode');
paper_position_default = get(f, 'PaperPosition');

set(f, 'PaperUnits', 'centimeters');
paper_size_cm = get(f, 'PaperSize');
paper_position_cm = get(f, 'PaperPosition');

set(f, 'PaperUnits', 'points');
paper_size_points = get(f, 'PaperSize');

set(f, 'PaperUnits', 'normalized');
paper_size_normalized = get(f, 'PaperSize');
paper_position_normalized = get(f, 'PaperPosition');

set(f, 'PaperUnits', 'inches');
set(f, 'PaperType', 'a4');
paper_type_a4 = get(f, 'PaperType');
paper_size_a4 = get(f, 'PaperSize');

set(f, 'PaperSize', [12, 14]);
paper_type_custom = get(f, 'PaperType');
paper_size_custom = get(f, 'PaperSize');

set(f, 'PaperOrientation', 'landscape');
paper_orientation_landscape = get(f, 'PaperOrientation');
paper_position_landscape = get(f, 'PaperPosition');

set(f, 'PaperPositionMode', 'manual');
set(f, 'PaperPosition', [1, 2, 3, 4]);
paper_mode_manual = get(f, 'PaperPositionMode');
paper_position_manual = get(f, 'PaperPosition');

lower = subplot(212);
plot([0, 1], [1, 0]);
set(lower, 'Legend', {'Down'});
title(lower, "Lower");
xlabel(lower, "distance");
ylabel(lower, "magnitude");
zlabel(lower, "depth");
caxis(ax1, [1, 9]);
top_clim_after_target = caxis(ax1);
lower_clim_after_target = get(lower, 'CLim');
colormap(ax1, 'spring');
top_colormap_after_target = get(ax1, 'Colormap');
lower_colormap_after_target = get(lower, 'Colormap');
colorbar(ax1, 'off');
top_colorbar_after_target = get(ax1, 'Colorbar');
lower_colorbar_after_target = get(lower, 'Colorbar');
legend(ax1, {'Top'});
top_legend_after_target = get(ax1, 'Legend');
lower_legend_after_target = get(lower, 'Legend');
current_lower = gca();
figure_current = get(f, 'CurrentAxes');
legend_value = get(lower, 'Legend');
lower_title = get(lower, 'Title');
lower_xlabel = get(lower, 'XLabel');
lower_ylabel = get(lower, 'YLabel');
lower_zlabel = get(lower, 'ZLabel');
upper_title_after_lower_label = get(ax1, 'Title');

set(f, 'CurrentAxes', ax1);
restored_axes = get(f, 'CurrentAxes');
restored_gca = gca();
