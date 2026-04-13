f = figure(81);
implicit_handle = quiver([1, -1, 0.5], [0, 1, 0], 0);
implicit_x = xlim();
implicit_y = ylim();

clf(f);
quiver_main = quiver([0, 1, 2], [0, 1, 0], [1, 0, -1], [0, 1, 0.5], 0);
hold on
[xg, yg] = meshgrid([0, 2], [0, 1]);
matrix_quiver = quiver(xg, yg, [0.5, 1; 0.25, -0.5], [1, 0.5; 0.25, -0.5], 0);
legend_handle = legend("Explicit Vectors", "Matrix Grid");
x_bounds = xlim();
y_bounds = ylim();
current = gcf();
