f = figure(41);
styled = line('XData', [0, 1, 2], 'YData', [0, 1, 0], 'LineStyle', '--', 'Marker', 'o', 'MarkerSize', 8, 'LineWidth', 4);
hold on
plain = plot([0, 1, 2], [2, 1, 2]);

styled_line_style = get(styled, 'LineStyle');
styled_marker = get(styled, 'Marker');
styled_marker_size = get(styled, 'MarkerSize');
styled_line_width = get(styled, 'LineWidth');

set(plain, 'LineStyle', ':', 'Marker', 's', 'MarkerSize', 6, 'LineWidth', 3);
plain_line_style = get(plain, 'LineStyle');
plain_marker = get(plain, 'Marker');
plain_marker_size = get(plain, 'MarkerSize');
plain_line_width = get(plain, 'LineWidth');

dashed_lines = findobj(f, 'LineStyle', '--');
square_markers = findobj(f, 'Marker', 's');
