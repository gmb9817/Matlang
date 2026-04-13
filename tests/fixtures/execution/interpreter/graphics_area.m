f = figure(60);
main_area = area([0, 1, 2, 3], [0, 1, 4, 1]);
hold on
overlay_area = area([2, 1, 3]);
legend_handle = legend("Main Area", "Overlay Area");
x_bounds = xlim();
y_bounds = ylim();
current = gcf();
