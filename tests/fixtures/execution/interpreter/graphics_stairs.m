f = figure(57);
stairs_main = stairs([0, 1, 2, 3], [0, 1, 4, 9]);
hold on
stairs_default_x = stairs([3, 1, 2]);
legend_handle = legend("Quadratic Steps", "Default X Steps");
x_bounds = xlim();
y_bounds = ylim();
current = gcf();
