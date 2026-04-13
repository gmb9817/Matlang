f = figure(37);
curve = plot([0, 1, 2], [0, 1, 0]);
hold on
vx = xline(1, 'r--', 'Center', 'LineWidth', 1.5);
hy = yline(0.5, 'k:', 'Threshold');
x_limits = xlim();
y_limits = ylim();
current = gcf();
