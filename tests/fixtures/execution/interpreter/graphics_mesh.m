z = [0, 1, 0; 1, 2, 1; 0, 1, 0];
f = figure(25);
mesh_one = mesh(z);
colormap("hot");
scale_one = caxis();
hold on
mesh_two = mesh([1, 2, 3], [1, 2, 3], [1, 0, 1; 0, 1, 0; 1, 0, 1]);
legend_handle = legend("Peak Mesh", "Checker Mesh");
cb = colorbar();
title("Meshes");
current = gcf();
