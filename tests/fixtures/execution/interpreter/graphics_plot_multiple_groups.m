x = [0, 1, 2];
y1 = [0, 1, 0];
y2 = [1, 0, 1];

f = figure(51);
h = plot(x, y1, 'r--', x, y2, 'k^', 'LineWidth', 4);
styles = get(h, 'LineStyle');
markers = get(h, 'Marker');
widths = get(h, 'LineWidth');
line2_x = get(h(2), 'XData');
line2_y = get(h(2), 'YData');

f3 = figure(52);
h3 = plot3(x, y1, [1, 2, 3], 'b:s', x, y2, [3, 2, 1], 'm-.d', 'LineWidth', 2);
styles3 = get(h3, 'LineStyle');
markers3 = get(h3, 'Marker');
widths3 = get(h3, 'LineWidth');
z2 = get(h3(2), 'ZData');
