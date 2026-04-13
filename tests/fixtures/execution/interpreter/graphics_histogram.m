f = figure(63);
main_hist = histogram([0, 0, 1, 1, 1, 2, 3], 4);
hold on
edge_hist = histogram([0.2, 0.4, 0.8, 1.2, 1.9], [0, 0.5, 1.0, 1.5, 2.0]);
legend_handle = legend("Auto Bins", "Explicit Edges");
x_bounds = xlim();
y_bounds = ylim();
current = gcf();
