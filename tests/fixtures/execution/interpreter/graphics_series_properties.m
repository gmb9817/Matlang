f = figure(31);
line = plot([0, 1, 2, 3], [0, 1, 4, 9]);
hold on
dots = scatter([0, 1, 2, 3], [1, 0, 1, 0]);

line_type = get(line, 'Type');
line_x = get(line, 'XData');
line_y = get(line, 'YData');
set(line, 'DisplayName', "Curve");
set(line, 'YData', [0, 2, 8, 18]);
line_name = get(line, 'DisplayName');
updated_y = get(line, 'YData');

set(dots, 'DisplayName', 'Dots');
legend_handle = legend('show');
legend_labels = get(gca(), 'Legend');
set(dots, 'Visible', 'off');
dots_visible = get(dots, 'Visible');
visible_xlim = xlim();

upper = subplot(212);
curve3 = plot3([0, 1, 2], [0, 1, 0], [1, 2, 3]);
curve3_type = get(curve3, 'Type');
curve3_z = get(curve3, 'ZData');
set(curve3, 'DisplayName', "Arc");
set(curve3, 'ZData', [1, 3, 5]);
curve3_name = get(curve3, 'DisplayName');
curve3_z_updated = get(curve3, 'ZData');
