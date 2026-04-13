x = [0, 1, 2];
y = [0, 1; 1, 0; 4, 2];

figure;
hxy = plot(x, y, 'LineWidth', 3);
hxy_widths = get(hxy, 'LineWidth');
hxy_first_x = get(hxy(1), 'XData');
hxy_first_y = get(hxy(1), 'YData');
hxy_second_y = get(hxy(2), 'YData');
hxy_types = get(hxy, 'Type');

figure(2);
hy = plot(y, '--');
hy_styles = get(hy, 'LineStyle');
hy_first_x = get(hy(1), 'XData');
hy_second_y = get(hy(2), 'YData');
