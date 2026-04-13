f = figure(48);
ax = axes('Position', [0.2, 0.2, 0.4, 0.3]);
set(ax, 'Box', 'off', 'Grid', 'on', 'XLim', [0, 2]);
line_handle = line('XData', [0, 1, 2], 'YData', [0, 1, 0], 'Color', 'r', 'LineStyle', '--', 'Marker', 'o');

before_box = get(ax, 'Box');
before_grid = get(ax, 'Grid');
before_color = get(line_handle, 'Color');
before_style = get(line_handle, 'LineStyle');
before_marker = get(line_handle, 'Marker');

reset(ax);
after_box = get(ax, 'Box');
after_grid = get(ax, 'Grid');
after_xlim = get(ax, 'XLim');

reset(line_handle);
after_color = get(line_handle, 'Color');
after_style = get(line_handle, 'LineStyle');
after_marker = get(line_handle, 'Marker');
after_x = get(line_handle, 'XData');
