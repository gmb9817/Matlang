f = figure(85);
implicit = quiver3([1, 0, -1], [0, 1, 0], [0.5, 0.5, 1], 0);
top_view = view(2);

clf(f);
[xg, yg] = meshgrid([0, 2], [0, 1]);
zg = [0, 1; 2, 3];
field = quiver3(xg, yg, zg, [1, 0.5; -0.5, 1], [0.25, 1; 0.5, -0.5], [0.5, 0.25; 1, -0.5], 0);
custom_view = view(45, 20);
queried_view = view();
z_bounds = zlim();
current = gcf();
